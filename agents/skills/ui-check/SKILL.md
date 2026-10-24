---
name: ui-check
description: Verify affected UI states, contracts, accessibility and observable behavior with explicit limits.
---

# UI behavior checklist

Trace the affected user action through state and the real API contract. Exercise relevant loading, empty, success, validation, error, permission-denied and retry states. Avoid replacing established repo UI patterns with a new framework.

Check accessible names, labels, keyboard operation, focus on dialogs/errors, disabled/pending actions and feedback that does not rely only on color. Test behavior through the project's component/browser tools; static markup and mocked responses do not prove native passkey or provider behavior.

When a browser is available, inspect the changed flow at representative sizes. Record what was observed and what could not run. A screenshot alone does not prove an action succeeded. Store useful evidence; report contract defects by owning task instead of building around a guessed response.
