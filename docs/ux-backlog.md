# PlanTool setup and navigation improvements

Recorded: 2026-10-24 (Asia/Kathmandu)
Status: implementation authorized; tracked in the Engineering OS UX PR.

## 1. Dedicated Settings page

Replace the crowded settings dialog with a dedicated Settings page and clear navigation
between sections. Organize global settings, provider/authentication configuration and
project-specific configuration so their scope is apparent.

## 2. Stop All placement

The isolated Stop All button above the application header feels out of place. Integrate
this global control into the application navigation/status area, keeping it easy to find.
Make its global scope and stopped/running state clear. Preserve existing cancellation,
token revocation and passkey requirements for resuming execution.

## 3. Engineering OS with multiple projects

The Local checkout (absolute path) field in GitHub App setup makes Engineering OS appear
tied to one repository. The product must support an owner managing multiple projects.

Separate GitHub App/installation configuration from project onboarding. Provide a project
list and a way to add/select projects, with repository and local checkout configured per
project. Clearly distinguish global controls/settings from project-specific schedules,
provider policy, oversight and workspace context. A local checkout is a property of a
project, not the identity or scope of the whole Engineering OS.

For the later PR, inspect existing multi-project backend support before deciding which
changes require backend work versus clearer navigation and onboarding.

## 4. Dedicated Workspace page with a Slack-like experience

Move Workspace out of the settings dialog into its own main-navigation page. Use a
Slack-like layout and interaction model: project/workspace selection, a channel sidebar,
thread navigation and a central conversation view. Make project/channel/item context clear
and provide space for search and unread indicators when that work is scheduled.

Preserve turn-based agent discussion and owner replies as context, never approvals.
Keep the existing no-DMs rule and approval/security boundaries. This UI work is now authorized. Other Phase 6 work remains deferred.

## 5. Engineering OS application flow and execution visibility

Once setup is complete, Engineering OS should feel like a dedicated application area.
Show the selected project, worker availability, active role/model attempts, current graph
step, recorded graph history, waiting gates, failures and recovery needs. Lead the owner
to the next useful action instead of requiring them to inspect settings. Keep setup,
project operations and Personal sessions clearly separated.

## 6. One-step-at-a-time setup, with resume

Provide setup instructions and controls inside the application, without requiring a README.
Walk through model tools, GitHub App/project connection, worker installation, model route
verification, project policies and a first real run. Persist the current step and selected
project in daemon-owned storage so the owner can close the browser or restart and resume.
Progress is navigation context, never an approval or a claim that an unverified step passed.

## PR acceptance criteria

- Dedicated routes for Settings, Engineering OS project operations, work-item activity and Workspace.
- Guided setup presents one step at a time and resumes its daemon-saved step/project.
- Global Stop All and execution status integrated into the shared application header.
- Multiple projects visible/selectable; App credentials can be reused when adding repositories.
- Owner can see recorded attempts/models, gate waits and graph transitions for an item.
- Slack-like Workspace with project/channel/thread navigation and context-only owner replies.
- Existing passkey, run-token, sandbox and GitHub approval boundaries remain intact.
- No Phoenix, tracing, eval expansion or automatic chain changes in this PR.
- Document native Mac verification and any information the current persistence model cannot show.
