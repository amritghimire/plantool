import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import * as Popover from "@radix-ui/react-popover";
import {
  BookOpenIcon,
  CaretDownIcon,
  CheckCircleIcon,
  CircleIcon,
  CircleNotchIcon,
  DotsThreeIcon,
  MagnifyingGlassIcon,
  PlusIcon,
  QuestionIcon,
  WarningCircleIcon,
} from "@phosphor-icons/react";
import { api } from "../api";
import type { SessionView } from "../types";
import { QUEUES, age, groupQueue, sessionAttention, type Mark } from "../lib/inbox";
import { STEPS, isLive } from "../lib/notice";
import { stepFocus, useShortcuts } from "../lib/shortcuts";
import { DropSessionDialog } from "./DropSessionDialog";
import { NewSessionDialog } from "./NewSessionDialog";
import { SettingsDialog } from "./SettingsDialog";
import { ThemeToggle } from "./ThemeToggle";
import { ShortcutsHelp } from "./ShortcutsHelp";

const DONE_CAP = 10;
const README = "https://github.com/amritghimire/plantool#readme";

const MARK_ICON: Record<Mark, typeof CircleIcon> = {
  waiting: CircleIcon,
  question: QuestionIcon,
  recovery: WarningCircleIcon,
  running: CircleNotchIcon,
  done: CheckCircleIcon,
  idle: CircleIcon,
};

const squash = (value: string) => value.toLowerCase().replace(/[^a-z0-9]/g, "");

function SessionRow({ view, onDrop }: { view: SessionView; onDrop: () => void }) {
  const a = sessionAttention(view);
  const s = view.session;
  const Icon = MARK_ICON[a.mark];
  const live = view.runs.some(isLive);
  const showSlug = squash(s.title) !== squash(s.slug);
  const pr = s.pull_request;
  return (
    <li className={`queue-row mark-${a.mark}`}>
      <span className="row-mark" title={a.status}>
        <Icon size={16} weight={a.mark === "waiting" ? "fill" : "regular"} aria-hidden="true" />
        <span className="sr-only">{a.status}</span>
      </span>
      <span className="row-title">
        <Link to={view.url_path} className="row-link" aria-describedby={`row-${view.key}`}>
          <span className="sr-only">Open </span>
          {s.title}
        </Link>
        {showSlug && <span className="row-slug">{s.slug}</span>}
      </span>
      <span className="row-repo" title={s.repo.root}>
        {s.repo_slug}
      </span>
      <span className="row-reason" id={`row-${view.key}`}>
        <span className="sr-only">{a.status}: </span>
        {a.action}
        {a.open > 0 && <span className="sr-only"> · {a.open} open comments</span>}
      </span>
      <span className="row-open" aria-hidden="true">
        {a.open > 0 ? `${a.open} open` : ""}
      </span>
      <span className="row-age" title={new Date(a.since).toLocaleString()}>
        {age(a.since)}
      </span>
      <span className="row-pr">
        {pr && (
          <a href={pr.url} target="_blank" rel="noreferrer" aria-label={`Open PR #${pr.number}`}>
            #{pr.number}
          </a>
        )}
      </span>
      <Popover.Root>
        <Popover.Trigger asChild>
          <button className="row-menu" type="button" aria-label={`More for ${s.title}`}>
            <DotsThreeIcon size={18} weight="bold" aria-hidden="true" />
          </button>
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content className="menu" align="end" sideOffset={4} collisionPadding={12} aria-label={`More for ${s.title}`}>
            <Popover.Close asChild>
              <button className="menu-item" type="button" disabled={live} onClick={onDrop}>
                Drop session…
              </button>
            </Popover.Close>
            {live && <p className="menu-label">Stop the live run before dropping this session.</p>}
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
    </li>
  );
}

export function FirstRun({ onCreate }: { onCreate: () => void }) {
  return (
    <section className="first-run form-box" aria-labelledby="first-run-title">
      <header>How plantool works</header>
      <div className="form-box-body">
        <h2 id="first-run-title">No sessions yet</h2>
        <ol className="first-run-figs" aria-label="Steps">
          {STEPS.map((name, i) => (
            <li key={name}>
              <span className="fig-n">FIG. {i + 1}</span> {name}
            </li>
          ))}
        </ol>
        <p>An agent writes the research and the plan. You comment on any line, and it revises.</p>
        <p>
          Only you can approve, here in the browser. The CLI never can.
        </p>
        <div className="first-run-actions">
          <button className="btn primary" type="button" onClick={onCreate}>
            <PlusIcon size={16} aria-hidden="true" />
            New session
          </button>
          <a className="link" href={README} target="_blank" rel="noreferrer">
            How it works
          </a>
        </div>
      </div>
    </section>
  );
}

