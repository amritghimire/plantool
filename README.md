# plantool

Research, plan and implement with your coding agent, and review each step in your browser.

plantool gives the research → plan → implement loop a shared surface. You open a **session** for a
piece of work on a repo; the agent writes the research and the plan; you read them in a local web
UI, comment on any line, and watch the agent revise while you read. Only you can approve the plan.
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
- **Hosted runs.** Start Claude Code or Codex from the browser; the transcript, tool activity and
  permission prompts stream into the page. Or run your agent in a terminal and let it use the CLI.
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
plantool changes fix-login-timeout  # difftool if installed, else git difftool
```

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

For hosted runs you need `claude` (Claude Code 2.1+) and/or `codex` on your `PATH`. difftool is
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
approve straight away. In that case the agent first writes a ticket list (todo items) to
`plan.md` from the brief and works through it ticket by ticket, so progress still shows.

## Commands

| Command | What it does |
| --- | --- |
| `plantool new <slug> [--brief <text> \| --brief-file <path>] [--repo <path>] [--worktree] [--base <branch>] [--mirror]` | Create or reopen a session for the git checkout containing the current directory, and print the prompt for the next stage |
| `plantool list [--repo .] [--stage <s>]` | The inbox. The browser's list page has a **New session** button with your recent repositories to pick from |
| `plantool open <ref>` | Open a session in the browser |
| `plantool research\|plan\|implement <ref> [--provider claude\|codex] [--model <m>] [--permission ask\|accept-edits\|auto\|allow-all] [--no-worktree] [--resume <run\|last>]` | Start a hosted run; implement runs work in a git worktree unless told otherwise; `--resume` continues an earlier run's provider session with its context |
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
