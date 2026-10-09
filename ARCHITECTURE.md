# plantool architecture

plantool hosts the research → plan → implement workflow for a human and a coding agent at once.
Its shape follows difftool: one long-lived daemon owns the state, the human uses a browser, the
agent uses a thin CLI, and both see each other's changes live.

```
   human  →  Web UI (React, embedded in the binary)  ──HTTP + WS──┐
                                                                    ▼
   agent  →  CLI (`plantool …`, clap)  ───────HTTP───────▶  Daemon (axum) on 127.0.0.1:41200
                                                              Registry: sessions in memory
                                                              Providers: claude / codex processes
                                                                    │
                                                        ~/.plantool/sessions/<repo>/<slug>/
```

## Crates

- `crates/core` (`plantool-core`): pure data and logic, no I/O. Models (`Session`, `State`,
  `Comment`, `Run`, `Stage`, `DocKind`), the stage machine (`transition`), Markdown indexing
  (headings, checkboxes, per-phase progress), and comment anchoring (`--match` resolution and
  re-anchoring after a document changes).
- `crates/daemon` (`plantool-daemon`): the server. `registry.rs` holds every session in memory
  and is the only writer of the store; `store.rs` is the on-disk layout; `git.rs` shells out to
  git with argv (never a shell); `watcher.rs` captures documents when files change; `providers/`
  hosts agent processes; `runs.rs` turns provider events into a durable transcript; `changes.rs`
  bridges to difftool or `git difftool`; `routes/` is the HTTP and WebSocket API;
  `security.rs` guards every request; `assets.rs` serves the embedded web build.
- `crates/cli` (`plantool` binary): commands, a blocking HTTP client, `ensure_daemon`, and the
  `serve` command that runs the daemon in-process. The web build and the agent skill are embedded.
- `web/`: Vite + React + TypeScript. Built to `web/dist`, embedded by `rust-embed`.

## Sessions, documents, stages

A session is `<repo_slug>/<slug>`. The repo slug comes from the `origin` remote
(`owner-repo`) or the checkout's directory name, computed once and stored in `meta.json`. The
slug is the CLI ref when unique.

Documents are Markdown files in the session folder: `research.md`, `plan.md`, `investigation.md`,
`quick-fix.md`, `design.md`. The agent writes them directly (the CLI prints the path); the daemon
notices (a recursive `notify` watcher plus a 2 s poll), stores every revision content-addressed
under `revisions/<kind>/<sha>.md`, re-anchors comments, and broadcasts `doc-refreshed`. Writing
research or plan the first time advances the stage to the matching review stage.

Stages are a fixed sequence. `core::stage::transition(from, to, actor)` is the single rule: an
agent may only move forward, never into `approved` or `done`, and never across `approved`. The
daemon derives the actor from a per-daemon browser token that is injected into the served
`index.html` and sent back as `X-Plantool-Actor`. The CLI never has it and has no approve command.
This is a guard against honest mistakes, not against an agent with a shell on the same machine.

## Persistence

```
~/.plantool/
  daemon.json                         pid, port, protocol
  daemon.log
  prompts/<stage>.md                  optional prompt overrides
  sessions/<repo>/<slug>/
    meta.json                         Session (rare writes)
    state.json                        stage, comments, doc shas, seq, review (per mutation, atomic)
    research.md plan.md …             the live documents
    revisions/<kind>/<sha>.md         every captured revision
    runs/<id>.json, runs/<id>.ndjson  run metadata and append-only transcript
```

Writes are temp-file-plus-rename with fsync. A batch of comments is one persist and one broadcast.
`state.seq` increments on every mutation and is the cursor for `watch --since`.

## Live updates

`GET /api/sessions/:repo/:slug/live` is a WebSocket. Every mutation broadcasts a `LiveMessage`
(`seq`, `at`, plus a tagged `LiveEvent`: comment events, `doc-refreshed`, `stage-changed`,
`run-*`, `changes-opened`, `navigate`). The web client applies events directly and re-fetches on
reconnect. The CLI's `watch` opens the same socket, folds in a `since` reconcile, prints one
NDJSON event and exits, so an agent can block until the human acts.

Commits started from the browser (milestone approval, the PR dialog) go through
`commits::run_commit`. The session records the running commit (one at a time, a second request
gets 409), streams git's and the hooks' output as throttled `commit-progress` events carrying the
last 40 lines, and returns the record in the session view so a reloaded tab still sees it. The
commit runs in its own task, so a dropped request still finishes and clears the record.
`POST …/commit/cancel` kills the commit's process group; the files stay staged. These events are
never persisted, and `watch` ignores them.

## Hosted runs

`providers/mod.rs` defines a neutral `ProviderEvent` vocabulary (messages, text deltas, tool
activities, permissions, input requests, turn boundaries, provider session id). Each provider is a
task owning a child process:

- **Claude Code**: `claude -p --input-format stream-json --output-format stream-json --verbose
  --include-partial-messages --permission-mode default --permission-prompt-tool stdio`. The daemon
  sends an `initialize` control request, then user messages as JSON lines. Permission prompts
  arrive as `control_request` / `can_use_tool`; read-only tools are auto-allowed, everything else
  is surfaced to the browser and answered with a `control_response`. `AskUserQuestion` becomes an
  input request. The session id from `system/init` is stored for `--resume`.
- **Codex**: `codex app-server --stdio`, JSON-RPC over stdio. `initialize`, `thread/start`
  (workspace-write sandbox, approvals on request), `turn/start` with the session folder as an extra
  writable root, `turn/steer` for follow-ups mid-turn. Server requests for command, file-change and
  permission approvals and for user input are surfaced and answered with the native decision.

