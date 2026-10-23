import { useState } from "react";
import { api } from "../api";
import type { RepoInfo } from "../types";
export function NeverSentSettings({ repos }: { repos: RepoInfo[] }) {
  const [repo, setRepo] = useState("");
  const [patterns, setPatterns] = useState("");
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const load = async (root: string) => {
    setRepo(root); setMessage(""); setBusy(true);
    try { if (root) { const result = await api.neverSent(root); setPatterns(result.patterns.join("\n")); } }
    catch (e) { setMessage((e as Error).message); }
    finally { setBusy(false); }
  };
  const save = async () => {
    setBusy(true); setMessage("");
    try { await api.setNeverSent(repo, patterns.split("\n").map((p) => p.trim()).filter(Boolean)); setMessage("Never-sent rules saved. Resume execution with your passkey when ready."); }
    catch (e) { setMessage((e as Error).message); }
    finally { setBusy(false); }
  };
  return <details className="disclosure settings-group form-stack"><summary>Never-sent files</summary>
    <p>Always protected: .env*, *.pem, *.key and secrets/ at any depth. Use Stop All before changing these additional project rules.</p>
    <label>Protected repository<select value={repo} disabled={busy} onChange={(e) => void load(e.target.value)}><option value="">Select repository</option>{repos.map((r) => <option key={r.root} value={r.root}>{r.root}</option>)}</select></label>
    <label>Additional never-sent globs<textarea value={patterns} onChange={(e) => setPatterns(e.target.value)} placeholder={"private/**\nconfig/credentials.json"} /></label>
    <p>One relative glob per line; *, ** and ? are supported. Defaults cannot be removed.</p>
    <button className="btn" disabled={busy || !repo} onClick={() => void save()}>Save protected paths with passkey</button>
    {message && <p role="status">{message}</p>}
  </details>;
}
