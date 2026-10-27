# Workflow reliability verification — 2026-10-26

Branch: `fix/workflow-reliability-macos`, based on main `19b6b6d`.
PR #31 and #32 are merged; checked again before preparing the changes.
The host is macOS arm64. The owner explicitly authorized the main instance at
`~/.plantool`, port 41200, overriding the isolated-development instruction.

## Confirmed bugs and changes

- Operational notification replay compared historical immutable keys against new
  wording, aborting dispatcher ticks. Preserve original snapshots; discussion
  identity checks remain strict.
- Six sprint tests depended on the wall clock. The reported CI job reproduced this;
  acknowledgement now accepts the same injected clock as the scheduling fixture.
- Read-only checkpoint recovery incorrectly accepted old interrupt handoffs and
  could lose a current resume. Match exact interrupts and preserve active gates.
- Positive plan-review observations could route an unapproved plan toward coding.
  Observations retain the plan approval gate; actual requested changes return to PM.
- Supervisor `Docs`/`CTO` owners and a historical round cutoff froze otherwise valid
  retry recommendations. Normalize supported owners, honor validated retries/replans,
  and delay repeated retries rather than immediately demanding owner intervention.
- Known transient read failures retry with bounded backoff. Closed items, uncertain
  effects, active work and pending gates remain protected.
- Clean interrupted approved coding can reconcile only with proof: no live process,
  unresolved proxy/action or open gate; unchanged immutable approved plan; completed
  Supervisor retry; clean registered worktree at the saved checkpoint or a verified
  fast-forward to synchronized default. It accepts no implementation/task completion
  and never deletes worktrees or blindly replays effects.
- Empty weeks bootstrap eligible preparation and recover durable planning events;
  approved/in-progress work remains intact. The in-app issue board supports manual
  columns independently of real execution and approvals.
- Diagnostics queue through the global model lease, persist per-route results and
  show serial batch progress. Structured probes now include an actual user message.
- Provider details retain sanitized upstream errors; normalize millisecond quota
  reset values. Native failed/interrupted turns stop promptly and use bounded read
  fallback instead of sitting idle or classifying the worker as broken.
- macOS Codex OAuth routing failed under Seatbelt TLS-root discovery. Supplying the
  system CA bundle fixes the real account-routing RPC without granting Keychain access
  or disabling TLS. Full inference is a separate check.
- Background Keychain access is noninteractive. Restore existing workers periodically;
  ignore zombie-only process groups while fencing living descendants. Retention skips
  sockets. CLI restart waits for ownership release; graceful lock drop explicitly
  unlocks across concurrent fork/exec windows.
- A provider EOF/task-completion race discarded actual errors and showed a generic
  missing-result summary. Await the terminal task briefly before aborting; regression
  covers EOF delivered before the upstream error.
- Historical errors no longer inflate the owner's attention count. Questions,
  approval gates and unresolved recovery retain clear next actions.

## State preservation and actual progress

Consistent SQLite backups of both databases use SQLite's backup API and pass
integrity_check. Backups include `20261026T145417Z` under `~/.plantool/backups`.
No ad hoc SQL cleared freezes; recovery uses audited, guarded application paths.

The source-built main daemon restarted successfully at 15:01 UTC. After that restart,
items #1, #4 and #6 resumed read-only review, #7 resumed planning, and #5 changed from
frozen recovery to backend automatically. Item #5's clean trusted checkpoint and
approved plan were preserved; the interrupted attempt was recorded as failed, never
fabricated as successful. Two approved gates remain; the second was approved by the
owner through the existing UI, not by this development session.

Playground baseline: main includes merged PR #3 at `54c1183`; its original
`scripts/check.sh` and `verify-scenario.sh baseline` pass without weakening criteria.
Dedicated issue #7 tested an exact clarification: the owner answered Ada in the real
item thread at 13:57 UTC; continuation preserved the answer and required no approval.
Dedicated issue #8 was ingested and closed on GitHub at 14:00 UTC; local work became
closed/cancelled and stayed closed across restarts, without new attempts or comments.

