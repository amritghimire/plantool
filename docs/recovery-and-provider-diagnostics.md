# Recovering a stalled Engineering OS run

## What was observed

The alpha.2 playground issue and project scheduler froze independently:

- PM triage selected the first PM route (Gemma); Codex was not in that chain. An upstream
  exception escaped the worker role endpoint as HTTP 500. The dispatcher reported failure,
  and triage escalated before considering its quota retry flag.
- Sprint planning failed when no available, permitted PM route was within budget.
- Later direct probes on the owner's Mac confirmed Gemma's free upstream pool returned
  429 and Nvidia's Nemotron Super deployment returned 404. Both requests used the Keychain
  OpenRouter key. Catalog membership does not prove executable provider availability.

The historical response body was discarded, so the exact original upstream status cannot
be proved. A mocked 429/404 reproduces the error path without real model calls. The fix
returns safe error classification and upstream status rather than a generic worker 500.

## Short-term diagnosis on the existing alpha

Do not reset database flags, delete checkpoints or create duplicate issues to force retries.
The old alpha has no supported owner recovery control for those escalated workflows.

From this PR's checkout, on the daemon Mac:

```bash
python3 scripts/provider-smoke.py
python3 scripts/provider-smoke.py nvidia/nemotron-3-super-120b-a12b:free
```

The script uses Python's standard library, reads `plantool/openrouter` through macOS
Keychain into memory and passes it in the Authorization header. It never prints the key.
It sends two tiny requests (basic chat and tools plus structured JSON) to an explicit
`:free` ID. It does not run or change a PlanTool graph. Provider error metadata may include
account identifiers; redact these before sharing publicly. A 429 is upstream quota or
rate limiting; a provider 404 can mean its backing deployment is unavailable. Neither is
fixed by pulling local Ollama models or recreating the OpenRouter Keychain item.

As a separate local service check, `ollama run devstral-small-2 'Reply with OK only'`
tests the installed Ollama model. This is not an Engineering OS acceptance test.

## After installing this fix

1. Keep the database, worker checkpoints and passkeys. Stop the idle daemon before
   replacing its binary. Install a build containing this PR; alpha.2 assets do not update
   when source changes merge. Use the normal installer pinned to the new published tag.
2. Remove only the temporary diagnostic hooks created during troubleshooting, if present:

   ```bash
   rm -f "$HOME/.plantool/worker/v0.1.0-alpha.2/sitecustomize.py"
   rm -f "$HOME/.plantool/worker/v0.1.0-alpha.1/sitecustomize.py"
   ```

3. Restart the daemon. The Git transport check now ignores system/global helpers that the
   actual transport ignores, so the `GIT_CONFIG_NOSYSTEM=1` launch workaround is unnecessary.
   An existing worker installation restores automatically across upgrades when execution
   is enabled. First installation of a new version still requires Repair with passkey.
4. Verify routes and the project's allowed providers, budgets and current worker health.
   Do not save chain changes while other work is running: Stop All cancels globally.
5. In the project dashboard, use **Owner recovery → Retry failed step with passkey** for
   the frozen scheduler. On issue #1's activity page, do the same for failed PM triage.
   Recovery requires a failed read-only step and refuses active or recovery-needed attempts,
   coding steps, Docs steps that can apply repository edits, pending external effects and open gates. Other uncertainty requires
   manual reconciliation; this PR does not provide an override for uncertain writes.
6. Watch Role attempts and `#general`. Each failed read attempt ends before a fresh
   generation tries another permitted catalog-verified route. Failed routes cool down;
   budgets and provider allowlists remain in effect. No route changes mid-attempt.
   All routes exhausted means owner attention, not an infinite retry loop.
7. Open a general-channel status message's item/project link for its next action. Worker
   availability, frozen work, fallback, recovery and gate status are context only. Approve
   the exact current plan/commit on GitHub; a channel reply cannot approve anything.
8. Inspect safe role-error diagnostics in `~/.plantool/worker-state/errors.log` if needed.
   Structured role diagnostics include exception class, upstream status and stack locations,
   not prompts, provider bodies, raw model outputs or credentials. The file is owner-only.

Recovery keeps old LangGraph checkpoint epochs and bridge snapshots, retains failed attempts,
bumps sequence/generation fencing, retires old pending worker events and queues a new
idempotent worker recovery through the outbox. Old interrupts cannot resume the new epoch.
The same frozen step can only be retried once per fresh attempt/proof; approvals are retained
as records, never forged or bypassed.

## New repositories and GitHub rulesets

Preflight reads active effective rules for the default branch, including inherited GitHub
rulesets, and classic branch protection. It paginates the rules response. Missing checks
are an advisory: setup, planning and the first PR can proceed. Run real CI first, then add
its check and `plantool/policy` to a ruleset. Existing required checks and reviews remain
subject to GitHub enforcement at merge; PlanTool still checks current-head evidence and SHA.
No repository settings are changed by preflight, and no synthetic passing CI is created.

