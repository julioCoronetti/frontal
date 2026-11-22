---
name: frontal-context
description: Generates a compact repository, test-domain, and optional Project Board #8 snapshot. Use when refreshing local workspace context, listing .rs file line counts, or inspecting open Board #8 cards.
---

## Steps
1. Capture the local workspace snapshot (version, git/wiki state, hook status, recent commits, `.rs` files `>= 420` lines, and dynamic test counts per domain):
   `.agents/skills/frontal-context/scripts/bootstrap.sh`
2. List all tracked `.rs` file line counts before targeting edits near the 500-line ceiling:
   `.agents/skills/frontal-context/scripts/bootstrap.sh --files`
3. Query open Project Board #8 items (or a single issue) when network access is authorized:
   `.agents/skills/frontal-context/scripts/bootstrap.sh --board [card_id]`
4. Verify each invocation exits `0` and reports `AgentHooks: active`.
