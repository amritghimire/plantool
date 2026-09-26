# plantool

Research, plan and implement with your coding agent, and review each step in your browser.

plantool gives the research → plan → implement loop a shared surface. You open a **session** for a
piece of work on a repo; the agent writes the research and the plan; you read them in a local web
UI, comment on any line, and watch the agent revise while you read. Only you can approve the plan.
Once it is approved the agent implements, ticking the plan's checkboxes live, and you review the
changes in [difftool](https://github.com/amritghimire/difftool) or your `git difftool`.

- **Any repo, nothing written into it.** Documents live in
  `~/.plantool/sessions/<repo>/<slug>/` (`research.md`, `plan.md`, …), captured with full history.
- **Inline comments on documents.** Click a paragraph, a list item or a source line. Comments follow
  their line when the file changes and are marked *outdated* instead of lost.
- **Agents drive it from the CLI.** `plantool session …` reads the board, writes comments, waits
  for your reply and moves the stage forward. `plantool skill` prints the instructions.
- **Hosted runs.** Start Claude Code or Codex from the browser; the transcript, tool activity and
  permission prompts stream into the page. Or run your agent in a terminal and let it use the CLI.
- **Human-only approval.** `approved` and `done` can only be set from the browser. There is no CLI
  command for them.
- One self-contained binary, no Node at runtime.

```bash
cd your-repo
plantool new fix-login-timeout      # opens http://127.0.0.1:41200/s/<repo>/fix-login-timeout
plantool research fix-login-timeout # start a hosted research run (claude by default)
plantool plan fix-login-timeout     # then a plan run; comment inline, approve in the browser
plantool implement fix-login-timeout
plantool changes fix-login-timeout  # difftool if installed, else git difftool
```

## Install

Download the archive for your platform from the latest release, extract the `plantool` binary and
put it on your `PATH`:

```bash
tar -xzf plantool-darwin-arm64.tar.gz -C ~/.local/bin
plantool --version
```

On macOS you may need to clear the quarantine flag: `xattr -d com.apple.quarantine ~/.local/bin/plantool`.
Upgrade later with `plantool update`, which verifies the release checksum before swapping the binary.

For hosted runs you need `claude` (Claude Code 2.1+) and/or `codex` on your `PATH`. difftool is
optional and used for the change review when present.

## How a session moves

```
new → researching → research-review → planning → plan-review → approved → implementing → implementation-review → done
```

The agent moves the stage forward up to `plan-review`, and from `approved` on. You set `approved`
and `done`. Writing `research.md` or `plan.md` advances the stage automatically.

## Commands

| Command | What it does |
| --- | --- |
| `plantool new <slug> [--repo <path>] [--worktree] [--base <branch>] [--mirror]` | Create or reopen a session for the git checkout containing the current directory |
| `plantool list [--repo .] [--stage <s>]` | The inbox |
| `plantool open <ref>` | Open a session in the browser |
| `plantool research\|plan\|implement <ref> [--provider claude\|codex] [--model <m>]` | Start a hosted run |
| `plantool changes <ref>` | Open the change review in difftool or `git difftool` |
| `plantool status [stale\|<stage>\|<text>]` | Progress from the plan's checkboxes |
| `plantool skill` | Print the agent instructions |
| `plantool daemon start\|stop\|status`, `plantool serve` | The background daemon |
| `plantool update` | Install the latest release |

`<ref>` is the slug when it is unique, otherwise `<repo_slug>/<slug>`.

### For agents

```bash
plantool session get --session <ref> --json
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
