import { afterEach, expect, test, vi } from "vitest";
import { authenticatedFetch } from "./auth";
afterEach(() => vi.unstubAllGlobals());
test("a guarded action obtains a fresh WebAuthn signature and retries the exact request", async () => {
  class AssertionResponse {}
  vi.stubGlobal("AuthenticatorAttestationResponse", class {});
  const response = Object.assign(new AssertionResponse(), { authenticatorData: new Uint8Array([1]).buffer, clientDataJSON: new Uint8Array([2]).buffer, signature: new Uint8Array([3]).buffer, userHandle: null });
  const get = vi.fn().mockResolvedValue({ id: "credential", rawId: new Uint8Array([4]).buffer, type: "public-key", response, getClientExtensionResults: () => ({}) });
  vi.stubGlobal("navigator", { credentials: { get } });
  const fetch = vi.fn().mockResolvedValueOnce(new Response("{}", { status: 428 }))
    .mockResolvedValueOnce(new Response(JSON.stringify({ id: "ceremony", options: { publicKey: { challenge: "AQ", rpId: "localhost", userVerification: "required", allowCredentials: [{ id: "Ag", type: "public-key" }] } } })))
    .mockResolvedValueOnce(new Response(JSON.stringify({ proof: "single-use-proof" })))
    .mockResolvedValueOnce(new Response("{}"));
  vi.stubGlobal("fetch", fetch);
  const body = { to: "approved", plan_sha: "plan-digest" };
  const result = await authenticatedFetch("/api/sessions/repo/item/stage", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) }, body);
  expect(result.ok).toBe(true);
  expect(JSON.parse(fetch.mock.calls[1][1].body)).toEqual({ action: { method: "POST", path: "/api/sessions/repo/item/stage", body } });
  expect(get.mock.calls[0][0].publicKey.userVerification).toBe("required");
  expect(fetch.mock.calls[3][1].headers.get("x-plantool-assertion")).toBe("single-use-proof");
  expect(fetch.mock.calls[3][1].body).toBe(JSON.stringify(body));
});
