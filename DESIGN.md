---
name: plantool
description: A local review desk where the human examines an agent's specification and issues the allowance.
colors:
  sheet: "#ffffff"
  sheet-2: "#f6f6f5"
  ink: "#111111"
  ink-2: "#5c5c5c"
  rule: "#d9d9d9"
  rule-strong: "#8a8a8a"
  human: "#1f3a93"
  human-ink: "#ffffff"
  human-wash: "#e8ecf6"
  warn: "#8a5300"
  warn-wash: "#fbf1e1"
  danger: "#b3261e"
  danger-wash: "#fbeae8"
  ok: "#1d6b3a"
  sheet-dark: "#0e1220"
  sheet-2-dark: "#131829"
  ink-dark: "#eceef3"
  ink-2-dark: "#a3a9b8"
  rule-dark: "#2a3043"
  rule-strong-dark: "#666e86"
  human-dark: "#93abf2"
  human-ink-dark: "#0b1020"
  human-wash-dark: "#1f2a4c"
  warn-dark: "#e8b060"
  warn-wash-dark: "#2e2414"
  danger-dark: "#f49a93"
  danger-wash-dark: "#361a1d"
  ok-dark: "#7fcf9b"
typography:
  display:
    fontFamily: "Public Sans, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "28px"
    fontWeight: 700
    lineHeight: 1.3
  headline:
    fontFamily: "Public Sans, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "20px"
    fontWeight: 700
    lineHeight: 1.4
  title:
    fontFamily: "Public Sans, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "15px"
    fontWeight: 600
    lineHeight: 1.35
  prose:
    fontFamily: "Source Serif 4, Georgia, serif"
    fontSize: "17px"
    fontWeight: 400
    lineHeight: 1.8
  body:
    fontFamily: "Public Sans, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.5
  control:
    fontFamily: "Public Sans, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "13px"
    fontWeight: 500
    lineHeight: 1.5
  label:
    fontFamily: "Public Sans, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.5
  form-header:
    fontFamily: "Public Sans, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "12px"
    fontWeight: 700
    letterSpacing: "0.12em"
  ref:
    fontFamily: "Courier Prime, Courier New, monospace"
    fontSize: "12px"
    fontWeight: 400
  ref-numeral:
    fontFamily: "Courier Prime, Courier New, monospace"
    fontSize: "15px"
    fontWeight: 700
    lineHeight: 1
  code:
    fontFamily: "ui-monospace, SF Mono, Menlo, Consolas, monospace"
    fontSize: "12px"
    lineHeight: 1.6
rounded:
  none: "0px"
spacing:
  s-1: "4px"
  s-2: "8px"
  s-3: "12px"
  s-4: "16px"
  s-5: "24px"
  s-6: "32px"
  s-7: "48px"
  s-8: "64px"
components:
  button-primary:
    backgroundColor: "{colors.human}"
    textColor: "{colors.human-ink}"
    rounded: "{rounded.none}"
    padding: "6px 12px"
    height: "34px"
    typography: "{typography.control}"
  button-secondary:
    backgroundColor: "{colors.sheet}"
    textColor: "{colors.ink}"
    rounded: "{rounded.none}"
    padding: "6px 12px"
    height: "34px"
    typography: "{typography.control}"
  button-secondary-hover:
    backgroundColor: "{colors.sheet-2}"
  button-ghost:
    textColor: "{colors.ink}"
    rounded: "{rounded.none}"
    padding: "6px 12px"
  button-danger:
    backgroundColor: "{colors.danger}"
    textColor: "{colors.sheet}"
    rounded: "{rounded.none}"
    padding: "6px 12px"
  button-small:
    padding: "3px 9px"
    height: "28px"
    typography: "{typography.label}"
  chip:
    backgroundColor: "{colors.sheet}"
    textColor: "{colors.ink}"
    rounded: "{rounded.none}"
    padding: "4px 8px"
    typography: "{typography.label}"
  chip-active:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.sheet}"
  segmented-active:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.sheet}"
    padding: "3px 10px"
    height: "26px"
  drawer-toggle-active:
    backgroundColor: "{colors.human-wash}"
    textColor: "{colors.ink}"
    padding: "3px 8px"
    height: "28px"
  input:
    backgroundColor: "{colors.sheet}"
    textColor: "{colors.ink}"
    rounded: "{rounded.none}"
    padding: "7px 10px"
    height: "36px"
  ref-numeral-selected:
    backgroundColor: "{colors.human}"
    textColor: "{colors.human-ink}"
    typography: "{typography.ref-numeral}"
    padding: "3px 3px 2px"
  queue-row:
    padding: "8px"
    height: "56px"
  selected-paragraph:
    backgroundColor: "{colors.human-wash}"
