---
name: supervise
description: Choose one bounded recovery action from actual failures, task ownership and retry budgets.
---

# Recovery decision workflow

Read the current step, failed task/owner, retry counts, verified logs and prior attempted remedies. Diagnose whether the problem is implementation, missing requirements, unavailable infrastructure or authority. Do not trust a fluent success claim over failed evidence.

Choose one Strategy action:

- retry: a concrete in-scope repair by backend/frontend/devops/qa/docs with owner/task_key and reason.
- replan: PM must revise scope/criteria/tasks; the new revision requires fresh owner approval.
- escalate: severe, unresolved or unauthorized work freezes side effects for owner attention.

Respect the existing three task retries and two Supervisor rounds. Never reset counters, select an arbitrary new role, switch model mid-run, clear a fence or pretend an uncertain external action failed safely. Summarize observations, tried remedies, next action and verification needed. The daemon decides transitions; next is advisory.
