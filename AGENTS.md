# Agent instructions
- The Engineering OS spec is docs/engineering-os-v0.1.md and html Implement ONLY the phase named in your task.
- Never use ~/.plantool. For any dev daemon run: PLANTOOL_HOME=~/.plantool-dev PLANTOOL_PORT=41300.
- Before finishing: (cd web && npm ci && npm run build) && cargo test --workspace && (cd web && npx vitest run) && scripts/smoke.sh
- Small commits. Don't change behaviour outside the phase. If the spec is ambiguous, stop and ask.
