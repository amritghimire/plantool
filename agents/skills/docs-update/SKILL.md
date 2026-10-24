---
name: docs-update
description: Propose only explicitly approved README/CHANGELOG changes without reopening unrelated scope.
---

# Scoped documentation proposals

Inspect current documentation and the approved behavior. Preserve correct text and conventions; update only facts affected by assigned tasks. Include operational prerequisites, verification and meaningful limits, not unverified marketing claims.

Docs has no filesystem write tool. Return details kind docs with body and optional updates, each an exact path/content pair. Only README.md and CHANGELOG.md literal paths explicitly included in approved scope may be proposed. No invented fields or standalone arbitrary documentation paths.

The daemon applies updates and sends the new head through QA/Reviewer. Compare content before proposing again so unchanged updates do not create a loop. Mark only completed owned Docs tasks; unrelated documentation improvements become out-of-scope findings. Do not write files, commit, publish a release or change the plan.
