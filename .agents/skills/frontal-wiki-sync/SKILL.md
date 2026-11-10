---
name: frontal-wiki-sync
description: Audit, synchronize, and commit architectural documentation in the local .wiki repository mirror.
---

# Frontal Wiki Sync Skill

Automates synchronization and invariant auditing of the local GitHub Wiki mirror (`.wiki/`), ensuring strict compliance with repository pre-commit hooks and `architecture.rs` metadata invariants.

## Wiki Mirror Governance Rules

1. **Pre-review audit**: Commits declaring `Card-Status: In Review` or `Card-Status: Done` strictly require a clean `.wiki/` working tree.
2. **Extinct token prevention**: Wiki documentation must not reference extinct tokens or outdated legacy nomenclature.
3. **Version parity**: Active package versions in `Home.md`, `Frontal-Architecture.md`, and `Board-Roadmap.md` must stay aligned with `Cargo.toml`.

## Usage

Execute commands via the bundled runner script from the repository root:

Check working tree status of the `.wiki/` mirror:
```bash
.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh status
```

Verify invariant tokens and uncommitted changes:
```bash
.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh check
```

Stage and commit documentation updates in the `.wiki/` mirror:
```bash
.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh commit "update architecture docs for card #<id>"
```
