# Agent instructions
- Spec: docs/engineering-os-v0.1.md. Implement ONLY the task/phase you are given; nothing from later phases.
- Build & test before finishing:
  (cd web && npm ci && npm run build) && cargo test --workspace && (cd web && npx vitest run) && scripts/smoke.sh
- You run on Linux. macOS-only code (passkeys/Touch ID UX, sandbox-exec, Keychain) must be behind
  #[cfg(target_os = "macos")] with a Linux-compiling stub, and unit-testable logic kept platform-independent.
  List every check you could not run in the PR under "Manual verification on macOS".
- Never use ~/.plantool; dev runs use PLANTOOL_HOME=$(mktemp -d) PLANTOOL_PORT=41300.
- Small, focused commits. If the spec is ambiguous, say so in the PR instead of guessing.
