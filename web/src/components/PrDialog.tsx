import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { CommitJob, Session } from "../types";
import { CommitProgress } from "./CommitProgress";

type Preview = Awaited<ReturnType<typeof api.prPreview>>;

export function blockReason(preview: Preview | null, preflightError: string | null, form: { title: string; body: string }): string | null {
  if (!preview) return preflightError ? "Checks failed; see the error above" : "Running checks…";
  if (preview.existing) return null;
  if (preview.commits_ahead === 0) return preview.dirty ? "Commit your changes first" : `No commits ahead of ${preview.base}`;
  if (!form.title.trim() && !form.body.trim()) return "Draft the PR or write a title and body";
  if (!form.title.trim()) return "Title is empty";
  if (!form.body.trim()) return "Body is empty";
  return null;
}

function Checks({ preview, busy, onRefresh }: { preview: Preview; busy: boolean; onRefresh: () => void }) {
  return <div className="pr-preflight">
    <div className="pr-checks-head"><strong>Checks</strong><span className="spacer" /><button className="btn ghost small" type="button" disabled={busy} onClick={onRefresh}>Refresh</button></div>
    <div>Repository <strong>{preview.repository}</strong></div>
    <div>Head <strong>{preview.head}</strong> → Base <strong>{preview.base}</strong></div>
    <div>Push remote <strong>{preview.push_remote}</strong> · {preview.commits_ahead} commit{preview.commits_ahead === 1 ? "" : "s"} ahead</div>
    {preview.existing && <div>An existing PR #{preview.existing.number} is {preview.existing.state.toLowerCase()}. Link it to this session.</div>}
  </div>;
}

function CommitSection({ preview, message, onMessage, busy, error, onCommit, commit, onCancelCommit }: { preview: Preview; message: string; onMessage: (message: string) => void; busy: boolean; error: string | null; onCommit: () => void; commit: CommitJob | null; onCancelCommit: () => void }) {
  const count = preview.changed_files.length;
  return <div className="pr-notice">
    <div><strong>{count} uncommitted file{count === 1 ? "" : "s"}</strong> will not be in the PR. Commit them here if they belong there.</div>
    {count > 0 && <details><summary className="small">Files in this commit</summary><ul>{preview.changed_files.map((file) => <li key={file}>{file}</li>)}</ul></details>}
    <label>Commit message<textarea rows={4} value={message} onChange={(e) => onMessage(e.target.value)} disabled={busy} placeholder="Subject line, blank line, body" /></label>
    {preview.live_run && <div className="muted small">An agent run is live in this checkout. It may be editing files while you commit.</div>}
    {error && <div className="error">{error}</div>}
    {commit ? <CommitProgress job={commit} label="Committing…" onCancel={onCancelCommit} /> : <div className="composer-actions"><span className="spacer" /><button className="btn primary" type="button" disabled={busy || !message.trim()} onClick={onCommit}>Commit all changes</button></div>}
  </div>;
}

export function PrDialog({ sessionKey, commit, onCancelCommit, onClose, onDraftAgent, onSaved }: { sessionKey: string; commit: CommitJob | null; onCancelCommit: () => void; onClose: () => void; onDraftAgent: () => void; onSaved: (session: Session) => void }) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const [preflightError, setPreflightError] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [hasDraft, setHasDraft] = useState(false);
  const [mode, setMode] = useState<"draft" | "ready">("draft");
  const [commitMessage, setCommitMessage] = useState("");
  const commitEdited = useRef(false);
  const [commitError, setCommitError] = useState<string | null>(null);
  const [commitNote, setCommitNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const loadPreview = () => api.prPreview(sessionKey).then((result) => { setPreview(result); setPreflightError(null); }).catch((e: Error) => { setPreview(null); setPreflightError(e.message); });
  const reload = () => {
    void api.prDraft(sessionKey).then((draft) => { setTitle(draft.title); setBody(draft.body); setHasDraft(draft.exists); }).catch((e: Error) => setError(e.message));
    void api.prCommitDraft(sessionKey).then((draft) => { if (!commitEdited.current) setCommitMessage(draft.message); }).catch(() => {});
    void loadPreview();
  };
  useEffect(reload, [sessionKey]);
  const commitAll = async () => {
    setBusy(true);
    setCommitError(null);
    setCommitNote(null);
    try {
      const result = await api.prCommit(sessionKey, commitMessage);
      commitEdited.current = false;
      setCommitNote(`Committed ${result.sha.slice(0, 12)}${result.rewritten.length ? `; hooks rewrote ${result.rewritten.join(", ")}` : ""}`);
      await loadPreview();
    } catch (e) { setCommitError((e as Error).message); }
    finally { setBusy(false); }
  };
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
  const reason = blockReason(preview, preflightError, { title, body });
  return <div className="modal-backdrop" onClick={() => !busy && onClose()}><div className="modal pr-dialog" role="dialog" aria-modal="true" aria-label="Create pull request" onClick={(e) => e.stopPropagation()}>
    <h3>Pull request</h3>
    <p className="muted small">The agent writes the first draft. Review and edit it before creating the PR.</p>
    <div className="composer-actions"><button className={`btn ${hasDraft ? "ghost" : "primary"}`} type="button" disabled={busy} onClick={onDraftAgent}>{hasDraft ? "Redraft with agent" : "Draft with agent"}</button></div>
    {preflightError && <div className="error">{preflightError}</div>}
    {preview && <Checks preview={preview} busy={busy} onRefresh={reload} />}
    {preview?.dirty && <CommitSection preview={preview} message={commitMessage} onMessage={(message) => { commitEdited.current = true; setCommitMessage(message); }} busy={busy} error={commitError} onCommit={() => void commitAll()} commit={commit} onCancelCommit={onCancelCommit} />}
    {commitNote && <div className="muted small">{commitNote}</div>}
    {preview && !preview.dirty && !preview.existing && preview.commits_ahead === 0 && <div className="pr-notice">Nothing to open a PR with yet: no commits ahead of {preview.base}.</div>}
    <label>Title<input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Short summary of the change" /></label>
    <label>Body<textarea rows={8} value={body} onChange={(e) => setBody(e.target.value)} placeholder="What changed and why" /></label>
    {!preview?.existing && <fieldset className="handoff-options"><legend>Create as</legend><label><input type="radio" name="pr-mode" checked={mode === "draft"} onChange={() => setMode("draft")} /> Draft</label><label><input type="radio" name="pr-mode" checked={mode === "ready"} onChange={() => setMode("ready")} /> Ready for review</label></fieldset>}
    {error && <div className="error">{error}</div>}
    <div className="composer-actions"><span className="spacer" /><button className="btn ghost" disabled={busy} onClick={onClose} type="button">Cancel</button><button className="btn primary" disabled={busy || reason !== null} title={reason ?? undefined} onClick={() => void submit()} type="button">{preview?.existing ? "Link existing PR" : "Create PR"}</button></div>
    {reason && <p className="muted small pr-reason">{reason}</p>}
  </div></div>;
}