---

# Design System: plantool

## Overview

**Creative North Star: "The Examiner's Desk"**

The plan is a specification under examination, and the screen is laid out like the paperwork of a patent office. A white sheet, black ink, hairline rules, and one blue that belongs to the examiner. Documents carry bracketed paragraph numbers in a left gutter ([0001]); comments are reference numerals pinned to those paragraphs on curved leader lines in the right margin; the session's steps read FIG. 1 to FIG. 5; and the single decision for the step sits in a ruled form box titled like an official notice ("NOTICE OF ALLOWANCE"). Approval is an allowance only the human issues.

The document owns the canvas. Chrome is one slim left column (wordmark, way back to the inbox, session identity, the FIG list, the notice) and a sticky canvas header; comments, the agent run and session details are drawers that open on the right and close again. The inbox is a queue ordered by who moves next (Waiting on you, Agent running, Recently done) with fixed columns, so rows restyle on state change rather than reflow.

The material is flat print: square corners, 1px rules, no shadows, no gradients, no transitions. State changes are instant swaps of fill and rule. Hierarchy comes from rule weight (hairline grey for separation, solid ink for structure, a 2px ink border for the notice) and from the three type families each doing one job.

**Key Characteristics:**
- White sheet, black ink, grey hairlines; one patent-office blue reserved for the human.
- Three families with fixed jobs: sans for controls, serif for prose, typewriter for references.
- Bracketed paragraph numbers and underlined reference numerals with curved leaders.
- Square corners everywhere; the only round shape is the 8px run-status dot.
- Flat, instant, no shadows, no motion.
- Dark theme is the same sheet translated to a deep blue-black, not an inverted grey.

## Colors

Ink on paper, with exactly one chromatic voice for the human and three quiet signal colors for state.

### Primary
- **Examiner's Blue** (human): the human's acts and the human's attention. The filled primary button (Approve, Accept, Start, Send, Allow), the "Waiting on you" queue heading and its row marks, blocker and selected reference numerals and their leaders, the current FIG number and state word, the selected paragraph's number, the text caret, and the focus ring.
- **Allowance Wash** (human-wash): the pale field of selection. Selected paragraph background, the selected comment thread, an open drawer toggle, a focused inbox row, text selection, and info banners.

### Secondary
- **Ochre Warning** (warn) with **Ochre Wash** (warn-wash): recovery and waiting states. A stopped run needing resume, a run waiting on permission, warning phase titles.
- **Docket Red** (danger) with **Docket Wash** (danger-wash): destructive buttons, invalid inputs, failed runs and errors.
- **Filed Green** (ok): completion only. Done and approved stage words, the done row mark.

### Neutral
- **Sheet** (sheet): every surface: page, spine, canvas, drawers, modals, menus. There is no card colour.
- **Second Sheet** (sheet-2): hover fill, code and pre backgrounds, table heads, disabled inputs, the empty run bar track.
- **Ink** (ink): text, structural rules (spine edge, canvas header, drawer edge, form boxes, secondary button borders), active segmented and chip fills, the agent's live state.
- **Grey Ink** (ink-2): secondary text, paragraph numbers, FIG states, metadata, placeholders.
- **Hairline** (rule): separators between rows, list items, menu groups, code borders.
- **Strong Rule** (rule-strong): input and chip borders, link underlines, finished run bars, scrollbar thumb on hover.

The dark theme (the `-dark` tokens) re-sheets the page in blue-black and lifts the blue to a periwinkle so it stays readable; roles are identical. It applies by `data-theme="dark"` or by system preference unless the user forced light.

### Named Rules
**The Examiner's Blue Rule.** Blue marks the human: their decision, their blockers, their selection, their place. Agent activity is drawn in ink (a running agent is an ink dot and an ink bar), never in blue.

**The One Fill Rule.** Each region has at most one blue-filled button, and it is the human's committing act there. Everything else is outlined in ink or ghost.

## Typography

**UI Font:** Public Sans (with -apple-system, Segoe UI, sans-serif)
**Prose Font:** Source Serif 4 (with Georgia, serif)
**Reference Font:** Courier Prime (with Courier New, monospace)
**Code Font:** the system monospace (ui-monospace, SF Mono, Menlo)

**Character:** A plain civic sans runs the desk, a book serif carries the long reading, and a typewriter face stamps every number and identifier the way a filing clerk would.