## Live provider results

The owner authorized a serial batch through the physical passkey. All 11 configured
routes finished. These are real outcomes, not mocked-provider claims:

| Route | Observed outcome | Next action |
| --- | --- | --- |
| codex-sol / gpt-6.1-sol | Initial full probe failed workspace routing discovery. After CA fix, actual sandbox account/read passed in 1.59s. | Recheck full model/tool/JSON turn; account RPC alone is insufficient. |
| local-devstral / devstral-small-2 | Basic, tools and structured JSON passed. | Available fallback; real role schemas still validate independently. |
| local-qwen36a3 / qwen3.6:35b-a3b | Basic passed; exact tool-argument probe failed. | Select a tool-capable route or retest after model update. |
| local-qwen38 / qwen3.8:27b | Basic/tools passed; JSON returned 500 no user query found. | User-message probe defect corrected; repeat live JSON check. |
| ollama-cloud / gemma4:31b-cloud | Basic/tools passed; strict JSON probe failed. | Use validated fallback; do not accept plain text as a plan. |
| Five configured OpenRouter free routes | Catalog verified; actual basic requests returned 429 free-models-per-day. | Wait for Oct 27 00:00 UTC reset or explicitly configure another permitted route. |
| zen-free / resolved beam-free | Catalog verified; actual request returned 429 free usage limit reached. | Wait for quota reset or configure another permitted route. |

Actual aliases and underlying model IDs remain distinct. No credentials were printed,
no provider key was substituted for ChatGPT sign-in, and existing chains were retained.
Live diagnostics and role executions were observed sharing one lease; waiting work
queued until the holder released it. At 15:11 UTC item #4 reached plan_gate and
the project scheduler advanced to sprint_plan; #1/#6 moved through provider timeout
fallback without demanding owner attention.

## Automated verification

Final required checks passed: npm ci/build, cargo test --workspace (311 passed,
one opt-in native account test ignored by default), Vitest (43 files, 130 tests),
worker pytest (37 tests), CLI build and isolated smoke (port 41409). The opt-in
native account test was also run explicitly and passed. Native account
routing test passed using production environment preparation. Security sandbox tests
retain credential/Keychain restrictions. Regression coverage includes CI clock,
notification replay, exact checkpoint handoff, plan approval protection, supported
Supervisor owners, automatic recovery, clean/dirty interrupted worktrees, quota units,
board authority separation, route errors and diagnostic visibility.

The host's global Git commit signing is overridden only for test subprocesses with
`GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=commit.gpgsign GIT_CONFIG_VALUE_0=false`; the user's
global configuration is unchanged.

## Manual verification on macOS

Passed: source daemon restart and health, installed worker restoration, actual
physical passkey authorization and thread answer, serial provider batch, live
single-slot queueing within the configured project, read-only restart recovery,
automatic recovery of validated Supervisor recommendations, approved clean-checkpoint
reconciliation, dedicated issue closure and retained closure across restart.

Still unverified: successful full Codex model turn after CA correction; corrected
Qwen JSON live probe; actual concurrent work across multiple projects (only one is
configured); fresh physical Stop All/resume/Repair cycle; dedicated live prerequisite
satisfaction and scope-revision approval scenario; populated sprint membership;
new optional-advisory 403 scenario and end-to-end comment deduplication under those
scenarios. Automated checks are not substitutes for these live checks. Do not bypass
passkeys, approve plans or force provider quota to complete them.

### Follow-up retry on Oct 26 UTC

After the owner reported approving the macOS permission prompt, the prior Codex
diagnostic still showed only its earlier cooldown result; it had not started a new
model turn. A consistent SQLite backup was created at
`~/.plantool/backups/20261026T230748Z/`. The required checks were rerun successfully:
Rust workspace 318 passed / 1 ignored, Vitest 130, worker pytest 37, frontend build,
CLI build and smoke on port 41377.

