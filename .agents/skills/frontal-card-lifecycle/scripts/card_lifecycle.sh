#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

PROJECT_ID="PVT_kwHOC8mDy84Blf4U"
STATUS_FIELD_ID="PVTSSF_lAHOC8mDy84Blf4UzhkMioY"
DONE_OPTION_ID="98236657"
DEFAULT_COMMIT_MSG_FILE="$ROOT_DIR/.git/FRONTAL_COMMIT_EDITMSG"

usage() {
  echo "Usage: $0 {check|bump-patch|bump-minor|tag <version>|close-board <item_id>|validate-commit <file|--head>|commit [--file <path> | --header ... --why ... --what ... --invariants ... --refs ... --status ... --advisor ...]}" >&2
  exit 1
}

replace_literal() {
  local file="$1"
  local old="$2"
  local new="$3"
  if [[ -f "$file" ]]; then
    python3 -c '
import sys
path, old, new = sys.argv[1], sys.argv[2], sys.argv[3]
with open(path, "r", encoding="utf-8") as f:
    data = f.read()
updated = data.replace(old, new)
if updated != data:
    with open(path, "w", encoding="utf-8") as f:
        f.write(updated)
' "$file" "$old" "$new"
  fi
}

apply_version_bump() {
  local current_version="$1"
  local new_version="$2"
  local old_series="$3"
  local new_series="$4"

  if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
    echo "Error: Root repository has uncommitted tracked changes; commit or stash before bumping version." >&2
    exit 1
  fi
  if [[ -d ".wiki/.git" ]] && [[ -n "$(git -C .wiki status --porcelain)" ]]; then
    echo "Error: .wiki mirror has uncommitted changes; commit .wiki before bumping version." >&2
    exit 1
  fi

  sed -i "s/^version = \"$current_version\"/version = \"$new_version\"/" Cargo.toml

  replace_literal "AGENTS.md" "\`$current_version\`" "\`$new_version\`"
  if [[ "$old_series" != "$new_series" ]]; then
    replace_literal "AGENTS.md" "\`$old_series\`" "\`$new_series\`"
  fi

  replace_literal "README.md" "\`$current_version\`" "\`$new_version\`"
  replace_literal "README.md" "(v$current_version)" "(v$new_version)"

  if [[ -d ".wiki" ]]; then
    replace_literal ".wiki/Home.md" "\`$current_version\`" "\`$new_version\`"
    replace_literal ".wiki/Frontal-Architecture.md" "\`$current_version\`" "\`$new_version\`"
  fi

  if [[ "$(grep -Fc "$new_version" AGENTS.md)" -lt 2 ]] || ! grep -Fq "$new_series" AGENTS.md; then
    echo "Error: Version bump verification failed in AGENTS.md." >&2
    exit 1
  fi
  if ! grep -Fq "$new_version" README.md; then
    echo "Error: Version bump verification failed in README.md." >&2
    exit 1
  fi
  if [[ -d ".wiki" ]]; then
    local wdoc
    for wdoc in ".wiki/Home.md" ".wiki/Frontal-Architecture.md"; do
      if [[ -f "$wdoc" ]] && ! grep -Fq "$new_version" "$wdoc"; then
        echo "Error: Version bump verification failed in $wdoc." >&2
        exit 1
      fi
    done
  fi

  cargo check --quiet

  if [[ -d ".wiki/.git" ]]; then
    "$ROOT_DIR/.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh" commit "bump version to v$new_version"
  fi

  echo "Bumped version from $current_version to $new_version across Cargo.toml, Cargo.lock, AGENTS.md, README.md, and .wiki."
}