`runs.rs` consumes provider events, appends each to the run's NDJSON transcript with a sequence
number, tracks the run status (running, waiting on a prompt, idle, stopped, failed), and
broadcasts. The browser replays `runs/:id/events?since=` after a reconnect. Runs that were live
when the daemon restarts are marked stopped; a new run can resume the provider session.

Stage prompts (`prompts.rs`) are short: they tell the agent to run `plantool skill` and
`plantool skill <stage>`, substitute the session's document paths so nothing lands in
`REVIEWS/`, and render the session's **brief** (`Session.brief`, set with `plantool new --brief`,
`plantool session brief` or the sidebar; `POST …/brief` broadcasts `session-updated`).
`GET …/prompt/{research|plan|implement|next}` returns the rendered prompt; the browser shows it
with a copy button while a document is missing, and `plantool new` prints it.
`~/.plantool/prompts/<stage>.md` overrides a template.

The stage skills live in `skills/{plantool,research,plan,implement}/SKILL.md` and are embedded in
the CLI. `plantool skill <name>` prints one, so no agent needs anything installed; `plantool skill
install` is an explicit opt-in that writes them as slash commands under `~/.claude/skills` or
`~/.codex/skills`.

A run has a permission mode (`ask`, `accept-edits`, `auto`, `allow-all`), chosen when the run
starts and changeable from the run panel (`POST …/runs/:id/input` with `permission_mode`). Claude
gets it as `--permission-mode` (plus `--allow-dangerously-skip-permissions` so it can be switched
mid-run with `set_permission_mode`); `allow-all` also makes the daemon answer any remaining
`can_use_tool` request with allow. Codex gets it as `approvalPolicy` and the sandbox policy on the
next `turn/start`.

Implementation runs also store `implementation_mode` (`all-at-once` by default, or
`step-by-step`). Step mode adds a one-ticket stop to the implement prompt. The agent leaves the
stage at `implementing` between tickets. On turn completion, the daemon opens or refreshes a
milestone review and watches difftool for human comments. It sends new comment ids to the same
provider run. The sidebar approves a milestone only when the run is idle and difftool has no open
human comments; approval commits the milestone through `commits::run_commit` and sends the next turn. A stopped run can resume its provider session, or
start a fresh step run with its pending review.

## Change review

`changes.rs` prefers `difftool` (`PLANTOOL_DIFFTOOL_PATH`, then `PATH`). The first time it runs
`difftool diff -C <checkout> <base> --design <plan.md> --no-open`, stores the review URL on the
session, and then `difftool open <review>` so difftool's own daemon opens the browser tab. Later
clicks run `difftool refresh <review>` and `open` again. Step mode opens a new review against
`HEAD` for each milestone and refreshes that review during feedback, so committed milestones drop
out of the next diff. The page never navigates to the review
itself, because a navigation from the plantool origin is rejected by difftool as cross-site.
An implement run creates the session's worktree first (`ensure_worktree`, also
`POST …/worktree` and `plantool session worktree`) unless the run says `worktree: false`. Without difftool it launches `git difftool --dir-diff --no-prompt <base>` (retrying
without `--dir-diff`). `git diff --numstat` against the base plus untracked files is always
available for the stat table.

## Security

The daemon binds loopback only. A middleware rejects any request or WebSocket upgrade whose
`Host` is not loopback or whose `Origin`, when present, is not loopback, which blocks DNS
rebinding and cross-site reads. Request bodies are capped at 16 MB. All external commands use
argv, never a shell string.

## Release

`scripts/package-binaries.sh` builds a release binary for a target triple and packages it as
`plantool-<platform>.tar.gz` (zip on Windows). The GitHub workflow runs the tests and the smoke
script, then builds the five platforms and attaches the archives and `SHA256SUMS.txt` to the
release. `plantool update` downloads the matching archive, verifies its checksum against
`SHA256SUMS.txt`, and swaps the binary atomically.

## Design principles

1. One daemon, many sessions, one source of truth; agents and humans share live state.
2. The repo is never touched: documents live in the plantool home.
3. Approval is a server-side rule, not a sentence in a prompt.
4. Never lose a comment or a revision; re-anchor or mark outdated.
5. The CLI is thin, the web is a view, the daemon owns disk and processes.
6. Integrate difftool instead of rebuilding a diff viewer.

## Review state and compatibility

New stored fields have serde defaults. Protocol version 2 adds typed and scoped comments,
approval pins, viewed revisions, activity, proposals, pause rules and milestone history.
Old stage strings remain readable. `core::review` derives thread states and owner blockers;
`core::handoff` derives the step, handoff and primary action for daemon, browser and CLI.
Plan task comparison distinguishes checkbox progress from scope changes. Completed task text
is retained in milestone records even when the live plan changes.

Context mutations use one Where route. Agents propose; browser requests or explicit CLI
confirmation apply after prechecks. Provider context is saved before restart. Scoped milestone
runs record a plan sha and a worktree snapshot made with a separate temporary git index, so
uncommitted earlier phases do not enter the next phase's diff. The user's index is preserved.

Built-in code comments share the session comment store, with a relative path, new-side line
and exact context. Reanchoring follows unique context and marks ambiguous or missing lines
outdated. Difftool counts are read-only. Drift uses the affected-files table and checkbox state;
older plans say drift is not checked.

Zip archives contain a versioned session bundle and readable documents and summary. Extra
revisions and transcripts are opt-in. Import validates paths, checksums and sizes, changes local
repository paths, stops archived runs and assigns collision-safe names. Recorded approval is
restored only by an owner request or explicit CLI confirmation.

The local approval token guards against honest mistakes, not a hostile agent. No multi-user
trust boundary is claimed. Idle run shutdown releases provider processes while retaining their
session identifiers for later feedback or resume.
