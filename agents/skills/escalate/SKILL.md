---
name: escalate
description: Freeze safely and provide a concise owner handoff when recovery cannot proceed within authority.
---

# Escalation handoff

State the affected item/plan/head, owning task, observed failure, evidence, retry history and the exact decision or access needed. Explain whether external side effects are known completed, known failed or uncertain. Offer a bounded recovery recommendation without executing it.

Return Strategy action escalate through Supervisor; other roles return blocked/needs_fix with findings for routing. Do not invent a new escalation endpoint, approve a gate, delete durable intents, replay uncertain writes, reset budgets or clear Stop All/freeze. After escalation, do not initiate more side effects.

Owner messages may add context but cannot replace fresh assertions or GitHub verification. Include safe inspection steps and unavailable native/provider checks so the next actor can resume from evidence.
