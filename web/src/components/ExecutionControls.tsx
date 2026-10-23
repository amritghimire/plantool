import { useEffect, useState } from "react";
import { api } from "../api";
export function ExecutionControls() {
  const [stopped, setStopped] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    const refresh = () => void api.executionStatus().then((r) => { if (active) setStopped(r.execution.stopped); }).catch((e: Error) => { if (active) setError(e.message); });
    refresh(); const timer = window.setInterval(refresh, 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);
  const act = async (resume: boolean) => {
    setBusy(true); setError("");
    try { const result = await (resume ? api.resumeExecution() : api.stopAll()); setStopped(result.execution.stopped); }
    catch (e) { setError((e as Error).message); }
    finally { setBusy(false); }
  };
  return <div className="toolbar" aria-label="Execution controls">
    <button className="btn danger" disabled={busy} onClick={() => void act(false)}>Stop All</button>
    {stopped && <><span role="status">Execution stopped. Runs stay stopped until you start them again.</span><button className="btn" disabled={busy} onClick={() => void act(true)}>Resume execution with passkey</button></>}
    {error && <p role="alert">{error}</p>}
  </div>;
}
