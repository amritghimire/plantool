---
name: test-first
description: Prove relevant regressions with meaningful failing and passing checks in the isolated checkout.
---

# Test-first verification

For a bug fix, identify a concrete failure hypothesis and a focused assertion tied to the defect. Run it before the fix when possible and retain the observed failure. Implement the smallest approved fix, rerun the focused case and relevant regression checks, and retain passing output.

When safe and useful, use a separate temporary checkout or reversible scoped patch to confirm the test fails without the fix. Never reset unrelated changes or rewrite history merely to obtain this proof. If the before state cannot be exercised, say so; do not invent red output.

Use the project's actual Rust/Python/TypeScript test tools, not copied scaffolding commands. Do not add tests for every reversible cosmetic edit or tests that just mirror implementation. Keep logs/caches in permitted temp paths; publish real evidence and its tested SHA/state.
