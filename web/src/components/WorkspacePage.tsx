import { useEffect, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { api } from "../api";
import { usePolling } from "../lib/usePolling";
type Draft = { body: string; key: string };
const readMarkers = () => {
  try {
    return JSON.parse(
      localStorage.getItem("plantool.workspace.read") ?? "{}",
    ) as Record<string, string>;
  } catch {
    return {};
  }
};
export function WorkspacePage() {
  const { data, error, refresh } = usePolling(api.workspace);
  const [params, setParams] = useSearchParams();
  const [query, setQuery] = useState("");
  const [browsing, setBrowsing] = useState(false);
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const [sending, setSending] = useState("");
  const [sendError, setSendError] = useState("");
  const [markers, setMarkers] = useState(readMarkers);
  const requested = params.get("project");
  const project =
    data?.projects.find((p) => p.id === requested) ??
    (!requested ? data?.projects[0] : undefined);
  const threads = data?.threads.filter((t) => t.project === project?.id) ?? [];
  const chosen = params.get("thread");
  const item = params.get("item");
  const thread =
    threads.find((t) =>
      chosen ? t.id === chosen : item ? t.item === item : false,
    ) ??
    (!chosen && !item
      ? (threads.find((t) => t.channel === "general") ?? threads[0])
      : undefined);
  const unread = (t: (typeof threads)[number]) => {
    const index = t.messages.findIndex((m) => m.id === markers[t.id]);
    return markers[t.id] === t.messages.at(-1)?.id
      ? 0
      : t.messages.length - (index + 1);
  };
  useEffect(() => {
    const last = thread?.messages.at(-1)?.id;
    if (
      !thread ||
      !last ||
      document.visibilityState === "hidden" ||
      markers[thread.id] === last
    )
      return;
    const next = { ...markers, [thread.id]: last };
    setMarkers(next);
    try {
      localStorage.setItem("plantool.workspace.read", JSON.stringify(next));
    } catch {
      /* Read state still works for this tab. */
    }
  }, [thread, markers]);
  const choose = (id: string) => {
    setBrowsing(false);
    setParams({ project: project!.id, thread: id });
    setSendError("");
  };
  const matches = threads.filter(
    (t) =>
      !query ||
      `${t.title} ${t.channel ?? ""} ${t.messages.map((m) => m.body).join(" ")}`
        .toLowerCase()
        .includes(query.toLowerCase()),
  );
  const reply = async () => {
    if (!thread) return;
    const draft = drafts[thread.id];
    if (!draft?.body.trim()) return;
    const id = thread.id;
    setSending(id);
    setSendError("");
    try {
      await api.workspaceReply({
        thread: id,
        body: draft.body,
        idempotency_key: draft.key,
        ...(thread.question ? {question_id:thread.question.id} : {}),
      });
      setDrafts((d) => ({
        ...d,
        [id]: { body: "", key: crypto.randomUUID() },
      }));
      refresh();
    } catch (e) {
      setSendError((e as Error).message);
    } finally {
      setSending("");
    }
  };
  return (
    <main className="workspace-page">
      <aside className={`workspace-sidebar ${browsing ? "browsing" : ""}`}>
        <div className="eyebrow">TEAM WORKSPACE</div>
        <h1>Workspace</h1>
        <label>
          Project
          <select
            value={project?.id ?? ""}
            onChange={(e) => {
              setParams({ project: e.target.value });
              setQuery("");
              setSendError("");
            }}
          >
            <option value="" disabled>
              Select a project
            </option>
            {data?.projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name ?? p.id}
              </option>
            ))}
          </select>
        </label>
        <label className="workspace-search">
          Search this project
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Channels, threads or messages"
          />
        </label>
        <button
          className="btn workspace-nav-toggle"
          aria-expanded={browsing}
          onClick={() => setBrowsing((v) => !v)}
        >
          Channels & item threads
        </button>
        <nav
          className={browsing ? "open" : ""}
          aria-label="Workspace channels and threads"
        >
          {[
            ["Channels", matches.filter((t) => t.channel)],
            ["Item threads", matches.filter((t) => !t.channel)],
          ].map(([name, list]) => (
            <section key={name as string}>
              <h2>{name as string}</h2>
              {(list as typeof threads).map((t) => (
                <button
                  key={t.id}
                  aria-current={thread?.id === t.id ? "page" : undefined}
                  onClick={() => choose(t.id)}
                >
                  <span>{t.channel ? `# ${t.channel}` : t.title}</span>
                  {unread(t) > 0 && (
                    <span
                      className="unread-badge"
                      aria-label={`${unread(t)} unread messages`}
                    >
                      {unread(t)}
                    </span>
                  )}
                </button>
              ))}
            </section>
          ))}
        </nav>
        <p className="workspace-read-note small muted">
          Unread markers are saved in this browser. All threads are
          project-visible; no DMs.
        </p>
        <Link
          to={
            project
              ? `/engineering?project=${encodeURIComponent(project.id)}`
              : "/engineering"
          }
        >
          ← Project overview
        </Link>
      </aside>
      <section
        className="workspace-conversation"
        aria-label="Discussion thread"
      >
        <header className="conversation-header">
          <div>
            <div className="eyebrow">{project?.name ?? "ENGINEERING OS"}</div>
            <h2>{thread?.title ?? "Team discussions"}</h2>
          </div>
          <button className="btn" onClick={refresh}>
            Refresh messages
          </button>
        </header>
        {thread?.channel === "ai-activity" && <p className="muted">Detailed AI discussions, attempts and automatic retries. Questions, approvals and material blockers appear in #general and item threads.</p>}
        {thread?.channel === "general" && <p className="muted">Decisions and updates that matter to you. Routine model activity is in #ai-activity.</p>}
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        {!data ? (
          <p role="status">Loading workspace…</p>
        ) : !project ? (
          <div className="empty-state">
            <h3>
              {requested ? "Project not found" : "Connect your first project"}
            </h3>
            <Link to="/setup/engineering">Open guided setup</Link>
          </div>
        ) : !thread ? (
          <div className="empty-state">
            <h3>
              {chosen || item ? "Thread not found" : "No discussions yet"}
            </h3>
            <p>
              Project channels and item threads appear as the scheduler and
              agents begin work.
            </p>
          </div>
        ) : (
          <>
            <div
              className="message-list"
              role="log"
              aria-label="Project messages"
            >
              {thread.messages.length ? (
                thread.messages.map((m) => (
                  <article className="workspace-message" key={m.id}>
                    <span
                      className={`role-avatar ${m.sender === "owner" ? "owner" : ""}`}
                    >
                      {m.sender.slice(0, 2).toUpperCase()}
                    </span>
                    <div>
                      <header>
                        <strong>{m.sender}</strong>
                        <time dateTime={m.at}>
                          {new Date(m.at).toLocaleString()}
                        </time>
                      </header>
                      <p>{m.body}</p>
                      {m.sender === "daemon" && m.body.match(/\/engineering(?:\/items\/|\?project=)[A-Za-z0-9%._~-]+/) && <Link to={m.body.match(/\/engineering(?:\/items\/|\?project=)[A-Za-z0-9%._~-]+/)![0]}>View item and next action →</Link>}
                    </div>
                  </article>
                ))
              ) : (
                <div className="empty-state">
                  <h3>Start with context</h3>
                  <p>
                    Your replies help the team understand the work. Approval
                    decisions stay on GitHub.
                  </p>
                </div>
              )}
            </div>
            <form
              className="context-composer"
              onSubmit={(e) => {
                e.preventDefault();
                void reply();
              }}
            >
              {thread.question && <section className="panel" role="status"><h3>Answer needed</h3><p>{thread.question.question}</p>{thread.question.settling && <p>Recording the question… You can answer when this finishes.</p>}<p>Your answer queues a read-only review. It does not approve implementation or change approval gates.</p></section>}
              <label htmlFor="owner-context">{thread.question ? "Your answer" : "Owner reply · context only"}</label>
              <textarea
                id="owner-context"
                placeholder={`Share context in ${thread.title}`}
                value={drafts[thread.id]?.body ?? ""}
                disabled={sending === thread.id}
                onChange={(e) =>
                  setDrafts((d) => ({
                    ...d,
                    [thread.id]: {
                      body: e.target.value,
                      key: crypto.randomUUID(),
                    },
                  }))
                }
              />
              <div className="section-heading">
                <span className="small muted">
                  Messages never approve plans, sprints or milestones.
                </span>
                <button
                  className="btn primary"
                  disabled={!!sending || !!thread.question?.settling || !drafts[thread.id]?.body.trim()}
                >
                  {sending === thread.id ? "Sending…" : thread.question ? "Answer question" : "Send context"}
                </button>
              </div>
              {sendError && <p role="alert">{sendError}</p>}
            </form>
          </>
        )}
        {query && matches.length === 0 && (
          <p className="small muted">No discussions match this search.</p>
        )}
      </section>
    </main>
  );
}
