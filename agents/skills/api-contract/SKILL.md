---
name: api-contract
description: Implement approved shared interfaces before consumers and verify compatibility and generated drift.
---

# Contract-first implementation

1. Inspect producers, consumers, existing types/schema and error conventions. Identify the approved task and its file boundary.
2. Specify request/response fields, validation, errors and compatibility before consumer changes. Include authorization and idempotency semantics where relevant.
3. Update the authoritative source, then use the repository generator if one exists. Never hand-edit generated output as a replacement for source changes.
4. Exercise normal, invalid, unauthorized and stale/replayed inputs where affected. Verify generated drift and consumer integration with real commands.

Report exact source/consumer paths and artifact evidence. If a necessary interface change exceeds approval, return needs_fix/blocked for replan. Do not invent an endpoint or change a contract solely to make a consumer test pass.