Restarting exposed an older daemon still blocked in `SecItemCopyMatching` during
runtime shutdown. It was terminated after sampling confirmed the Keychain call was
still blocked; its orphan worker was stopped only after the database showed no active
attempts or model lease. The current source-built daemon now owns port 41200 and the
installed worker restarted. Background Keychain reads return promptly instead of
hanging, but the GitHub App credential is currently unavailable to the new binary;
GitHub sync is waiting for Keychain authorization. Several durable worker resume
records returned stale-interrupt 409s and need reconciliation; no plan approval or
GitHub write was replayed. The old Codex diagnostic result remains unchanged pending
a fresh passkey-authorized run. These are current blockers, distinct from the earlier
historical provider failures above.

### Keychain query follow-up

The owner completed a passkey-authorized GitHub preflight and the saved preflight
shows `rulesets_checked: true`. Review of that path showed background reads were
explicitly filtering out authenticated Keychain entries. The filter was removed while
keeping macOS interaction disabled for background reads, so an authorized entry can
be read without prompting and an unauthorized read returns promptly. Required checks
passed again (Rust workspace 318 passed / 1 ignored, Vitest 130, worker pytest 37,
frontend/CLI builds and smoke on port 41349). A consistent backup was made before
the next restart. That restart uses a new executable hash, so the earlier Keychain
grant does not apply to it. The current build still logs a Keychain access error;
verification awaits the owner authorizing this exact build with “Always Allow.”

### QA checkpoint recovery and signed-in Codex retest (Oct 27 UTC)

The owner reran the Codex diagnostic with the physical passkey. The saved result
confirms the exact `gpt-6.1-sol` catalog entry and a successful signed-in Codex
app-server turn using the read tool and JSON output. No OpenAI API key was used.

Item #5 had an approved implementation commit (`079c375d`) and a QA attempt
interrupted during daemon restart. A guarded recovery path now rechecks the approved
plan hash, clean project checkout, checkpoint ancestry and approved file scope, then
marks the uncertain QA attempt failed and starts a new QA attempt with a fresh key.
It never accepts the interrupted run's evidence. Older QA work with no checkpoint,
including item #4, is left untouched for owner recovery. A regression test also
verifies that unrelated uncommitted files are preserved. After deployment, item #5
advanced to QA and attempt 33 was observed running against the preserved approved
commit; successful QA completion remains pending.

The final `scripts/check-continuity.sh` run passed the playground baseline, Rust
workspace tests (320 passed, 1 ignored), Vitest (43 files / 130 tests), worker pytest
(37 tests), frontend and CLI builds, and isolated smoke (port 41432). A prior full
run had one API setup failure (403 during shared passkey registration); its isolated
rerun passed, and the subsequent full run passed. A consistent backup was created
before each main-daemon restart. The current source build is healthy on port 41200.
Rebuilding changed the executable identity, so GitHub App background sync again
requires the owner to authorize this exact binary in Settings → GitHub; no data,
approval or checkpoint was modified to work around it.

Still unverified: GitHub sync after this latest binary's physical authorization;
completion of item #5's new QA attempt; live multi-project concurrency, Stop All / resume
/ worker repair, prerequisite and revised-scope scenarios, populated sprint membership,
optional advisory permission failure, and end-to-end message deduplication. These
remain explicit follow-ups, not implied by the automated checks.

### Automated commit-signing recovery

The resumed QA role exposed a separate deterministic failure: automatic WIP commits
used the synthetic `PlanTool <plantool@localhost>` identity while inheriting the
owner's global `commit.gpgsign=true`, which cannot sign without that synthetic
identity's private key. Automatic route-change checkpoints and PlanTool-authored
documentation commits now pass `-c commit.gpgsign=false` for that individual Git
command only. Manual user commits and global/repository settings are unchanged. A
regression test configures signing on in a temporary repository, verifies the
automatic checkpoint succeeds, and confirms the on-disk setting stays on. The full
required suite passed again (Rust 321 passed / 1 ignored, Vitest 130, worker pytest
37, frontend/CLI builds, playground baseline and smoke on port 41432). After a
consistent backup and restart, item #5 restarted QA at generation 38; it was observed
running under the global model lease. Its final result and GitHub sync remain pending.

