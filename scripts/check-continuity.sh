#!/bin/sh
# Isolated regression checks; no real provider calls or GitHub issue mutations.
set -eu
cd "$(dirname "$0")/.."
fixture=${1:-}
if [ -n "$fixture" ]; then
  [ -f "$fixture/scripts/check.sh" ] || { echo 'Playground fixture is missing scripts/check.sh' >&2; exit 1; }
  (cd "$fixture" && ./scripts/verify-scenario.sh baseline)
fi
(cd web && npm ci && npm run build)
cargo test --workspace
(cd web && npx vitest run)
uv run --project worker --frozen pytest -c worker/pyproject.toml worker/tests
cargo build -p plantool
scripts/smoke.sh target/debug/plantool
