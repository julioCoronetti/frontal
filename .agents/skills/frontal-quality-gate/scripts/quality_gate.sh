#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

MODE="full"
DOMAIN=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --fast)
      MODE="fast"
      shift
      ;;
    --full)
      MODE="full"
      shift
      ;;
    --domain)
      if [[ $# -lt 2 ]]; then
        echo "Error: --domain requires a domain name (e.g. architecture, engine_and_session)." >&2
        exit 1
      fi
      MODE="domain"
      DOMAIN="${2#domains::}"
      shift 2
      ;;
    --domain=*)
      MODE="domain"
      DOMAIN="${1#--domain=}"
      DOMAIN="${DOMAIN#domains::}"
      shift
      ;;
    -h|--help)
      echo "Usage: $0 [--fast | --domain <name> | --full]"
      exit 0
      ;;
    *)
      echo "Error: Unknown argument '$1'." >&2
      exit 1
      ;;
  esac
done

if [[ "$(git config --local core.hooksPath || true)" == ".githooks" ]]; then
  git config --local --unset core.hooksPath
fi

errors=0

if [[ ! -f "$ROOT_DIR/.agents/hooks.json" ]]; then
  echo "Error: .agents/hooks.json is missing." >&2
  errors=$((errors + 1))
elif ! python3 -c 'import json, sys; json.load(open(sys.argv[1], encoding="utf-8"))' "$ROOT_DIR/.agents/hooks.json"; then
  echo "Error: .agents/hooks.json contains invalid JSON." >&2
  errors=$((errors + 1))
fi

for h in session_start.sh pre_tool_use.sh post_tool_use.sh stop_cleanup.sh; do
  hpath="$ROOT_DIR/.agents/hooks/$h"
  if [[ ! -x "$hpath" ]]; then
    echo "Error: $hpath must exist and be executable." >&2
    errors=$((errors + 1))
  elif ! bash -n "$hpath"; then
    errors=$((errors + 1))
  fi
done

wf_script="$ROOT_DIR/.agents/skills/frontal-quality-gate/scripts/audit_workflow.py"
if [[ ! -f "$wf_script" ]] || ! python3 -c '
import sys
with open(sys.argv[1], encoding="utf-8") as f:
    wrapped = "async def _wf():\n" + "".join("    " + line for line in f)
compile(wrapped, sys.argv[1], "exec")
' "$wf_script"; then
  echo "Error: $wf_script is missing or failed syntax check." >&2
  errors=$((errors + 1))
fi

if [[ "$MODE" == "full" ]]; then
  staged_count=$(git diff --cached --name-only | wc -l)
  unstaged_count=$(git diff --name-only | wc -l)

  if [[ "${ALLOW_PARTIAL_STAGING:-0}" != "1" ]] && [[ "$staged_count" -gt 0 ]] && [[ "$unstaged_count" -gt 0 ]]; then
    echo "Error: Partial staging detected ($staged_count staged, $unstaged_count unstaged tracked file(s)). Stage all slice files or stash unstaged changes (set ALLOW_PARTIAL_STAGING=1 to override)." >&2
    errors=$((errors + 1))
  fi

  if [[ "${ATOMIC_OVERRIDE:-0}" != "1" ]]; then
    if [[ "$staged_count" -gt 0 ]]; then
      if [[ "$staged_count" -gt 5 ]]; then
        echo "Error: Atomic commit threshold exceeded ($staged_count staged files, max 5 allowed; set ATOMIC_OVERRIDE=1)." >&2
        errors=$((errors + 1))
      fi
    else
      tracked_dirty=$(git status --porcelain --untracked-files=no | wc -l)
      if [[ "$tracked_dirty" -gt 5 ]]; then
        echo "Error: Atomic commit threshold exceeded ($tracked_dirty modified tracked files, max 5 allowed). Stage <= 5 files per slice or set ATOMIC_OVERRIDE=1." >&2
        errors=$((errors + 1))
      fi
    fi
  fi
fi

if ! find src tests -name '*.rs' | sort | xargs awk '
  FNR == 1 {
    if (prev > 500) {
      printf "Error: %s exceeds 500-line ceiling (%d lines).\n", pfile, prev > "/dev/stderr"
      fail = 1
    }
    pfile = FILENAME
  }
  { prev = FNR }
  END {
    if (prev > 500) {
      printf "Error: %s exceeds 500-line ceiling (%d lines).\n", pfile, prev > "/dev/stderr"
      fail = 1
    }
    exit fail
  }
'; then
  errors=$((errors + 1))
fi

if ! cargo fmt --check; then
  errors=$((errors + 1))
fi

if [[ "$MODE" == "full" ]] && [[ -d "$ROOT_DIR/.wiki/.git" ]]; then
  if ! "$ROOT_DIR/.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh" check-invariants; then
    errors=$((errors + 1))
  fi
fi

if [[ "$errors" -gt 0 ]]; then
  echo "Quality gate aborted during static pre-checks ($errors failure(s)). Fix before compiling." >&2
  exit 1
fi

case "$MODE" in
  fast)
    cargo check --all-targets --quiet
    echo "Quality gate (--fast) passed: hooks, 500-line ceiling, fmt, and cargo check verified."
    ;;
  domain)
    cargo clippy --all-targets -- -D warnings
    cargo test --test suite "domains::${DOMAIN}" --quiet
    echo "Quality gate (--domain ${DOMAIN}) passed: hooks, 500-line ceiling, fmt, clippy, and domain tests verified."
    ;;
  full)
    cargo clippy --all-targets -- -D warnings
    cargo test --quiet
    echo "Quality gate (--full) passed: hooks, atomic limit, 500-line ceiling, fmt, wiki invariants, clippy, and full test suite verified."
    ;;
esac
