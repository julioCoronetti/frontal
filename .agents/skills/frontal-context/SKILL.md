---
name: frontal-context
description: Generates a compact single-shot repository, test-domain, and optional Project Board #8 snapshot to eliminate exploratory tool-call cascades at session start.
---

## Execution & Verification
1. Capture the default local repository snapshot (version, git/wiki status, recent commits, `.rs` files `>= 420` lines, and dynamic test counts per domain):
   `.agents/skills/frontal-context/scripts/bootstrap.sh`
2. List all tracked/workspace `.rs` file line counts on demand before locating or editing modules:
   `.agents/skills/frontal-context/scripts/bootstrap.sh --files`
3. Query open Project Board #8 items (or a specific issue) in a single compact table (requires authorized network access):
   `.agents/skills/frontal-context/scripts/bootstrap.sh --board [card_id]`
