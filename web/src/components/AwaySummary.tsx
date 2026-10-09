import { useEffect, useState } from "react";
import type { State } from "../types";
export function AwaySummary({ sessionKey, state }: { sessionKey: string; state: State }) {
  const storageKey = `plantool.seen:${sessionKey}`;
  const [seen, setSeen] = useState(() => { try { return Number(localStorage.getItem(storageKey) ?? state.seq); } catch { return state.seq; } });
  useEffect(() => { try { if (localStorage.getItem(storageKey) === null) localStorage.setItem(storageKey, String(seen)); } catch { /* Storage may be disabled. */ } }, [storageKey, seen]);
  const updates = (state.activity ?? []).filter((a) => a.seq > seen);
  return <details className="away-summary"><summary>Since you were away · {updates.length} updates</summary>
    {updates.length ? <ul>{updates.map((a) => <li key={a.seq}><time dateTime={a.at}>{new Date(a.at).toLocaleString()}</time> · {a.summary}</li>)}</ul> : <p>No new recorded activity.</p>}
    <button className="btn ghost" type="button" onClick={() => { setSeen(state.seq); try { localStorage.setItem(storageKey, String(state.seq)); } catch { /* Storage may be disabled. */ } }}>Mark updates seen</button>
  </details>;
}
