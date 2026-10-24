---
name: security-review
description: Review authorization, secrets, untrusted inputs and agent tool boundaries at the exact HEAD.
---

# Required security pass

Map entry points, trusted actors, protected assets and side-effect boundaries. Inspect only affected risks and their callers, including:

- Authentication/authorization: current numeric allowlist, action/head/hash binding, stale/edited/replayed decisions and session/run-token scope.
- Untrusted issue/repo/tool/model text: attempts to override instructions, forge evidence, widen paths or invoke extra tools. Prompt-injection pattern detection is not an authorization boundary.
- Secrets/egress: daemon-held credentials, inherited descriptors, logs/artifacts, provider allowlists and local-vs-cloud classification. Never retrieve real secrets to prove they are protected.
- Files/processes: traversal, symlinks/hardlinks, never-sent rules, writable mounts, Git hooks/config, credential helpers, environment stripping, cancellation and Stop All.
- Persistence/side effects: transactional state, idempotency, retries after unknown outcomes, subprocess argument injection and least-privilege CI/dependencies.

Use harmless fixtures when reproduction is permitted; do not run external attacks or access production systems. Return concrete findings and exact-head Security details. passed is true only when the required pass is clean; missing essential verification is explicit, never an inferred pass. No code changes, permission relaxation or owner approval.
