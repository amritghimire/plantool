# Continuous planning and recovery

Open a project in Engineering OS to see its sprint board. **Plan this week** starts
planning in the project's timezone even before the usual planning time. Backlog
issues are prepared, then PM selects from their completed proposals. **Replan this
week** requests another selection without interrupting approved or started work.
Repeated clicks are combined while planning is queued or running. Old sprints stay
available in the selector; their issues show their current progress. Proposed
changes still need the current GitHub approval. The board does not grant approval.

Temporary provider failures move to another permitted model. When all usable
models are cooling down or waiting for a daily/hourly allowance to reset, the item
waits and shows its retry time. The wait survives daemon restarts. Catalog checks
are retried at most every five minutes; they do not send model prompts. Invalid
credentials, permissions, policy exclusions and unknown provider-budget resets
need the action shown in Settings. Repeated invalid model output stops after three
failures rather than spending requests indefinitely. Coding attempts with unknown
changes retain their existing recovery checks.

Issue dependencies hold implementation in the queue until their prerequisite work finishes; research and planning can proceed in the meantime.

Supervisor can wait for a repository PR to merge or a literal file to appear.
GitHub synchronization checks the prerequisite at most once a minute; files are
accepted only after a successful default-branch fetch. The team reviews the new
evidence automatically. Missing, inaccessible, protected or stale evidence is
never treated as success. Scope corrections are proposals in a new plan revision,
not silent edits to approved work. Owner questions are reserved for unresolved
choices. Repeated identical discussion messages end the discussion and move to
preparing a proposal; this is not an approval or a claim of agreement.

On restart, an interrupted read-only attempt without an active gate or unresolved
external action can continue in a new attempt. Completed results and their receipts
are reused. Unknown coding actions and worker checkpoint failures keep their
recovery fence. Stop All still stops automatic work; a retry never resumes global
execution itself.

## Repeatable checks

Run from the PlanTool source checkout:

```sh
scripts/check-continuity.sh /absolute/path/to/plantool-playground
```

The nightly **Continuous workflow checks** action checks the pinned playground
baseline and the complete daemon, browser and Python worker regression suites.
Normal pull-request CI also runs these suites. Tests use isolated temporary homes,
mock providers and GitHub responses. They test fallback, durable waits, current
approval rules, replies, closure, restart behavior and one global model lease.
They do not prove real model compatibility or native macOS operation.

For an installed macOS run, use the playground's `docs/system-test.md` first, then:

1. Plan the current week from the board. Confirm issues appear only after their
   plans are prepared; approve the current proposals on GitHub.
2. Follow an approved issue through its PR and check current-head CI and approval.
3. During a disposable read-only task, temporarily make its model unavailable.
   Check fallback or a timed wait, restart the daemon, restore the provider and
   verify progress resumes without a duplicate decision or publication.
4. Use a task with a missing prerequisite PR. Merge that prerequisite, then verify
   planning continues after synchronization without an owner reply.
5. Answer a genuine scope question in its item thread; confirm it queues planning
   but does not approve implementation.
6. Queue several issues and verify only one model owns the execution slot. Close
   one issue on GitHub and confirm its pending work stops while history remains.

Record the issue/PR links, plan version, head commit and selected model. Keep live
native/provider results separate from the passing simulated regression checks.
Do not erase state or weaken repository checks to force a scenario through.
