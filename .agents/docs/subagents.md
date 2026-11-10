# Subagent Guidelines and Patterns for Frontal

Governance and delegation standards for subagents in Frontal development, aligned with empirical findings from SWE-bench, OpenHands, and Argon Advisor directives.

## 1. Context Asymmetry Principle
Empirical benchmarks demonstrate that LLM subagents introduce a 2x to 3x token overhead and latency penalty due to context re-initialization.
Delegation to subagents is justified only under **context asymmetry**: tasks with heavy reading or noisy intermediate exploration where transient steps must be discarded, returning only the final structured synthesis to the primary agent.

---

## 2. Recommended Profiles and Use Cases

### `research` Subagent (Isolated Read and Investigation)
- **When to use**:
  - Deep auditing or flow tracing across 10 or more `.rs` files (e.g., `EngineEvent` lifecycle, `TabState` transitions, or `RenderCache` invalidation).
  - Sweeping large transcript logs or technical documentation.
- **Benefits**:
  - All raw reading stays inside the isolated sandbox.
  - The primary agent receives only structured conclusions, preserving context window hygiene and preventing context rot.
- **Constraints**:
  - Read-only sandbox without external network access.

### `self` Subagent (Isolated Diagnostic Execution)
- **When to use**:
  - Running speculative troubleshooting experiments or generating transient diagnostic traces before converging on a solution.
- **Benefits**:
  - Discards noisy failed attempts, keeping the primary agent context pristine.

---

## 3. Forbidden Anti-Patterns

1. **Subagents for Deterministic Mechanical Tasks**:
   - Never invoke LLM subagents for routines such as:
     - Running the Cargo triad (`fmt`, `clippy`, `test`).
     - Bumping SemVer in `Cargo.toml`.
     - Creating annotated Git tags.
     - Updating status on GitHub Project Board #8.
     - Synchronizing the `.wiki/` mirror.
   - **Rationale**: These tasks are fully deterministic and must execute through **local skill scripts** (`.agents/skills/`) with zero token overhead and zero hallucination risk.

2. **Concurrent Mutations in the Same Workspace**:
   - Subagents must not edit files or attempt concurrent commits in the shared working tree, preventing lock collisions (`.git/index.lock` and `target/`).

3. **Unauthorized Network Operations**:
   - Subagents must not perform external network calls without prior explicit authorization from the Senhor.
