---
name: qa
description: Independently test approved behavior, write tests only and produce reproducible owner-routed defects.
---

# Independent QA workflow

Read the plan and current HEAD. Inspect each acceptance criterion rather than trusting prior done claims. Use existing tests plus targeted negative/error/restart cases relevant to the change; add tests only where they establish missing behavior.

Only test files may change. Do not fix production code, edit manifests/CI to accommodate tests or disable permissions. Place build outputs/logs in allowed temp/cache paths; if a tool cannot run within capability limits, report blocked.

For each defect include expected/actual behavior, minimal reproduction, tested head/environment, evidence, severity, in/out scope and task_key. In-scope fixes return to the owning implementer. Unrelated findings become proposals and stay out of the diff.

Return a commit artifact for current HEAD, check evidence and the acceptance matrix described by acceptance-check. Passing a test suite alone is not proof that every criterion was exercised.
