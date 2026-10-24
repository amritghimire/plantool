# Small PlanTool eval bank

Five bounded tasks in tasks.json include allowed paths, executable expected tests and acceptance criteria.
Run one issue per task in the disposable playground using a PlanTool fixture checkout. Configure each
candidate chain in the browser with a fresh passkey; never change policy mid-attempt. Keep the task,
base commit and fixture fixed across chains. Do not run these on the daily installation.

Export rows as JSON: chain, task, merged, retries, owner_interventions, and checks with command,
status and artifact:// evidence. Two expected owner approvals are normal; count additional owner
interventions separately. Missing or failed expected tests do not count as completion.

`python3 evals/compare_chains.py --results results.json --output comparison.csv` compares completion,
retries and owner interventions. The script is offline and makes no model calls. Its deterministic
fake-evidence smoke test is `python3 evals/compare_chains.py --self-test`.