### Hierarchy
- **Display** (700, 28px, 1.3; 25px on phones): the document's own H1. There is no second page title above it.
- **Headline** (700, 20px, 1.4): document H2s and the inbox page title.
- **Title** (600, 15px, 1.35): session title in the spine, drawer headings, modal section heads; the notice heading uses 700.
- **Prose** (400, 17px, 1.8; 16px on phones): document paragraphs and lists inside a 600px measure. Headings inside the document switch back to Public Sans.
- **Body** (400, 14px, 1.5): the base UI text and canvas tabs.
- **Control** (500, 13px): buttons, menus, drawer toggles, spine links, drawer body text.
- **Label** (400, 12px): metadata, segmented controls, chips, small buttons, field labels.
- **Form Header** (700, 12px, 0.12em tracking, uppercase): the title bar of a ruled form box and the urgency headings of inbox queues (0.1em there).
- **Reference** (Courier Prime 400, 12px): paragraph numbers [0001], FIG numbers and states, repo and branch, revisions, line refs, counts, and form field values (13px in the notice).
- **Reference Numeral** (Courier Prime 700, 15px, underlined): comment numerals in the margin and the comments drawer.

### Named Rules
**The Typewriter Rule.** Anything that is a number, an identifier or a field value (revision, slug, repo, branch, line, count, paragraph) is set in Courier Prime. Words that describe are set in Public Sans.

**The Serif Is For Reading Rule.** Source Serif 4 appears only for document prose. Controls inside the document, headings and every piece of chrome stay in Public Sans.

## Layout

The session is a grid: a 248px spine with a 1px ink right edge, a fluid canvas, and an optional right drawer (360px for comments and details, 420px for the agent). The canvas has a sticky 48px header (Research / Plan / Changes tabs with a 3px ink underline on the active one; Document / Slides / Source segmented control; Comments and Agent toggles; review options).

The document column is a 600px measure flanked by a 76px number gutter on the left and a 92px reference margin on the right, centred in the canvas. Each block reserves a row height (31px paragraphs, 36px H1, 28px H2, 40px code) so numbers and numerals sit on the block's first line, and blocks with stacked numerals grow by 26px per extra numeral.

Spacing runs on a 4px base (4, 8, 12, 16, 24, 32, 48, 64). Rows and form padding are tight (6 to 8px); section gaps are 24 to 32px.

Responsive behaviour:
- **Up to 1100px** with a drawer open, the spine collapses to its notice alone, docked under the drawer; the gutter and margin narrow to 56px and 80px; the view-mode control folds into a menu.
- **Up to 760px** everything stacks into one column: identity, FIG list as horizontal tabs, the document, and the notice docked sticky at the bottom with a 2px top rule so the human reads before they can approve. Touch targets grow to 44px.
- **Inbox** rows keep fixed columns (mark, title, repo, reason, open count, age, PR, menu) down to 1100px, then drop to three columns at 860px and two at 760px.

## Elevation & Depth

There is no elevation. Surfaces are one sheet; drawers, menus, popovers and modals are separated by a 1px ink rule and sit on the same white. Modals dim the page with a 40% ink overlay, which is the only depth cue in the system. Rule weight replaces shadow: hairline grey separates, solid ink structures, and the notice's 2px ink border marks the one thing on screen that is a decision.

### Named Rules
**The Paper Rule.** No box-shadow, no gradient, no blur, no transition. A state change swaps a fill or a rule and happens instantly.

## Shapes

Every corner is square (0px): buttons, inputs, chips, tags, form boxes, drawers, menus, modals, code, kbd. Borders are 1px, except the notice (2px) and the active tab and active view-mode markers (3px). The only curves are the run-status dot (8px circle) and the reference-numeral leader, a single 1px S-curve that runs from the paragraph's edge up to the numeral.

## Components

### Buttons
Printed and plain: an ink rectangle with text, no rounding, no shadow.
- **Shape:** square (0px), 34px tall, 6px 12px padding, 13px medium text.
- **Primary:** blue fill, white text, 600 weight; hover mixes 12% ink into the blue. Reserved for the human's committing act.
- **Secondary:** white with a 1px ink border; hover fills Second Sheet, active fills 8% ink.
- **Ghost / Quiet:** no border or fill; hover underlines the label.
- **Danger:** red fill, sheet text, 600 weight.
- **Small:** 28px tall, 12px text.
- **Disabled:** 50% opacity with a not-allowed cursor; the reason is given beside the button rather than hidden.

