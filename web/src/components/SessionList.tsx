import { useCallback, useEffect, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { api } from "../api";
import { relTime, shortPath, workspacePath } from "../lib/format";
import { STAGE_LABEL, type SessionView } from "../types";
import { DropSessionDialog } from "./DropSessionDialog";
import { NewSessionDialog } from "./NewSessionDialog";
import { SettingsDialog } from "./SettingsDialog";
import { AppRail } from "./AppRail";
import {
  Stack,
  Plus,
  FolderSimple,
  MagnifyingGlass,
  GitBranch,
  GitPullRequest,
  TrashSimple,
  ArrowUpRight,
  CheckCircle,
  Circle,
  Desktop,
} from "@phosphor-icons/react";
import { cn } from "../lib/cn";

type Filter = "all" | "needs-you" | "working" | "done";
const FILTERS: { id: Filter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "needs-you", label: "Needs you" },
  { id: "working", label: "Working" },
  { id: "done", label: "Done" },
];
const LIVE = ["starting", "running", "waiting", "idle"];

export function sessionAttention(
  view: SessionView,
): { label: string; filter: Filter } | null {
  const active = [...view.runs].reverse().find((r) => LIVE.includes(r.status));
  if (active?.status === "waiting")
    return { label: "Needs input", filter: "needs-you" };
  if (active?.status === "idle")
    return { label: "Your move", filter: "needs-you" };
  if (active) return { label: "Agent working", filter: "working" };
  if (view.state.stage === "done") return { label: "Done", filter: "done" };
  if (
    ["research-review", "plan-review", "implementation-review"].includes(
      view.state.stage,
    )
  )
    return { label: "Ready for review", filter: "needs-you" };
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
  if (
    filter === "needs-you" &&
    ["research-review", "plan-review", "implementation-review"].includes(
      view.state.stage,
    )
  )
    return true;
  return sessionAttention(view)?.filter === filter;
}

