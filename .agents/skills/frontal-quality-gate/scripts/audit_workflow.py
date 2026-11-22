phase("Audit Scope")
targets = args.get("targets") or [
    "src/core",
    "src/infra",
    "src/ui/app",
    "src/ui",
]
objective = args.get("objective") or "Trace architecture invariants, error handling, and test coverage gaps."

phase("Parallel Research")
findings = await parallel(
    targets,
    lambda t: agent(
        (
            f"Inspect {t} in read-only mode for: {objective}\n"
            "Return concise findings citing exact file paths and line ranges only."
        ),
        schema={
            "type": "object",
            "properties": {
                "target": {"type": "string"},
                "summary": {"type": "string"},
                "files": {"type": "array", "items": {"type": "string"}},
                "risks": {"type": "array", "items": {"type": "string"}},
            },
            "required": ["target", "summary", "files", "risks"],
        },
        role=f"Audit {t}",
        type_name="research",
        workspace="inherit",
    ),
)

for item in findings:
    log(f"[{item['target']}] {item['summary']}")
    for r in item["risks"]:
        log(f"  - RISK: {r}")

findings
