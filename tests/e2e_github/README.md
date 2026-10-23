# Opt-in GitHub playground tests

Default workspace tests perform no GitHub calls. This suite returns immediately unless
`PLANTOOL_E2E_GITHUB=1`. Run only on macOS, against **amritghimire/plantool-playground**
(the target is hard-coded), after installing the test GitHub App there.

1. Create/install the App using the browser setup wizard with the permissions in spec §13.1.
   Its RSA key must be in macOS Keychain service `plantool/github-app`, account `<app-id>`.
   The wizard writes it; no PEM path or private-key environment variable is needed.
2. Authenticate `gh` as `amritghimire`. The test runner uses `gh api` to post a real human
   approval. Production agents/worker never receive that credential.
3. From the checkout, run:

   ```bash
   PLANTOOL_E2E_GITHUB=1 \
   PLANTOOL_E2E_APP_ID=<numeric-app-id> \
   PLANTOOL_E2E_INSTALLATION_ID=<numeric-playground-installation-id> \
   cargo test -p plantool-daemon --test e2e_github -- --nocapture
   ```

The test creates its own temporary PlanTool home and local checkout, installs the seven
labels, runs preflight, creates an issue, publishes a new App plan comment, rejects an App
approval, accepts one owner approval and checks replay plus durable resume. No model,
work-item graph, role, push, PR or merge is exercised in this half-phase. It closes its test
issue on success or assertion failure and retains comments as evidence. A killed test
process/network failure may require manually closing issues titled `[PlanTool e2e]`.
The labels are shared setup state and are retained. Never point these tests at the daily repo.
