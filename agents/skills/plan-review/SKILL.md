---
name: plan-review
description: Review keyed plans for scope, ownership, dependencies and verifiable acceptance.
---

# Independent plan review

Read the proposed revision and CTO research. For each criterion, identify its T-key, owning area, affected paths and a test/procedure that could prove it. Check sequencing across contracts and consumers, negative cases, migrations/recovery and security consequences.

Reject ambiguous tasks, blanket scope, missing checks or assumptions presented as facts. Distinguish a blocking requirement from a harmless implementation choice. Check stable keys and immutable revision rules; [review] tags do not waive a gate.

Return concrete findings with task_key, scope, severity and an actionable explanation. Use needs_fix for in-scope defects and blocked for missing decisions. A clean plan review may return done; it does not approve the plan. Do not alter code or start later-phase workflows.
