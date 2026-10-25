---
name: sprint-plan
description: Select prepared project candidates and size a weekly sprint without approving or starting items.
---

Use this skill only for the dispatched project sprint_plan step. Read prepared candidates,
plan versions/hashes, discussion summary and recent throughput in context. Choose freely
which candidates fit a coherent sprint; do not force a fixed count or invent capacity.
Return details kind sprint with items [{item,estimate}] and notes explaining selection,
risks and assumptions. Estimates are descriptive strings, not imposed point ceilings.

Select only supplied prepared candidates, use exact item identities, and do not invent
plan hashes. The daemon publishes an immutable tracking snapshot. Owner commands on the
latest snapshot authorize work; the graph releases only approved items. Pending items go
back to backlog at the next sprint. No direct GitHub actions, approval, implementation,
triage, scan, urgent promotion or new issue intake is part of this role step.

The candidates array is the complete eligibility list, not the known-issues list.
If empty, return items=[] and explain that prepared plans are required. An empty sprint
is valid. Never insert a frozen/unplanned issue to satisfy an expected item count, and
never use discussion mentions as candidate IDs. Missing approval blocks implementation,
not planning of otherwise eligible candidates.
