# plantool

Research, plan and implement with your coding agent, and review each step in your browser.

plantool gives the research → plan → implement loop a shared surface. You open a **session** for a
piece of work on a repo; the agent writes the research and the plan; you read them in a local web
UI, comment on any line, and watch the agent revise while you read. Only you can approve the plan. This guards against honest mistakes, not a hostile agent.
Once it is approved the agent implements, ticking the plan's checkboxes live, and you review the
changes in [difftool](https://github.com/skshetry/difftool) or your `git difftool`.

- **Any repo, nothing written into it.** Documents live in
  `~/.plantool/sessions/<repo>/<slug>/` (`research.md`, `plan.md`, …), captured with full history.
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
                                    # opens http://127.0.0.1:41200/s/<repo>/fix-login-timeout
                                    # and prints the research prompt to paste into any agent
plantool research fix-login-timeout # or start a hosted research run (claude by default)
plantool plan fix-login-timeout     # then a plan run; comment inline, approve in the browser
plantool implement fix-login-timeout
plantool implement fix-login-timeout --step-by-step
plantool changes fix-login-timeout  # difftool if installed, else git difftool
```

At implementation start, choose **All at once** or **Step by step**. Step by step pauses after each
plan phase and opens the current uncommitted changes for review. With difftool, new human comments
go to the same agent run so it can fix the current milestone. Resolve the comments in difftool,
commit the milestone in the session checkout if you want a separate commit, then choose
**Approve milestone and continue** in the sidebar. The CLI offers the same mode with
`plantool implement <ref> --step-by-step`.

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

A finished session can be dropped from the bottom of the sidebar (documents, comments and
transcripts are deleted; the git worktree only if you say so). Only the browser can do this.

Steps can be skipped from the sidebar: start planning without research, or skip planning and
approve straight away. In that case the agent first writes a task list to
`plan.md` from the brief and works through it phase by phase, so progress still shows.

## Commands

| Command | What it does |
| --- | --- |
| `plantool new <slug> [--brief <text> \| --brief-file <path>] [--repo <path>] [--worktree] [--base <branch>] [--mirror]` | Create or reopen a session for the git checkout containing the current directory, and print the prompt for the next stage |
| `plantool list [--repo .] [--stage <s>]` | The inbox. The browser's list page has a **New session** button with your recent repositories to pick from |
| `plantool open <ref>` | Open a session in the browser |
| `plantool research\|plan\|implement <ref> [--provider claude\|codex\|copilot\|ollama] [--model <m>] [--permission ask\|accept-edits\|auto\|allow-all] [--no-worktree] [--resume <run\|last>] [--step-by-step]` | Start a hosted run; implement runs work in a git worktree unless told otherwise; `--resume` continues an earlier run's provider session with its context. Ollama requires `--model`. |
| `plantool critique <ref>` | Have an agent critically review the research or plan and post findings as anchored comments (also the "Review with agent" button) |
| `plantool changes <ref>` | Open the change review in difftool or `git difftool` |
| `plantool status [stale\|<stage>\|<text>]` | Progress from the plan's checkboxes |
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
```

## Configuration

| Variable | Purpose | Default |
| --- | --- | --- |
| `PLANTOOL_HOME` | sessions, prompts and daemon state | `~/.plantool` |
| `PLANTOOL_PORT` | daemon port (loopback only) | `41200` |
| `PLANTOOL_AUTHOR` | author name on agent CLI comments | `agent` |
| `PLANTOOL_PROVIDER` | default hosted-run provider | `claude` |
| `PLANTOOL_DIFFTOOL_PATH`, `PLANTOOL_CLAUDE_PATH`, `PLANTOOL_CODEX_PATH` | explicit executables | found on `PATH` |
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
Approval pins the plan revision. Task additions, removals, rewording, unticking, and phase
renaming require acceptance of the revised plan before another build run. Checkbox progress
and other edits show a revision banner. Compare with the approved or last-viewed revision.

Each named plan phase has its own run and diff window. Choose the next phase in the run
dialog or `plantool implement <session> --milestone "Phase 2: UI"`. All-at-once proceeds
through phases without pauses. Checkpoints pause after each milestone or when an agent-judged
plain-language rule applies. Change the rule between milestones in Where or with
`plantool session set --session <ref> --pause-rule "Pause before an API change" --confirm`.
The agent signals a checkpoint with `plantool session pause --session <ref> --reason "…"`.

Where changes workspace, branch, base, and diff tool. Agents use `session propose`; owners
apply proposals in the browser or use `session set --confirm`. A context switch checks dirty
files, refs, pending milestone approval, and unanswered requests before stopping and resuming
a live run. `plantool settings difftool built-in --confirm` sets the global default. A session
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

## Portable sessions

`plantool export <ref> -o session.zip` includes documents, comments, decisions, a readable
summary, and pinned approved and last-viewed revisions. Add `--transcripts` or `--revisions`
to include more history, and `--note "…"` for a note in the summary.
`plantool import session.zip --repo /local/repo [--workspace /local/worktree]` rewrites local
paths and adds a suffix when the session name already exists. Imported plans need owner review;
`--confirm` explicitly restores recorded approvals. The browser offers the same zip actions.

Approval controls guard against honest mistakes, not a hostile agent with access to local
files or the browser's local token. Plantool is a personal, local tool.

If the daemon cannot bind its port, run `plantool daemon status` and check which process owns
it, or start plantool with `PLANTOOL_PORT` set to a free local port. The browser reconnects
and reloads state after a dropped connection. After a restart, stopped agent runs offer Resume.
Copilot does not expose a sign-in status command: setup reports that sign-in is unverified
and offers `copilot login`. Claude and Codex use their CLI status commands with bounded checks.
