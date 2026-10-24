---
name: report-progress
description: Record only verified, owned task completion and give a scoped typed handoff.
---

# Progress and handoff

Read assigned T-keys and their criteria. Use plantool task done <item> T<n> only for a completed task owned by the dispatched role, after its meaningful checks succeed. Do not tick immutable plan checkboxes, renumber keys or mark another area's task complete.

Capture logs under TMPDIR and publish them with plantool attempt artifact --file <log>. Report actual command, exit/result, environment and current commit; a path on your machine is not an artifact reference.

Use plantool attempt report --file <result.json> for coding-role results. Report the exact context identity, HEAD commit, tasks_done and evidenced checks. For unfinished work, explain what changed, what failed, owner/task, reproduction and safe next action. A local commit/checkpoint never grants publication or approval authority.
