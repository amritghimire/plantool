import { useState } from "react";
import { token } from "../api";
export function ArchivePanel({ sessionKey, repo }: { sessionKey: string; repo: string }) {
  const [transcripts, setTranscripts] = useState(false);
  const [revisions, setRevisions] = useState(false);
  const [repository, setRepository] = useState(repo);
  const [workspace, setWorkspace] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const download = async () => {
    setBusy(true); setError(null);
    try {
      const response = await fetch(`/api/sessions/${sessionKey}/export?transcripts=${transcripts}&revisions=${revisions}`, { headers: { "x-plantool-actor": token() } });
      if (!response.ok) throw new Error(await response.text());
      const url = URL.createObjectURL(await response.blob());
      const link = document.createElement("a"); link.href = url; link.download = `${sessionKey.split("/").pop()}.zip`; link.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (e) { setError((e as Error).message); } finally { setBusy(false); }
  };
  const upload = async (file: File) => {
    setBusy(true); setError(null);
    try {
      const query = new URLSearchParams({ repo: repository }); if (workspace) query.set("workspace", workspace);
      const response = await fetch(`/api/import?${query}`, { method: "POST", headers: { "x-plantool-actor": token(), "content-type": "application/zip" }, body: file });
      if (!response.ok) throw new Error(await response.text());
      const result = await response.json() as { session: { url_path: string } }; window.location.assign(result.session.url_path);
    } catch (e) { setError((e as Error).message); } finally { setBusy(false); }
  };
  return <details className="side-block disclosure context-form"><summary>Export or import session</summary>
    <p>Documents, comments, decisions, a summary, and approved and last-viewed revisions are included.</p>
    <label className="check-row"><input type="checkbox" checked={transcripts} onChange={(e) => setTranscripts(e.target.checked)} /> Include agent transcripts</label>
    <label className="check-row"><input type="checkbox" checked={revisions} onChange={(e) => setRevisions(e.target.checked)} /> Include all revisions</label>
    <button className="btn" type="button" disabled={busy} onClick={() => void download()}>Download zip</button>
    <label>Import repository <input value={repository} onChange={(e) => setRepository(e.target.value)} /></label>
    <label>Import workspace (optional) <input value={workspace} onChange={(e) => setWorkspace(e.target.value)} /></label>
    <label>Import zip <input type="file" accept=".zip" disabled={busy || !repository.trim()} onChange={(e) => { const file = e.target.files?.[0]; if (file) void upload(file); }} /></label>
    <p className="muted">An existing session keeps its files. The import gets a new name.</p>
    {error && <p role="alert">{error}</p>}
  </details>;
}
