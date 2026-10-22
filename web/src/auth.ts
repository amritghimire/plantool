export const SESSION_CHANGED_EVENT = "plantool:session-changed";
function reportSessionFailure(response: Response) {
  if (response.status === 401 || response.status === 403) window.dispatchEvent(new Event(SESSION_CHANGED_EVENT));
}
// WebAuthn's binary fields use base64url on the wire and ArrayBuffer in the browser.
function decode(s: string): Uint8Array<ArrayBuffer> {
  return Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));
}
function encode(buffer: ArrayBuffer): string {
  return btoa(String.fromCharCode(...new Uint8Array(buffer))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
export async function authRequest<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/auth/${path}`, { method: body === undefined ? "GET" : "POST", credentials: "same-origin", headers: { "content-type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
  reportSessionFailure(res);
  const data = await res.json();
  if (!res.ok) throw new Error(data.error ?? "Passkey authentication failed");
  return data as T;
}
type Ceremony = { id: string; options: { publicKey: Record<string, unknown> } };
function credentialJSON(credential: PublicKeyCredential) {
  const response = credential.response;
  const common = { id: credential.id, rawId: encode(credential.rawId), type: credential.type, extensions: credential.getClientExtensionResults() };
  if (response instanceof AuthenticatorAttestationResponse) return { ...common, response: { attestationObject: encode(response.attestationObject), clientDataJSON: encode(response.clientDataJSON), transports: response.getTransports?.() ?? [] } };
  const assertion = response as AuthenticatorAssertionResponse;
  return { ...common, response: { authenticatorData: encode(assertion.authenticatorData), clientDataJSON: encode(assertion.clientDataJSON), signature: encode(assertion.signature), userHandle: assertion.userHandle ? encode(assertion.userHandle) : null } };
}
export async function authenticate(action?: { method: string; path: string; body: unknown }): Promise<string | undefined> {
  if (!navigator.credentials) throw new Error("Use a passkey-capable browser at the daemon’s localhost URL.");
  const start = await authRequest<Ceremony>("assertion/start", { action });
  const p = start.options.publicKey;
  const publicKey = { ...p, challenge: decode(p.challenge as string), allowCredentials: (p.allowCredentials as { id: string; type: PublicKeyCredentialType }[]).map((c) => ({ ...c, id: decode(c.id) })) } as PublicKeyCredentialRequestOptions;
  const credential = await navigator.credentials.get({ publicKey }) as PublicKeyCredential | null;
  if (!credential) throw new Error("Passkey confirmation was cancelled");
  const result = await authRequest<{ proof?: string }>("assertion/finish", { id: start.id, credential: credentialJSON(credential) });
  return result.proof;
}
export async function registerPasskey(code: string | null) {
  const body = { code };
  const proof = code === null ? await authenticate({ method: "POST", path: "/api/auth/register/start", body }) : undefined;
  const res = await fetch("/api/auth/register/start", { method: "POST", credentials: "same-origin", headers: { "content-type": "application/json", ...(proof ? { "x-plantool-assertion": proof } : {}) }, body: JSON.stringify(body) });
  const start = await res.json() as Ceremony & { error?: string };
  reportSessionFailure(res);
  if (!res.ok) throw new Error(start.error ?? "Could not start passkey registration");
  const p = start.options.publicKey;
  const publicKey = { ...p, challenge: decode(p.challenge as string), user: { ...(p.user as PublicKeyCredentialUserEntity), id: decode((p.user as { id: string }).id) }, excludeCredentials: (p.excludeCredentials as { id: string; type: PublicKeyCredentialType }[] | undefined)?.map((c) => ({ ...c, id: decode(c.id) })) } as unknown as PublicKeyCredentialCreationOptions;
  const credential = await navigator.credentials.create({ publicKey }) as PublicKeyCredential | null;
  if (!credential) throw new Error("Passkey registration was cancelled");
  await authRequest("register/finish", { id: start.id, credential: credentialJSON(credential) });
}
export async function authenticatedFetch(path: string, init: RequestInit = {}, bodyValue?: unknown): Promise<Response> {
  const response = await fetch(path, { ...init, credentials: "same-origin" });
  reportSessionFailure(response);
  if (response.status !== 428) return response;
  let boundBody = bodyValue ?? null;
  if (init.body instanceof Blob) {
    const hash = await crypto.subtle.digest("SHA-256", await init.body.arrayBuffer());
    boundBody = { binary_sha256: [...new Uint8Array(hash)].map((b) => b.toString(16).padStart(2, "0")).join("") };
  }
  const proof = await authenticate({ method: init.method ?? "GET", path, body: boundBody });
  const headers = new Headers(init.headers);
  if (proof) headers.set("x-plantool-assertion", proof);
  const retried = await fetch(path, { ...init, credentials: "same-origin", headers });
  reportSessionFailure(retried);
  return retried;
}
