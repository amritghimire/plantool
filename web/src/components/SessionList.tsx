import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import { relTime, shortPath, workspacePath } from "../lib/format";
import { STAGE_LABEL, type SessionView } from "../types";
import { DropSessionDialog } from "./DropSessionDialog";
import { NewSessionDialog } from "./NewSessionDialog";
import { SettingsDialog } from "./SettingsDialog";
import { ThemeToggle } from "./ThemeToggle";

type Filter = "all" | "needs-you" | "working" | "done";
const FILTERS: { id: Filter; label: string }[] = [
  { id: "all", label: "All" }, { id: "needs-you", label: "Needs you" },
  { id: "working", label: "Working" }, { id: "done", label: "Done" },
];
const LIVE = ["starting", "running", "waiting", "idle"];

export function sessionAttention(view: SessionView): { label: string; filter: Filter } | null {
  const active = [...view.runs].reverse().find((r) => LIVE.includes(r.status));
  if (active?.status === "waiting") return { label: "Needs input", filter: "needs-you" };
  if (active?.status === "idle") return { label: "Your move", filter: "needs-you" };
  if (active) return { label: "Agent working", filter: "working" };
  if (view.state.stage === "done") return { label: "Done", filter: "done" };
  if (["research-review", "plan-review", "implementation-review"].includes(view.state.stage)) return { label: "Ready for review", filter: "needs-you" };
  return null;
}

export function workspaceLabel(view: SessionView): string {
  const path = workspacePath(view.session);
  const root = view.session.repo.root;
  if (path === root) return ".";
  if (path.startsWith(`${root}/`)) return path.slice(root.length + 1);
  return shortPath(path, 2);
}

function matchesFilter(view: SessionView, filter: Filter): boolean {
  if (filter === "all") return true;
  if (filter === "done") return view.state.stage === "done";
  if (filter === "needs-you" && ["research-review", "plan-review", "implementation-review"].includes(view.state.stage)) return true;
  return sessionAttention(view)?.filter === filter;
}

