#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
PAYLOAD="$(cat)"

python3 - "$ROOT_DIR" "$PAYLOAD" <<'PY'
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys

root_dir = sys.argv[1]
data = json.loads(sys.argv[2] or "{}")
conv_id = data.get("conversationId") or "unknown"
tool_call = data.get("toolCall") or {}
tool_name = tool_call.get("name") or ""
tool_id = tool_call.get("id") or "step"
args = tool_call.get("args") or {}

ws_hash = hashlib.md5(root_dir.encode("utf-8")).hexdigest()[:12]
state_dir = f"/tmp/frontal-hooks-{ws_hash}"
os.makedirs(state_dir, exist_ok=True)
state_path = os.path.join(state_dir, f"{conv_id}.json")

state = {"role": "primary", "rs_reads": 0}
if os.path.isfile(state_path):
    try:
        with open(state_path, "r", encoding="utf-8") as f:
            state = json.load(f)
    except Exception:
        pass

role = state.get("role", "primary")
rs_reads = int(state.get("rs_reads", 0))

def save_state():
    with open(state_path, "w", encoding="utf-8") as f:
        json.dump({"role": role, "rs_reads": rs_reads}, f)

def deny(reason):
    print(json.dumps({"decision": "deny", "reason": reason}))
    sys.exit(0)

def allow(overwrite=None):
    out = {"decision": "allow"}
    if overwrite:
        out["overwrite"] = overwrite
    print(json.dumps(out))
    sys.exit(0)

def split_shell_segments(cmd):
    try:
        lex = shlex.shlex(cmd, posix=True, punctuation_chars=";&|()")
        lex.whitespace_split = True
        tokens = list(lex)
    except ValueError:
        return [seg.split() for seg in re.split(r"(?:&&|\|\||[;\n|&()])", cmd) if seg.strip()]

    segments = []
    cur = []
    seps = {";", ";;", "&", "&&", "|", "||", "(", ")", "\n"}
    for tok in tokens:
        if tok in seps:
            if cur:
                segments.append(cur)
                cur = []
        else:
            cur.append(tok)
    if cur:
        segments.append(cur)
    return segments

def unwrap_command(seg):
    idx = 0
    wrappers = {"env", "command", "builtin", "exec", "nohup", "time"}
    while idx < len(seg):
        tok = seg[idx]
        if re.match(r"^[A-Za-z_][A-Za-z0-9_]*=.*", tok):
            idx += 1
            continue
        base = os.path.basename(tok)
        if base in wrappers:
            idx += 1
            while idx < len(seg) and seg[idx].startswith("-"):
                idx += 1
            continue
        if base == "timeout":
            idx += 1
            while idx < len(seg) and seg[idx].startswith("-"):
                if seg[idx] in ("-k", "--kill-after", "-s", "--signal") and idx + 1 < len(seg):
                    idx += 2
                else:
                    idx += 1
            if idx < len(seg):
                idx += 1
            continue
        return base, seg[idx:]
    return "", []

def extract_git_subcommand(git_args):
    value_flags = {"-C", "-c", "--git-dir", "--work-tree", "--namespace", "--super-prefix", "--exec-path", "--config-env"}
    i = 0
    while i < len(git_args):
        tok = git_args[i]
        if tok == "--":
            return git_args[i + 1] if i + 1 < len(git_args) else ""
        if tok in value_flags:
            i += 2
            continue
        if tok.startswith("-"):
            i += 1
            continue
        return tok
    return ""

def has_rust_comment_or_banned_macro(text):
    if not text:
        return None
    n = len(text)
    i = 0
    prev = " "
    while i < n:
        ch = text[i]
        if ch == "'" and i + 2 < n and text[i + 1] != "\\" and text[i + 2] == "'":
            i += 3
            prev = "'"
            continue
        if ch == "'" and i + 3 < n and text[i + 1] == "\\" and text[i + 3] == "'":
            i += 4
            prev = "'"
            continue
        is_ident = prev.isalnum() or prev == "_"
        if not is_ident and (ch == "r" or (ch in ("b", "c") and i + 1 < n and text[i + 1] == "r")):
            r_idx = i if ch == "r" else i + 1
            h = r_idx + 1
            while h < n and text[h] == "#":
                h += 1
            if h < n and text[h] == '"':
                hashes = h - (r_idx + 1)
                closing = '"' + ("#" * hashes)
                end = text.find(closing, h + 1)
                if end == -1:
                    return None
                i = end + len(closing)
                prev = "#" if hashes > 0 else '"'
                continue
        if ch == '"':
            i += 1
            while i < n:
                if text[i] == "\\":
                    i += 2
                    continue
                if text[i] == '"':
                    i += 1
                    break
                i += 1
            prev = '"'
            continue
        if ch == "/" and i + 1 < n and text[i + 1] == "/":
            return "line comment '//'"
        if ch == "/" and i + 1 < n and text[i + 1] == "*":
            return "block comment '/* ... */'"
        if ch == "#":
            seg = "".join(text[i:i + 40].split())
            if seg.startswith("#[doc") or seg.startswith("#![doc"):
                return "doc attribute"
            if seg.startswith("#[rustfmt::skip") or seg.startswith("#![rustfmt::skip"):
                return "rustfmt::skip attribute"
        if ch == "i" and not is_ident:
            seg = text[i:i + 16]
            if seg.startswith(("include!(", "include_str!(", "include_bytes!(")):
                return "include*! macro"
        prev = ch
        i += 1
    return None

