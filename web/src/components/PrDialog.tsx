import { useEffect, useState } from "react";
import { api } from "../api";
import type { Session } from "../types";

type Preview = Awaited<ReturnType<typeof api.prPreview>>;

export function PrDialog({ sessionKey, onClose, onDraftAgent, onSaved }: { sessionKey: string; onClose: () => void; onDraftAgent: () => void; onSaved: (session: Session) => void }) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const [preflightError, setPreflightError] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [hasDraft, setHasDraft] = useState(false);
  const [mode, setMode] = useState<"draft" | "ready" | "">("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reload = () => {
    void api.prDraft(sessionKey).then((draft) => { setTitle(draft.title); setBody(draft.body); setHasDraft(draft.exists); }).catch((e: Error) => setError(e.message));
    void api.prPreview(sessionKey).then((result) => { setPreview(result); setPreflightError(null); }).catch((e: Error) => { setPreview(null); setPreflightError(e.message); });
  };
  useEffect(reload, [sessionKey]);
  const submit = async () => {
    if (!preview) return;
    setBusy(true);
    setError(null);
    try {
      if (!preview.existing) await api.savePrDraft(sessionKey, title, body);
      const result = await api.createPr(sessionKey, { head: preview.head, repository: preview.repository, title, body, draft: mode === "draft" });
      onSaved(result.session);
      onClose();
    } catch (e) { setError((e as Error).message); }
    finally { setBusy(false); }
  };
  return <div className="modal-backdrop" onClick={() => !busy && onClose()}><div className="modal pr-dialog" role="dialog" aria-modal="true" aria-label="Create pull request" onClick={(e) => e.stopPropagation()}>
    <h3>Pull request</h3>
    <p className="muted small">The agent writes the first draft. Review and edit it before creating the PR.</p>
    <div className="composer-actions"><button className="btn ghost" type="button" disabled={busy} onClick={onDraftAgent}>Draft with agent</button><button className="btn ghost" type="button" disabled={busy} onClick={reload}>Refresh draft and checks</button></div>
    {preflightError && <div className="error">{preflightError}</div>}
    {preview && <div className="pr-preflight"><div>Repository <strong>{preview.repository}</strong></div><div>Head <strong>{preview.head}</strong> → Base <strong>{preview.base}</strong></div><div>Push remote <strong>{preview.push_remote}</strong> · {preview.commits_ahead} commit{preview.commits_ahead === 1 ? "" : "s"} ahead</div>
      {preview.dirty && <div className="error">Uncommitted changes will not appear in the PR. Commit them before creating it if they belong there.</div>}
      {preview.existing && <div>An existing PR #{preview.existing.number} is {preview.existing.state.toLowerCase()}. Link it to this session.</div>}
      {!preview.existing && preview.commits_ahead === 0 && <div className="error">No committed changes are ahead of the base branch.</div>}
    </div>}
    <label>Title<input value={title} onChange={(e) => setTitle(e.target.value)} placeholder={hasDraft ? "PR title" : "Ask the agent to draft the PR"} /></label>
    <label>Body<textarea rows={8} value={body} onChange={(e) => setBody(e.target.value)} /></label>
    {!preview?.existing && <fieldset className="handoff-options"><legend>Create as</legend><label><input type="radio" name="pr-mode" checked={mode === "draft"} onChange={() => setMode("draft")} /> Draft</label><label><input type="radio" name="pr-mode" checked={mode === "ready"} onChange={() => setMode("ready")} /> Ready for review</label></fieldset>}
    {error && <div className="error">{error}</div>}
    <div className="composer-actions"><span className="spacer" /><button className="btn ghost" disabled={busy} onClick={onClose} type="button">Cancel</button><button className="btn primary" disabled={busy || !preview || (!preview.existing && (!hasDraft || !title.trim() || !body.trim() || !mode || preview.commits_ahead === 0))} onClick={() => void submit()} type="button">{preview?.existing ? "Link existing PR" : "Create PR"}</button></div>
  </div></div>;
}
