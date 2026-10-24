---
name: code-review
description: Review the current diff for correctness and scoped risks using evidence and language-specific checks.
---

# Read-only review workflow

Compare the exact HEAD with its base and approved plan. Start with high-impact paths, then inspect the rest of the diff and affected callers. Check error paths, compatibility, concurrency, resource cleanup, cancellation, durability and test quality rather than cosmetic thresholds.

For Rust, inspect ownership/lifetimes at boundaries, locks across awaits, panic/error propagation, transactions and unsafe/FFI changes. For Python, inspect async blocking, cancellation, validation, checkpoint replay and resource lifetimes. For TypeScript/React, inspect runtime API validation, stale state/effects, error handling, permissions and accessibility. Apply only relevant checks to the repo.

Findings need a location, concrete failure/impact, reproduction or reasoning, severity, scope and task_key. Separate verified facts from suspicions. Run only permitted read-only verification; never edit the checkout or silently repair findings. Required security review is separate coverage within this same Reviewer step. A clean review is not a gate approval.