validate_commit_msg() {
  local target="$1"
  local enforce_staged="${2:-1}"
  local commit_msg

  if [[ "$target" == "--head" ]]; then
    commit_msg=$(git log -1 --pretty=format:"%B")
    enforce_staged=0
  else
    if [[ ! -f "$target" ]]; then
      echo "Error: Commit message file '$target' not found." >&2
      exit 1
    fi
    commit_msg=$(cat "$target")
  fi

  if [[ "$enforce_staged" == "1" ]]; then
    local staged_count
    staged_count=$(git diff --cached --name-only | wc -l)
    if [[ "$staged_count" -eq 0 ]]; then
      echo "Error: No staged files to commit. Stage files for this atomic slice first." >&2
      exit 1
    fi
    if [[ "${ATOMIC_OVERRIDE:-0}" != "1" ]] && [[ "$staged_count" -gt 5 ]]; then
      echo "Error: Atomic commit threshold exceeded ($staged_count staged files, max 5 allowed; set ATOMIC_OVERRIDE=1)." >&2
      exit 1
    fi
  fi

  local first_line
  first_line=$(printf '%s\n' "$commit_msg" | head -n 1)
  local header_regex="^(feat|fix|refactor|test|docs|chore|perf)\((config|protocol|transport|engine|session|cli|ui/app|ui/cache|ui/editor|ui/markdown|ui/modal|ui/tab|ui/viewport|tests|agents|wiki)\): .{1,72}$"
  if ! [[ "$first_line" =~ $header_regex ]]; then
    echo "Error: Invalid commit header format." >&2
    echo "Header: '$first_line'" >&2
    echo "Expected: <type>(<scope>): <summary in english, max 72 chars>" >&2
    exit 1
  fi

  local section
  for section in "^- Why:" "^- What:" "^- Invariants:"; do
    if ! printf '%s\n' "$commit_msg" | grep -q "$section"; then
      echo "Error: Commit message missing mandatory '$section' section." >&2
      exit 1
    fi
  done

  if ! printf '%s\n' "$commit_msg" | grep -qE "^Refs: #[0-9a-zA-Z_-]+"; then
    echo "Error: Commit message missing mandatory 'Refs: #<id>' trailer." >&2
    exit 1
  fi

  if ! printf '%s\n' "$commit_msg" | grep -qE "^Card-Status: (In Progress|In Review|Done)"; then
    echo "Error: Commit message missing mandatory 'Card-Status: <In Progress|In Review|Done>'." >&2
    exit 1
  fi

  if ! printf '%s\n' "$commit_msg" | grep -qE "^Reviewed-By: Argon-Advisor <mode:(none|review|design|troubleshoot|deep-dive|refine)>"; then
    echo "Error: Commit message missing mandatory 'Reviewed-By: Argon-Advisor <mode:...>' trailer." >&2
    exit 1
  fi

  if printf '%s\n' "$commit_msg" | grep -q "^Card-Status: In Review"; then
    if printf '%s\n' "$commit_msg" | grep -q "Reviewed-By: Argon-Advisor <mode:none>"; then
      echo "Error: Cannot mark Card-Status as 'In Review' with 'mode:none'." >&2
      exit 1
    fi
  fi

  if printf '%s\n' "$commit_msg" | grep -qE "^Card-Status: (In Review|Done)"; then
    if [[ -d ".wiki/.git" ]] && [[ -n "$(git -C .wiki status --porcelain 2>/dev/null)" ]]; then
      echo "Error: Uncommitted changes detected in .wiki/ mirror." >&2
      exit 1
    fi
  fi
}

cmd="${1:-}"

