# Provider diagnostics and issue status

Open Settings → Models & providers. Save intended routes while execution is stopped,
resume execution, then Verify routes with passkey. This checks catalogs/authentication;
it does not prove inference, tools, structured output, or a complete engineering run.

Wait until the global model slot is free and select Test <route> with passkey. This
is an explicit, fresh-authorized live call using synthetic data only. HTTP checks
make up to three normally counted requests: basic OK response, synthetic add-tool
arguments, then structured JSON. They do not execute provider-authored code. Codex
uses its signed-in CLI/app-server with a read-only sandbox, a disposable synthetic
file and JSON verification; it never uses an OpenAI API key. A Codex turn can include
multiple model/tool calls. This test does not change project provider policy or
send project files. Passing this small probe does not prove every large role schema
or real task will succeed.

The test shares the global lease with Personal and Engineering OS, respects Stop All
and counts against unreserved provider budgets. It does not exhaust or substitute
routes or automatically Repair the worker. Results show the actual resolved model,
checked time, failed stage, safe error category, HTTP status/retry time if available,
and a suggested next action. Saving configuration invalidates saved probe results.
Copy redacted diagnostics copies only these fields: no keys, raw responses, prompts,
project content or Codex output. Retesting is explicit and may consume quota.

## Provider checklist

- Codex: `codex login status` on the daemon host; ensure the signed-in CLI executable
  is available on its PATH. Use the same subscription login as Personal. No OpenAI
  API key is needed. Native probe requires the macOS sandbox.
- Local Ollama: `ollama list`, `ollama pull <exact-model>`, then
  `ollama run <exact-model> 'Reply with OK only'`. Use `ollama ps` to inspect loaded
  models. Default endpoint is localhost:11434; large models may load slowly.
- Ollama Cloud: `ollama signin`, pull the exact cloud reference and run it through
  that same local service. Your Ollama login handles remote authentication. Cloud
  quotas still apply, and cloud models can differ in tools/JSON support.
- OpenRouter: a generic-password service `plantool/openrouter` in the daemon user's
  Keychain. To check existence without printing the credential:
  `/usr/bin/security find-generic-password -s plantool/openrouter -w >/dev/null`.
  Never paste the key or Authorization header. Use exact current catalog IDs.
- OpenCode Zen: matching `plantool/opencode-zen` Keychain service and a current catalog
  ID. The existing route transport uses this credential; merely installing an
  unrelated OpenCode login does not populate the daemon's Keychain service.

401/403: inspect login/account/access. 404: catalog ID or backing endpoint may be
unavailable. 429: quota/shared upstream rate limits; wait or use another permitted
route. 400/422 at tools/JSON: model/provider capability mismatch. 5xx: upstream service
failure. Timeout: local loading/service/network delay. Catalog presence alone cannot
rule these out. A provider error is not evidence that the Python worker needs Repair.

Real provider execution, native Keychain/passkey/sandbox behavior and account-specific
limits must be checked on the installed Mac. Automated tests use fake providers.

## Running versus queued

Browser overview and attempt history derive running from the single global lease,
not the persisted attempt lifecycle flag. Lease waiters are queued; non-model result
handling is processing. The stored lifecycle state is retained for reconciliation.
At most one attempt is displayed running across projects; a provider diagnostic may
hold the global slot without belonging to an item. A running card does not certify
successful execution; inspect its accepted result and evidence afterward.

## Mirrored issue updates

Daemon-bound triage outcomes, blocked/failed results, discussion/strategy outcomes,
recovery notices and gate state changes appear in #general and the item's own thread.
For items linked to a GitHub issue they also enter a durable App-comment outbox. New
updates carry a dedupe marker and are observed before replay. Uncertain writes are
not blindly repeated. Comments for frozen items are informational only; neither
comments nor thread text unfreeze work, approve gates or grant capabilities. Stop All
pauses external delivery. Unrelated issue numbers mentioned in free text are not
inferred as targets for publication.

Item detail shows pending issue updates. Delivery retries with backoff after access,
quota or network failures. If a post outcome is uncertain, observation continues
without repeating the write; inspect the GitHub issue and logs if it persists.
Pending informational status delivery does not prevent eligible owner recovery.
Existing messages/decisions are not rewritten or fabricated on upgrade.