if tool_name in ("invoke_subagent", "run_workflow"):
    if tool_name == "invoke_subagent" and args.get("TypeName") == "self":
        ws = args.get("Workspace") or "inherit"
        if ws not in ("branch", "share"):
            deny("Subagent 'self' requires isolated Workspace ('branch' or 'share'); use TypeName='research' for read-only investigation.")
    if role == "primary":
        rs_reads = 0
        save_state()
    safe_id = re.sub(r"[^A-Za-z0-9._-]", "_", str(tool_id))
    prefix = "spawning.subagent" if tool_name == "invoke_subagent" else "spawning.wf"
    with open(os.path.join(state_dir, f"{prefix}.{safe_id}"), "w", encoding="utf-8") as f:
        f.write(conv_id)
    allow()

if tool_name == "manage_task":
    action = str(args.get("Action") or "").lower()
    if action in ("status", "list"):
        deny("Polling via manage_task is prohibited. End the turn to wait for reactive completion notifications.")
    allow()

if tool_name == "read_file":
    path = str(args.get("Path") or "")
    if role == "primary" and path.endswith(".rs"):
        if rs_reads >= 8:
            deny(
                f"Context guard: primary agent already read {rs_reads} .rs files sequentially without editing or delegating. "
                "Delegate multi-file exploration to invoke_subagent(TypeName='research') or run_workflow(audit_workflow.py)."
            )
        rs_reads += 1
        save_state()
    allow()

if tool_name in ("edit_file", "write_file"):
    path = str(args.get("Path") or "")
    if role == "primary":
        rs_reads = 0
        save_state()
    if path.endswith(".rs"):
        text = args.get("Content") if tool_name == "write_file" else args.get("Replacement")
        violation = has_rust_comment_or_banned_macro(str(text or ""))
        if violation:
            deny(f"AGENTS.md invariant violation: {violation} is forbidden in Rust source files ({os.path.basename(path)}).")
    allow()

if tool_name == "run_command":
    cmd = str(args.get("CommandLine") or "")
    segments = split_shell_segments(cmd)

    for seg in segments:
        base, cmd_tokens = unwrap_command(seg)
        if not base:
            continue
        if base == "rg":
            deny("ripgrep ('rg') is not installed in this environment. Use 'grep -rn' or 'grep -rnE' instead.")
        if base == "git":
            sub = extract_git_subcommand(cmd_tokens[1:])
            if sub in ("commit", "tag"):
                deny("Direct 'git commit' or 'git tag' by the agent is blocked. Use card_lifecycle.sh commit/tag or wiki_sync.sh commit.")
        if base == "advisor.sh":
            f_count = sum(1 for t in cmd_tokens[1:] if t in ("-f", "--file") or t.startswith("--file="))
            if f_count > 4:
                deny(f"Too many -f attachments ({f_count}) for advisor.sh; attach at most 4 files to prevent 115s print timeouts.")

    overwrite = {}
    if not args.get("IsDaemon") and int(args.get("WaitMsBeforeAsync") or 0) < 10000:
        overwrite["WaitMsBeforeAsync"] = 10000

    if "quality_gate.sh" in cmd and not re.search(r"--(?:fast|full|domain)\b", cmd):
        if "card_lifecycle.sh" not in cmd:
            st = subprocess.run(["git", "status", "--porcelain", "--untracked-files=no"], cwd=root_dir, capture_output=True, text=True)
            lines = [l for l in st.stdout.splitlines() if len(l) >= 2]
            staged = sum(1 for l in lines if l[0] != " ")
            unstaged = sum(1 for l in lines if l[1] != " ")
            if staged == 0 and unstaged > 0:
                new_cmd = re.sub(r"(quality_gate\.sh)\b", r"\1 --fast", cmd, count=1)
                if new_cmd != cmd:
                    overwrite["CommandLine"] = new_cmd

    allow(overwrite if overwrite else None)

allow()
PY