case "$cmd" in
  check)
    current_version=$(grep '^version = ' Cargo.toml | cut -d '"' -f 2)
    echo "Active Cargo.toml version: $current_version"
    git status -s
    ;;
  bump-patch)
    current_version=$(grep '^version = ' Cargo.toml | cut -d '"' -f 2)
    IFS='.' read -r major minor patch <<< "$current_version"
    new_version="${major}.${minor}.$((patch + 1))"
    series="${major}.${minor}.x"
    apply_version_bump "$current_version" "$new_version" "$series" "$series"
    ;;
  bump-minor)
    current_version=$(grep '^version = ' Cargo.toml | cut -d '"' -f 2)
    IFS='.' read -r major minor patch <<< "$current_version"
    next_minor=$((minor + 1))
    new_version="${major}.${next_minor}.0"
    apply_version_bump "$current_version" "$new_version" "${major}.${minor}.x" "${major}.${next_minor}.x"
    ;;
  tag)
    version="${2:-}"
    if [[ -z "$version" ]]; then
      echo "Error: Version argument required for tag (e.g. 0.9.1)." >&2
      exit 1
    fi
    version="${version#v}"
    current_version=$(grep '^version = ' Cargo.toml | cut -d '"' -f 2)
    if [[ "$version" != "$current_version" ]]; then
      echo "Error: Tag version $version does not match Cargo.toml version $current_version." >&2
      exit 1
    fi
    if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
      echo "Error: Cannot tag release with uncommitted tracked changes in git working tree." >&2
      exit 1
    fi
    git tag -a "v$version" -m "Release v$version"
    echo "Created annotated git tag v$version"
    ;;
  close-board)
    item_id="${2:-}"
    if [[ -z "$item_id" ]]; then
      echo "Error: item_id required for close-board." >&2
      exit 1
    fi
    gh project item-edit --id "$item_id" --project-id "$PROJECT_ID" --field-id "$STATUS_FIELD_ID" --single-select-option-id "$DONE_OPTION_ID"
    echo "Updated card $item_id to Done on Project Board #8"
    ;;
  validate-commit)
    target="${2:-}"
    if [[ -z "$target" ]]; then
      echo "Error: target file or --head required." >&2
      exit 1
    fi
    validate_commit_msg "$target" 1
    echo "Commit message validation passed."
    ;;
  commit)
    shift
    msg_file="$DEFAULT_COMMIT_MSG_FILE"
    custom_file=""
    header=""
    why=""
    what=""
    invariants=""
    refs=""
    status=""
    advisor=""

    while [[ $# -gt 0 ]]; do
      case "$1" in
        --file) custom_file="${2:-}"; shift 2 ;;
        --header) header="${2:-}"; shift 2 ;;
        --why) why="${2:-}"; shift 2 ;;
        --what) what="${2:-}"; shift 2 ;;
        --invariants) invariants="${2:-}"; shift 2 ;;
        --refs) refs="${2:-}"; shift 2 ;;
        --status) status="${2:-}"; shift 2 ;;
        --advisor) advisor="${2:-}"; shift 2 ;;
        *)
          echo "Error: Unknown option '$1' for commit." >&2
          exit 1
          ;;
      esac
    done

    if [[ -n "$header" || -n "$why" || -n "$what" || -n "$invariants" || -n "$refs" || -n "$status" || -n "$advisor" ]]; then
      if [[ -z "$header" || -z "$why" || -z "$what" || -z "$invariants" || -z "$refs" || -z "$status" || -z "$advisor" ]]; then
        echo "Error: All structured flags (--header, --why, --what, --invariants, --refs, --status, --advisor) are required." >&2
        exit 1
      fi
      if [[ "$refs" != Refs:* ]]; then
        refs="Refs: $refs"
      fi
      printf '%s\n\n- Why: %s\n- What: %s\n- Invariants: %s\n\n%s\nCard-Status: %s\nReviewed-By: Argon-Advisor <mode:%s>\n' \
        "$header" "$why" "$what" "$invariants" "$refs" "$status" "$advisor" > "$DEFAULT_COMMIT_MSG_FILE"
      msg_file="$DEFAULT_COMMIT_MSG_FILE"
    elif [[ -n "$custom_file" ]]; then
      msg_file="$custom_file"
    fi

    validate_commit_msg "$msg_file" 1
    git commit -F "$msg_file"
    if [[ "$msg_file" == "$DEFAULT_COMMIT_MSG_FILE" ]]; then
      rm -f "$DEFAULT_COMMIT_MSG_FILE"
    fi
    ;;
  *)
    usage
    ;;
esac
