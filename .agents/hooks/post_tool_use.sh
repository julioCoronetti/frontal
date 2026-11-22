#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
PAYLOAD="$(cat)"

python3 - "$ROOT_DIR" "$PAYLOAD" <<'PY'
import hashlib
import json
import os
import re
import sys

root_dir = sys.argv[1]
data = json.loads(sys.argv[2] or "{}")
conv_id = data.get("conversationId") or "unknown"
tool_call = data.get("toolCall") or {}
tool_name = tool_call.get("name") or ""
tool_id = tool_call.get("id") or "step"
args = tool_call.get("args") or {}
err = data.get("error") or ""

ws_hash = hashlib.md5(root_dir.encode("utf-8")).hexdigest()[:12]
state_dir = f"/tmp/frontal-hooks-{ws_hash}"

if tool_name in ("invoke_subagent", "run_workflow") and err:
    safe_id = re.sub(r"[^A-Za-z0-9._-]", "_", str(tool_id))
    prefix = "spawning.subagent" if tool_name == "invoke_subagent" else "spawning.wf"
    lock_path = os.path.join(state_dir, f"{prefix}.{safe_id}")
    if os.path.isfile(lock_path):
        try:
            os.remove(lock_path)
        except OSError:
            pass

if tool_name == "run_command" and not err:
    cmd = str(args.get("CommandLine") or "")
    if "card_lifecycle.sh" in cmd and "commit" in cmd:
        state_path = os.path.join(state_dir, f"{conv_id}.json")
        if os.path.isfile(state_path):
            try:
                with open(state_path, "r", encoding="utf-8") as f:
                    st = json.load(f)
                st["rs_reads"] = 0
                with open(state_path, "w", encoding="utf-8") as f:
                    json.dump(st, f)
            except Exception:
                pass

print("{}")
PY
