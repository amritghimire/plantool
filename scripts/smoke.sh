#!/usr/bin/env bash
# End-to-end smoke test against a built binary: temp home, temp repo, temp port.
set -euo pipefail
ARG="${1:-$(dirname "$0")/../target/debug/plantool}"
BIN="$(cd "$(dirname "$ARG")" && pwd)/$(basename "$ARG")"
TMP="$(mktemp -d)"
PORT=$((41300 + RANDOM % 200))
export PLANTOOL_HOME="$TMP/home" PLANTOOL_PORT="$PORT"
trap '"$BIN" daemon stop >/dev/null 2>&1 || true; rm -rf "$TMP"' EXIT
mkdir -p "$TMP/repo" && cd "$TMP/repo"
git init -q -b main && git config user.email t@t && git config user.name t
echo hi > a.txt && git add . && git commit -qm init

expect() {
  local flag="" pat="$1"; shift
  if [ "$pat" = "-E" ]; then flag="-E"; pat="$1"; shift; fi
  local out; out="$("$@")"
  grep -q $flag -- "$pat" <<<"$out" || { echo "FAIL: expected '$pat' in output of: $*"; echo "$out"; exit 1; }
}

expect "created repo/smoke-test" "$BIN" new smoke-test --title "Smoke" --no-open
PLAN="$("$BIN" session doc path --session smoke-test --kind plan)"
printf '# Plan\n\nalpha\n\n### Phase 1: A\n- [ ] one\n' > "$PLAN"
# The watcher may capture the file before the explicit touch; either way a sha must be reported.
expect -E "^(captured|unchanged) [0-9a-f]{64}$" "$BIN" session doc touch --session smoke-test --kind plan
expect plan-review "$BIN" session get --session smoke-test
ID="$("$BIN" session comment add --session smoke-test --kind plan --match alpha --body hello | cut -d' ' -f1)"
expect "\"id\": \"$ID\"" "$BIN" session comment list --session smoke-test --json
if "$BIN" session stage --session smoke-test --set approved 2>/dev/null; then echo "FAIL: agent approved"; exit 1; fi
"$BIN" session watch --session smoke-test --timeout 1 >/dev/null && { echo "FAIL: watch returned early"; exit 1; } || [ $? -eq 124 ]
expect "tool:" "$BIN" session changes --session smoke-test
expect smoke-test "$BIN" status
echo "smoke ok on port $PORT"
