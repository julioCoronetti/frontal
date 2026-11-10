#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

PROJECT_ID="PVT_kwHOC8mDy84Blf4U"
STATUS_FIELD_ID="PVTSSF_lAHOC8mDy84Blf4UzhkMioY"
DONE_OPTION_ID="98236657"

usage() {
  echo "Usage: $0 {check|bump-patch|bump-minor|tag <version>|close-board <item_id>|validate-commit <file|--head>}" >&2
  exit 1
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
    next_patch=$((patch + 1))
    new_version="${major}.${minor}.${next_patch}"
    sed -i "s/^version = \"$current_version\"/version = \"$new_version\"/" Cargo.toml
    sed -i "s/(versão \`$current_version\`)/(versão \`$new_version\`)/g" AGENTS.md
    sed -i "s/version \`$current_version\`/version \`$new_version\`/g" AGENTS.md
    cargo check --quiet
    echo "Bumped version from $current_version to $new_version in Cargo.toml, Cargo.lock, and AGENTS.md"
    ;;
  bump-minor)
    current_version=$(grep '^version = ' Cargo.toml | cut -d '"' -f 2)
    IFS='.' read -r major minor patch <<< "$current_version"
    next_minor=$((minor + 1))
    new_version="${major}.${next_minor}.0"
    sed -i "s/^version = \"$current_version\"/version = \"$new_version\"/" Cargo.toml
    sed -i "s/(versão \`$current_version\`)/(versão \`$new_version\`)/g" AGENTS.md
    sed -i "s/version \`$current_version\`/version \`$new_version\`/g" AGENTS.md
    cargo check --quiet
    echo "Bumped version from $current_version to $new_version in Cargo.toml, Cargo.lock, and AGENTS.md"
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
    if [[ -n "$(git status --porcelain)" ]]; then
      echo "Error: Cannot tag release with uncommitted changes in git working tree." >&2
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

    if [[ "$target" == "--head" ]]; then
      commit_msg=$(git log -1 --pretty=format:"%B")
    else
      if [[ ! -f "$target" ]]; then
        echo "Error: Commit message file '$target' not found." >&2
        exit 1
      fi
      commit_msg=$(cat "$target")
    fi

    first_line=$(echo "$commit_msg" | head -n 1)
    header_regex="^(feat|fix|refactor|test|docs|chore|perf)\((config|protocol|transport|engine|session|cli|ui/app|ui/cache|ui/editor|ui/markdown|ui/modal|ui/tab|ui/viewport|tests|agents|wiki)\): .{1,72}$"
    if ! [[ "$first_line" =~ $header_regex ]]; then
      echo "Error: Invalid commit header format." >&2
      echo "Header: '$first_line'" >&2
      echo "Expected: <type>(<scope>): <summary in english, max 72 chars>" >&2
      exit 1
    fi

    if ! echo "$commit_msg" | grep -q "^- Why:"; then
      echo "Error: Commit message missing mandatory '- Why:' section." >&2
      exit 1
    fi

    if ! echo "$commit_msg" | grep -q "^- What:"; then
      echo "Error: Commit message missing mandatory '- What:' section." >&2
      exit 1
    fi

    if ! echo "$commit_msg" | grep -q "^- Invariants:"; then
      echo "Error: Commit message missing mandatory '- Invariants:' section." >&2
      exit 1
    fi

    if ! echo "$commit_msg" | grep -qE "^Refs: #[0-9a-zA-Z_-]+"; then
      echo "Error: Commit message missing mandatory 'Refs: #<id>' trailer." >&2
      exit 1
    fi

    if ! echo "$commit_msg" | grep -qE "^Card-Status: (In Progress|In Review|Done)"; then
      echo "Error: Commit message missing mandatory 'Card-Status: <In Progress|In Review|Done>'." >&2
      exit 1
    fi

    if ! echo "$commit_msg" | grep -qE "^Reviewed-By: Argon-Advisor <mode:(none|review|design|troubleshoot|deep-dive|refine)>"; then
      echo "Error: Commit message missing mandatory 'Reviewed-By: Argon-Advisor <mode:...>' trailer." >&2
      exit 1
    fi

    if echo "$commit_msg" | grep -q "^Card-Status: In Review"; then
      if echo "$commit_msg" | grep -q "Reviewed-By: Argon-Advisor <mode:none>"; then
        echo "Error: Cannot mark Card-Status as 'In Review' with 'mode:none'." >&2
        exit 1
      fi
    fi

    if echo "$commit_msg" | grep -qE "^Card-Status: (In Review|Done)"; then
      if [[ -d ".wiki/.git" ]] && [[ -n "$(git -C .wiki status --porcelain 2>/dev/null)" ]]; then
        echo "Error: Uncommitted changes detected in .wiki/ mirror." >&2
        exit 1
      fi
    fi

    echo "Commit message validation passed."
    ;;
  *)
    usage
    ;;
esac
