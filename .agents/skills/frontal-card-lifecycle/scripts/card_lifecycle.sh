#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

PROJECT_ID="PVT_kwHOC8mDy84Blf4U"
STATUS_FIELD_ID="PVTSSF_lAHOC8mDy84Blf4UzhkMioY"
DONE_OPTION_ID="98236657"

usage() {
  echo "Usage: $0 {check|bump-patch|bump-minor|tag <version>|close-board <item_id>}" >&2
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
    cargo check --quiet
    echo "Bumped version from $current_version to $new_version in Cargo.toml and Cargo.lock"
    ;;
  bump-minor)
    current_version=$(grep '^version = ' Cargo.toml | cut -d '"' -f 2)
    IFS='.' read -r major minor patch <<< "$current_version"
    next_minor=$((minor + 1))
    new_version="${major}.${next_minor}.0"
    sed -i "s/^version = \"$current_version\"/version = \"$new_version\"/" Cargo.toml
    cargo check --quiet
    echo "Bumped version from $current_version to $new_version in Cargo.toml and Cargo.lock"
    ;;
  tag)
    version="${2:-}"
    if [[ -z "$version" ]]; then
      echo "Error: Version argument required for tag (e.g. 0.9.1)." >&2
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
  *)
    usage
    ;;
esac
