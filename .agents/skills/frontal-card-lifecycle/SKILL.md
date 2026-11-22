---
name: frontal-card-lifecycle
description: Validates commit messages and trailers, bumps SemVer versions, creates annotated release tags, and transitions cards on GitHub Project Board #8. Use before committing slices, marking cards In Review, or closing a release.
---

## Card Workflow
1. Implement in test-driven slices, keeping `Cargo.toml` version unchanged during `Card-Status: In Progress` and committing via `card_lifecycle.sh commit`.
2. Before committing `Card-Status: In Review`, verify `.wiki/` is clean via `.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh check` and attach a valid advisor mode in the commit trailer (`Reviewed-By: Argon-Advisor <mode:none>` is rejected for `In Review`).
3. After explicit operator authorization for `Card-Status: Done`, bump SemVer (`bump-patch` for `fix`/`refactor`; `bump-minor` for structural features), create the annotated `vX.Y.Z` tag, and close the Board #8 item, keeping CLI (`-V`) and TUI (`/help`) versions identical.

## Commit Format
- Allowed types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`.
- Allowed scopes: `config`, `protocol`, `transport`, `engine`, `session`, `cli`, `ui/app`, `ui/cache`, `ui/editor`, `ui/markdown`, `ui/modal`, `ui/tab`, `ui/viewport`, `tests`, `agents`, `wiki`.
- Structure every card commit message as:
```text
<type>(<scope>): <imperative summary in english, max 72 chars>

- Why: <technical justification or root cause>
- What: <concise summary of structural or behavioral changes>
- Invariants: <preserved invariants: L1/L2 cache, O(1) TabState Deref, POSIX pgid, etc.>

Refs: #<card_or_issue>
Card-Status: <In Progress | In Review | Done>
Reviewed-By: Argon-Advisor <mode:none|review|design|troubleshoot|deep-dive|refine>
```

## Steps
1. Inspect active version and working tree status:
   `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh check`
2. Stage slice files (`<= 5` unless `ATOMIC_OVERRIDE=1`), write `.git/FRONTAL_COMMIT_EDITMSG` (or pass structured flags), and commit:
   `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh commit`
3. Validate a commit message file or `HEAD` directly (must exit `0`):
   `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh validate-commit <file|--head>`
4. On an authorized clean-tree `Done` release, bump version, tag, and transition Board #8:
   - `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh bump-patch` (or `bump-minor`)
   - `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh tag <version>`
   - `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh close-board <ITEM_ID>`
