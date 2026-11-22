#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

MODE="default"
CARD_ID=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --files)
      MODE="files"
      shift
      ;;
    --board)
      MODE="board"
      if [[ $# -gt 1 && "${2:-}" != --* ]]; then
        CARD_ID="$2"
        shift 2
      else
        shift
      fi
      ;;
    -h|--help)
      echo "Usage: $0 [--files] [--board [card_id]]"
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

if [[ "$MODE" == "files" ]]; then
  find src tests -name '*.rs' | sort | xargs awk '
    FNR == 1 {
      if (prev > 0) printf "%4d %s\n", prev, pfile
      pfile = FILENAME
    }
    { prev = FNR }
    END {
      if (prev > 0) printf "%4d %s\n", prev, pfile
    }
  '
  exit 0
fi

if [[ "$MODE" == "board" ]]; then
  echo "=== PROJECT BOARD #8 (OPEN ITEMS) ==="
  gh project item-list 8 --owner julioCoronetti --limit 100 --format json \
    --jq '.items[] | select(.status != "Done") | "\(.id)\t\(.status)\t#\(.content.number // "-")\t\(.title)"'
  if [[ -n "$CARD_ID" ]]; then
    echo ""
    echo "=== ISSUE #$CARD_ID ==="
    gh issue view "$CARD_ID" --json number,title,state,labels,body \
      --jq '"#\(.number) [\(.state)] \(.title)\nLabels: \([.labels[].name] | join(", "))\n\n\(.body)"'
  fi
  exit 0
fi

VERSION=$(grep '^version = ' Cargo.toml | cut -d '"' -f 2)
BRANCH=$(git rev-parse --abbrev-ref HEAD)
HOOK_STATE="missing"
if [[ -f "$ROOT_DIR/.agents/hooks.json" ]] \
  && [[ -x "$ROOT_DIR/.agents/hooks/session_start.sh" ]] \
  && [[ -x "$ROOT_DIR/.agents/hooks/pre_tool_use.sh" ]] \
  && [[ -x "$ROOT_DIR/.agents/hooks/post_tool_use.sh" ]] \
  && [[ -x "$ROOT_DIR/.agents/hooks/stop_cleanup.sh" ]]; then
  HOOK_STATE="active"
fi
WIKI_STATE="missing"
if [[ -d ".wiki/.git" ]]; then
  WIKI_DIRTY=$(git -C .wiki status --porcelain)
  if [[ -z "$WIKI_DIRTY" ]]; then
    WIKI_STATE="clean"
  else
    WIKI_STATE="dirty ($(printf '%s\n' "$WIKI_DIRTY" | wc -l) files)"
  fi
fi

echo "=== FRONTAL SNAPSHOT ==="
echo "Version: $VERSION | Branch: $BRANCH | AgentHooks: $HOOK_STATE | .wiki: $WIKI_STATE"

echo ""
echo "=== GIT STATUS (TRACKED) ==="
TRACKED_STATUS=$(git status -s --untracked-files=no)
if [[ -z "$TRACKED_STATUS" ]]; then
  echo "(clean)"
else
  printf '%s\n' "$TRACKED_STATUS"
fi

echo ""
echo "=== RECENT COMMITS ==="
git log -n 5 --oneline

echo ""
echo "=== NEAR-CEILING .RS FILES (>= 420 LINES) ==="
NEAR_CEILING=$(find src tests -name '*.rs' | sort | xargs awk '
  FNR == 1 {
    if (prev >= 420) printf "%4d %s\n", prev, pfile
    pfile = FILENAME
  }
  { prev = FNR }
  END {
    if (prev >= 420) printf "%4d %s\n", prev, pfile
  }
')
if [[ -z "$NEAR_CEILING" ]]; then
  echo "(none)"
else
  printf '%s\n' "$NEAR_CEILING"
fi

echo ""
echo "=== DYNAMIC TEST COUNTS BY DOMAIN ==="
total=0
for entry in tests/domains/*; do
  name=$(basename "$entry")
  if [[ "$name" == "mod.rs" ]]; then
    continue
  fi
  if [[ -d "$entry" ]]; then
    cnt=$(find "$entry" -name '*.rs' ! -name 'mod.rs' -exec grep -hE '^\s*#\[(tokio::)?test\]' {} + | wc -l)
    key="tests/domains/$name/"
  else
    cnt=$(grep -cE '^\s*#\[(tokio::)?test\]' "$entry" || true)
    key="tests/domains/$name"
  fi
  printf "%-36s %4d\n" "$key" "$cnt"
  total=$((total + cnt))
done
printf "%-36s %4d\n" "TOTAL" "$total"
