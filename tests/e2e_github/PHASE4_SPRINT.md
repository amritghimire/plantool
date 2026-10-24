# Native Phase 4 verification: a three-item sprint

This is a real macOS/App/provider test, not a result claimed by Linux mock tests. Keep the
daily installation/home untouched. Use the full PR stack or merged main, a disposable clone
of amritghimire/plantool-playground, and a separate daemon. No Phase 5 features are needed.

## Prepare the isolated environment

1. Run the repository checks before starting a manual daemon:

   ```sh
   (cd web && npm ci && npm run build)
   cargo test --workspace
   (cd web && npx vitest run)
   cargo build -p plantool
   scripts/smoke.sh
   uv run --project worker --frozen pytest worker/tests
   cargo test -p plantool-daemon --test sandbox --test phase3
   ```

2. Export PLANTOOL_HOME to a fresh mktemp directory and PLANTOOL_PORT=41300. Start
   target/debug/plantool serve. In another terminal with the same variables, run setup and
   enroll/login with the platform passkey at localhost. Do not point development at ~/.plantool.
3. Configure the GitHub App for the disposable playground clone and the numeric allowlist.
   Confirm Issues/PR/Contents/Checks permissions, labels and branch protection. Use Repair
   to install the locked worker and verify routes. Codex uses its CLI login; signed-in local
   Ollama serves local/cloud references; OpenRouter/Zen keys stay in the daemon Keychain.
   Confirm Personal still works if the worker is stopped or broken.
4. Before creating issues, Stop All and open Project schedule and oversight in Settings.
   Select Standard, the intended IANA timezone (for example Asia/Kathmandu), today's day and
   a local time sufficiently in the future for planning. Save with fresh Touch ID, then
   resume execution. Day values are Monday=0 through Sunday=6. Unconfigured schedules use
   Monday 09:00 UTC; scheduling backfills only the current local ISO week.

## Run three items

5. Create three small issues as an allowlisted owner after setup, each changing a separate
   playground fixture/file with a concrete test. Keep work bounded and independent. Issue
   creation is fixture preparation; do not add sprint/triage/urgent labels to force a run.
6. Open Workspace. Verify #sprint, #general and one thread for each item. Observe read-only
   role messages. Mentions choose one next speaker; after three contribution turns PM closes,
   or Supervisor closes when PM participated. Post an owner reply, including natural-language
   approval text in a disposable case, and verify it is only context: no decision/gate advances.
   No DM, message-edit or message-delete action should exist.
7. Let CTO research, discussion, PM plans and CTO review finish. Inspect T-keys, areas,
   criteria, bounded file paths, versions and hashes. For at least one Standard plan, inspect
   suggested review tags, change a tag before approval with Touch ID, and verify a new
   immutable revision/hash and App comment appear. The prior publication stays unchanged.
8. At the scheduled local time the project graph should plan a sprint and publish one
   tracking issue titled Sprint YYYY-Www. It may initially find no prepared candidates;
   that cycle must return to event wait, then replan when candidates become ready after the
   due time. PM selects/sizes freely; inspect its reasoning instead of assuming three items
   will be selected. The Phase 4 exit is demonstrated only when the latest snapshot lists
   all three intended items with current plan versions/hashes, estimates and recent throughput.
9. On the latest tracking snapshot, post /plantool approve <issue-number> for one item.
   Confirm only it starts after the project graph releases it. Then post /plantool
   approve-sprint for the remaining current snapshot items. Confirm each starts once and
   repeated polling does not duplicate authorization, dispatch or publication. Changed plans
   must replace the snapshot with a new App comment; old/edited/bot/non-allowlisted commands
   and commands posted before the new snapshot must not authorize it.

## Milestones and final PRs

10. For tagged Standard phases, implementation must stop after the phase's QA/Reviewer/Docs
    checks. The daemon pushes plantool/<issue>-<slug>; the App opens a draft PR and posts one
    Milestone N/M ready at <full-sha> comment with a compare link from the previous stop.
    Confirm CI runs for each new pushed head and no next-phase task runs before approval.
11. On a draft PR, try a scoped /plantool request-changes <reason>. Confirm the owning role
    receives the feedback and QA/Reviewer rerun. At the next current-head stop, post
    /plantool continue. A previous comment or old-head approval must not pass the new gate.
    For two tagged phases, observe two separate stops/comments and advances.
12. After the final milestone, the existing draft becomes ready through the App and receives
    the final Docs review guide. It must still require a fresh allowlisted PR review on the
    exact head and green checks before merge. Milestone continuation never substitutes for
    PR approval. Verify all three items reach their PRs; optional owner reviews/App merges
    can demonstrate completion and updated throughput. Avoid moving main until the stable
    three-item exit is recorded; test rebase separately.

## Separate negative and Strict cases

- Leave a candidate unapproved through the next sprint boundary; it returns to backlog,
  without a coding dispatch or invented approval. Approving one item must not release others.
- Test a default-branch move separately: rebase invalidates affected milestone identities,
  reruns QA/Reviewer, and requires fresh stops. Conflicts go to an owning implementer;
  old milestone SHAs cannot certify rewritten commits.
- In a fresh isolated setup, choose Strict before intake. Research must stop for the App's
  /plantool approve-research sha256:<current 12+-hex prefix> command. It does not approve the
  plan. Approve the plan separately, then observe a milestone for every phase even untagged.
- Compare Minimal in another disposable setup: plan and exact-head PR approvals still apply,
  without research or milestone gates. Safety stops and scope re-approval remain mandatory.
- Verify settings/tag writes require fresh assertions bound to project/version or item/hash;
  agent tokens and owner workspace replies cannot change policy or approve work. Stop All
  must still revoke/kill runs, and resuming requires fresh owner assertion.

Record tracking issue, snapshot, gate/decision IDs, head SHAs, evidence links, provider/model
provenance and unavailable checks. The target is one complete three-item sprint, not merely
passing mock tests. Stop the isolated daemon before removing its home or starting another
fixture against the same App/repository. Retain GitHub artifacts for review and clean up
only the disposable issues/branches after preserving evidence.
