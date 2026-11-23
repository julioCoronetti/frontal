# Subagent Guidelines and Patterns for Frontal

Delegation rules for isolating heavy reads, speculative diagnostics, and parallel multi-module sweeps away from the primary conversation context.

## 1. Context Asymmetry
Delegate tasks that require broad reading or noisy intermediate steps so transient output stays isolated and the primary agent receives only file paths, line ranges, and verified contracts.

## 2. Profiles and Invocation

### `research` Subagent (Read-Only Investigation)
- Use when tracing call sites or auditing across `>= 8` `.rs` files, transcript logs, or documentation.
- Invoke with `invoke_subagent(TypeName="research", Workspace="inherit", ...)`.

### `self` Subagent (Isolated Diagnostics)
- Use for speculative troubleshooting experiments that require command execution without polluting the primary trajectory.
- Invoke with `invoke_subagent(TypeName="self", Workspace="branch" | "share", ...)` to isolate mutations from the primary working tree.

### Parallel Audit Workflow (`run_workflow`)
- Use `.agents/skills/frontal-quality-gate/scripts/audit_workflow.py` when auditing multiple `src/` or `tests/domains/` slices concurrently instead of running sequential `advisor.sh` loops.
- Invoke with `run_workflow(ScriptPath="<repo-root>/.agents/skills/frontal-quality-gate/scripts/audit_workflow.py", Args={"targets": [...], "objective": "..."})`, resolving `<repo-root>` per environment via `git rev-parse --show-toplevel`.

## 3. Boundaries
- Execute deterministic routines (`cargo fmt`/`clippy`/`test`, SemVer bumps, tags, Board #8 updates, `.wiki/` sync) directly through `.agents/skills/` scripts rather than LLM subagents.
- Never run concurrent mutations from subagents inside the primary `inherit` workspace.
- Never execute outbound network requests in subagents without explicit authorization.
