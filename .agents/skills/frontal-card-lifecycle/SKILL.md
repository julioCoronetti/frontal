---
name: frontal-card-lifecycle
description: Validates commit messages and trailers, bumps SemVer versions, creates annotated release tags, and transitions cards on GitHub Project Board #8. Use before committing, when closing a card, or when preparing a release.
---

## Card Workflow
1. On card kickoff (`Todo` -> `In Progress`), run `~/.gemini/config/scripts/advisor.sh -m design -f <file>... "<objective and context>"`.
2. Implement in minimal test-driven slices, running `.agents/skills/frontal-quality-gate/scripts/quality_gate.sh` and committing each slice with `Card-Status: In Progress`.
3. Keep `version` in `Cargo.toml` unchanged during `Card-Status: In Progress` commits.
4. On completion of all slices, ensure `.wiki/` is clean via `.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh check` and run adversarial diff review via `~/.gemini/config/scripts/advisor.sh -m review -d -f <file>... "<objective and context>"` before committing with `Card-Status: In Review` (commits with `Reviewed-By: Argon-Advisor <mode:none>` are rejected for `In Review`).
5. After explicit operator authorization, bump SemVer in the closing commit (`Card-Status: Done`), create the annotated tag `vX.Y.Z`, and transition the Project Board #8 item to `Done`:
   - Patch (`0.20.x`): `fix(...)` or pure `refactor(...)`.
   - Minor (`0.x.0`): structural feature cards.
   - Keep CLI (`frontal --version` / `-V`) and TUI (`/help`) versions identical.

## Commit Governance
- Allowed types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`.
- Allowed scopes: `config`, `protocol`, `transport`, `engine`, `session`, `cli`, `ui/app`, `ui/cache`, `ui/editor`, `ui/markdown`, `ui/modal`, `ui/tab`, `ui/viewport`, `tests`, `agents`, `wiki`.
- Format every commit message strictly as:
```text
<type>(<scope>): <imperative summary in english, max 72 chars>

- Why: <technical justification or root cause>
- What: <concise summary of structural or behavioral changes>
- Invariants: <preserved invariants: L1/L2 cache, O(1) TabState Deref, POSIX pgid, etc.>

Refs: #<card_or_issue>
Card-Status: <In Progress | In Review | Done>
Reviewed-By: Argon-Advisor <mode:none|review|design|troubleshoot|deep-dive|refine>
```

## Execution & Verification
1. Inspect active version and repository state:
   `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh check`
2. Bump version only on authorized `Done` release:
   - Patch: `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh bump-patch`
   - Minor: `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh bump-minor`
3. Validate commit message format and trailers before or after committing:
   - File: `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh validate-commit <file>`
   - Head: `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh validate-commit --head`
4. Create annotated release tag after the `Done` commit is clean:
   `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh tag <version>`
5. Transition the card on GitHub Project Board #8:
   `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh close-board <ITEM_ID>`
