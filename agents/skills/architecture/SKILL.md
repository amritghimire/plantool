---
name: architecture
description: Evaluate bounded architecture changes using inspected source, tradeoffs and recovery constraints.
---

# Architecture decision workflow

1. State the requested behavior, existing interfaces and hard constraints from the supplied conventions. Trace the relevant paths/symbols before recommending changes.
2. Compare extending the existing design with at most a few viable alternatives. Consider complexity, compatibility, failure recovery, trust boundaries, operational cost and reversibility.
3. Describe the chosen approach, why alternatives lose, affected interfaces/data and a small verification strategy. State uncertainty and the experiment that could resolve it.
4. Record migration/rollback implications when state or contracts change. Do not design speculative scale or prescribe a framework/database contrary to repo constraints.

Output in Research.markdown: observed current behavior with citations; options/tradeoffs; recommendation; risks; decisions needed. Use an ADR-shaped section only when a material decision warrants it. CTO uses available read tools and never implements the recommendation.