export function SessionList() {
  const [params, setParams] = useSearchParams();
  const [sessions, setSessions] = useState<SessionView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(params.get("new") === "1");
  const [settings, setSettings] = useState(params.get("settings") === "1");
  const [query, setQuery] = useState("");
  const [allDone, setAllDone] = useState(false);
  const [dropping, setDropping] = useState<SessionView | null>(null);
  const [busy, setBusy] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const repo = params.get("repo") ?? "";
  const setParam = (key: string, value: string | null) =>
    setParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (value) next.set(key, value);
        else next.delete(key);
        return next;
      },
      { replace: true },
    );
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
  const rowLinks = () =>
    [...(listRef.current?.querySelectorAll<HTMLElement>(".row-link") ?? [])];
  useShortcuts({
    j: () => stepFocus(rowLinks(), 1),
    k: () => stepFocus(rowLinks(), -1),
    "/": () => searchRef.current?.focus(),
  });

  const search = query.trim().toLowerCase();
  const repositories = [
    ...(sessions ?? [])
      .reduce(
        (m, v) => m.set(v.session.repo_slug, (m.get(v.session.repo_slug) ?? 0) + 1),
        new Map<string, number>(),
      )
      .entries(),
  ].sort(([a], [b]) => a.localeCompare(b));
  const visible = (sessions ?? []).filter(
    (s) =>
      (!repo || s.session.repo_slug === repo) &&
      (!search ||
        [
          s.session.title,
          s.session.slug,
          s.session.repo_slug,
          s.session.repo.root,
          s.session.brief ?? "",
          s.workspace_branch ?? "",
        ].some((value) => value.toLowerCase().includes(search))),
  );
  const groups = groupQueue(visible);
  const clear = () => {
    setQuery("");
    setParam("repo", null);
  };
  const drop = async (removeWorktree: boolean) => {
    if (!dropping) return;
    setBusy(true);
    try {
      await api.removeSession(dropping.key, removeWorktree);
      setSessions(
        (current) => current?.filter((item) => item.key !== dropping.key) ?? null,
      );
      setDropping(null);
      await refresh();
    } finally {
      setBusy(false);
    }
  };
  const closeNew = () => {
    setCreating(false);
    setParam("new", null);
  };
  const closeSettings = () => {
    setSettings(false);
    setParam("settings", null);
  };
  return (
    <div className="inbox">
      <header className="inbox-head">
        <Link to="/" className="wordmark">
          plantool
        </Link>
        <h1>Sessions</h1>
        <div className="inbox-search">
          <MagnifyingGlassIcon size={16} aria-hidden="true" />
          <input
            ref={searchRef}
            type="search"
            aria-label="Search sessions"
            aria-keyshortcuts="/"
            placeholder="Search title, repo, branch, brief"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
          <kbd aria-hidden="true">/</kbd>
        </div>
        <div className="inbox-tools">
          <button className="link" type="button" onClick={() => setSettings(true)}>
            Settings
          </button>
          <a
            className="spine-icon"
            href={README}
            target="_blank"
            rel="noreferrer"
            aria-label="Documentation"
            title="Documentation"
          >
            <BookOpenIcon size={16} aria-hidden="true" />
          </a>
          <ThemeToggle className="spine-icon" size={16} />
          {sessions?.length !== 0 && (
            <button className="btn primary" onClick={() => setCreating(true)} type="button">
              <PlusIcon size={16} aria-hidden="true" />
              New session
            </button>
          )}
        </div>
      </header>
      <main className="inbox-body" ref={listRef}>
        {error && (
          <p className="error banner" role="alert">
            {error}
          </p>
        )}
        {repositories.length > 1 && (
          <label className="repo-filter">
            <span>Repository</span>
            <span className="select-wrap">
            <select value={repo} onChange={(e) => setParam("repo", e.target.value || null)}>
              <option value="">All repositories ({sessions?.length ?? 0})</option>
              {repositories.map(([name, n]) => (
                <option key={name} value={name}>
                  {name} ({n})
                </option>
              ))}
            </select>
            <CaretDownIcon className="select-caret" size={14} aria-hidden="true" />
            </span>
          </label>
        )}
        {!sessions && !error && (
          <div className="session-skeleton" role="status" aria-label="Loading sessions">
            {[0, 1, 2].map((n) => (
              <div key={n}>
                <span />
                <span />
                <span />
              </div>
            ))}
          </div>
        )}
        {sessions?.length === 0 && <FirstRun onCreate={() => setCreating(true)} />}
        {sessions && sessions.length > 0 && visible.length === 0 && (
          <div className="inbox-empty" role="status">
            <h2>No matching sessions</h2>
            <p>
              Nothing matches
              {search && (
                <>
                  {" "}
                  <code>{query.trim()}</code>
                </>
              )}
              {repo && (
                <>
                  {" "}in <code>{repo}</code>
                </>
              )}
              . Try a title, repository, branch or words from the brief.
            </p>
            <button className="btn" type="button" onClick={clear}>
              Clear search and filters
            </button>
          </div>
        )}
        {visible.length > 0 &&
          QUEUES.map(({ id, title }) => {
            const items = groups[id];
            if (!items.length && id !== "waiting") return null;
            const shown = id === "done" && !allDone ? items.slice(0, DONE_CAP) : items;
            return (
              <section key={id} className={`queue queue-${id}`} aria-labelledby={`queue-${id}`}>
                <h2 id={`queue-${id}`}>
                  {title}
                  <span className="queue-count">{items.length}</span>
                </h2>
                {items.length === 0 ? (
                  <p className="queue-empty">Nothing is waiting on you.</p>
                ) : (
                  <ul>
                    {shown.map((s) => (
                      <SessionRow key={s.key} view={s} onDrop={() => setDropping(s)} />
                    ))}
                  </ul>
                )}
                {shown.length < items.length && (
                  <button className="link queue-more" type="button" onClick={() => setAllDone(true)}>
                    Show all {items.length}
                  </button>
                )}
              </section>
            );
          })}
      </main>
      {creating && <NewSessionDialog onClose={closeNew} />}
      {settings && <SettingsDialog onClose={closeSettings} />}
      <ShortcutsHelp />
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
