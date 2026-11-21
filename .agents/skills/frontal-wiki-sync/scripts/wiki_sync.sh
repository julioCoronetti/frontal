#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

WIKI_DIR="$ROOT_DIR/.wiki"

usage() {
  echo "Usage: $0 {status|check|commit <summary>}" >&2
  exit 1
}

validate_docs() {
  local current_version
  current_version=$(grep '^version = ' Cargo.toml | cut -d '"' -f 2)

  local extinct_tokens=(
    "tests/ui_polish_gate.rs"
    "< 400 linhas"
    "< 400 lines"
    " testes"
    "Fase 1"
    "Fase 2"
    "Fase 3"
    "Fase 4"
    "Fase 5"
    "Fases 1"
    "Phase Dumps"
  )

  local scan_targets=("$ROOT_DIR/AGENTS.md" "$ROOT_DIR/README.md")
  local md
  for md in "$WIKI_DIR"/*.md; do
    if [[ -f "$md" ]]; then
      scan_targets+=("$md")
    fi
  done

  local token target
  for token in "${extinct_tokens[@]}"; do
    for target in "${scan_targets[@]}"; do
      if [[ -f "$target" ]] && grep -Fq -- "$token" "$target"; then
        echo "Error: Extinct token '$token' found in $target." >&2
        exit 1
      fi
    done
  done

  local doc
  for doc in "$WIKI_DIR/Home.md" "$WIKI_DIR/Frontal-Architecture.md" "$ROOT_DIR/AGENTS.md" "$ROOT_DIR/README.md"; do
    if [[ -f "$doc" ]] && ! grep -Fq "$current_version" "$doc"; then
      echo "Error: Version $current_version not found in $doc." >&2
      exit 1
    fi
  done
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
    uncommitted=$(git -C "$WIKI_DIR" status --porcelain)
    if [[ -n "$uncommitted" ]]; then
      echo "Error: Uncommitted changes detected in .wiki mirror:" >&2
      echo "$uncommitted" >&2
      exit 1
    fi
    validate_docs
    echo "Wiki invariant checks passed for version $(grep '^version = ' Cargo.toml | cut -d '"' -f 2)."
    ;;
  commit)
    summary="${2:-}"
    if [[ -z "$summary" ]]; then
      echo "Error: Commit summary required (e.g. 'update architecture for card #25')." >&2
      exit 1
    fi
    if [[ -z "$(git -C "$WIKI_DIR" status --porcelain)" ]]; then
      echo "No changes to commit in .wiki mirror."
      exit 0
    fi
    validate_docs
    git -C "$WIKI_DIR" add -A
    git -C "$WIKI_DIR" commit -m "docs(wiki): $summary"
    echo "Committed changes in .wiki mirror."
    ;;
  *)
    usage
    ;;
esac