### Chips and Tags
- **Chips** (comment filters): white, 1px Strong Rule border, 12px text; the active chip fills ink with sheet text.
- **Tags** (comment kind, author kind): 1px Strong Rule border, Grey Ink text, 12px uppercase with 0.04em tracking.
- **Segmented control:** a 1px Strong Rule frame with hairline dividers; the active segment fills ink.

### Form Box and the Notice
The signature container. A white box with a 1px ink border whose header is a caps-tracked title bar over a 1px ink rule. Field labels are 12px Grey Ink sans; field values are Courier Prime 13px. The notice for the current step (Notice of Allowance, and its siblings for other steps) is the form box with a 2px border, a 15px bold heading saying whose turn it is, the evidence (document and revision, open and blocker counts), a blue primary act and an outlined "Ask agent" menu.

### Inputs / Fields
- **Style:** white, 1px Strong Rule border, square, 36px minimum, 7px 10px padding, blue caret.
- **Hover:** border goes to ink.
- **Focus:** a 2px blue outline offset 2px (the global focus ring).
- **Error / Disabled:** red border for invalid; Second Sheet fill and Grey Ink text for disabled. Checkboxes and radios use ink as their accent.

### Navigation
- **Spine:** wordmark (14px bold), "Inbox" back link with the waiting count, session title, repo and branch in Courier Prime, the FIG list, the notice, and a footer of quiet links (Session details, Settings, docs, theme).
- **FIG list:** rows split by hairlines under a 1px ink top rule; "FIG. n" and the state word in Courier Prime 12px Grey Ink, state uppercase. The current FIG turns its number and state blue and bold, its name semibold.
- **Canvas tabs:** 14px Grey Ink; active is ink, 600 weight, with a 3px ink underline sitting on the header rule.
- **Drawer toggles:** outlined ink buttons (28px) carrying a count or the agent state; open toggles fill Allowance Wash.

### Paragraph Numbers and Reference Numerals
Paragraph numbers are bracketed four-digit Courier Prime at 12px in Grey Ink, unselectable, in the left gutter. A selected block fills Allowance Wash and its number turns blue. Comments appear in the right margin as reference numerals: bold 15px Courier Prime, underlined, joined to the text by a curved Grey Ink leader. Hover draws a 1px ink box around the numeral; a blocker turns numeral and leader blue; the selected numeral fills blue with white text. More than three numerals on one block collapse into a "+n" link. The comment-add control in the margin is a 24px outlined square that appears only on hover or focus.

### Comments Drawer
Filter chips across the top, then threads separated by hairlines, each led by its reference numeral, kind tag and line ref. The selected thread takes a blue border and Allowance Wash, matching its numeral in the margin.

### Inbox Queue
Sections headed by caps-tracked 12px titles over a 1px ink rule; "Waiting on you" is blue, with the count in Courier Prime. Rows are 56px with fixed columns and hairline separators; a 20px status mark leads (blue for waiting, ochre for recovery, ink for running, green for done). Hover fills Second Sheet; keyboard focus fills Allowance Wash.

### Agent Run
Run state is an 8px dot (ink when live, ochre when waiting, red when failed, Grey Ink otherwise). Run bars are 6px tall on a Second Sheet track with a hairline inset, and as long as the run took: ink while live, Strong Rule when finished, red when failed. Durations sit in a right-aligned tabular column.

## Do's and Don'ts

### Do:
- **Do** keep blue for the human: one filled primary per region, blockers, selection, the current FIG, focus.
- **Do** set every number, identifier and field value in Courier Prime, and every control in Public Sans.
- **Do** let the document own the canvas: a 600px serif measure with the 76px number gutter and 92px reference margin.
- **Do** separate with 1px hairlines and structure with 1px ink; reserve the 2px border for the notice.
- **Do** put session facts and secondary tools in drawers that the human opens, not in permanent columns.
- **Do** keep inbox rows on fixed columns so a state change restyles the row without moving it.
- **Do** dock the notice below the document on phones so the human reads before approving.

### Don't:
- **Don't** round a corner, add a shadow or gradient, or animate a state change.
- **Don't** paint agent activity, navigation or progress in blue; the agent is drawn in ink.
- **Don't** add a page title above the document's own H1, or label anything twice.
- **Don't** use caps-tracked text except as the title bar of a form box, an inbox queue heading, or a tag.
- **Don't** expand comment threads inside the prose; they live in the margin as numerals and open in the drawer.
- **Don't** use the serif for controls or the typewriter face for prose.
