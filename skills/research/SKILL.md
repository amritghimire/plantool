---
name: plantool-research
description: Research a codebase for a plantool session before planning. Reads the area in depth and writes research.md to the session folder, where the human reviews it in the browser. Use when a plantool session is in the new or researching stage, or the user asks to research for a session.
user-invocable: true
---

# Research (plantool)

Session: $ARGUMENTS

You are doing the research phase of a plantool session. The human reads what you write in their
browser and comments on it line by line. Run `plantool skill` once for the CLI reference.

## Before you start

1. Find the session. If the ref above is empty, run `plantool list --repo . --json` and pick the
   session for this work. Then `plantool session get --session <ref> --json`: it has the title,
   the checkout (`session.worktree` or `session.repo.root`), and the **brief** (`session.brief`),
   which is what the user wants researched or built. Treat the brief as the assignment.
2. Ask for the file to write: `plantool session doc path --session <ref> --kind research`.
3. If the brief mentions a GitHub issue, PR or failed run, pull it with `gh issue view`,
   `gh pr view` or `gh run view <id> --log-failed`.
4. Move the stage: `plantool session stage --session <ref> --set researching`.

## Research

Start by naming the destination: in one or two lines, what does "done" look like for this brief
(a decision, a change, a spec)? Everything you read should serve that. Read the target area
thoroughly, not just the entry points. Trace the code paths, the data flow and the dependencies.
Base every statement on files you opened. Write the findings, not a tour. Refer to things by
name (file paths, function names, section titles), never by a bare number or id.

## The document

Write the whole document to the path from step 2. Never write into `REVIEWS/`. The human reads it
as a **slide deck** (one `##` section per slide) as well as a page, and comments on any block, so:

- One idea per `##` section, and keep a section to what fits on one screen (roughly 15 lines).
  Lead each section with a one-line takeaway in bold, then the evidence.
- Prefer bullets and tables to paragraphs. A table for "what lives where" (path, role).
- Draw the flow. Use a ` ```mermaid ` block for anything with more than two steps: `flowchart`
  for code paths and data flow, `sequenceDiagram` for request/response or process interaction,
  `stateDiagram-v2` for lifecycles, `erDiagram` for models. Keep labels short; the diagram is
  rendered in the browser and can be commented on like any other block.
- Numbers get a chart: `pie` or `xychart-beta` in mermaid, or a small table when a chart would
  not add anything.
- Refer to code by path and symbol name; the reader can search for it.

Use this shape and drop any section that has nothing to say:

```markdown
# Research: <title>

## Overview
What was researched and the one-paragraph answer.

## How it works
**Takeaway in one line.** Then the code flow as a mermaid flowchart or sequence diagram, and
the few sentences the diagram cannot carry, with file paths.

## Key components
A table: path | role | notes.

## Patterns and conventions
What the codebase does that the plan must follow.

## Dependencies and relationships
Internal and external, and how they interact.

## Edge cases and error handling

## Open questions
Decisions the human has to make before planning. Each as one sharp question.

## Out of scope
What you deliberately did not look at, and why.

## Risks and tech debt

## Context
Issues, PRs, docs, prior sessions.
```

The daemon captures the file within a second and moves the stage to `research-review`.
Run `plantool session doc touch --session <ref> --kind research` if you want that confirmed.

## After writing

Stop. Do not plan or implement. Post each open question as a comment on its own line in the
document so the human can answer it inline in the browser:
`plantool session comment add --session <ref> --kind research --match "<the question line>" --body "<why it matters and the options you see>"`.
A question with no line in the document is a question the human will not see.

Then read any comments the human leaves:
`plantool session comment list --session <ref> --kind human --unresolved --context`. Revise
the document, reply on each thread with what changed, resolve it, and wait again with
`plantool session watch --session <ref> --since <seq> --timeout 900` until the human moves on
or tells you to plan.
