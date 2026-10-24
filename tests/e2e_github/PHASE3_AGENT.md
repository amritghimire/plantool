# Native Phase 3 run: one allowlisted issue to merged PR

Run this after installing/testing the complete stack. These steps require macOS, Touch ID,
Keychain, sandbox-exec, Git, configured coding CLIs and real model accounts. Default tests
make no real model calls and do not claim this native run was verified in Linux.

1. Use a separate checkout of the PR stack and a fresh disposable playground clone. Run all
   AGENTS.md checks with port 41300 free before starting a manual daemon. Also run:
   `uv run --project worker --frozen pytest -c worker/pyproject.toml worker/tests` and
   `cargo test -p plantool-daemon --test phase3 --test sandbox`.
2. Export `PLANTOOL_HOME=$(mktemp -d)` and `PLANTOOL_PORT=41300`, build the binary and start
   `target/debug/plantool serve`. Use another terminal with the same variables for
   `plantool setup`; enroll the platform passkey at localhost. Keep the daily home untouched.
3. Configure the App for **amritghimire/plantool-playground** and the disposable matching
   clone. The App needs the §13.1 permissions. Confirm numeric approver amritghimire,
   labels, provider allowlist and never-sent patterns. Use Repair and route verification
   with fresh Touch ID. Check that Personal works when the worker is unavailable.
4. Configure playground default-branch protection and required CI checks/reviews, dismiss
   stale approvals, and require the App's `plantool/policy` check. The App has Administration
   read permission only: use the wizard's GitHub settings link to change protection as owner.
   Install/verify models and provider policy before starting work; do not change routes mid-run.
5. Seed one small issue authored by the allowlisted owner **after** project setup, for example:
   add a focused unit test and a bounded error-message fix in the playground fixture. Issue
   creation/setup are fixture preparation. Count the following two approval decisions as the
   owner's only workflow approvals. No sprint, workspace or triage step is involved.
6. Watch Settings → Engineering OS → Refresh work items. The graph should run CTO research,
   PM plan and CTO plan review, then wait. Inspect the App's immutable plan comment, keyed
   tasks, areas, acceptance criteria, affected files, full SHA and localhost plan link.
7. **Owner approval 1:** post `/plantool approve-plan sha256:<current 12+-hex prefix>` on the
   issue. No browser approval advances this Engineering OS gate. Verify that implementation
   starts only after the command is refetched and accepted.
8. Watch Backend/Frontend skip rules, conditional DevOps, QA, read-only Reviewer security
   pass and Docs. Coding attempts use private Git checkouts; QA may write tests only.
   Check evidence artifacts, scoped CLI task progress, commit trailers and model provenance.
   The daemon pushes `plantool/<issue>-<slug>` and the App opens the PR with `Closes #n`.
9. Inspect the generated review guide and CI for the exact head. The policy check must remain
   non-success until current-head owner approval. **Owner approval 2:** submit an APPROVED
   review on the current PR head. With green checks and protection satisfied, the App merges
   using the approved SHA and the repository's allowed method. Confirm the issue closes,
   merged state/audit are recorded and no duplicate PR/push appears on subsequent polls.

Exactly two approvals is the stable happy path. Changes requested, a scope replan or a new
push intentionally needs fresh approval. Keep the default branch still for the two-approval
measurement. Test main movement separately: a successful rebase must rerun QA and Reviewer;
a conflict returns to its implementer. Old-head approvals must not count. A refused merge
must be shown and not repeatedly retried; an owner merge must be detected.

Use separate disposable cases for three failed attempts → Supervisor, two Supervisor rounds,
replan → new revision and plan approval, severe/unclear findings, frozen side effects and
out-of-scope findings filed as `plantool:proposed` issues instead of being fixed in the PR.
Do not unblock an uncertain external action by deleting its intent or blindly retrying it.
Owner attention/recovery is required for unknown outcomes; Stop All remains available.

Afterward stop the isolated daemon before removing its home. Retain GitHub comments/PRs as
review evidence, clean disposable branches as owner, and leave the daily installation alone.
The existing PLANTOOL_E2E_GITHUB=1 test target exercises the GitHub gate fixture separately;
it does not substitute for this real agent/provider/sandbox run.
