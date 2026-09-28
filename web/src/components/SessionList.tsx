import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import { relTime, shortPath, workspacePath } from "../lib/format";
import { STAGE_LABEL, type SessionView } from "../types";
import { NewSessionDialog } from "./NewSessionDialog";
import { SettingsDialog } from "./SettingsDialog";
import { ThemeToggle } from "./ThemeToggle";

export function SessionList() {
  const [sessions, setSessions] = useState<SessionView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [settings, setSettings] = useState(false);
  useEffect(() => {
    let active = true;
    let request = 0;
    const refresh = () => {
      if (document.visibilityState === "hidden") return;
      const current = ++request;
      api.sessions().then((items) => {
        if (!active || current !== request) return;
        setSessions(items);
        setError(null);
      }).catch((e: Error) => {
        if (active && current === request) setError(e.message);
      });
    };
    refresh();
    const timer = window.setInterval(refresh, 10_000);
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refresh);
    return () => {
      active = false;
      window.clearInterval(timer);
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", refresh);
    };
  }, [creating]);
  const groups = new Map<string, SessionView[]>();
  for (const s of sessions ?? []) {
    const list = groups.get(s.session.repo_slug) ?? [];
    list.push(s);
    groups.set(s.session.repo_slug, list);
  }
  return (
    <div className="page">
      <header className="topbar">
        <h1 className="brand">plantool</h1>
        <div className="spacer" />
        <button className="btn primary" onClick={() => setCreating(true)} type="button">
          New session
        </button>
        <button className="btn" onClick={() => setSettings(true)} type="button">
          Settings
        </button>
        <ThemeToggle />
      </header>
      {creating && <NewSessionDialog onClose={() => setCreating(false)} />}
      {settings && <SettingsDialog onClose={() => setSettings(false)} />}
      <main className="list">
        {error && <p className="error">{error}</p>}
        {sessions && sessions.length === 0 && (
          <div className="empty">
            <p>No sessions yet.</p>
            <pre>cd your-repo{"\n"}plantool new fix-something</pre>
          </div>
        )}
        {[...groups.entries()].map(([repo, items]) => (
          <section key={repo} className="repo-group">
            <h2>
              {repo} <span className="muted repo-path" title={items[0].session.repo.root}>{shortPath(items[0].session.repo.root, 4)}</span>
            </h2>
            <ul>
              {items.map((s) => {
                const attention = sessionAttention(s);
                return (
                  <li key={s.key}>
                    <Link to={s.url_path} className="session-row">
                      <span className={`stage-pill stage-${s.state.stage}`}>{STAGE_LABEL[s.state.stage]}</span>
                      <span className="session-details">
                        <span className="session-title">{s.session.title} <span className="muted">{s.session.slug}</span></span>
                        <span className="workspace-path" title={workspacePath(s.session)}><span className="muted">Workspace</span> {workspacePath(s.session)}</span>
                      </span>
                      {attention && <span className="attention">{attention}</span>}
                      {s.open_comments > 0 && <span className="badge">{s.open_comments} open</span>}
                      <span className="muted">{relTime(s.state.updated_at || s.session.created_at)}</span>
                    </Link>
                  </li>
                );
              })}
            </ul>
          </section>
        ))}
      </main>
    </div>
  );
}

function sessionAttention(view: SessionView): string | null {
  const active = [...view.runs].reverse().find((r) => ["starting", "running", "waiting", "idle"].includes(r.status));
  if (active?.status === "waiting") return "Needs input";
  if (active?.status === "idle") return "Your move";
  if (active) return "Agent working";
  if (["research-review", "plan-review", "implementation-review"].includes(view.state.stage)) return "Ready for review";
  return null;
}
