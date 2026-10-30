# Plantool workspace reconstruction

## Product and mode

A local review workspace for developers directing coding agents. Success is a
clear decision at each handoff: understand, plan, approve, build, review, accept.
The interface is an operating tool and a reading surface. It is not a marketing
page. Preserve the CLI/daemon protocol, human approvals, comments, provider
permissions, resume behavior, and pinned diff contexts.

## Direction

Reconstruct the UI around a persistent application rail, a session workflow
panel, a central document/code canvas, and a contextual review inspector. Agent
activity replaces the inspector when a run is selected. Artifact navigation is
horizontal and separate from workflow status. Current actions are expanded by
default, close to the current workflow step. Supporting details are secondary.

The session list becomes a workspace inbox with real attention counts and a
compact repository index. No decorative metrics, invented agent activity, or
placeholder destinations. Every primary navigation control has a real route or
action.

## Design intelligence

Applied [UI UX Pro Max](https://github.com/nextlevelbuilder/ui-ux-pro-max-skill)
and [UI Skills baseline](https://github.com/ibelick/ui-skills/tree/main/skills/baseline-ui).
The Pro Max search matched developer-tool colors and Minimalism/Swiss style.
Its FAQ/documentation landing pattern remained off-topic after one narrower
retry, so that pattern is rejected; this app architecture is a product-specific
decision, not a claimed catalog match. Its suggested scroll reveals are also
rejected: no decorative animation was requested. The UI Skills registry CLI was
unreachable; published GitHub skill files were read directly.

## Tokens and behavior

- Slate-neutral surfaces; green for decisions and selected states. Semantic
  warning/error colors remain distinct. Dark and light themes both ship.
- Self-hosted IBM Plex Sans for controls and bold headings; IBM Plex Serif for
  plan/research prose. System monospace remains reserved for source, paths, and revisions.
- 4/8-pixel spacing scale, compact 12–14px interface text and 16–17px document text.
- Naturally wrapping headings, readable prose, tabular numbers, no custom tracking,
  gradients, glow, or decorative animation.
- Consistent Phosphor outline icons. Icon controls have accessible names.
- Radix Dialog and AlertDialog own modal focus, dismissal, and destructive
  confirmation. Native buttons/selects remain native.
- Dialog errors sit near the action and retain input. Loading states preserve
  structure. Empty states offer a next action.
- Narrow screens retain current actions, reflow document navigation, and expose
  secondary session context through an explicit details control.

## Verification

Use actual production builds with clearly labeled sample API data for visuals.
Exercise keyboard focus, cancellation, slow/failed requests, long names/paths,
both themes, 375/768/1024/1440px widths, and 200% zoom. Keep all workflow and
backend regression checks. Refresh PR screenshots around the final reconstruction.

## No AI Slop review

Applied [no-ai-slop](https://github.com/claudiusararu/unslop-ui-skill/blob/main/.claude/skills/no-ai-slop/SKILL.md)
and its root TELLS.md catalog. The style direction is a technical reading desk:
Plex Sans controls and 700-weight headings, Plex Serif document prose, crisp
white paper, green decision controls, small 4px control corners, and square
reading surfaces. The sample marketing hero is not a product screen template.

Removed Geist, blanket balanced headings, the constrained workspace subtitle,
unnecessary monospace chrome, soft segmented-control elevation, rounded paper,
and em-dash UI connectors. Mobile buttons, summaries, and selects share a 44px
minimum height. Existing accessible navigation stays visible on mobile, so no
hidden navigation requires a new menu. Screenshots and real documents are the
product imagery; adding stock photography to code review would obscure the task.
