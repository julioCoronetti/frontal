---
name: frontal-card-lifecycle
description: Manage card closure routines, SemVer bumping rules, annotated release tagging, and GitHub Project Board #8 transitions.
---

# Frontal Card Lifecycle Skill

This skill standardizes the finalization workflow for cards on GitHub Project Board #8 in accordance with `AGENTS.md` and repository governance.

## SemVer Governance Rules

1. **In Progress commits**: Bumping `version` in `Cargo.toml` is strictly forbidden during active implementation.
2. **Done commit only**: Version increments occur exclusively in the final closing commit (`Card-Status: Done`) authorized explicitly by the Senhor:
   - **Patch (`0.9.x`)**: Bug fixes (`fix(...)`) or pure refactorings (`refactor(...)`).
   - **Minor (`0.x.0`)**: Closing structural feature cards from the Project Board.
3. **Annotated Tag**: Every closing commit must be immediately accompanied by an annotated Git tag (`git tag -a vX.Y.Z -m "Release vX.Y.Z"`).
4. **Project Board Sync**: Update card state to `Done` on GitHub Project Board #8 using canonical `gh project item-edit` parameters.

## Usage

Run the bundled lifecycle runner from within the repository root:

```bash
# Check active version and status
.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh check

# Bump patch version in Cargo.toml and synchronize Cargo.lock
.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh bump-patch

# Bump minor version in Cargo.toml and synchronize Cargo.lock
.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh bump-minor

# Create annotated git release tag
.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh tag <version>

# Transition item to Done on GitHub Project Board #8
.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh close-board <ITEM_ID>
```
