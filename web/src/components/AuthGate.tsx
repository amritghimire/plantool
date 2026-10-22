import { useEffect, useState, type ReactNode } from "react";
import { authenticate, authRequest, registerPasskey } from "../auth";

export function AuthGate({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<{ registered: boolean; authenticated: boolean } | null>(null);
  const [code, setCode] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const refresh = () => authRequest<{ registered: boolean; authenticated: boolean }>("status").then(setStatus).catch((e: Error) => setError(e.message));
  useEffect(() => { void refresh(); }, []);
  const run = async (operation: () => Promise<unknown>) => {
    setBusy(true); setError("");
    try { await operation(); setCode(""); await refresh(); } catch (e) { setError((e as Error).message); } finally { setBusy(false); }
  };
  if (status?.authenticated) return <>
    <div className="toolbar" aria-label="Account"><button className="btn ghost" disabled={busy} onClick={() => void run(() => registerPasskey(null))}>Add passkey</button><button className="btn ghost" onClick={() => void run(() => authRequest("logout", {}))}>Sign out</button>{error && <p role="alert">{error}</p>}</div>
    {children}
  </>;
  return <main className="context-form" style={{ maxWidth: 440, margin: "10vh auto" }}>
    <h1>PlanTool</h1>
    {status ? status.registered ? <><p>Sign in with your passkey.</p><button className="btn" disabled={busy} onClick={() => void run(() => authenticate())}>Sign in</button></> : <>
      <p>Run <code>plantool setup</code> in your terminal, then enter its one-time code here to create your first passkey.</p>
      <form onSubmit={(e) => { e.preventDefault(); void run(() => registerPasskey(code.trim())); }}><label>Setup code<input autoComplete="off" value={code} onChange={(e) => setCode(e.target.value)} /></label><button className="btn" disabled={busy || !code.trim()}>Create passkey</button></form>
    </> : <p>Connecting…</p>}
    {error && <p role="alert">{error}</p>}
  </main>;
}
