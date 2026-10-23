import { createContext, useContext, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { authenticate, authRequest, registerPasskey, SESSION_CHANGED_EVENT } from "../auth";

const AccountContext = createContext<{ busy: boolean; error: string; addPasskey: () => void; signOut: () => void } | null>(null);

export function AccountMenu() {
  const account = useContext(AccountContext);
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!open) return;
    const dismiss = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", dismiss);
    return () => document.removeEventListener("pointerdown", dismiss);
  }, [open]);
  if (!account) return null;
  return <div className="account-menu" ref={root} onKeyDown={(event) => {
    if (event.key === "Escape") { setOpen(false); trigger.current?.focus(); }
  }} onBlur={(event) => {
    if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false);
  }}>
    <button ref={trigger} className="btn" type="button" aria-expanded={open} onClick={() => setOpen(!open)}>Account <span aria-hidden="true">▾</span></button>
    {open && <div className="account-panel" role="group" aria-label="Account actions">
      <div className="muted small account-label">Account</div>
      <button className="btn ghost" type="button" disabled={account.busy} onClick={account.addPasskey}>Add passkey</button>
      <button className="btn ghost account-signout" type="button" disabled={account.busy} onClick={account.signOut}>Sign out</button>
      {account.error && <p className="error small" role="alert">{account.error}</p>}
    </div>}
  </div>;
}

export function AuthGate({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<{ registered: boolean; authenticated: boolean } | null>(null);
  const [code, setCode] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const refreshVersion = useRef(0);
  const refresh = useCallback(async () => {
    const version = ++refreshVersion.current;
    try {
      const next = await authRequest<{ registered: boolean; authenticated: boolean }>("status");
      if (version === refreshVersion.current) setStatus(next);
    } catch (e) {
      if (version === refreshVersion.current) { setStatus(null); setError((e as Error).message); }
    }
  }, []);
  useEffect(() => {
    const recheck = () => { void refresh(); };
    const visible = () => { if (document.visibilityState === "visible") recheck(); };
    recheck();
    window.addEventListener("focus", recheck);
    window.addEventListener("online", recheck);
    window.addEventListener(SESSION_CHANGED_EVENT, recheck);
    document.addEventListener("visibilitychange", visible);
    return () => {
      ++refreshVersion.current;
      window.removeEventListener("focus", recheck);
      window.removeEventListener("online", recheck);
      window.removeEventListener(SESSION_CHANGED_EVENT, recheck);
      document.removeEventListener("visibilitychange", visible);
    };
  }, [refresh]);
  const run = async (operation: () => Promise<unknown>) => {
    setBusy(true); setError("");
    try { await operation(); setCode(""); await refresh(); } catch (e) { setError((e as Error).message); } finally { setBusy(false); }
  };
  if (status?.authenticated) return <AccountContext.Provider value={{ busy, error,
    addPasskey: () => void run(() => registerPasskey(null)),
    signOut: () => void run(() => authRequest("logout", {})),
  }}>{children}</AccountContext.Provider>;
  return <main className="context-form" style={{ maxWidth: 440, margin: "10vh auto" }}>
    <h1>PlanTool</h1>
    {status ? status.registered ? <><p>Sign in with your passkey.</p><button className="btn" disabled={busy} onClick={() => void run(() => authenticate())}>Sign in</button></> : <>
      <p>Run <code>plantool setup</code> in your terminal, then enter its one-time code here to create your first passkey.</p>
      <form onSubmit={(e) => { e.preventDefault(); void run(() => registerPasskey(code.trim())); }}><label>Setup code<input autoComplete="off" value={code} onChange={(e) => setCode(e.target.value)} /></label><button className="btn" disabled={busy || !code.trim()}>Create passkey</button></form>
    </> : <p>Connecting…</p>}
    {error && <p role="alert">{error}</p>}
  </main>;
}
