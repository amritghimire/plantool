import { useEffect, useState } from "react";
import type { State } from "../types";
export function AwaySummary(props: { sessionKey: string; state: State }) {
  return <SessionActivity key={props.sessionKey} {...props} />;
}
function SessionActivity({ sessionKey, state }: { sessionKey: string; state: State }) {
  const storageKey = `plantool.seen:${sessionKey}`;
  const [seen, setSeen] = useState(() => { try { return Number(window.localStorage.getItem(storageKey) ?? state.seq); } catch { return state.seq; } });
  useEffect(() => { try { if (window.localStorage.getItem(storageKey) === null) window.localStorage.setItem(storageKey, String(seen)); } catch { /* Storage may be disabled. */ } }, [storageKey, seen]);
  const updates = (state.activity ?? []).filter((a) => a.seq > seen);
  if (!updates.length) return null;
  return <details className="away-summary side-block disclosure"><summary>Since you were away · {updates.length} updates</summary>
    {updates.length ? <ul>{updates.map((a) => <li key={a.seq}><time dateTime={a.at}>{new Date(a.at).toLocaleString()}</time> · {a.summary}</li>)}</ul> : <p>No new recorded activity.</p>}
    <button className="btn ghost" type="button" onClick={() => { setSeen(state.seq); try { window.localStorage.setItem(storageKey, String(state.seq)); } catch { /* Storage may be disabled. */ } }}>Mark updates seen</button>
  </details>;
}