### Frozen plan-review evidence failure (Oct 27 UTC)

Item #9 was frozen after repeated CTO `plan_review` outputs failed with
`Checks require artifact evidence`. The daemon correctly requires stored `artifact://`
evidence, but the model-facing schema only described check evidence as an arbitrary
string. The shared contract now constrains it to an artifact SHA-256 URI; worker
schemas were regenerated, and the one allowed same-route format correction explains
that read-only reviews with no executed checks must return `checks: []`. CTO review
instructions now say the same.

One narrowly guarded automatic recovery handles this exact old failure after a
Supervisor escalation: it verifies the latest failed review matches the current plan
hash, the plan is still unapproved, and there are no active attempts, gates or pending
effects. It retries `plan_review` once on the current code, without altering the plan
or approval state. A regression test verifies the retry is idempotent and leaves the
approval gate intact. It cannot repeat after a second failure.

The next live run exposed a second defect at the approval boundary. Plan review
threads are attached to a shared project activity channel through an item-specific
discussion topic, but gate validation accepted only item-owned threads. The gate
transaction therefore rejected a valid thread and rolled back before creating any
gate or GitHub publication intent. Validation now accepts this explicit topic
relationship and rejects channels with no link to the item. When the worker reports
an uncertain checkpoint after a gate request, recovery retries only if the current
read-only review receipt is verified, the current plan revision and GitHub binding
still match, and no gate, gate receipt, publication intent, active work or other
external effect exists. It can retry once and leaves approval false. Regression
tests cover the thread relationship, bounded retry and clearing the exact stale
checkpoint message only after the current gate and publication intent are durable.
The latest full required suite passed: Rust workspace 326 passed / 1 ignored, Vitest 130, worker pytest 38,
playground baseline, frontend/CLI build and smoke on port 41311. Before restarting
the main daemon, consistent SQLite backups were saved at
`~/.plantool/backups/20261027T141234Z`. The source-built daemon is healthy on port
41200. On restart, item #9 automatically recovered to `plan_gate`; its local gate
is pending, the item is unfrozen, and `approved=false`. The GitHub plan-publication
outbox entry remains queued while this binary lacks Keychain access. No approval was
recorded; the owner must still review the plan through the normal gate.
Before the final restart that deployed stale-error cleanup, another consistent
backup was saved at `~/.plantool/backups/20261027T142457Z`. The exact stale checkpoint
warning is now cleared because the current pending gate and publication intent match
the plan revision. The item remains at `plan_gate`, unfrozen, and unapproved.

### Saved live provider diagnostics

The saved serial batch completed for all 11 configured routes (Oct 26 UTC); the
Codex test was rerun later and succeeded on Oct 27 01:10 UTC. `codex-sol`
(`gpt-6.1-sol`) passed its signed-in Codex app-server, read-tool and JSON check;
`local-devstral` (`devstral-small-2`) passed chat, tools and structured JSON.
`local-qwen36a3` (`qwen3.6:35b-a3b`) passed chat but its model response failed the
tool check. `local-qwen38` (`qwen3.8:27b`) passed chat and tools; the JSON request
returned upstream HTTP 500, “no user query found in messages.” `ollama-cloud`
(`gemma4:31b-cloud`) passed chat and tools but its response did not satisfy the
structured-JSON check. All five configured OpenRouter routes returned HTTP 429 at
the basic-chat stage, with the provider reporting its daily free-model limit:
`or-gemma4-31b`, `or-glm-5.2`, `or-laguna-xs`, `or-minimax-m3`, and `or-nemotron-s`.
`zen-free` resolved to `beam-free` and returned HTTP 429 for the provider's free
usage limit. Catalog checks passed for every route. These are saved live outcomes;
the provider-limited and capability-failing routes were not counted as verified.
