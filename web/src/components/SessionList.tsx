import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import { relTime } from "../lib/format";
import { STAGE_LABEL, type SessionView } from "../types";
import { ThemeToggle } from "./ThemeToggle";

export function SessionList() {
  const [sessions, setSessions] = useState<SessionView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.sessions().then(setSessions).catch((e: Error) => setError(e.message));
  }, []);
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
        <ThemeToggle />
      </header>
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
            <h2>{repo}</h2>
            <ul>
              {items.map((s) => (
                <li key={s.key}>
                  <Link to={s.url_path} className="session-row">
                    <span className={`stage-pill stage-${s.state.stage}`}>{STAGE_LABEL[s.state.stage]}</span>
                    <span className="session-title">{s.session.title}</span>
                    <span className="muted">{s.session.slug}</span>
                    <span className="spacer" />
                    {s.open_comments > 0 && <span className="badge">{s.open_comments} open</span>}
                    <span className="muted">{relTime(s.state.updated_at || s.session.created_at)}</span>
                  </Link>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </main>
    </div>
  );
}
