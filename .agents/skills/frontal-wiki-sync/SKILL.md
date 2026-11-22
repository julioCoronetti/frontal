---
name: frontal-wiki-sync
description: Audits invariant tokens, verifies Cargo.toml version parity, and commits documentation updates in the local .wiki/ mirror. Use when editing .wiki/ pages or before committing Card-Status: In Review or Done.
---

## Mirror Governance
- Keep `.wiki/` clean before `Card-Status: In Review` or `Done` commits.
- Keep package versions in `Home.md`, `Frontal-Architecture.md`, `AGENTS.md`, and `README.md` identical to `Cargo.toml`.

## Steps
1. Inspect `.wiki/` working tree status:
   `.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh status`
2. Stage, validate, and commit `.wiki/` updates (exits `0` cleanly when no-op):
   `.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh commit "update architecture docs for card #<id>"`
3. Verify clean mirror state, invariant tokens, and version parity (must exit `0`):
   `.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh check`
