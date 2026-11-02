---
version: 1
slug: "web-src-components-sessionpage-tsx"
primary_target: "web/src/components/SessionPage.tsx"
related_targets: ["web/src/components/SessionList.tsx"]
---

# Surface brief: plantool web app (inbox + session workspace)

Scope: the whole web UI in `web/src` — inbox (`/`), session workspace (`/s/:repo/:slug`) with Research, Plan, Changes, runs, dialogs. Mode: Operate (with a Read core: long documents).

Audience and job: one developer reviewing coding-agent work across many sessions; reads long plans, comments line by line, glances to approve, watches runs beside a terminal (often at ~960px). Only the human approves.

Decided structure (2026-11-02): document-first layout (one slim left column with steps + the single decision for this step; document fills the rest; comments, agent run and session details are drawers); comments as margin markers + drawer with filters and n/p; inbox as an urgency queue (Waiting on you / Agent running / Recently done), repo as a filter.

## Direction contract

THESIS: The plan is a specification under examination; every comment is a reference numeral pinned to a numbered paragraph, and approval is the examiner's allowance that only the human issues. Refuses the category default: grey sidebar + blue pill buttons + expanded comment cards stacked inside the prose.

OWN-WORLD: White sheet #FFFFFF, ink #111111, secondary #5C5C5C, hairline rules #D9D9D9; patent-office blue #1F3A93 reserved for human acts only (Approve, your blockers, selected comment, current step), pale #E8ECF6 for selection. Public Sans for UI (13/14px), Courier Prime for paragraph numbers, revisions, line refs and form field values, Source Serif 4 at 17px for document prose. Square corners, 1px rules, no shadows or gradients; caps-tracked form headers ("NOTICE OF ALLOWANCE"). Steps read FIG. 1–5.

STORY: The owner opens a session and immediately sees which FIG. they are on, what the examiner (they) must decide, and the evidence (revision, open count, blockers). They read the specification with paragraph numbers, step through reference numerals with n/p, then issue the allowance or ask the agent to amend.

FIRST VIEWPORT: Left column 248px: wordmark, "← Inbox · N waiting on you", session title, repo and branch in mono, FIG. 1–5 list with state words, then the Notice of Allowance form (Application: plan.md rev …, Status: Open · Blockers, blocker note, blue "Approve plan", outlined "Ask agent ▾"). Canvas header: Research / Plan / Changes tabs, Document/Slides/Source, "Comments N" and "Agent · state" toggles. Document column ~68ch with [0001] numbers in the left gutter and reference numerals on curved leader lines in the right margin. Comments drawer 360px on the right, filter chips, selected thread in blue.

FORM: Patent specification (numbered paragraphs, reference numerals, notice of allowance) — position 1 on the round-3 grounded list, shown as IMPECCABLE’S PICK and chosen by the user over the roll. seed:2769350b0ad665ce
Kept disciplines from the round: the current document owns the canvas; run bars are as long as the run took; flat colour only; inbox rows keep fixed columns and restyle on state change rather than reflow; nothing labeled twice; chrome uses three type sizes.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Unresolved

- Onboarding depth (provider check, explaining the human-only gates) — planned as its own phase.
- Dark theme translation of the sheet (ink-on-white inverted to a deep blue-black sheet) — decide in the build.
