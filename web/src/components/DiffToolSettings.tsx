import { useState } from "react";
import { api } from "../api";
export function DiffToolSettings() {
  const [value, setValue] = useState("auto");
  const [message, setMessage] = useState<string | null>(null);
  const check = () => void api.diffToolSetting().then((r) => { setValue(r.value ?? "auto"); setMessage(`Difftool: ${r.difftool ?? "not installed"}. Built-in review is always available.`); }).catch((e: Error) => setMessage(e.message));
  return <details><summary onClick={check}>Default diff tool</summary>
    <select aria-label="Default diff tool" value={value} onChange={(e) => setValue(e.target.value)}>{["auto", "built-in", "difftool", "git-difftool"].map((v) => <option key={v} value={v}>{v}</option>)}</select>
    <button className="btn" type="button" onClick={() => void api.setDiffToolSetting(value).then(() => setMessage("Default saved. Session overrides still apply.")).catch((e: Error) => setMessage(e.message))}>Save diff tool default</button>
    <button className="btn ghost" type="button" onClick={check}>Re-check tools</button>
    {message && <p role="status">{message}</p>}
  </details>;
}