export function SessionList() {
  const [params, setParams] = useSearchParams();
  const [sessions, setSessions] = useState<SessionView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(params.get("new") === "1");
  const [settings, setSettings] = useState(params.get("settings") === "1");
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [dropping, setDropping] = useState<SessionView | null>(null);
  const [busy, setBusy] = useState(false);
  const refresh = useCallback(
    () =>
      api
        .sessions()
        .then((items) => {
          setSessions(items);
          setError(null);
        })
        .catch((e: Error) => setError(e.message)),
    [],
  );
  useEffect(() => {
    let active = true;
    let request = 0;
    const poll = () => {
      if (!active || document.visibilityState === "hidden") return;
      const current = ++request;
      api
        .sessions()
        .then((items) => {
          if (active && current === request) {
            setSessions(items);
            setError(null);
          }
        })
        .catch((e: Error) => {
          if (active && current === request) setError(e.message);
        });
    };
    poll();
    const timer = window.setInterval(poll, 10_000);
    window.addEventListener("focus", poll);
    document.addEventListener("visibilitychange", poll);
    return () => {
      active = false;
      window.clearInterval(timer);
      window.removeEventListener("focus", poll);
      document.removeEventListener("visibilitychange", poll);
    };
  }, [creating]);
  const search = query.trim().toLowerCase();
  const visible = (sessions ?? [])
    .filter(
      (s) =>
        matchesFilter(s, filter) &&
        (!search ||
          [
            s.session.title,
            s.session.slug,
            s.session.repo_slug,
            s.session.repo.root,
            s.session.brief ?? "",
            s.workspace_branch ?? "",
          ].some((value) => value.toLowerCase().includes(search))),
    )
    .sort((a, b) => {
      const priority = (view: SessionView) => {
        const attention = sessionAttention(view)?.filter;
        return attention === "needs-you"
          ? 0
          : attention === "working"
            ? 1
            : attention === "done"
              ? 3
              : 2;
      };
      return (
        priority(a) - priority(b) ||
        (b.state.updated_at || b.session.created_at).localeCompare(
          a.state.updated_at || a.session.created_at,
        )
      );
    });
  const groups = new Map<string, SessionView[]>();
  for (const s of visible)
    groups.set(s.session.repo_slug, [
      ...(groups.get(s.session.repo_slug) ?? []),
      s,
    ]);
  const drop = async (removeWorktree: boolean) => {
    if (!dropping) return;
    setBusy(true);
    try {
      await api.removeSession(dropping.key, removeWorktree);
      setSessions(
        (current) =>
          current?.filter((item) => item.key !== dropping.key) ?? null,
      );
      setDropping(null);
      await refresh();
    } finally {
      setBusy(false);
    }
  };
  const closeNew = () => {
    setCreating(false);
    setParams({}, { replace: true });
  };
  const closeSettings = () => {
    setSettings(false);
    setParams({}, { replace: true });
  };
  const repositories = [
    ...new Map(
      (sessions ?? []).map((view) => [
        view.session.repo_slug,
        view.session.repo.root,
      ]),
    ).entries(),
  ];
  return (
    <div className="inbox-layout">
      <AppRail
        onNew={() => setCreating(true)}
        onSettings={() => setSettings(true)}
      />
      <aside className="inbox-sidebar">
        <div className="workspace-brand">
          <Stack size={23} aria-hidden="true" />
          <strong>plantool</strong>
          <span>Workspace</span>
        </div>
        <button
          type="button"
          className={cn(
            "inbox-nav-item",
            !search && filter === "all" && "active",
          )}
          onClick={() => {
            setQuery("");
            setFilter("all");
          }}
        >
          <FolderSimple size={18} aria-hidden="true" />
          All sessions<span>{sessions?.length ?? "—"}</span>
        </button>
        <h2>Repositories</h2>
        <nav aria-label="Repositories">
          {repositories.map(([name, path]) => (
            <button
              type="button"
              className={cn(
                "inbox-nav-item",
                search === name.toLowerCase() && "active",
              )}
              key={path}
              title={path}
              onClick={() => {
                setQuery(name);
                setFilter("all");
              }}
            >
              <GitBranch size={17} aria-hidden="true" />
              <span className="repository-name">{name}</span>
              <span>
                {
                  sessions?.filter((view) => view.session.repo_slug === name)
                    .length
                }
              </span>
            </button>
          ))}
        </nav>
        {!repositories.length && (
          <p className="muted small">
            Your repositories appear here after your first session.
          </p>
        )}
        <div className="inbox-local">
          <Desktop size={18} aria-hidden="true" />
          <div>
            <strong>Local workspace</strong>
            <span>Research. Plan. Build. Review.</span>
          </div>
        </div>
      </aside>
      <main className="inbox-main">
        <header className="topbar inbox-topbar">
          <span>Workspace</span>
          <span className="muted">/</span>
          <strong>Sessions</strong>
          <span className="spacer" />
          <button
            className="btn primary"
            onClick={() => setCreating(true)}
            type="button"
          >
            <Plus size={17} aria-hidden="true" />
            New session
          </button>
        </header>
        <div className="inbox-content">
          <header className="inbox-heading">
            <div>
              <h1>Your sessions</h1>
              <p>Turn an idea into a change you can confidently approve.</p>
            </div>
            <span className="session-total">
              {sessions
                ? `${sessions.length} session${sessions.length === 1 ? "" : "s"}`
                : "Loading sessions"}
            </span>
          </header>
          {error && (
            <p className="error banner" role="alert">
              {error}
            </p>
          )}
          {sessions && sessions.length > 0 && (
            <div className="inbox-controls">
              <div
                className="list-filters"
                role="group"
                aria-label="Filter sessions"
              >
                {FILTERS.map(({ id, label }) => (
                  <button
                    key={id}
                    className={cn(filter === id && "active")}
                    type="button"
                    aria-pressed={filter === id}
                    onClick={() => setFilter(id)}
                  >
                    {label}
                    <span>
                      {
                        sessions.filter((view) => matchesFilter(view, id))
                          .length
                      }
                    </span>
                  </button>
                ))}
              </div>
              <div className="inbox-search">
                <MagnifyingGlass size={17} aria-hidden="true" />
                <input
                  type="search"
                  aria-label="Search sessions"
                  placeholder="Find a session…"
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                />
              </div>
            </div>
          )}
          {!sessions && !error && (
            <div
              className="session-skeleton"
              role="status"
              aria-label="Loading sessions"
            >
              {[0, 1, 2].map((n) => (
                <div key={n}>
                  <span />
                  <span />
                  <span />
                </div>
              ))}
            </div>
          )}
          {sessions?.length === 0 && (
            <div className="inbox-empty">
              <Stack size={38} aria-hidden="true" />
              <h2>Your next idea starts here</h2>
              <p>
                Choose a repository, describe what you need, and review the
                approach before your agent starts building.
              </p>
              <button
                className="btn primary"
                type="button"
                onClick={() => setCreating(true)}
              >
                <Plus size={17} aria-hidden="true" />
                Create your first session
              </button>
            </div>
          )}
          {sessions && sessions.length > 0 && visible.length === 0 && (
            <div className="inbox-empty">
              <MagnifyingGlass size={32} aria-hidden="true" />
              <h2>No matching sessions</h2>
              <p>Try a different title, repository, or brief.</p>
              <button
                className="btn"
                type="button"
                onClick={() => {
                  setQuery("");
                  setFilter("all");
                }}
              >
                Clear search and filters
              </button>
            </div>
          )}
          {[...groups.entries()].map(([repo, items]) => (
            <section key={repo} className="repo-group">
              <h2>
                <FolderSimple size={17} aria-hidden="true" />
                {repo}
                <span className="badge">{items.length}</span>
                <span className="repo-path" title={items[0].session.repo.root}>
                  {shortPath(items[0].session.repo.root, 4)}
                </span>
              </h2>
              <ul>
                {items.map((s) => {
                  const attention = sessionAttention(s);
                  const live = s.runs.some((run) => LIVE.includes(run.status));
                  const showSlug = s.session.title !== s.session.slug;
                  return (
                    <li key={s.key} className="session-row">
                      <Link
                        to={s.url_path}
                        className="session-main"
                        aria-describedby={`session-meta-${s.key}`}
                      >
                        <span className="sr-only">Open </span>
                        <span
                          className={cn("session-row-icon", attention?.filter)}
                        >
                          {s.state.stage === "done" ? (
                            <CheckCircle size={22} aria-hidden="true" />
                          ) : (
                            <Circle size={22} aria-hidden="true" />
                          )}
                        </span>
                        <span className="session-details">
                          <span className="session-title">
                            {s.session.title}
                          </span>
                          <span
                            className="workspace-path"
                            aria-hidden="true"
                            title={workspacePath(s.session)}
                          >
                            {workspaceLabel(s)}
                            {s.workspace_branch && (
                              <span> · {s.workspace_branch}</span>
                            )}
                            {s.session.worktree && !s.workspace_branch && (
                              <span className="error">
                                {" "}
                                · Missing or detached
                              </span>
                            )}
                          </span>
                          {showSlug && (
                            <span className="sr-only" aria-hidden="true">
                              {s.session.slug}
                            </span>
                          )}
                        </span>
                        <span
                          aria-hidden="true"
                          className={`stage-pill stage-${s.state.stage}`}
                        >
                          {STAGE_LABEL[s.state.stage]}
                        </span>
                        <span aria-hidden="true" className="session-meta">
                          {attention && (
                            <span className="attention">{attention.label}</span>
                          )}
                          {s.open_comments > 0 && (
                            <span className="badge">
                              {s.open_comments} open
                            </span>
                          )}
                          <span className="muted small">
                            {relTime(
                              s.state.updated_at || s.session.created_at,
                            )}
                          </span>
                        </span>
                        <ArrowUpRight
                          size={17}
                          className="session-open-icon"
                          aria-hidden="true"
                        />
                      </Link>
                      <span className="sr-only" id={`session-meta-${s.key}`}>
                        {STAGE_LABEL[s.state.stage]}. Workspace:{" "}
                        {workspacePath(s.session)}. {attention?.label}.{" "}
                        {s.open_comments} open comments.
                      </span>
                      {s.session.pull_request && (
                        <a
                          className="list-pr"
                          href={s.session.pull_request.url}
                          target="_blank"
                          rel="noreferrer"
                          aria-label={`Open PR #${s.session.pull_request.number}`}
                        >
                          <GitPullRequest size={15} aria-hidden="true" />#
                          {s.session.pull_request.number}
                        </a>
                      )}
                      <button
                        className="list-drop"
                        type="button"
                        aria-label={`Drop ${s.session.title}`}
                        title={
                          live
                            ? "Stop the live run before dropping this session"
                            : "Drop session"
                        }
                        disabled={live}
                        onClick={() => setDropping(s)}
                      >
                        <TrashSimple size={17} aria-hidden="true" />
                      </button>
                    </li>
                  );
                })}
              </ul>
            </section>
          ))}
        </div>
      </main>
      {creating && <NewSessionDialog onClose={closeNew} />}
      {settings && <SettingsDialog onClose={closeSettings} />}
      {dropping && (
        <DropSessionDialog
          view={dropping}
          busy={busy}
          onClose={() => setDropping(null)}
          onDelete={drop}
        />
      )}
    </div>
  );
}
