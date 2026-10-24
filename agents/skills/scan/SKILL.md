---
name: scan
description: Read-only default-branch scan and routing of findings.
---

For scan_devops, inspect supplied default-branch CI and dependency advisory evidence;
report unavailable API access as unavailable, never as clean. For scan_qa, inspect test
coverage gaps, the file inventory and TODO/FIXME matches. Both return
`details: {kind: "scan", summary: "evidence, links, gaps and limitations"}`.
PM scan_pm combines both handoffs; route_finding routes the supplied review finding
immediately. Return `details: {kind: "routes", findings: [{title, body, route, reason}]}`.
Route is proposed (needs owner acceptance), candidate (ready for sprint planning), or
backlog (routine work). Explain each choice. Include actionable evidence and expected
tests. Avoid duplicates using known items. An empty findings list is valid for a scan;
a review finding must be routed, not dropped. No diffs, shell execution or GitHub writes.
Do not mark urgent: request urgency using request_urgent; only an allowlisted owner can
promote it. No route approves a plan or PR. Never claim a test ran from static inventory.
