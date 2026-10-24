# plantool

Research, plan and implement with your coding agent, and review each step in your browser.

plantool gives the research → plan → implement loop a shared surface. You open a **session** for a
piece of work on a repo; the agent writes the research and the plan; you read them in a local web
UI, comment on any line, and watch the agent revise while you read. Only you can approve the plan. This guards against honest mistakes, not a hostile agent.
Once it is approved the agent implements, reports task completion, and you review the
changes in [difftool](https://github.com/skshetry/difftool) or your `git difftool`.

- **Any repo, nothing written into it.** Documents live in
  `~/.plantool/sessions/<repo>/<slug>/docs/` (`research.md`, `plan.md`, …), captured with full history.
- **Inline comments on documents.** Click a paragraph, a list item or a source line. Comments follow
  their line when the file changes and are marked *outdated* instead of lost.
- **Read it as slides.** The Slides view shows one heading per slide with an outline, keyboard
  navigation and fullscreen; Mermaid flowcharts, sequence diagrams and charts render inline and
  take comments like any other block. The shipped skills ask the agent to write for that: one idea
  per section, a diagram for any flow, a table for components.
- **Agents drive it from the CLI.** `plantool session …` reads the board, writes comments, waits
  for your reply and moves the stage forward. `plantool skill` prints the instructions, and
  `plantool skill research|plan|implement` prints the stage skills, so any agent with a shell
  works without installing anything.
- **A brief per session.** `plantool new <slug> --brief "…"` (or edit it in the sidebar) says what
  to research or build. It is rendered into every stage prompt, and the browser shows the prompt to
  paste with a copy button while a document is still missing.
- **Hosted runs.** Start Claude Code, Codex, GitHub Copilot CLI, or OpenCode with a local Ollama model from the browser. The transcript streams into the page; Claude Code and Codex also show permission prompts. Or run your agent in a terminal and let it use the CLI.
- **Human-only approval.** `approved` and `done` can only be set from the browser. There is no CLI
  command for them.
- One self-contained binary, no Node at runtime.

```bash
cd your-repo
plantool new fix-login-timeout --brief "Sessions expire after 5 minutes; see issue #42"
                                    # opens http://localhost:41200/s/<repo>/fix-login-timeout
                                    # and prints the research prompt to paste into any agent
plantool research fix-login-timeout # or start a hosted research run (claude by default)
plantool plan fix-login-timeout     # then a plan run; comment inline, approve in the browser
plantool implement fix-login-timeout
plantool changes fix-login-timeout  # difftool if installed, else git difftool
```

At implementation start, choose **All at once** or **Step by step**. Step by step pauses after each
plan phase and opens the current uncommitted changes for review. With difftool, new human comments
go to the same agent run so it can fix the current milestone. Resolve the comments in difftool,
commit the milestone in the session checkout if you want a separate commit, then choose
**Approve milestone and continue** in the sidebar. Choose elevated permissions, milestone scope and Step by step in the browser with passkey confirmation; CLI run starts use Ask permissions.

## Install

macOS and Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/amritghimire/plantool/main/install.sh | sh
```

It picks the right archive for your platform, verifies it against the release's
`SHA256SUMS.txt`, installs to `~/.local/bin` (or `/usr/local/bin` when that is on your `PATH`
and writable), and clears the macOS quarantine flag. Set `PLANTOOL_INSTALL_DIR` to choose the
directory and `PLANTOOL_VERSION` to pin a tag.

Homebrew:

```bash
brew tap amritghimire/plantool https://github.com/amritghimire/plantool
brew install plantool
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/amritghimire/plantool/main/install.ps1 | iex
```

Or download the archive for your platform from the
[releases page](https://github.com/amritghimire/plantool/releases), extract the `plantool`
binary and put it on your `PATH`. Upgrade later with `plantool update`, which verifies the
release checksum before swapping the binary.

For hosted runs you need `claude` (Claude Code 2.1+), `codex`, and/or `copilot` on your `PATH`. For Ollama runs, start Ollama, install `opencode`, and pull a model. The model picker reads the installed models from Ollama. Choose Auto or Allow all permissions for Ollama stage runs so OpenCode can run the session commands. Ask agent can attach files up to 10 MB each and lets you choose a live run, resume a stopped run, or start a new one with a model and reasoning effort. Copilot and OpenCode run in programmatic mode; tool approval prompts are unavailable in those modes. difftool is
optional and used for the change review when present.

## How a session moves

```
new → researching → research-review → planning → plan-review → approved → implementing → implementation-review → done
```

The agent moves the stage forward up to `plan-review`, and from `approved` on. You set `approved`
and `done`. Writing `research.md` or `plan.md` advances the stage automatically.

A finished session can be dropped from the bottom of the sidebar. It disappears from the board;
permanent revision, approval, audit and run-summary records remain in SQLite. Only the browser can do this.
Unpinned transcripts follow the retention policy; explicit worktree removal still requires confirmation.

Steps can be skipped from the sidebar: start planning without research, or skip planning and
approve straight away. In that case the agent first writes a task list to
`plan.md` from the brief with stable task keys, then waits for you to approve that revision
before reporting progress or implementing it.

## Commands

| Command | What it does |
| --- | --- |
| `plantool new <slug> [--brief <text> \| --brief-file <path>] [--repo <path>] [--worktree] [--base <branch>] [--mirror]` | Create or reopen a session for the git checkout containing the current directory, and print the prompt for the next stage |
| `plantool list [--repo .] [--stage <s>]` | The inbox. The browser's list page has a **New session** button with your recent repositories to pick from |
| `plantool open <ref>` | Open a session in the browser |
| `plantool research\|plan\|implement <ref> [--provider claude\|codex] [--model <m>] [--no-worktree] [--resume <run\|last>]` | Start a hosted run; implement runs work in a git worktree unless told otherwise; `--resume` continues an earlier run's provider session with its context. Other providers and elevated permissions are started in the browser. |
| `plantool critique <ref>` | Have an agent critically review the research or plan and post findings as anchored comments (also the "Review with agent" button) |
| `plantool changes <ref>` | Open the change review in difftool or `git difftool` |
| `plantool status [stale\|<stage>\|<text>]` | Approved plan progress |
| `plantool task done <ref> T3` | Report completion of a task in the approved plan |
| `plantool skill [research\|plan\|implement]` | Print the agent instructions, or one stage skill, for any agent to read inline |
| `plantool skill install [--agent claude\|codex] [--dir <d>] [--force]` | Optional: write the skills as slash commands (`/plantool-research <ref>` …). Nothing is installed unless you ask |
| `plantool daemon start\|stop\|restart\|status`, `plantool serve` | The background daemon; `restart` picks up an upgraded binary and refuses while runs are live unless `--force` |
| `plantool update` | Install the latest release |

`<ref>` is the slug when it is unique, otherwise `<repo_slug>/<slug>`.

### For agents

```bash
plantool session get --session <ref> --json
plantool session brief --session <ref> [--set "…" | --file <path>]     # what the user wants
plantool session prompt --session <ref> [--stage research|plan|implement|next]
plantool session worktree --session <ref>                              # create and print the worktree
plantool session doc path --session <ref> --kind plan                  # the file to write
plantool session comment list --session <ref> --kind human --unresolved --context
plantool session comment add --session <ref> --kind plan --match "<line text>" --body "…"
plantool session comment apply --session <ref> --input batch.json
plantool session watch --session <ref> --since <seq> --timeout 600     # block until you act
plantool session goto --session <ref> --kind plan --match "<line text>"
plantool session stage --session <ref> --set planning
plantool task done <ref> T3                                          # approved task key
```

## Configuration

| Variable | Purpose | Default |
| --- | --- | --- |
| `PLANTOOL_HOME` | sessions, prompts and daemon state | `~/.plantool` |
| `PLANTOOL_PORT` | daemon port (loopback only) | `41200` |
| `PLANTOOL_AUTHOR` | author name on agent CLI comments | `agent` |
| `PLANTOOL_PROVIDER` | default hosted-run provider | `claude` |
| `PLANTOOL_DIFFTOOL_PATH`, `PLANTOOL_CLAUDE_PATH`, `PLANTOOL_CODEX_PATH` | explicit executables | found on `PATH` |
| `PLANTOOL_RUN_TOKEN` | per-attempt hosted-agent credential (injected by daemon; never a human login) | unset for external Personal executors |
| `PLANTOOL_LOG` | daemon log filter | `info` |

Hosted-run prompts can be overridden per stage in `~/.plantool/prompts/{research,plan,implement}.md`.
Placeholders: `{slug}`, `{title}`, `{key}`, `{checkout}`, `{session_dir}`, `{research_path}`,
`{plan_path}`, `{extra}`.

Session worktrees go to `<repo>/.worktrees/<slug>` unless `--worktree-dir` / `--dir` says otherwise.
To change the default, set the git config key `plantool.worktreeDir`, per repo or with `--global`.
It takes `{repo}` (the main checkout's folder name) and `{slug}` (appended when missing); relative
paths start at the main checkout and `~/` is your home directory. It is read each time a worktree is
created, so the daemon does not need a restart. The **Settings** button on the session list edits the
same key, globally or for one repository, and previews the resulting path.

```bash
git config --global plantool.worktreeDir '../{repo}-worktrees/{slug}'   # sibling folder
git config plantool.worktreeDir '~/worktrees/{repo}/{slug}'             # this repo only
```

## Develop

Browser login and approvals use passkeys. Run `plantool setup` once and enter its
one-time code at `http://localhost:41200/setup` (use your `PLANTOOL_PORT` when set).
The code expires after 10 minutes. Additional passkeys require confirmation with
an existing passkey through **Add passkey**. Login sessions expire after 12 hours
of inactivity; approvals and security changes require a fresh passkey confirmation.
Use the daemon's `localhost` URL for passkey testing, including when running Vite.

Nix is optional. Install it with the [Determinate Systems installer](https://github.com/DeterminateSystems/nix-installer)
(`curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install`),
then run `nix develop` from the repository root. The shell provides stable Rust
(via rust-overlay, including rustfmt, clippy and rust-src), Node 22, Python 3.12,
uv, SQLite, gh and cargo-nextest on Linux and macOS. Run `nix flake check` to
validate the shell; CI runs the same check. Use `nix develop -c <command>` for
agent build and test commands on PlanTool itself.

Without Nix, keep using rustup's stable toolchain, Node 22/npm and uv as usual;
none of the commands below require Nix. Rust builds use vendored OpenSSL for
portable binaries and need Perl plus a C build toolchain; the Nix shell supplies
these build tools.
Python is not required for Personal mode.
For dev daemon runs, set `PLANTOOL_HOME=$(mktemp -d) PLANTOOL_PORT=41300` to
keep development data separate from your installed tool.

```bash
(cd web && npm ci && npm run build)   # the daemon embeds web/dist
cargo build && cargo test
scripts/smoke.sh                      # end-to-end against target/debug/plantool
(cd web && npm run dev)               # Vite on :5273, proxying /api to a daemon on :41200
```

`scripts/package-binaries.sh [target]` builds and packages a release archive; CI runs it for every
platform and attaches the archives plus `SHA256SUMS.txt` to GitHub Releases.

See [ARCHITECTURE.md](ARCHITECTURE.md) for how the pieces fit.

## Review and resume

A **Session** holds the brief, documents and review history. Research, Plan, Build, Review,
and Done are its five **Steps**. A **Phase** groups checkbox **Tasks** in a plan; when it is
built and reviewed it becomes a **Milestone**. An **Agent run** is one provider process.
Stored stage names and old CLI commands remain compatible.

Comment on a line, section, or whole document. Choose note, blocker, question, suggestion,
or change approach. An agent can propose a resolution; the owner resolves the thread.
Open owner blockers stop ordinary plan approval. A recorded reason allows an override.
Approval pins the immutable plan revision and its SHA-256 hash. Tasks use stable keys such as
`- [ ] T3: Add the endpoint`. Agents call `plantool task done <ref> T3`; progress lives separately
in SQLite and the browser overlays it on the approved revision. Checkbox-only draft edits are
ignored. Task additions, removals, rewording and phase renaming need a new plan approval before
building continues. Compare the current draft with the approved or last-viewed revision.

Each named plan phase has its own run and diff window. Choose the next phase in the run
dialog or `plantool implement <session> --milestone "Phase 2: UI"`. All-at-once proceeds
through phases without pauses. Checkpoints pause after each milestone or when an agent-judged
plain-language rule applies. Change the rule between milestones in Where with a fresh passkey confirmation.
The agent signals a checkpoint with `plantool session pause --session <ref> --reason "…"`.

Where changes workspace, branch, base, and diff tool. Agents use `session propose`; owners
apply proposals in the browser with a fresh passkey confirmation. A context switch checks dirty
files, refs, pending milestone approval, and unanswered requests before stopping and resuming
a live run. Settings in the browser set the global diff tool default. A session
can override it. Missing tools leave the built-in diff available, including code-line comments.
The drift panel compares changed paths with the plan's affected-files table. It is a heuristic,
not proof that checked tasks are complete. Difftool comment counts are read-only; resolve those
comments in difftool.

Idle hosted runs stop after `plantool.idleMinutes` (30 by default; 0 disables this) and keep
provider context. Owner feedback resumes an idle-stopped run. Stopped runs have a Resume action.
Reconnect reloads state; a disconnected tab says its view may be stale. Activity and review
markers help you return after a break. Setup checks Claude and Codex sign-in; Copilot has no
noninteractive status command, so its sign-in status is shown as unverified unless a token
is available. Use the setup command and Re-check, or copy the prompt into a terminal agent.

## Storage and upgrading to v0.1

Personal mode runs entirely in the Rust daemon: no Python, worker or GitHub App is required.
SQLite at `$PLANTOOL_HOME/db/plantool.db` owns sessions, comments, approvals, task progress,
settings and runs. Connections use WAL, foreign keys and a 5-second busy timeout. Immutable
revisions, evidence and NDJSON transcript chunks live at `artifacts/<sha256>`; live drafts live
under `sessions/<repo>/<slug>/docs/`. The daemon publishes files before committing their rows
and sweeps unreferenced files older than one hour at startup. Do not edit the database directly.
Prompt override files and Git configuration remain live owner inputs, with snapshots in SQLite.

The first v0.1 start moves an old-layout home to the sibling
`<home>.v0-backup-<unix-ts>` (normally `~/.plantool.v0-backup-<unix-ts>`) and starts fresh.
Nothing is imported. Run `plantool setup` again to enroll a fresh passkey, then sign in to see
the backup banner; dismissing it persists across restarts. Keep the backup for reference.
v0 session archives are rejected; v0.1 archives can still be exported and restored below.

Retention runs at startup and daily. Transcripts/traces remain for 30 days after an item
closes; local evidence remains for 90 days after merge, or forever for failed/recovery items.
Pin a run in its run panel to keep it forever. Audit, decisions, gates, revisions, workspace
messages and run summaries are permanent. Clean owned worktrees and local branches are removed
after merge or decline; dirty or adopted worktrees are kept with a warning.
Settings → Retention changes the periods and default 20 GiB disk cap with fresh passkey
confirmation. Over the cap, oldest unpinned inactive transcripts are removed first; protected
records can leave usage above the cap. Pinning/unpinning also requires fresh confirmation.

## Portable sessions

`plantool export <ref> -o session.zip` includes documents, comments, decisions, a readable
summary, and pinned approved and last-viewed revisions. Add `--transcripts` or `--revisions`
to include more history, and `--note "…"` for a note in the summary.
`plantool import session.zip --repo /local/repo [--workspace /local/worktree]` rewrites local
paths and adds a suffix when the session name already exists. Imported plans need owner review;
Restoring recorded approvals requires a signed-in browser and a fresh passkey
confirmation using **Restore recorded approvals** in the browser import panel. CLI imports always require a new plan review; `--confirm` is no longer accepted.

Browser approvals require a verified passkey. Other local agent APIs remain available
without a browser session; process and filesystem isolation are separate groundwork.

If the daemon cannot bind its port, run `plantool daemon status` and check which process owns
it, or start plantool with `PLANTOOL_PORT` set to a free local port. The browser reconnects
and reloads state after a dropped connection. After a restart, stopped agent runs offer Resume.
Copilot does not expose a sign-in status command: setup reports that sign-in is unverified
and offers `copilot login`. Claude and Codex use their CLI status commands with bounded checks.

### Agent identity and local transport

The CLI always acts as an agent. On Unix it connects to `$PLANTOOL_HOME/run/daemon.sock`
(default `~/.plantool/run/daemon.sock`), with a private `0700` run directory and `0600`
socket. The browser continues using the loopback HTTP port. Windows uses loopback HTTP
for the CLI; Unix-socket code is compiled only on Unix.

Hosted providers receive a fresh `PLANTOOL_RUN_TOKEN` and the daemon's home/port on each
run, including resumes. Tokens live only in memory, are scoped to that repo/session/run,
and are revoked on provider exit, launch failure, cancellation, or daemon restart.
They cannot access another session or run, approve, change owner policy, create nested
runs, or stop/restart the daemon. Existing watch streams close when their token is revoked.
A supplied invalid token is rejected rather than treated as anonymous. The token is sent
in a header, never in a URL or prompt.

External Personal executors without a token retain agent actions such as writing documents,
posting comments, proposing context changes, and advancing stages. `session set` and
`session propose` only propose; apply changes in **Where**. Change global diff settings
in browser **Settings** and restore archived approvals in the browser import panel.
There are no CLI approval/confirmation commands. `export` stays available. Owner-terminal
`setup` remains the one-time passkey bootstrap; hosted/sandbox-marked processes cannot run
it or daemon lifecycle commands. The actual sandbox is a separate Phase 0 task.

The worker identity primitive passes a random token through inherited fd 0, never a token
environment variable or command-line argument. Worker credentials have no Personal or
browser authority; only socket health is available until the Phase 2 contract API exists.

### Sandboxed runs and Stop All

On macOS, enable **Sandbox this run** when starting a Personal run. The daemon
launches the provider through `sandbox-exec`, with a separate temporary directory
and a stripped environment. Linux and Windows return an explicit unsupported
error for isolated runs; ordinary Personal runs remain available.

Sandboxed agents cannot read SSH/GitHub credentials, browser cookies, Keychain,
daemon secrets, or never-sent files (`.env*`, `*.pem`, `*.key`, `secrets/` at any
depth). Reviewers also cannot write the worktree. Provider state is temporary;
known model-auth files are copied into the attempt, while Keychain-based provider
login is unavailable inside the sandbox. Use a provider's supported file/API-key
login. Network access remains enabled; the later LLM proxy is outside this phase.

Use **Stop All** in the browser to revoke run tokens, kill supervised process
groups and latch execution off, including across daemon restarts. **Resume
execution with passkey** requires a fresh assertion and never restarts old runs.
In Settings → Never-sent files, add project-relative globs (`*`, `**`, `?`);
stop execution before saving, then approve the change with your passkey.
These owner-only rules live outside the repository. Defaults cannot be removed.

`cargo test -p plantool --test personal_no_python` exercises the Personal CLI and browser
approval flow with Python, uv, gh and workers absent from the daemon’s PATH. The plan and
implement instructions are in `agents/skills/{plan,implement}/SKILL.md`.

`cargo test -p plantool-daemon --test sandbox` executes the section 19 sandbox
acceptance tests on macOS, including actual file denials and Keychain Mach-service
denials; CI runs them on macOS. The test file is excluded on other platforms.

## Engineering OS worker bridge

Engineering OS requires macOS and `sandbox-exec`. Its checkpointed work-item graph runs
roles through owner plan/PR gates; see the work-item agents section below.
Personal remains the Rust engine and never downloads or starts Python. Both modes share a single
model lease, so a hosted coding run or LLM request waits while another holds it. Coding runs
checkpoint after at most 45 minutes; model changes happen between attempts.

Open **Engineering OS → Guided setup** for the in-app, one-step-at-a-time guide. It saves
your step and selected project on the daemon, so you can close the browser or restart and
resume. Saved configuration is retained; unfinished form inputs must be entered again.
For worker repair, open **Settings → Worker** and choose **Repair with passkey**. The binary unpacks its embedded worker and agent instructions under
`$PLANTOOL_HOME/worker/<version>/`, downloads checksum-verified uv **0.12.23**, and runs
`uv sync --frozen --no-dev --python 3.12`. Python installs under the same home, not system-wide.
The worker runs sandboxed on `run/worker.sock`, receives its token through an inherited descriptor,
and owns only `worker-state/checkpoints.db`. Both sides check contract and graph versions;
unknown/older checkpoint versions require a migration and are not resumed. Linux and Windows
show **Engineering OS unavailable**, while Personal continues to work.

**Settings → Model routes and providers** edits the embedded `agents/chains.default.yaml`
defaults and each project's provider allowlist. Use Stop All before changing policy, save with a
fresh passkey, resume execution, then **Verify routes with passkey**. Setup checks live catalogs
without making model calls. Missing model IDs are marked unavailable; explicit IDs are never
substituted. `auto` routes resolve from the advertised catalog and display their chosen model.
QA/Reviewer prefer a different model family; an unavoidable match is recorded in the result's
`review_guide.same_family_warning`.

Ollama local and cloud routes both connect through the installed local Ollama service
(default port 11434; `OLLAMA_HOST` must be loopback). Run `ollama signin` on the daemon host
for cloud access, pull the desired model references, then verify routes in Settings.
PlanTool sends no Ollama API key and never reads Ollama's login files. The old
`plantool/ollama-cloud` Keychain entry is unused; remove it manually if desired.

Cloud references are discovered from `/api/tags` and remain the remote `ollama-cloud`
provider for allowlist, quota and audit purposes. Local-only `ollama` routes reject cloud
models and renamed remote aliases before sending prompts. Model chains and fallback order
are unchanged. See [native provider setup](docs/ollama-native-transport.md).

OpenRouter and OpenCode Zen API keys are read only by the daemon from macOS Keychain services
`plantool/openrouter` and `plantool/opencode-zen`. Add these using Keychain Access;
no provider keys belong in worker/agent environment variables or chain YAML.
Codex retains its own subscription auth file and uses the existing app-server adapter under the
lease. Engineering OS OpenCode uses a custom provider pointing at the daemon's OpenAI-compatible
proxy, with a scoped run token. It does not inherit legacy OpenCode provider credentials.
The proxy screens never-sent files, checks the lease/provider policy, counts requests/tokens,
and records quota resets. OpenRouter's default daily reserve protects Backend, Frontend and
Reviewer capacity. A quota response checkpoints the current attempt; its next attempt selects a
lower route. Higher routes become eligible again when their reset window passes.

The daemon commits attempts and outbox entries together; worker deliveries retry with the same
idempotency key. Results require matching item/role/generation, existing commits and evidence,
and approved task keys. Agents can inspect/report the result contract:

```bash
plantool attempt schema
plantool attempt report --file result.json   # hosted attempt token required
```

A per-home OS lock prevents a second daemon from changing recovery state.
A launch barrier prevents a provider from executing before its process identity is recorded.
After a crash, uncertain attempts become `recovery_needed`; uncertain model requests are not
replayed. Recovery inspects saved process groups read-only. A surviving agent group fences new
model work; a surviving worker group fences worker repair only. Stop the orphan using your OS
process tools, then choose Repair to recheck the fence. Recovery never kills a reused PID.
Stop All cancels worker graphs durably, revokes tokens, terminates live supervised processes,
and drains the lease. Resuming requires a fresh passkey and does not restart old work.

The shared types live in `crates/contract`; regenerate checked-in schema/Pydantic models with:

```bash
scripts/generate-contract.sh
uv run --project worker --frozen pytest -c worker/pyproject.toml worker/tests
cargo test -p plantool-daemon --test phase2
```

CI regenerates and rejects drift. The tests use fake providers, including worker process death,
replayed dispatch/resume delivery, quota fallback, lease serialization and never-sent screening.
Nix's dev shell uses the same pinned uv version. `nix build` builds the web assets, Rust binary
with crane and worker venv with uv2nix from the same `worker/uv.lock`; `nix run` starts the packaged
CLI. A Nix-built worker uses its packaged Python environment and skips network bootstrap.

### GitHub App setup and plan gates

In Settings → GitHub App setup, follow the browser wizard to create/install the App,
disable webhooks, and grant Issues, Pull requests, Contents and Checks write;
Commit statuses, Actions, Administration and Metadata read. Connect the numeric App and
installation IDs, `owner/repository`, a matching local checkout, and the RSA PEM once.
The daemon stores the key using security-framework in macOS Keychain service
`plantool/github-app`, account `<app-id>`. JWTs use RS256; installation credentials are
cached for at most 59 minutes and expire earlier when GitHub requires it. No App key or
installation token is passed to the worker or agents. Linux has an explicit unavailable
Keychain stub; Personal still works without App setup.

The wizard resolves `amritghimire` to a numeric ID once as the initial approver. Add/remove users
with a fresh passkey; changes are audited and the last approver cannot be removed. Names
are display metadata, never approval authority. Preflight warns about missing required
checks, approving reviews or dismissal of stale reviews. Warnings remain visible after reopening Settings; Refresh preflight with passkey checks the live default branch without re-entering credentials. Label setup records each write intent before the GitHub request and recovers a lost response by looking up the unique label. It links branch settings; add
`plantool/policy` once the later PR workflow actually publishes that check. Finish setup
using the existing worker Repair, route verification, allowed-provider and never-sent
controls. Provider settings apply to both public and private repositories; a visibility
change is audited but never changes them automatically.

The daemon polls every 60 seconds with paginated ETags and durable cursors/inbox receipts.
It rechecks repository numeric identity and visibility, synchronizes issues/comments,
label events (including actors), PR state/reviews and check runs. Only an allowlisted
human label actor counts; App labels are informational. This half-phase records that
information but does not run triage, roles, work-item graphs, pushes, PRs or merges.

A plan gate pins the newest immutable revision, policy version and waiting graph identity.
Each publication is a new App comment with the full hash, a discussion summary and a link
to a read-only PlanTool plan/thread page. The worker gate request may supply `summary` and
`discussion_thread_id`; the thread must belong to that item. Otherwise the page explicitly
has no supplied discussion summary. GitHub is where approvals happen: post exactly
`/plantool approve-plan sha256:<12-or-more-hex-prefix>` or
`/plantool request-changes <reason>` on its own line in the issue comment. Empty leading/trailing lines and GitHub line endings are accepted; prose and multiple commands are rejected. PlanTool refetches the comment and issue, then revalidates the comment immediately before committing,
requires an allowlisted numeric human ID, rejects edits, deletions, comments at/before the
publication, stale revisions/policies and used comment IDs. Decision, gate closure and
idempotent graph-resume outbox entry commit together. Natural language never approves.
Near-miss replies are limited to allowlisted humans, with eligibility rechecked at delivery. Publication recovery precedes command polling; open gates also backfill issue comments so a previously advanced cursor cannot lose an approval posted while the daemon was down.

The parser and verification context also cover sprint, per-item sprint, milestone and
urgent command syntax. Their workflow effects remain deferred to the scoped later
implementations. No PR approval/merge gate is implemented here. After an uncertain GitHub
POST, publication searches all comment pages for its App-authored marker. If it cannot
prove the outcome, it stops for owner inspection instead of risking duplicate comments;
the repository's last sync error appears in Settings. Definite API rejections (for example, missing permission) remain retryable once corrected. Do not delete an uncertain outbox
entry or repeat the publication blindly.

Default GitHub tests use mocks only:

```bash
cargo test -p plantool-daemon --test github
```

The opt-in real playground suite and local macOS instructions are in
[tests/e2e_github/README.md](tests/e2e_github/README.md). It creates and closes a disposable
issue, checks App publication/forged approval rejection/real owner approval/replay, and
leaves comments as evidence. It does not exercise the deferred PR workflow or call models.

### Engineering OS work-item agents

Admitted issues enter read-only PM triage. Fits items wait in backlog until plan preparation;
verified urgent items proceed immediately. Phase 4 supplies sprint scheduling and workspace discussions.
The graph dispatches each role, interrupts and resumes with a daemon-validated result:
CTO research → PM keyed plan → CTO review → GitHub plan gate → approved coding areas →
conditional DevOps → QA → Reviewer security pass → Docs → App PR and current-head PR gate.
In-scope defects return to their owning task's role, with three retries per task and at most
two Supervisor rounds. Replanning creates a new immutable plan/hash; escalation freezes
side effects. **Engineering OS** shows projects, active role/model attempts and owner attention;
open an item for its recorded graph history and GitHub gate links. **Workspace** provides
project channels, item threads, search and browser-local unread markers. Owner replies are
context, never approvals. Global **Stop All** in the shared header affects every project
and Personal run. **Settings** has separate pages for connections, worker, model routes,
project policies and privacy.

Coding attempts use private checkouts. Reviewer is read-only; QA writes tests only. Agent
Git configuration/hooks are protected, and the dispatcher checks commits, scope and owned
artifact evidence. Store test output in `$TMPDIR`, run `plantool attempt artifact --file <log>`,
and cite the returned artifact:// reference in `plantool attempt report --file <result.json>`.
`plantool task done <item> T1` records approved task progress. Docs may propose scoped README/CHANGELOG updates; the daemon applies them and reruns
QA and Reviewer for the resulting head. No role receives App credentials
or authority to publish, approve or merge. Skills now live entirely under `agents/skills/`.

The daemon supplies commit trailers and App transport credentials through an inherited
file descriptor, never a URL/argv/environment value. It builds the PR review guide from
Docs plus recorded models, same-family warnings and filed proposals. `plantool/policy`
checks the current plan/tasks, QA/security review and owner approval for the exact PR head.
A new head resets the gate. Main movement rebases and reruns QA/Reviewer; conflicts return
to implementation. Merge uses `sha=<approved head>` and a permitted repository method;
GitHub enforces branch protection. Refusals and uncertain outcomes pause for owner review.

The graph/contract versions are now 4/5; older checkpoint versions are refused rather
than silently resumed. Personal continues without Python, worker setup or GitHub credentials.
See [the native two-approval playground guide](tests/e2e_github/PHASE3_AGENT.md) for later
installation testing, and [evals](evals/README.md) for five small tasks and offline chain comparison.

### Workspace, sprints and oversight

Engineering OS projects now have #sprint/#general channels and one public thread per item
in Settings → Workspace. Discussion is serial: mentions select the next role; three
contribution turns are followed by a PM conclusion (Supervisor if PM participated). Owner
replies are context only, never approval, and there are no direct messages.

Settings → Project schedule and oversight configures Minimal/Standard/Strict, sprint day,
local time, IANA timezone and review-rule globs using a fresh passkey. Use Stop All and finish
active work before changing policy. Existing null schedules default to Monday 09:00 UTC.
The event-driven project graph never sleeps in a polling node; daemon events resume it.
PM freely selects/sizes prepared plans. The App creates Sprint YYYY-Www with immutable
snapshot comments containing versions, full hashes, estimates and recent 28-day throughput.
On the tracking issue, /plantool approve-sprint approves the latest snapshot; /plantool
approve <issue-number> selects one item. Only approved items are released; unapproved items
return to backlog at rollover. Phase 5 adds triage, scanning and urgent execution below.

Minimal retains plan and exact-head PR gates. Standard suggests editable [review] tags for
risk-matching plans and stops at tagged phases. Editing a pending tag creates a new revision
and App publication, never an approval. Strict additionally requires a separate GitHub
/plantool approve-research sha256:<hash-prefix> decision and stops at every phase. Before
approval, Engineering OS phase headings are normalized to ### for stable task grouping.

At a milestone the App opens a draft PR and posts a commit/compare link. /plantool continue
and /plantool request-changes <reason> apply only to the current stopped head. The App marks
the draft ready after the final stop; current-head PR approval and CI remain required.
Contract/graph versions are now 5/4; incompatible checkpoints fail closed and require repair.
See [the native three-item sprint guide](tests/e2e_github/PHASE4_SPRINT.md) for real verification.


### Continuous project loop (Phase 5)

Intake accepts allowlisted human authors, current `plantool:accept` labels whose latest
issue event was added by an allowlisted human, and daemon-recorded agent findings.
Non-allowlisted comments are excluded from model context; owner comments remain untrusted
context and never count as decisions. An issue can predate project setup.

PM triage is read-only: fits → backlog, decline → reason comment and `plantool:declined`,
duplicate → `Duplicate of #N` and the same decline label. Declined items close after 30 days.
The daily check refetches live comments and labels; an allowlisted comment resets the clock,
label removal disables auto-close, and Settings → Continuous loop → Return to triage uses
a fresh passkey to override a decline. No override approves a plan.

Weekly scans run DevOps → QA → PM with read-only capabilities. Settings → Continuous loop
sets a local scan day/time using the project's timezone; default is 08:00 on the day before
sprint day. The selected time is interpreted strictly before the next sprint. Evidence
includes default-branch CI, dependency alerts, a bounded regular-file/test inventory and
TODO/FIXME matches. API permission failures and truncated inventories are reported as
limitations, never clean scans. Give the App Dependabot-alert read access for private alerts.
PM routes findings to proposed issues (owner accept required), sprint candidates, or backlog,
with the reason recorded in audit and on the latest sprint tracking issue. Review findings
use the same read-only PM routing immediately and are never fixed in the current PR.

An allowlisted `plantool:urgent` label event or exact `/plantool urgent` comment promotes an
item to immediate triage, research/discussion and planning. Its plan must be approved on
its own issue; sprint batch approvals exclude urgent items. Agents can only request
`plantool:urgent-requested`, shown as a notification in Continuous loop settings. Requests
do not admit or promote issues and never interrupt an already running model.

Nix agent environments are off by default and configured per project with a fresh passkey
while stopped. With the option enabled and a regular `flake.nix` in an attempt worktree,
the daemon launches the coding CLI under `nix develop --no-write-lock-file -c`; all its
build/test subprocesses inherit that dev shell. The sandbox and stripped environment remain
in force. Missing Nix fails the opted-in attempt visibly. Without a flake or with the option
off, execution uses the existing setup; rustup/npm/uv and Personal remain usable without Nix.

See [the week-long playground guide](tests/e2e_github/PHASE5_LOOP.md). Mocked Loop tests run
with `cargo test -p plantool-daemon --test phase3 phase5` and
`uv run --project worker --frozen pytest worker/tests/test_loop_graph.py`; they call no models.
