#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
PAYLOAD="$(cat)"

python3 - "$ROOT_DIR" "$PAYLOAD" <<'PY'
import hashlib
import json
import os
import sys
import time

root_dir = sys.argv[1]
data = json.loads(sys.argv[2] or "{}")
conv_id = data.get("conversationId") or "unknown"
fully_idle = bool(data.get("fullyIdle", True))

ws_hash = hashlib.md5(root_dir.encode("utf-8")).hexdigest()[:12]
state_dir = f"/tmp/frontal-hooks-{ws_hash}"

if os.path.isdir(state_dir):
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

    now = time.time()
    for entry in os.listdir(state_dir):
        if entry.startswith("spawning."):
            p = os.path.join(state_dir, entry)
            ttl = 45 if entry.startswith("spawning.subagent.") else 180
            try:
                with open(p, "r", encoding="utf-8") as f:
                    owner = f.read().strip()
                expired = (now - os.path.getmtime(p)) > ttl
                if expired or (fully_idle and owner == conv_id):
                    os.remove(p)
            except OSError:
                pass

print("{}")
PY