## Review order

1. `worker/plantool_worker/app.py`: error classification, idempotency and checkpoint epochs.
2. `dispatcher/runtime.rs`, `workflow.rs` and worker item/project graphs: failures end one
   attempt and retry the same step with new generation, instead of prematurely escalating.
   Read-only Codex roles use the native app-server with read-only sandbox and the lease.
3. `recovery.rs`, browser engineering routes and `auth.rs`: conservative eligibility,
   transaction rechecks, action-bound passkeys and owner audit.
4. RecoveryControls and EngineeringPage: separate scheduler/item recovery; no approval UI.
5. `workspace.rs`, WorkspacePage and runtime notices: deduped `#general` status, deep links,
   and no channel-to-decision authority.
6. GitHub API preflight and git_ops: effective rulesets, advisories and config isolation.
7. Rust, worker and UI tests; no real providers or playground writes in automated tests.

## Manual verification on macOS

Linux cannot establish native Keychain/Touch ID/sandbox-exec or real provider correctness.
Use a disposable home/project, real passkey, and the playground for these checks:

- Upgrade/install, repair once, restart and confirm the installed worker restores without
  another Repair. Verify Stop All prevents restore/launch and revokes active run tokens.
- Recover the existing frozen scheduler and triage separately. Verify old attempts and
  checkpoints remain, a stale proof fails, and coding/uncertain attempts cannot be retried.
- Simulate a provider 429, 404, 500, invalid JSON and exhaustion; confirm subsequent routes
  run in distinct attempts, the worker stays healthy and chain/policy/budget order holds.
- Put Codex first for a read-only role; confirm app-server execution under the lease,
  no HTTP Codex request, no worktree writes and daemon validation of the final typed result.
- Observe error/fallback/recovery/gate messages once in project #general, across restarts.
  Verify item and scheduler links, and that messages/replies never create decisions.
- Test a new repository with no CI and one protected by inherited rulesets. Confirm setup
  proceeds, effective protection is displayed, and actual merge requirements still apply.
- Keep Homebrew's system credential helper enabled. Confirm App Git fetch succeeds while
  repository-local URL/HTTP/credential/SSH overrides and malformed config remain denied.
- Complete one playground issue through the normal GitHub plan and PR approval gates.

Polling regression tests use an injected fixed clock. Previously they could pass before
Sunday's default scan time and start a real scheduled scan against a mock checkout after
that time; production polling continues to use the current time.


## Failure isolation review

This follow-up audits lifecycle, graph start/resume/recover, dispatch replay, role
execution, outbox delivery, browser recovery and scheduler navigation.

| Failure | Scope and way forward |
| --- | --- |
| Provider HTTP error, invalid typed output, timeout | Failed read-only attempt; existing bounded fallback selects another eligible route. Exhausted routes remain eligible for owner recovery after cooldown/configuration changes. |
| Broker rejects graph request | Structured graph conflict with operation and a known safe reason; health is probed independently. A frozen graph remains frozen until bound owner recovery. |
| One outbox recipient fails | Preserve original idempotency key and backoff; continue other recipients and runnable attempts. |
| Temporary request failure | Independent health probe decides availability; Refresh can restore readiness without reinstalling a healthy process. |
| Worker process exits, health fails or versions mismatch | Worker unavailable; Repair remains the explicit restart/install path. |
| Upgrade with an installed worker | Unpack this binary's embedded worker and sync its own frozen lock automatically; never reuse another version's environment. Stop All prevents startup. |
| Scheduler row selected | Same scoped attempt/history/gate view as other work, including recovery. |
| Stop All | Explicit resume with fresh passkey; recovery reports this separately from worker readiness. |
| Coding effects, uncertain checkpoint acknowledgement, pending external writes or open gates | Automatic retry stays blocked. Reconcile evidence and existing effects; no flag clearing, approval bypass or checkpoint deletion. |

The worker's daemon client allows ten minutes per model response (previously one
minute), with a five-second connect/pool bound; the enclosing read-role request
allows eleven minutes. This is not a promise that every local model fits that
budget. Read-only failures now record actual elapsed duration, including errors.

Manual verification on macOS: upgrade from an installed earlier alpha; restart
without Repair; confirm Stop All still prevents launch. Exercise a model timeout,
an upstream 429, malformed output, a frozen dispatch conflict, and one broken
outbox thread alongside healthy work. Confirm worker health remains ready when
healthy, no duplicate attempt is admitted on checkpoint replay, and owner recovery
still requires a fresh action-bound assertion. Open the project scheduler from the
running list and inspect its history. These native checks cannot run on Linux.
