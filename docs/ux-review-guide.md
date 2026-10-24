# Engineering OS UX review guide

## Reading order

1. `web/src/main.tsx`, `AppShell.tsx`, `SettingsPage.tsx`: routes, shared global controls,
   dedicated settings and separation from Personal sessions.
2. `SetupWizard.tsx` and `routes/engineering.rs`: guided setup and daemon-persisted cursor.
   The cursor stores only a step and optional project ID. Navigation does not approve an
   action; configuration retains its existing action-bound passkey checks. The guide
   configures model routes while stopped, connects projects and policies, then resumes
   execution and installs/verifies the worker. Completion checks current project routes
   and worker health. Catalog verification is not a real provider execution test.
3. `GithubSettings.tsx`, `EngineeringPage.tsx`: reusable App connections, per-project
   repository/checkout, worker health, active attempts and next-action links.
4. `engineering_view.rs`: human-only, read-only projections of persisted projects,
   attempts, gates and audit events. No raw run payloads, tokens or credentials are exposed.
   Item history is limited to the latest 200 recorded events/attempts and 100 gates.
   This does not reconstruct transitions absent from the audit log or add tracing.
5. `WorkspacePage.tsx`, `usePolling.ts`, `styles.css`: project-scoped channels/threads,
   search, browser-local unread markers, responsive navigation and context-only replies.
   Read markers do not synchronize between browsers; reply drafts stay in memory.
6. Component tests, `crates/daemon/tests/auth.rs` and engineering view tests: setup resume,
   failed saves, missing model routes, project isolation and human-only endpoint access.

## Manual verification on macOS

Linux cannot verify native Touch ID, Keychain, sandbox-exec or the installed worker's
startup. Browser screenshots here use fixture API data, not a live macOS execution.

1. Install a build including the worker certificate-startup fix (PR #23). Use a separate
   test home/port and two local repository checkouts. Register a platform passkey.
2. Open Engineering OS → Guided setup. Advance through tools and model routes; confirm
   all roles including Docs and Supervisor have verified, permitted routes. Stop All
   before policy changes; it cancels existing execution globally. Never test this against
   unrelated active projects. Do not assume a cancelled graph automatically restarts.
3. Close the browser mid-guide, restart the daemon and reopen `/setup/engineering`.
   Confirm the saved step/project resumes. Completed configuration persists, but unfinished
   text inputs and PEM contents are deliberately not saved. Check failed saves keep the step.
4. Create/install a GitHub App using the inline instructions. Connect the playground and
   another test repository using the same installation; confirm checkout paths remain
   project-specific and each preflight warning is visible. Native Keychain saves and
   configuration changes must request their existing bound passkey proof.
5. Set project oversight, schedules, allowed providers and never-sent files. Resume with
   passkey at the Worker step, repair and verify worker health. Finish setup and confirm
   the root URL opens Engineering OS for the selected project.
6. Run one small allowlisted playground issue. Observe its current step, role/model
   attempt, failures, history and gate links. Approve through the existing GitHub gates;
   there is no new approval path in the dashboard or workspace.
7. Switch projects in Workspace. Search threads/messages, reply in one item and confirm
   the reply is context only. Check local unread markers, polling/focus refresh and narrow
   mobile navigation. Verify switching thread cannot send a draft to another thread.
8. Open dedicated Settings and verify privacy lists Engineering OS-only checkouts as well
   as Personal repositories. Test Stop All from the shared header, then passkey-protected
   resume; confirm token revocation and cancellation retain their existing behavior.
9. Confirm Personal session creation, editing, approvals and hosted runs still work with
   Engineering OS unavailable. Test keyboard navigation, dark/light themes and browser
   back/forward with two projects.

No real model, GitHub App, playground issue, native passkey, Keychain or sandboxed worker
was exercised in this Linux environment. A real run is required before claiming provider
execution or end-to-end onboarding success. Phoenix, telemetry and eval expansion remain
outside this change.
