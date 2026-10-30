# UX comparison

These captures show the actual Plantool production frontend with illustrative,
mocked session and provider responses. No paid agent runs were started and no
customer repository was modified. The sample task is “Fix session timeout.”

- `before-plan.png`: main at `a577f8e8`, default collapsed actions.
- `after-plan.png`: visible plan approval and advanced controls separated.
- `after-changes.png`: workspace, pinned comparison base, and review scope.
- `after-run-dialog.png`: an available provider selected automatically.
- `after-new-session.png`: non-Latin title with a valid generated session ID.
- `after-inbox.png`: session list hierarchy.
- `after-dark.png`, `after-tablet.png`, `after-mobile.png`: theme and responsive checks.

Captured with Chromium at 1440×1040, 820×1180, and 390×844 CSS pixels.
Browser checks covered approval, choosing implementation pace, switching diff
scope, opening file diffs, modal focus trapping and restoration, mobile details,
and session creation. No page JavaScript errors or horizontal page overflow were
observed. The browser used mocked API responses; Rust tests separately exercise
the daemon and provider adapters.

Design guidance: [Impeccable polish](https://github.com/pbakaus/impeccable/blob/main/.agents/skills/impeccable/reference/polish.md)
and [Operate mode](https://github.com/pbakaus/impeccable/blob/main/.agents/skills/impeccable/reference/operate.md).
