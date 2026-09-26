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
   **If there is no plan**, the human skipped planning. Write the tickets first (below).
3. Work in the session's checkout: `session.worktree` when set, else `session.repo.root`. If there
   is no worktree and you would rather not touch the user's checkout, run
   `plantool session worktree --session <ref>` first; it creates `.claude/worktrees/<slug>` off
   the base branch and prints the path. Hosted implement runs do this automatically.
4. `plantool session stage --session <ref> --set implementing`.

## No plan? Write the tickets first

When the human approved without a plan, turn the brief into a ticket list before touching code,
so the browser shows what you are about to do and tracks it. Write this to the plan path:

```markdown
# Tickets: <title>

## Summary
One or two lines: what "done" looks like, from the brief.

## Todo
### Phase 1: <name>
- [ ] <ticket: one concrete, verifiable step>
- [ ] …
```

Tickets are todo items, not a plan: no architecture essay, no alternatives. Each ticket is a step
you can finish and verify on its own, and the list is the whole job. Post a comment on the first
ticket if a decision is needed, pick the option you recommend, and keep going; the human sees the
list and can comment while you work. Keep the file current: add a ticket when work surfaces one,
and tick each one as you finish it.

## Rules

- Follow the plan. If it turns out to be wrong, stop, update the plan file with what you
  learned, and say so, rather than patching around it.
- Tick each task in the plan file as you finish it: change `- [ ]` to `- [x]`. The browser
  shows the progress live. Do not tick tasks you did not do.
- Follow the codebase's existing patterns, style and naming. No unnecessary comments.
- Run the project's typecheck, lint and tests as you go, and once more at the end.
- Do not stop until every task is done or blocked; say clearly what is blocked and why.
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
   --context`), act on them, reply, resolve, and `watch` until the human accepts.
