> The complete workspace reconstruction and current design decisions are documented in [DESIGN.md](../DESIGN.md). This review records the initial customer-flow audit.

# Plantool: a workspace for reviewing agent work

The customer should always know what is happening, what needs their attention,
and what to do next. Research, plan approval, implementation, and code review
remain distinct decisions. Approval never starts an agent implicitly.

## Comparable products

Reviewed official documentation on 2026-10-29:

| Product | Useful pattern | Application to Plantool |
| --- | --- | --- |
| [Cursor Plan Mode](https://cursor.com/docs/agent/plan-mode) | Read and revise a plan before building | Keep approval and revision actions visible beside the plan |
| [Cursor Agents Window](https://cursor.com/docs/agent/agents-window) | Workspace, changes, commits, and PRs in one place | Preserve workspace context while reviewing |
| [Devin Review](https://docs.devin.ai/work-with-devin/devin-review) | Changes, discussion, and review actions together | Route milestone handoffs to code; make review scope explicit |
| [GitHub Copilot review](https://docs.github.com/en/copilot/how-tos/copilot-on-github/use-copilot-agents/review-copilot-output) | A clear handoff to human review | Keep acceptance separate from agent execution |
| [Linear Inbox](https://linear.app/docs/inbox) | Prioritize work requiring attention | Put current actions above history and configuration |

These are interaction references, not claims of hands-on testing or templates to copy.

## Session workspace

1. A quiet header identifies the session and repository.
2. Research, Plan, and Changes stay directly accessible.
3. Actions are expanded by default. An explicit collapse is remembered per session.
4. Approve plan, Start implementation, Approve milestone, and Accept implementation
   have primary visual weight in their respective steps.
5. Manual stage changes and workflow shortcuts live under Advanced controls.
6. Workspace, archives, activity, and run history follow the actions.
7. Milestone and final-result handoffs open Changes, not Plan.

## Starting work

Start with a repository, a title, and a brief. Generate the session ID; allow an
override in Workspace options. Non-Latin titles must still produce a valid ID.
Select a ready provider when the saved default is unavailable. Keep permissions
and implementation pace visible; move stage/model/effort choices into advanced
settings. Show setup expanded when no provider is ready.

## Review contract

Review context includes the workspace, tool, scope, and pinned base. A whole
implementation retains its original comparison after milestones, resumes, and
Done. An individual milestone compares against its own starting snapshot.
External reviews must be checked against both workspace and base before reuse.
New files must be included. A tool failure must preserve the built-in diff and
surface an actionable error. Never silently redirect comments to a different
review context.

## Visual system

Retain the existing blue accent, neutral surfaces, compact headings, generous
document spacing, clear focus rings, and one prominent decision per step.
Support light/dark themes and narrow screens. Color supplements explicit status
text. Keep code paths and revision identifiers selectable and readable.

## Validation

Exercise session creation, approval with blockers, revision acceptance, provider
fallback, milestone review, Done, stale external review reuse, and scope changes.
Check responsive layouts and both themes. Use mocked provider data for browser
checks so no paid agent run or real repository mutation is triggered.
