---
name: plantool-implement
description: Implement an approved plantool plan, ticking its checkboxes as you go so the browser shows progress, then hand the changes to the human for review. Use when a plantool session is approved or implementing, or the user asks to implement a session's plan.
user-invocable: true
---

# Implement (plantool)

Session: $ARGUMENTS

You are doing the implementation phase of a plantool session. Run `plantool skill` once for the
CLI reference.

## Before you start

1. Find the session (`plantool list --repo . --json` if the ref is empty) and read
   `plantool session get --session <ref> --json`. The stage must be `approved`, `implementing`
   or `implementation-review`; if it is not, stop and say so. Do not implement an unapproved plan.
2. Read the plan: `plantool session doc get --session <ref> --kind plan`, and the research if
   it exists. The plan file is at `plantool session doc path --session <ref> --kind plan`.
   **If there is no plan**, the human skipped planning. Write the tasks first (below).
3. Work in the session's checkout: `session.worktree` when set, else `session.repo.root`. If there
   is no worktree and you would rather not touch the user's checkout, run
   `plantool session worktree --session <ref>` first; it creates `.worktree/<slug>` (or `git config plantool.worktreeDir`) off
   the base branch and prints the path. Hosted implement runs do this automatically.
4. `plantool session stage --session <ref> --set implementing`.

## No plan? Write the tasks first

When the human approved without a plan, turn the brief into a task list before touching code,
so the browser shows what you are about to do and tracks it. Write this to the plan path:

```markdown
# Tasks: <title>

## Summary
One or two lines: what "done" looks like, from the brief.

## Tasks
### Phase 1: <name>
- [ ] <task: one concrete, verifiable step>
- [ ] …
```

Tasks are checklist items: no architecture essay, no alternatives. Each task is a step
you can finish and verify on its own, and the list is the whole job. Post a comment on the first
task if a decision is needed, pick the option you recommend, and keep going; the human sees the
list and can comment while you work. Keep the file current: add a task when work surfaces one,
and tick each one as you finish it.

## Plan revisions

Before changing task scope during implementation, stop at a safe boundary and run
`plantool session plan propose-revision --session <ref> --reason "…"`.
Then edit `plan.md`. The approval pins the previous revision. Added, removed, reworded or
unticked tasks and renamed phases need the owner's acceptance before another build run.
Checkbox progress and other text edits show a banner and revision diff.

Use `--scope document` for whole-plan feedback or `--scope section --match "## Heading"`
for section feedback. Comments accept `--type blocker|question|suggestion|change-approach`.
Reply with `--proposes-resolve` when you think a thread is addressed; let the owner review it.

## Rules

- Follow the plan. If it turns out to be wrong, stop, update the plan file with what you
  learned, and say so, rather than patching around it.
- Tick each task in the plan file as you finish it: change `- [ ]` to `- [x]`. The browser
  shows the progress live. Do not tick tasks you did not do.
- Follow the codebase's existing patterns, style and naming. No unnecessary comments.
- Run the project's typecheck, lint and tests as you go, and once more at the end.
- In all-at-once mode, finish the scoped phase and end your turn at `implementing`. Plantool starts a new run for the next phase without a pause. Run the full checks after the final phase. For a plan without phases, finish all tasks.
- In step-by-step mode, complete one plan phase per agent run. Tick its tasks and run the relevant checks,
  and pause with the stage at `implementing`, including after the last phase. Plantool opens or
  refreshes the change review. If difftool is available, the run watches its human comments and
  sends them back to you in this same provider session. Read the whole open board with the
  difftool-review skill, make the requested fixes, refresh the review, and reply. Stay on this milestone while feedback is open. The human approves the milestone in plantool after review;
  plantool optionally commits it on the session branch, so do not commit it yourself. Only then start
  the next phase in a new agent run. After the final approval, run
  the full checks and move to `implementation-review`. For hosted runs, finish each turn instead of
  starting a blocking difftool watch; plantool watches difftool and sends new comments to this run.
- Stay on the route the plan drew. Work that turns out to be beyond the plan goes into the
  plan's "Out of scope" section as one line, not into the diff.
- If a decision comes up that the plan does not cover, add it as a comment on the relevant plan
  line (`plantool session comment add --session <ref> --kind plan --match "…" --body "…"`),
  pick the option you recommend so work can continue, and say so in the comment.

## Finish

1. Every checkbox in the plan is `[x]`, or the ones left open are explained in the plan.
2. `plantool session stage --session <ref> --set implementation-review`.
3. Tell the human what changed, what you skipped, and how to review:
   `plantool changes <ref>` opens difftool (if installed) or `git difftool`. If a difftool review
   exists, the `difftool review` skill applies to comments there.
4. Read any comments (`plantool session comment list --session <ref> --kind human --unresolved
   --context`). Answer questions on their threads and leave them open for the human. Address
   change requests, reply with what changed and `--proposes-resolve`; only the owner resolves them. In a hosted run, finish the turn
   after handing off the implementation review; the app can send new feedback into the run.
   When working in a terminal and asked to stay available, use `plantool session watch` for
   further comments until the human accepts.

## Checkpoints and context

Read `session get --json` for the scoped milestone, approved revision, handoff, pause rule,
and context proposals. Build only the phase named in your run prompt.
A plain-language pause rule is an agent-judged checkpoint. When it applies, run
`plantool session pause --session <ref> --reason "…"`, explain why, and finish your turn.
Do not hold a live process while the owner reads slowly; provider context is saved for resume.

To change workspace, branch, base or diff tool, use `plantool session propose` with the
relevant flag and `--reason`. The owner applies it in the browser or through an explicit
`session set --confirm`. After a switch, re-read state and the scoped plan before work.
