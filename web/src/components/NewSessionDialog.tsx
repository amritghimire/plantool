import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api } from "../api";
import type { RepoInfo } from "../types";
import { relTime } from "../lib/format";

const OTHER = "__other__";

export function NewSessionDialog({ onClose }: { onClose: () => void }) {
  const nav = useNavigate();
  const [repos, setRepos] = useState<RepoInfo[] | null>(null);
  const [choice, setChoice] = useState<string>(OTHER);
  const [path, setPath] = useState("");
  const [slug, setSlug] = useState("");
  const [title, setTitle] = useState("");
  const [brief, setBrief] = useState("");
  const [worktree, setWorktree] = useState(false);
  const [base, setBase] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
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
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>New session</h3>
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
          Slug
          <input value={slug} placeholder="fix-login-timeout" onChange={(e) => setSlug(e.target.value.trim())} />
          {slug && !slugOk && <span className="error small">letters, digits, - _ . only</span>}
        </label>
        <label>
          Title <span className="muted small">(optional, derived from the slug)</span>
          <input value={title} onChange={(e) => setTitle(e.target.value)} />
        </label>
        <label>
          Brief <span className="muted small">(what to research or build; goes into every prompt)</span>
          <textarea rows={4} value={brief} onChange={(e) => setBrief(e.target.value)} />
        </label>
        <label>
          Base branch <span className="muted small">(optional; default origin/HEAD or main)</span>
          <input value={base} onChange={(e) => setBase(e.target.value)} />
        </label>
        <label className="check">
          <input type="checkbox" checked={worktree} onChange={(e) => setWorktree(e.target.checked)} />
          <span>
            Create a git worktree now <span className="muted small">(otherwise one is created when implementation starts)</span>
          </span>
        </label>
        {err && <div className="error">{err}</div>}
        <div className="composer-actions">
          <span className="spacer" />
          <button className="btn ghost" onClick={onClose} type="button">
            Cancel
          </button>
          <button className="btn primary" disabled={busy || !slugOk || !repo} onClick={() => void create()} type="button">
            Create
          </button>
        </div>
      </div>
    </div>
  );
}
