import { useState } from "react";
import { api, type PromptExtras } from "../api";

export function PromptExtrasSettings() {
  const [status, setStatus] = useState<PromptExtras | null>(null);
  const [force, setForce] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const check = () => void api.promptExtras().then(setStatus).catch((e: Error) => setMessage(e.message));
  const install = async () => {
    setBusy(true);
    setMessage(null);
    try {
      const r = await api.installPromptExtras(force);
      setMessage(`Installed ${r.installed.length ? r.installed.join(", ") : "nothing"}.${r.kept.length ? ` Kept your ${r.kept.join(", ")} files.` : ""} Downloaded ${r.skills.length} skills for agents that do not have them installed.`);
      check();
    } catch (e) {
      setMessage((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return <details className="disclosure settings-group form-stack"><summary onClick={check}>Skill hints for agent runs (optional)</summary>
    <p className="muted small">Each step can add a short file of instructions to its agent prompt, such as which installed skills to use (ponytail, poteto-mode, tdd, interrogate). Nothing is added until a file exists. Downloading also fetches the named skills from their own repositories (ponytail, tstack, pstack) into <code>~/.plantool/skills</code>, so agents without them installed can read them. The recommended set lives in the plantool repository{status && <> (<a href={status.source} target="_blank" rel="noreferrer">view on GitHub</a>)</>}.</p>
    {status && <p className="small">Files in <code>{status.dir}</code>: {status.files.map((f) => `${f.stage} ${f.installed ? "✓" : "–"}`).join(" · ")}</p>}
    <label className="check-row"><input type="checkbox" checked={force} onChange={(e) => setForce(e.target.checked)} /> Replace files I already have</label>
    <div className="button-row"><button className="btn" type="button" disabled={busy} onClick={() => void install()}>{busy ? "Downloading…" : "Download recommended hints"}</button></div>
    {message && <p role="status">{message}</p>}
  </details>;
}