export function SessionList() {
  const [sessions, setSessions] = useState<SessionView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [settings, setSettings] = useState(false);
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [dropping, setDropping] = useState<SessionView | null>(null);
  const [busy, setBusy] = useState(false);
  const refresh = useCallback(() => api.sessions().then((items) => { setSessions(items); setError(null); }).catch((e: Error) => setError(e.message)), []);
  useEffect(() => {
    let active = true;
    let request = 0;
    const poll = () => {
      if (!active || document.visibilityState === "hidden") return;
      const current = ++request;
      api.sessions().then((items) => { if (active && current === request) { setSessions(items); setError(null); } }).catch((e: Error) => { if (active && current === request) setError(e.message); });
    };
    poll();
    const timer = window.setInterval(poll, 10_000);
    window.addEventListener("focus", poll);
    document.addEventListener("visibilitychange", poll);
    return () => { active = false; window.clearInterval(timer); window.removeEventListener("focus", poll); document.removeEventListener("visibilitychange", poll); };
  }, [creating]);
  const search = query.trim().toLowerCase();
  const visible = (sessions ?? []).filter((s) => matchesFilter(s, filter) && (!search || [s.session.title, s.session.slug, s.session.repo_slug, s.session.repo.root, s.session.brief ?? "", s.workspace_branch ?? ""].some((value) => value.toLowerCase().includes(search)))).sort((a, b) => {
    const priority = (view: SessionView) => { const attention = sessionAttention(view)?.filter; return attention === "needs-you" ? 0 : attention === "working" ? 1 : attention === "done" ? 3 : 2; };
    return priority(a) - priority(b) || (b.state.updated_at || b.session.created_at).localeCompare(a.state.updated_at || a.session.created_at);
  });
  const groups = new Map<string, SessionView[]>();
  for (const s of visible) groups.set(s.session.repo_slug, [...(groups.get(s.session.repo_slug) ?? []), s]);
  const drop = async (removeWorktree: boolean) => {
    if (!dropping) return;
    setBusy(true);
    try { await api.removeSession(dropping.key, removeWorktree); setSessions((current) => current?.filter((item) => item.key !== dropping.key) ?? null); setDropping(null); await refresh(); }
    finally { setBusy(false); }
  };
  return <div className="page">
    <header className="topbar"><h1 className="brand">plantool</h1><div className="spacer" />
      <button className="btn primary" onClick={() => setCreating(true)} type="button">New session</button>
      <button className="btn" onClick={() => setSettings(true)} type="button">Settings</button><ThemeToggle />
    </header>
    {creating && <NewSessionDialog onClose={() => setCreating(false)} />}
    {settings && <SettingsDialog onClose={() => setSettings(false)} />}
    {dropping && <DropSessionDialog view={dropping} busy={busy} onClose={() => setDropping(null)} onDelete={drop} />}
    <main className="list">
      {error && <p className="error" role="alert">{error}</p>}
      {sessions && sessions.length > 0 && <div className="list-filters button-row" role="group" aria-label="Filter sessions">
        {FILTERS.map(({ id, label }) => <button key={id} className={filter === id ? "active" : ""} type="button" aria-pressed={filter === id} onClick={() => setFilter(id)}>{label} <span className="badge">{sessions.filter((s) => matchesFilter(s, id)).length}</span></button>)}
      </div>}
      {sessions && sessions.length > 0 && <input className="session-search" type="search" aria-label="Search sessions" placeholder="Search sessions, repositories, or briefs…" value={query} onChange={(event) => setQuery(event.target.value)} />}
      {sessions?.length === 0 && <div className="empty"><p>Start with a piece of work in a repository.</p><button className="btn primary" type="button" onClick={() => setCreating(true)}>Create your first session</button></div>}
      {sessions && sessions.length > 0 && visible.length === 0 && <div className="empty"><p>No matching sessions.</p><button className="btn" type="button" onClick={() => { setQuery(""); setFilter("all"); }}>Clear search and filters</button></div>}
      {[...groups.entries()].map(([repo, items]) => <section key={repo} className="repo-group"><h2>{repo} <span className="muted repo-path" title={items[0].session.repo.root}>{shortPath(items[0].session.repo.root, 4)}</span></h2><ul>
        {items.map((s) => {
          const attention = sessionAttention(s);
          const slugTitle = s.session.slug.replace(/[-_]/g, " ").toLowerCase();
          const showSlug = slugTitle !== s.session.title.toLowerCase();
          const live = s.runs.some((r) => LIVE.includes(r.status));
          return <li key={s.key} className="session-row">
            <Link to={s.url_path} className="session-main" aria-label={`Open ${s.session.title}`}>
              <span className={`stage-pill stage-${s.state.stage}`}>{STAGE_LABEL[s.state.stage]}</span>
              <span className="session-details"><span className="session-title">{s.session.title}{showSlug && <span className="muted small"> · {s.session.slug}</span>}</span><span className="workspace-path" title={workspacePath(s.session)}>{workspaceLabel(s)}{s.workspace_branch && <span className="muted"> · {s.workspace_branch}</span>}{s.session.worktree && !s.workspace_branch && <span className="error"> · Missing or detached</span>}</span></span>
              <span className="session-meta">{attention && <span className="attention">{attention.label}</span>}{s.open_comments > 0 && <span className="badge">{s.open_comments} open</span>}<span className="muted small">{relTime(s.state.updated_at || s.session.created_at)}</span></span>
            </Link>
            {s.session.pull_request && <a className="list-pr" href={s.session.pull_request.url} target="_blank" rel="noreferrer" aria-label={`Open PR #${s.session.pull_request.number}`} title={`${s.session.pull_request.state} · checked ${relTime(s.session.pull_request.updated_at)}`}>PR #{s.session.pull_request.number}</a>}
            <button className="list-drop" type="button" aria-label={`Drop ${s.session.title}`} title={live ? "Stop the live run before dropping this session" : "Drop session"} disabled={live} onClick={() => setDropping(s)}>×</button>
          </li>;
        })}
      </ul></section>)}
    </main>
  </div>;
}
