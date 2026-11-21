#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

if [[ -x "$ROOT_DIR/.githooks/commit-msg" ]] && [[ "$(git config core.hooksPath || true)" != ".githooks" ]]; then
  git config core.hooksPath .githooks
fi

errors=0

if [[ ! -x "$ROOT_DIR/.githooks/commit-msg" ]] || [[ "$(git config core.hooksPath || true)" != ".githooks" ]]; then
  echo "Error: .githooks/commit-msg must be executable and core.hooksPath must be .githooks." >&2
  errors=$((errors + 1))
fi

if [[ "${ATOMIC_OVERRIDE:-0}" != "1" ]]; then
  staged_count=$(git diff --cached --name-only | wc -l)
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

if [[ -d "$ROOT_DIR/.wiki/.git" ]]; then
  if ! "$ROOT_DIR/.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh" check; then
    errors=$((errors + 1))
  fi
fi

if [[ "$errors" -gt 0 ]]; then
  echo "Quality gate aborted during static pre-checks ($errors failure(s)). Fix before compiling." >&2
  exit 1
fi

cargo clippy --all-targets -- -D warnings
cargo test --test suite --quiet

echo "Quality gate passed: hooks, atomic limit, 500-line ceiling, fmt, wiki check, clippy, and suite verified."
