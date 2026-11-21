---
name: frontal-wiki-sync
description: Audits invariant tokens, verifies Cargo.toml version parity, and commits documentation updates in the local .wiki/ mirror. Use when editing wiki pages or before committing Card-Status: In Review or Done.
---

## Mirror Governance
- Commits declaring `Card-Status: In Review` or `Card-Status: Done` require a clean `.wiki/` working tree.
- Keep package versions in `Home.md`, `Frontal-Architecture.md`, `AGENTS.md`, and `README.md` aligned with `Cargo.toml`.
- Keep `.wiki/*.md`, `AGENTS.md`, and `README.md` free of the 11 extinct legacy tokens checked by `tests/domains/architecture.rs`.

## Execution & Verification
1. Inspect `.wiki/` working tree status:
   `.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh status`
2. Stage, validate, and commit pending `.wiki/` updates (no-op if clean):
   `.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh commit "update architecture docs for card #<id>"`
3. Verify clean mirror state, invariant tokens, and version parity (must exit `0`):
   `.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh check`
