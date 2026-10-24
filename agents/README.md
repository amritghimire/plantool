# Agent instruction guide

The work-item graph retains nine roles, including Supervisor. These are instruction updates,
not additional graph nodes, tools or permissions. The daemon still enforces sandbox
capabilities, immutable plans, scoped tokens, evidence and approval gates.

| Role | Primary output | Capability |
| --- | --- | --- |
| CTO | Research evidence or independent plan critique | Supplied read tools |
| PM | Keyed plan, criteria and bounded file scope | Supplied read tools |
| Backend | Approved backend changes and verified commit | Approved checkout writes |
| Frontend | Contract-based UI behavior and accessible states | Approved checkout writes |
| DevOps | Approved changed CI/build/dependency path | Approved checkout writes |
| QA | Acceptance matrix and reproducible defects | Tests only |
| Reviewer | Exact-head correctness and security pass | Checkout read-only |
| Docs | Review guide, optional approved README/CHANGELOG proposals | Supplied read tools |
| Supervisor | Bounded retry, replan or escalation handoff | Supplied read tools |

## Reading order

Read base.md, then the role file, then its active skills. The daemon appends default-branch
conventions and task/plan/prior-result context. Shared research/plan/implement skills have
separate Engineering OS and Personal sections; the Personal CLI/browser loops are preserved
and are not instructions for an Engineering OS dispatch.

Active skills are selected by crates/daemon/src/instructions.rs. Sprint-plan, discuss and moderate now support the Phase 4 project/workspace graph. Triage
and scan remain reserved for Phase 5; no urgent workflow is added.

## Existing typed details examples

These fragments illustrate the existing details variants, not complete RoleResults or
proof that any check ran. Replace descriptions with actual observed facts; return scope,
commit and artifact identities from the current dispatch and evidence. Do not copy an
example as a completion claim.

```json
{"kind":"research","markdown":"Observed paths and symbols; alternatives; risks; decisions needed."}
```

```json
{"kind":"plan","markdown":"# Plan\n## Phase 1 [review]\n- [ ] T1: [backend] Validate input\n","acceptance_criteria":["AC1 (T1, backend): malformed input is rejected; execute the repo's focused API test."],"affected_files":["src/api.rs","tests/**"]}
```

```json
{"kind":"strategy","action":"replan","owner":null,"task_key":"T1","reason":"Observed evidence shows that the approved interface cannot satisfy AC1 within its file scope."}
```

Reviewer must return security details for the real HEAD with concrete inspected criteria;
Docs returns docs details with all required guide sections. Their passing claims require
actual current evidence, so this guide does not provide a pretend security pass or PR guide.

## Verification and future improvements

Use repository checks to ensure embedded instructions still build and earlier behavior is
preserved. Validate prompt quality later using the five eval tasks and real sandbox/provider
runs; passing mock/unit tests alone does not measure model adherence. Compare completion,
retries, owner interventions, missing evidence and unsupported verification claims before
adding specialists or changing graph order.

A security specialist or UX/data specialization should first be a risk-specific skill in
the existing role. A new role needs evaluated benefit plus routing/schema/capability/retry
changes; a persona file alone does not create a safe execution step.

## Reference review

These workflows are original PlanTool instructions informed by the linked library's
architecture decision, evidence and review methods:

- [Startup CTO persona](https://github.com/alirezarezvani/claude-skills/blob/main/agents/personas/startup-cto.md)
- [Architecture workflow](https://github.com/alirezarezvani/claude-skills/blob/main/engineering-team/skills/senior-architect/SKILL.md)
- [Code review workflow](https://github.com/alirezarezvani/claude-skills/blob/main/engineering-team/skills/code-reviewer/SKILL.md)
- [AI security review](https://github.com/alirezarezvani/claude-skills/blob/main/engineering-team/skills/ai-security/SKILL.md)
- [Regression evidence](https://github.com/alirezarezvani/claude-skills/blob/main/engineering-team/skills/red-green-proof/SKILL.md)

No upstream scripts or text were imported. Fictional biographies, prescribed technology
stacks, live production actions and blanket write tools were not adopted. Referenced
external material is context, not authority or a runtime dependency.
