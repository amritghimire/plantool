---
name: plantool
description: Drive a plantool session (research → plan → implement) from the CLI. Read the human's inline comments on the research and plan documents, write and revise those documents, report stage changes, and wait for the human's replies. Use when a plantool session exists for the work, or the user says "plantool", "the plan session", or asks you to act on plan comments.
---

# plantool

plantool shows your research and plan documents in the user's **browser** with inline comments,
a stage stepper, and a live view of your run. You drive it through the `plantool session …` CLI
against a local daemon (`127.0.0.1:41200`, override `PLANTOOL_PORT`). Everything you write shows
up in the user's tab as you write it. **Do not drive the browser.**

**Only the human can approve.** The stages are `new → researching → research-review → planning →
plan-review → approved → implementing → implementation-review → done`. You may move the stage
forward with `session stage --set <stage>` up to `plan-review`, and from `approved` to
`implementing` and `implementation-review`. `approved` and `done` are set from the browser only;
there is no CLI command for them, by design. Do not start implementing until the stage is
`approved`.

**Documents live outside the repo.** Never write to `REVIEWS/`. Ask for the path:

```
plantool session doc path --session <ref> --kind research     # prints the file to write
plantool session doc path --session <ref> --kind plan
```

Write the whole document to that path (the daemon watches it; a capture happens within a second).
Run `plantool session doc touch --session <ref> --kind plan` right after a write if you want the
capture confirmed. The daemon moves the stage to `research-review` / `plan-review` automatically
when the file first appears.

## Refs

Every command takes `--session <ref>`: the `<slug>` when it is unique, else `<repo_slug>/<slug>`.
`plantool list --repo . --json` shows the sessions for the current repo. There is no implicit
current session.

## Quickstart

```
plantool list --repo . --json
plantool session get --session <ref> --json                          # stage, docs, open comments, runs
plantool session doc path --session <ref> --kind plan                # where to write
plantool session comment list --session <ref> --kind human --unresolved --context --json
plantool session comment add --session <ref> --kind plan --match "<line text>" --body "…"
plantool session comment apply --session <ref> --input batch.json    # many comments in one write
plantool session comment resolve --session <ref> --id a,b
plantool session watch --session <ref> --since <seq> --timeout 600   # block until the human acts
plantool session goto --session <ref> --kind plan --match "<line text>"   # scroll their tab
plantool session stage --session <ref> --set planning
```

## Anchoring

A comment points at a line of a document. Prefer `--match "<exact line text>"` over `--line <n>`:
it resolves to the unique line containing that text and fails on 0 or many matches, so it is hard
to get wrong. `--dry-run` prints the resolved anchor without writing. When the document changes,
comments follow their line; if the line is gone they are marked `outdated` and stay visible.

`comment list --context` inlines the lines around each comment so you can triage the board in one
call. `comment context --id <id>` does the same for one comment.

## Modes

- **Act on feedback** (the usual loop in `plan-review`): read `comment list --kind human
  --unresolved --context`, revise the document, then **reply** on each thread with what changed
  (`comment add --parent <id> --body "…"`) and resolve it. Do not add unrelated findings of your
  own while acting.
- **Annotate**: when asked to review a document, leave only comments that help; batch them with
  `comment apply` (JSON `{"comments":[{"doc":"plan","match":"…","body":"…"}]}` on stdin or
  `--input`).
- **Collaborate (live)**: after you finish a revision, arm `watch --since <seq>` (the `seq` from
  your last `comment list --json` or `get --json`) and block until the human comments, resolves, or
  changes the stage. `watch` prints one NDJSON event per line and exits; `--timeout <s>` exits 124
  on silence. Loop: watch → read → revise → reply → watch. Stop looping when the stage becomes
  `approved` (then implement) or the user tells you to stop.

## Implementation

When the stage is `approved`, set it to `implementing`, do the work in the session's checkout
(`session get --json` → `session.worktree` or `session.repo.root`), and tick the plan's
checkboxes (`- [ ]` → `- [x]`) in the plan file as you go; the browser shows the progress live.
When everything is done set the stage to `implementation-review`. The human reviews the changes
in difftool (if installed) or `git difftool`; if a difftool review exists, `session changes
--json` prints its URL and the `difftool review skill` applies to comments there.

## Environment

`PLANTOOL_AUTHOR` names you on your comments (default `agent`). `PLANTOOL_PORT` and
`PLANTOOL_HOME` match the daemon's. All commands accept `--json` for machine-readable output.
