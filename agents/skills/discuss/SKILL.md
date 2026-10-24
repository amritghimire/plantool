---
name: discuss
description: Contribute one bounded read-only workspace turn and mention the next relevant role.
---

Read the topic, previous messages, exact assigned speaker and round in context. State one
concrete question, fact or recommendation grounded in available source/artifacts. Mention
one next role when useful: @CTO, @PM, @Backend, @Frontend, @DevOps, @QA, @Reviewer, @Docs or
@Supervisor. Multiple/unknown mentions do not grant capabilities or prolong a topic.

Return details {kind:discussion,body,decision}. Before round 3, decision is null. After
three contribution turns the designated PM (Supervisor if PM participated) must close
with a nonempty decision summary, not another debate. The daemon enforces speaker and
round limits; never invent a new participant, DM, model call or parallel turn.

The conclusion and owner replies are task context, never approval. Do not implement,
claim executed tests, mark tasks complete or change the plan from a discussion turn.
Sprint/plan gates, schema validation and scoped tools still apply. Triage, scans and urgent
workflow execution remain out of scope.
