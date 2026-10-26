import { useEffect, useState } from "react";
import { api } from "../api";
export function ExecutionControls() {
  const [stopped, setStopped] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [message,setMessage]=useState("");
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    const refresh = () => void api.executionStatus().then((r) => { if (active) setStopped(r.execution.stopped); }).catch((e: Error) => { if (active) setError(e.message); });
    refresh(); const timer = window.setInterval(refresh, 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);
  const act = async (resume: boolean) => {
    setBusy(true); setError("");
    try {
      if (resume) {
        const result=await api.resumeExecution();setStopped(result.execution.stopped);
        if(result.recovery)setMessage(`${result.recovery.eligible} eligible workflows can continue. ${result.recovery.attention.length} paused or uncertain items still need review. ${(result.warnings??[]).join(" ")}`);
      } else { const result=await api.stopAll();setStopped(result.execution.stopped);setMessage(""); }
    }
    catch (e) { setError((e as Error).message); }
    finally { setBusy(false); }
  };
  return <div className="execution-controls" aria-label="Execution controls">
    <button className="btn danger" disabled={busy} onClick={() => void act(false)} title="Stop all projects and Personal runs">Stop All</button>
    {stopped === false && <span className="execution-state">Execution enabled</span>}
    {stopped && <><span role="status">Execution stopped. Resume restores eligible workflows; paused decisions and uncertain work still need review.</span><button className="btn" disabled={busy} onClick={() => void act(true)}>Resume execution with passkey</button></>}
    {message&&<p role="status">{message}</p>}
    {error && <p role="alert">{error}</p>}
  </div>;
}
