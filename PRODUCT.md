# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Mostly the author: a developer who hands real work on their own repositories to coding agents (Claude Code, Codex, Copilot CLI, OpenCode with Ollama) and stays the reviewer. Other developers can install and use it, but the main user decides what the tool needs.

They use the browser UI in four ways, all equally important:

- **Long reading sessions:** reading research and plans carefully and commenting line by line.
- **Glance and approve:** a quick check of where a session is, then approve or move on.
- **Watching runs live:** the tab stays open next to a terminal or editor while an agent streams its transcript.
- **Many sessions at once:** several sessions across several repos, handled from the session list.

## Product Purpose

plantool gives the research → plan → implement loop one shared surface for a human and a coding agent. The agent writes the research and the plan; the human reads them in a local web UI, comments on any line, and watches the agent revise. Only the human can approve the plan. After approval the agent implements, ticking the plan's checkboxes live, and the human reviews the changes in difftool or `git difftool`.

Success means the human keeps real control of agent work without babysitting a terminal: they know where every session stands, what is waiting on them, and what changed.

## Positioning

- Works on any repo and writes nothing into it. Documents live in `~/.plantool/sessions/<repo>/<slug>/` with full revision history.
- Inline comments that follow their line when the document changes, and are marked outdated instead of lost.
- Approval is human-only: `approved` and `done` can only be set from the browser, never from the CLI.
- Agents drive it through a plain CLI (`plantool session …`), so any agent with a shell works without installing anything.
- One self-contained binary with the web UI embedded; no Node at runtime.

## Operating Context

- Local only: a daemon on `127.0.0.1:41200`, a React UI in the browser, the CLI in the terminal. Both sides see each other's changes live over a WebSocket.
- A session moves through fixed steps: Research, Plan, Build, Review, Done (stored stages: `new → researching → research-review → planning → plan-review → approved → implementing → implementation-review → done`). Steps can be skipped.
- Documents are Markdown (`research.md`, `plan.md`, `investigation.md`, `quick-fix.md`, `design.md`) with Mermaid diagrams and charts. They can also be read as slides, one heading per slide.
- Hosted runs start an agent from the browser and stream the transcript into the page, including permission prompts for Claude Code and Codex.
- Build can run all at once or step by step, pausing after each plan phase (milestone) for review in difftool before continuing.
- The UI sits next to a terminal, an editor and difftool, often for hours.

## Capabilities and Constraints

- Scope is the session tool (v0.0.16 feature set, with the 0.1.0 UI reconstruction from #35). The shelved Engineering OS work (agent roles, sprints, project workspaces) is out of scope.
- Terms used across UI, CLI and docs: **Session**, **Step** (Research, Plan, Build, Review, Done), **Phase**, **Task**, **Milestone**, **Agent run**, **Brief**, **Workspace** (formerly "Where": workspace path, branch, base, diff tool). Comment kinds: note, blocker, question, suggestion, change approach.
- Open owner blockers stop ordinary plan approval; overriding needs a recorded reason. Approval pins the plan revision, and structural plan changes need re-acceptance.
- Light and dark themes exist (`ThemeToggle`).
- Stack: Vite + React + TypeScript in `web/`, built to `web/dist` and embedded in the Rust binary (`crates/cli`, `crates/daemon`, `crates/core`).
- The approval token guards against honest agent mistakes, not a hostile agent. There is no multi-user model.

## Brand Commitments

- Name: `plantool`, always lowercase.
- Voice: plain, direct English. Short sentences, no hype.

## Evidence on Hand

- README.md and ARCHITECTURE.md describe the product and its flow.
- No testimonials, user counts, logos or endorsements exist. Future work must not invent them.

## Product Principles

1. **The human decides.** Anything that approves, finishes or overrides stays a deliberate human action in the browser.
2. **Show what is waiting on me.** Every view should make clear which sessions need attention and why.
3. **Reading comes first.** Documents are the core of the product; long-form reading and line comments must stay comfortable for hours.
4. **Live, but calm.** Streaming runs and live updates should inform without pulling attention from reading.
5. **Nothing written into the repo.** The tool stays out of the user's project.
