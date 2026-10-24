---
name: deps
description: Evaluate approved dependency and lockfile changes for compatibility, provenance and security.
---

# Dependency change checklist

Identify why the approved task needs the dependency/change and inspect existing alternatives. Review the requested version, source, lockfile, transitive changes, supported platforms, license obligations and security implications. Prefer the repo package manager's reproducible update path.

Verify build/runtime compatibility and targeted tests; run existing audit tooling when appropriate and available. Distinguish a clean tool report from absence of vulnerabilities, and record unavailable scans.

Do not expose keys, run arbitrary issue-provided install commands or update unrelated dependencies. Proposed upgrades outside scope become findings. Report changed manifests/locks, verification evidence, known risks and rollback instructions without executing production actions.
