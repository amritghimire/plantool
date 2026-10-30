import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api } from "../api";
import type { RepoInfo } from "../types";
import { relTime } from "../lib/format";
import { useDialogFocus } from "../lib/dialog";

const OTHER = "__other__";
const slugFromTitle = (value: string) => value.toLowerCase().normalize("NFKD").replace(/[\u0300-\u036f]/g, "").replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 100);

export function NewSessionDialog({ onClose }: { onClose: () => void }) {
  const dialogRef = useDialogFocus();
  const nav = useNavigate();
  const [repos, setRepos] = useState<RepoInfo[] | null>(null);
  const [choice, setChoice] = useState<string>(OTHER);
  const [path, setPath] = useState("");
  const [slug, setSlug] = useState("");
  const [fallbackId] = useState(() => `session-${crypto.randomUUID().slice(0, 8)}`);
  const [slugEdited, setSlugEdited] = useState(false);
  const [title, setTitle] = useState("");
  const [brief, setBrief] = useState("");
  const [worktree, setWorktree] = useState(false);
  const [base, setBase] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => { if (event.key === "Escape" && !busy) onClose(); };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [busy, onClose]);
  useEffect(() => {
    api.repos()
      .then((r) => {
        setRepos(r.repos);
        if (r.repos.length) setChoice(r.repos[0].root);
      })
      .catch((e: Error) => setErr(e.message));
  }, []);
  const repo = choice === OTHER ? path.trim() : choice;
  const slugOk = /^[A-Za-z0-9_][A-Za-z0-9._-]{0,99}$/.test(slug);
  const create = async () => {
    setBusy(true);
    setErr(null);
    try {
      const r = await api.createSession({ slug, repo, title: title.trim() || undefined, brief: brief.trim() || undefined, worktree, base: base.trim() || undefined });
      onClose();
      nav(r.session.url_path);
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="modal-backdrop" onClick={() => { if (!busy) onClose(); }}>
      <div ref={dialogRef} tabIndex={-1} className="modal form-stack" role="dialog" aria-modal="true" aria-label="New session" onClick={(e) => e.stopPropagation()}>
        <h3>What would you like to build?</h3>
        <p className="dialog-intro">Start with a goal. Research and review the plan before the agent changes your code.</p>
        <label>
          Repository
          <select value={choice} onChange={(e) => setChoice(e.target.value)}>
            {(repos ?? []).map((r) => (
              <option key={r.root} value={r.root}>
                {r.root} · {r.branch} ({r.sessions} session{r.sessions === 1 ? "" : "s"}, {relTime(r.last_used)})
              </option>
            ))}
            <option value={OTHER}>Another path…</option>
          </select>
          {choice === OTHER && <input value={path} placeholder="/absolute/path/to/a/git/checkout" autoFocus onChange={(e) => setPath(e.target.value)} />}
        </label>
        <label>
          What are you working on?
          <input value={title} placeholder="Fix the login timeout" onChange={(e) => { const next = e.target.value; setTitle(next); if (!slugEdited) setSlug(slugFromTitle(next) || (next.trim() ? fallbackId : "")); }} />
        </label>
        <label>
          Brief <span className="muted small">(what the agent should research or build)</span>
          <textarea rows={4} value={brief} placeholder="Describe the goal, important constraints, and what done looks like." onChange={(e) => setBrief(e.target.value)} />
        </label>
        <details className="new-session-options disclosure"><summary>Workspace options</summary>
        <label>
          Session ID <span className="muted small">(used in links and file paths)</span>
          <input value={slug} placeholder="fix-login-timeout" onChange={(e) => { setSlugEdited(true); setSlug(e.target.value.trim()); }} />
          {slug && !slugOk && <span className="error small">Use letters, digits, hyphens, underscores, or periods.</span>}
        </label>

          <label>Base branch <span className="muted small">(default: origin/HEAD or main)</span><input value={base} onChange={(e) => setBase(e.target.value)} /></label>
          <label className="check check-row"><input type="checkbox" checked={worktree} onChange={(e) => setWorktree(e.target.checked)} /><span>Create a git worktree now <span className="muted small">(otherwise one is created when implementation starts)</span></span></label>
        </details>
        {slug && !slugOk && <p className="error">Open Workspace options to correct the session ID.</p>}
        {err && <div className="error" role="alert">{err}</div>}
        <div className="composer-actions button-row dialog-actions">
          <span className="spacer" />
          <button className="btn ghost" onClick={onClose} type="button">
            Cancel
          </button>
          <button className="btn primary" disabled={busy || !slugOk || !repo} onClick={() => void create()} type="button">
            {busy ? "Creating…" : "Create session"}
          </button>
        </div>
      </div>
    </div>
  );
}
