# Phase 5: week-long playground verification on macOS

This is the real-loop exit test. Unit tests use fake providers and simulated time; they do
not certify a week of unattended operation. Run only against
`amritghimire/plantool-playground`. Keep the daily installed tool/home unchanged.

## Isolated build and setup (day 0)

```bash
git fetch origin main
git switch main
git pull --ff-only
(cd web && npm ci && npm run build)
cargo test --workspace
(cd web && npx vitest run)
cargo build -p plantool
scripts/smoke.sh
uv run --project worker --frozen pytest worker/tests
export PLANTOOL_HOME=$(mktemp -d /tmp/plantool-phase5.XXXXXX)
export PLANTOOL_PORT=41300
./target/debug/plantool serve
```

Keep that terminal running and preserve the two exported values in a separate terminal
for setup. Run `./target/debug/plantool setup` with the same home/port, register the first
passkey at `http://localhost:41300`, and complete the GitHub App wizard. Select only the
playground checkout. Verify repository ID, visibility, numeric allowlist IDs, required
checks and the sandbox/worker/provider route checks. Grant the App Dependabot alerts read
permission if dependency alerts are to be included. Missing permissions must appear as an
unavailable scan source, never a clean result. No provider key belongs in the worker.

Use Settings → Project schedule and oversight to select Minimal for a two-gate baseline,
the real IANA timezone and a sprint day/time during the test week. In Settings → Continuous
loop, select a scan day/time before that sprint. Default is 08:00 the previous day. Use Stop
All before settings changes, save with Touch ID, then resume with Touch ID. Confirm the
worker reports contract 5 / graph 4. Incompatible old checkpoints must fail closed; do not
reuse the daily tool's worker state.

## Intake and triage evidence (days 0–1)

Create three small, independently testable issues as the allowlisted owner. For example:
a CLI error-message regression, a small UI empty-state fix, and a docs example correction.
Create one deliberately unrelated issue and one clear duplicate with an exact issue link.
Record all issue URLs and the corresponding workspace item IDs.

Confirm PM triage classifies fit items into backlog, posts a reason and `plantool:declined`
for the unrelated issue, and posts `Duplicate of #N` plus the decline label for the duplicate.
No declined item may dispatch an implementer or acquire a plan approval.

With a separate non-allowlisted human account, create another small issue and comment on an
admitted issue. It must not be admitted automatically and its comment must be absent from
agent context. An outsider's `plantool:accept` label must not admit it. Remove that label,
then add it as the allowlisted owner; admission must follow the owner's numeric-ID issue
label event. An App-authored acceptance label does not grant authority. If a second account
is unavailable, record this scenario as unverified; the mocked tests still run it.

## Weekly scan and sprint (days 1–3)

Before scan time, commit a harmless TODO/FIXME and a deliberately missing regression test
in the disposable playground default branch. A failed default-branch CI run can provide
additional evidence. Do not add secrets or a real vulnerable dependency merely to test.

At scan time verify one read-only DevOps turn, then QA, then PM. Evidence must identify the
default SHA, CI links, advisory availability, bounded file/test inventory and TODO/FIXME
locations. No scan role may edit code or claim that static file inventory is passing tests.
PM may route each finding to proposed (acceptance required), sprint candidate, or backlog.
Record each new issue, route and reason; verify an immutable routing comment appears on the
latest sprint tracking issue once it exists. A proposed App issue must wait for an owner
acceptance event. Candidates and backlog findings still need triage and plan approval.

At sprint time let the existing discussion/plan workflow prepare the three fit items.
Verify the latest `Sprint YYYY-Www` snapshot has their plan versions, hashes, estimates and
throughput. Approve the latest snapshot with `/plantool approve-sprint` or select an item
with `/plantool approve <number>`. Leave one item unapproved and check backlog rollover at
the next cycle. Approve each final PR only on its current head; CI and branch protection
must remain enforced. Record plan/snapshot comment IDs, gate decisions, PRs and merge SHAs.

## Urgency and review findings (days 2–4)

Ask an agent on an admitted item to request urgency with its supplied `request_urgent`
tool. Verify only `plantool:urgent-requested` and a PlanTool notification appear: priority
and approvals must stay unchanged. As the owner, post exact `/plantool urgent` on that
issue (or remove/re-add `plantool:urgent` yourself). It should proceed to immediate triage,
research, discussion and plan without waiting for sprint time. Approve the plan on the item
issue with `/plantool approve-plan sha256:<current-prefix>`. It must not be included in a
batch sprint approval. Urgency must not switch a model mid-run or skip plan/PR gates.

Try a bot/outsider, edited and replayed urgent command; none may promote work. Confirm an
allowlisted label must have a real allowlisted `labeled` event, not just the label name.

Have Reviewer identify a separate out-of-scope concern. Confirm a read-only PM routing turn
files a new issue with its route/reason, links it from the original PR guide, and leaves the
current PR diff unchanged for that concern. Do not approve broader scope implicitly.

## Decline clocks, override and restart (days 3–6)

Post an allowlisted owner comment on a declined issue and inspect its clock reset in
Continuous loop. An outsider/App comment must not reset it. Remove `plantool:declined` from
a different declined issue; automatic close must become disabled. Use Return to triage
with a fresh Touch ID assertion on another decline; it must remove the decline label and
resume PM triage without approving a plan. Refreshing/replaying the old wait must not
create another resume or decision.

Stop and restart only this development daemon once while a scan or backlog graph is
waiting. Confirm durable receipts preserve the current turn and that publication markers
prevent duplicate issues/comments. Exercise Stop All separately: no new loop effects or
models should start while stopped, and resume requires Touch ID.

The real 30-day close cannot finish during a one-week test. Run
`cargo test -p plantool-daemon --test phase3 phase5_decline` for simulated-clock acceptance
and record the native 30-day boundary as pending until observed. Do not change system time
or edit database clocks in the running playground instance to claim a real elapsed test.

## Opt-in Nix (day 6)

On a playground worktree with a regular, minimal `flake.nix`, enable project Nix while
stopped, save with Touch ID and resume. Run a small approved build/test task. Verify its
coding CLI was launched as `nix develop --no-write-lock-file -c <tool>` inside sandbox-exec,
and build/test tools resolve from that dev shell. Reviewer restrictions, never-sent reads,
run-token scope and Stop All must still hold. Missing Nix must fail the opted-in attempt
visibly rather than silently execute outside the dev shell. Disable the option and repeat
with rustup/npm/uv; Personal must work without Nix. A worktree without flake.nix must use the
normal setup even when the project option is on.

## Week exit record (day 7)

Let the loop remain running for seven elapsed days, apart from owner gates and the explicit
restart/Stop All checks. Note start/end timestamps, daemon downtime, scheduled scan/sprint
IDs, each route/reason, PM triage outcomes, decline reset/disable/override observations,
urgent source events, plan approvals, CI results and merged PR heads. Note any manual
intervention beyond owner gates. Use read-only SQLite queries on the isolated home if needed:

```bash
sqlite3 "$PLANTOOL_HOME/db/plantool.db" \
  'SELECT at,action,subject,metadata_json FROM audit WHERE action LIKE "loop.%" OR action LIKE "decline.%" OR action="finding.routed" ORDER BY id;'
```

Do not upload database files, credentials, raw model auth or secret-bearing transcripts.
Link sanitized evidence in the PR. The exit is unverified until this record exists. Finish
with Stop All, stop the development daemon, and preserve the isolated home until review;
remove disposable issues/branches only after recording their evidence.
