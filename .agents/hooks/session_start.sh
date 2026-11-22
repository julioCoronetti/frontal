#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
PAYLOAD="$(cat)"

python3 - "$ROOT_DIR" "$PAYLOAD" <<'PY'
import hashlib
import json
import os
import subprocess
import sys
import time

root_dir = sys.argv[1]
data = json.loads(sys.argv[2] or "{}")
conv_id = data.get("conversationId") or "unknown"

ws_hash = hashlib.md5(root_dir.encode("utf-8")).hexdigest()[:12]
state_dir = f"/tmp/frontal-hooks-{ws_hash}"
os.makedirs(state_dir, exist_ok=True)

now = time.time()
is_subagent = False

subagent_tokens = []
wf_tokens = []
for entry in os.listdir(state_dir):
    p = os.path.join(state_dir, entry)
    if entry.startswith("spawning.subagent."):
        try:
            mtime = os.path.getmtime(p)
            if now - mtime <= 45:
                subagent_tokens.append((mtime, p))
            else:
                os.remove(p)
        except OSError:
            pass
    elif entry.startswith("spawning.wf."):
        try:
            mtime = os.path.getmtime(p)
            if now - mtime <= 180:
                wf_tokens.append((mtime, p))
            else:
                os.remove(p)
        except OSError:
            pass

if subagent_tokens:
    subagent_tokens.sort(key=lambda item: item[0])
    _, token_path = subagent_tokens[0]
    try:
        os.remove(token_path)
    except OSError:
        pass
    is_subagent = True
elif wf_tokens:
    is_subagent = True

state_path = os.path.join(state_dir, f"{conv_id}.json")
role = "subagent" if is_subagent else "primary"
with open(state_path, "w", encoding="utf-8") as f:
    json.dump({"role": role, "rs_reads": 0}, f)

if role == "subagent":
    print(json.dumps({}))
    sys.exit(0)

boot_script = os.path.join(root_dir, ".agents/skills/frontal-context/scripts/bootstrap.sh")
snapshot = ""
if os.path.isfile(boot_script) and os.access(boot_script, os.X_OK):
    proc = subprocess.run([boot_script], cwd=root_dir, capture_output=True, text=True)
    if proc.returncode == 0:
        snapshot = proc.stdout.strip()

contract = (
    "Delegate multi-file .rs exploration (>= 8 reads) to invoke_subagent(research) or "
    "audit_workflow.py; run quality_gate.sh --fast or --domain <name> mid-edit and "
    "--full before commit; commit via card_lifecycle.sh."
)
msg = f"{contract}\n\n{snapshot}" if snapshot else contract

print(json.dumps({
    "injectSteps": [
        {
            "systemMessage": {
                "systemMessage": msg
            }
        }
    ]
}))
PY
