#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

WIKI_DIR="$ROOT_DIR/.wiki"

usage() {
  echo "Usage: $0 {status|check|commit <summary>}" >&2
  exit 1
}

cmd="${1:-}"

if [[ ! -d "$WIKI_DIR/.git" ]]; then
  echo "Error: .wiki repository mirror not found at $WIKI_DIR." >&2
  exit 1
fi

case "$cmd" in
  status)
    git -C "$WIKI_DIR" status -s
    ;;
  check)
    current_version=$(grep '^version = ' Cargo.toml | cut -d '"' -f 2)
    uncommitted=$(git -C "$WIKI_DIR" status --porcelain)
    if [[ -n "$uncommitted" ]]; then
      echo "Error: Uncommitted changes detected in .wiki mirror:" >&2
      echo "$uncommitted" >&2
      exit 1
    fi

    extinct_tokens=("tests/ui_polish_gate.rs" "< 400 linhas" "Fase 1" "Fase 2" "Fase 3" "Fase 4" "Fase 5" "Fases 1" "Phase Dumps")
    for token in "${extinct_tokens[@]}"; do
      if grep -Fq -- "$token" "$WIKI_DIR"/*.md 2>/dev/null; then
        echo "Error: Extinct token '$token' found in .wiki markdown files." >&2
        exit 1
      fi
    done

    for doc in "$WIKI_DIR/Home.md" "$WIKI_DIR/Frontal-Architecture.md" "$ROOT_DIR/AGENTS.md"; do
      if [[ -f "$doc" ]] && ! grep -Fq "$current_version" "$doc"; then
        echo "Error: Version $current_version not found in $doc." >&2
        exit 1
      fi
    done

    echo "Wiki invariant checks passed for version $current_version."
    ;;
  commit)
    summary="${2:-}"
    if [[ -z "$summary" ]]; then
      echo "Error: Commit summary required (e.g. 'update architecture for card #25')." >&2
      exit 1
    fi
    git -C "$WIKI_DIR" add -A
    git -C "$WIKI_DIR" commit -m "docs(wiki): $summary"
    echo "Committed changes in .wiki mirror."
    ;;
  *)
    usage
    ;;
esac
