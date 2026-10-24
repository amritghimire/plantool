# Native provider connections

Model chain order, fallback, budgets, family diversity and the global lease are unchanged.
Codex uses its CLI/app-server login. OpenRouter and OpenCode Zen use daemon-held Keychain
API keys; neither requires installing a local OpenRouter server. Engineering OS OpenCode
still connects to the daemon proxy rather than receiving provider credentials.

Both `ollama` and `ollama-cloud` connect to the installed Ollama service at
`http://localhost:11434` (or a loopback `OLLAMA_HOST`). PlanTool sends no API key for either.
Ollama handles cloud authentication using its own sign-in; PlanTool does not read or copy
Ollama's private key or change its login.

1. Install/start Ollama on the same machine as the daemon, then run `ollama signin` in the
   owner's terminal for cloud models.
2. Pull the desired local models and cloud model references using the exact names from
   Ollama's current catalog. Confirm them with `ollama list`. No model is downloaded or
   substituted automatically by PlanTool.
3. Stop All, inspect the existing role chains/provider policy, and verify routes in browser
   Settings with a fresh passkey. Resume after configuration is ready. The cloud `auto`
   route resolves only among installed cloud references; local routes only see local models.
4. The existing `plantool/ollama-cloud` Keychain entry is no longer used. It may be removed
   manually after verification; no automatic credential deletion is performed.

Cloud models still send data remotely. Keep `ollama-cloud` enabled only for projects that
allow this. A local-only `ollama` policy must not authorize cloud references, including
renamed remote-model aliases. The gateway checks Ollama's native catalog immediately before
sending prompt contents; cloud/local catalogs remain separate despite sharing an endpoint.
Non-loopback endpoints and redirects are refused, and loopback requests bypass HTTP proxies.

## Verification

Automated tests use a local fake Ollama HTTP service, never real model calls. Verify on macOS
with a signed-in Ollama installation: route discovery, one cloud and one local request,
sign-out/unavailable errors, quota fallback at the next attempt, and unchanged Codex/OpenRouter
connections. Do not share credentials or use the daily Plantool home for development runs.

## Linux sandbox assessment

The current Linux backend remains unavailable and fails closed. Installing bubblewrap alone
is insufficient: user/mount/PID isolation, protected files and aliases, QA tests-only writes,
Reviewer read-only access, daemon socket access, credentials and Stop All must all be tested.
This environment has `/usr/bin/bwrap`, but the probe below fails with
`bwrap: setting up uid map: Read-only file system`:

```sh
bwrap --unshare-user --unshare-pid --ro-bind / / --proc /proc --dev /dev --cap-drop ALL /bin/true
```

No isolation restriction is bypassed here. A Linux backend and its native security tests need
an environment permitting those namespaces. Linux Secret Service support for daemon-held
GitHub App/OpenRouter keys is also a separate prerequisite for full Engineering OS support;
Ollama login alone does not supply those credentials. This PR does not claim Linux execution.
