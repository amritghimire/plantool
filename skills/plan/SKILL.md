---
name: plantool-plan
description: Write a technical implementation plan for a plantool session and revise it against the human's inline comments until they approve it in the browser. Use when a plantool session is in research-review, planning or plan-review, or the user asks to plan for a session.
user-invocable: true
---

# Plan (plantool)

Session: $ARGUMENTS

You are doing the planning phase of a plantool session. The human reads the plan in their browser,
comments on lines, and approves it there. Only the browser can approve; never ask for approval in
chat and never implement before the stage is `approved`. Run `plantool skill` once for the CLI
reference.

## Before you start

1. Find the session (`plantool list --repo . --json` if the ref is empty) and read
   `plantool session get --session <ref> --json`. The **brief** (`session.brief`) is what the
   user wants. The checkout is `session.worktree` or `session.repo.root`.
2. Read the research if it exists: `plantool session doc get --session <ref> --kind research`.
3. Ask where to write: `plantool session doc path --session <ref> --kind plan`.
4. Pull GitHub context the brief points at (`gh issue view`, `gh pr view`).
5. Move the stage: `plantool session stage --session <ref> --set planning`.

## Plan

Read every source file you intend to change before writing about it. Base the plan on the actual
code, not on assumptions. Name blocking questions instead of guessing: a question is blocking
when its answer is a decision only the human can make, and you can state it sharply now. If you
cannot state it sharply yet, it goes under "Not yet specified" instead. Refer to things by name
(file paths, functions, section titles), not by a bare number or id.

Size the phases so one agent session can finish a phase; a plan larger than a few such phases
should be split into several sessions and say so in the summary.

Write the whole document to the path from step 3. Never write into `REVIEWS/`. The human reads it
as a slide deck (one `##` section per slide) and comments on any block: one idea per section,
about one screen each, bullets and tables over paragraphs, a bold one-line takeaway first. Draw
the change when a diagram makes a flow or service boundary easier to understand. Diagrams render
in the browser and take comments. Shape:

```markdown
# Plan: <title>

## Summary
One to three sentences: what changes, why, and what "done" looks like.

## Blocking questions
Decisions the human must make first, one sharp question each. Leave empty if none.

## Out of scope
What this plan deliberately does not do.

## Not yet specified
Work that is in scope but cannot be pinned down until a blocking question is answered.

## Acceptance criteria
### Must have
- [ ] …
### Must not
- [ ] … does not regress

## Approach
**Takeaway in one line**, then the proposed flow when it needs explanation.
### Affected files
A table: path | what changes | why.
### Steps
Ordered, with short code snippets that match the codebase's patterns.
### Decisions
- Option A vs B → chosen, because …
### Risks
- Risk → mitigation

## Testing
Unit, integration, manual.

## Tasks
### Phase 1: <name>
- [ ] task
### Phase 2: <name>
- [ ] task
```

The Todo checkboxes are the tasks the implementation works through, one at a time, and they
drive the progress bars, so make each one a concrete, verifiable step. For small work the plan
can be just Summary and Todo; drop the other sections rather than padding them.

Before you finish, review your own plan once: unstated assumptions, missing edge cases, whether
it is the simplest approach that follows existing patterns. Fix what you find.

## Review loop

The daemon captures the file and moves the stage to `plan-review`. Then:

0. Post every blocking question as a comment on its own line, so the human answers it in the
   browser: `plantool session comment add --session <ref> --kind plan --match "<question line>" --body "<the options and your recommendation>"`.
   When the human answers, write the decision under "Decisions", turn any "Not yet specified"
   item it unblocks into real steps and tasks, and ask the owner to resolve the thread.
1. `plantool session comment list --session <ref> --kind human --unresolved --context --json`
   (note the `seq`).
2. Answer questions on their threads and leave them open for the human. For requests, revise
   the plan, reply with what changed (`plantool session comment add --session <ref> --parent <id>
   --body "…"`), propose resolution with `--proposes-resolve` and leave resolution to the owner.
3. `plantool session watch --session <ref> --since <seq> --timeout 900` and go back to 1.

Stop looping when the stage becomes `approved` (then the implement skill applies) or the human
tells you to stop.

## Review sections

Include `## Assumptions`, `## Risks`, and `## Decisions needed` before the task list.
Write one decision per item with the recommended choice and the effect of each option.
If no decisions remain, say so. The browser shows these sections as review cards.

Include `### Affected files` with a Markdown table whose first column is `Path`.
Use repository-relative paths in backticks and `dir/*` for a whole directory. The Changes
view uses this table for a heuristic drift check; it does not prove that tasks are complete.
