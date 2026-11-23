---
name: frontal-quality-gate
description: Runs the Frontal quality gate (--fast, --domain <name>, and --full) and parallel run_workflow codebase audits. Use during edit iteration, before committing slices, and for multi-module audits.
---

## Steps
1. Run the fast check during edit iteration:
   `.agents/skills/frontal-quality-gate/scripts/quality_gate.sh --fast`
2. Run a targeted domain check when iterating on a single test domain:
   `.agents/skills/frontal-quality-gate/scripts/quality_gate.sh --domain <domain_name>`
3. Run the full gate before every commit (set `ATOMIC_OVERRIDE=1` only when an indivisible slice exceeds 5 files; `ALLOW_PARTIAL_STAGING=1` to override partial staging):
   `.agents/skills/frontal-quality-gate/scripts/quality_gate.sh --full`
4. Verify the selected command exits `0`; on failure, fix all reported formatting, clippy, or architecture errors in one batch and re-run.
5. For multi-module codebase audits or cross-domain sweeps, invoke `run_workflow` with `ScriptPath: "<repo-root>/.agents/skills/frontal-quality-gate/scripts/audit_workflow.py"` and pass `{"targets": [...], "objective": "..."}` in `Args`; resolve `<repo-root>` per environment via `git rev-parse --show-toplevel`.
