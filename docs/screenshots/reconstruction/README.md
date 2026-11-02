# Reconstructed workspace

Production frontend rendered in Chromium with illustrative API and WebSocket fixtures. No actual daemon, external difftool, or paid provider is invoked.

Screenshots cover the plan with its Notice of Allowance, file review, the agent drawer, the inbox queue, new session, run configuration, milestone review, Done, deletion confirmation, and the first-run empty state. Responsive captures use 1440, 1024, 768, and 390 CSS pixel widths. Light and dark themes are included.

Build and reproduce from the repository root:

```sh
npm --prefix web ci
npm --prefix web run build
python3 scripts/ux-browser-smoke.py --output /tmp/plantool-ui-smoke
```

Requires Python Playwright and Chromium. Use `--browser /path/to/chromium` if needed. To enable the accessibility checks, install axe-core separately and pass `--axe-script /path/to/axe.min.js`.

The checked run found no serious or critical axe violations across eight surfaces, no page JavaScript errors, and no horizontal page overflow at the tested widths. It also checks keyboard focus, approval, implementation configuration, provider fallback, review scope and file switching, mobile controls, creation, and cancellation of deletion. Automated scans do not replace a complete assistive-technology audit.

Design rationale and skill sources: [DESIGN.md](../../../DESIGN.md).

Latest captures show the Patent Filing redesign: a slim spine with FIG. 1–5 and the Notice of Allowance, numbered specification paragraphs with reference numerals in the margin, and comments, agent runs and session details in drawers.
