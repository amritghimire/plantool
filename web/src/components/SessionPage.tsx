import { cn } from "../lib/cn";
import { ReviewPreferences, VIEW_MODES } from "./ReviewPreferences";
import { useConfirmation } from "../lib/confirmation";
import { XIcon } from "@phosphor-icons/react";
import { PlanRevisions } from "./PlanRevisions";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Link,
  useNavigate,
  useParams,
  useSearchParams,
} from "react-router-dom";
import { api, authorName, setAuthorName } from "../api";
import { useLive } from "../lib/live";
import { docKindOf, tabForStage } from "../lib/stage";
import { isLive, openThreads, resumeStageFor, RUN_WORD } from "../lib/notice";
import { groupQueue } from "../lib/inbox";
import { documentOrder, referenceNumerals, stepThread } from "../lib/comments";
import { useShortcuts } from "../lib/shortcuts";
import type {
  Comment,
  DocKind,
  DocResponse,
  LiveEvent,
  Run,
  SessionView,
  Stage,
} from "../types";
import { ChangesTab } from "./ChangesTab";
import { FilesTab } from "./FilesTab";
import { DocView, type ViewMode } from "./DocView";
import type { RunLine } from "./RunPanel";
import { StartRunDialog, type RunStage } from "./StartRunDialog";
import { PrDialog } from "./PrDialog";
import { AskAgentDialog } from "./AskAgentDialog";
import { SettingsDialog } from "./SettingsDialog";
import type { ThreadActions } from "./Thread";
import { SessionSpine } from "./SessionSpine";
import { Drawer, drawerFrom, type DrawerId } from "./Drawer";
import { DetailsDrawer } from "./DetailsDrawer";
import { CommentsDrawer } from "./CommentsDrawer";
import { AgentDrawer } from "./AgentDrawer";
import { ShortcutsHelp } from "./ShortcutsHelp";

type Toast = { id: number; text: string; kind?: "info" | "error" | "warn" };

export function SessionPage() {
  const { confirm, dialog: confirmation } = useConfirmation();
  const { repo, slug } = useParams();
  const key = `${repo}/${slug}`;
  const [params, setParams] = useSearchParams();
  const nav = useNavigate();
  const [view, setView] = useState<SessionView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [docs, setDocs] = useState<
    Partial<Record<DocKind, DocResponse | null>>
  >({});
  const [paths, setPaths] = useState<Partial<Record<DocKind, string>>>({});
  const [prompts, setPrompts] = useState<Partial<Record<DocKind, string>>>({});
  const [reviewPrompt, setReviewPrompt] = useState<string | null>(null);
  const [comments, setComments] = useState<Comment[]>([]);
  const numerals = useMemo(() => referenceNumerals(comments), [comments]);
  const [tab, setTabState] = useState<string>(params.get("tab") ?? "plan");
  const [mode, setModeState] = useState<ViewMode>(() => {
    const m = localStorage.getItem("plantool.mode");
    return m === "source" || m === "slides" ? m : "rendered";
  });
  const setMode = (m: ViewMode) => {
    setModeState(m);
    localStorage.setItem("plantool.mode", m);
  };
  const [target, setTarget] = useState<{
    line: number;
    nonce: number;
    compose?: boolean;
  } | null>(null);
  const [highlight, setHighlight] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [dialog, setDialog] = useState<{
    stage: RunStage;
    resumeId?: string;
    initialMode?: "step-by-step";
    initialPrompt?: string;
  } | null>(null);
  const [askOpen, setAskOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [prOpen, setPrOpen] = useState(false);
  const [selectedRun, setSelectedRun] = useState<string | null>(
    params.get("run"),
  );
  const [runLines, setRunLines] = useState<Record<string, RunLine[]>>({});
  const [changesNonce, setChangesNonce] = useState(0);
  const [author, setAuthor] = useState(authorName());
  const toastId = useRef(0);
  const headObserver = useRef<ResizeObserver | null>(null);
  const observeHead = useCallback((el: HTMLDivElement | null) => {
    headObserver.current?.disconnect();
    if (!el || typeof ResizeObserver === "undefined") return;
    const sync = () =>
      el
        .closest<HTMLElement>(".session")
        ?.style.setProperty("--canvas-head-h", `${el.offsetHeight}px`);
    headObserver.current = new ResizeObserver(sync);
    headObserver.current.observe(el);
    sync();
  }, []);
  const [waiting, setWaiting] = useState<number | null>(null);
  useEffect(() => {
    let cancelled = false;
    const load = () =>
      api
        .sessions()
        .then((all) => {
          if (!cancelled) setWaiting(groupQueue(all).waiting.length);
        })
        .catch(() => {
          if (!cancelled) setWaiting(null);
        });
    void load();
    window.addEventListener("focus", load);
    return () => {
      cancelled = true;
      window.removeEventListener("focus", load);
    };
  }, []);
  const viewRef = useRef<SessionView | null>(null);
  useEffect(() => {
    viewRef.current = view;
  }, [view]);

  const toast = useCallback((text: string, kind: Toast["kind"] = "info") => {
    const id = ++toastId.current;
    setToasts((t) => [...t, { id, text, kind }]);
    if (kind === "info")
      window.setTimeout(
        () => setToasts((t) => t.filter((x) => x.id !== id)),
        4000,
      );
  }, []);

  const setTab = (t: string, file?: string) => {
    setTabState(t);
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.set("tab", t);
        if (file) next.set("file", file);
        return next;
      },
      { replace: true },
    );
  };
  const openFile = (path: string) => setTab("files", path);

  const drawer = drawerFrom(params.get("drawer"));
  const drawerRef = useRef<DrawerId | null>(drawer);
  drawerRef.current = drawer;
  const setDrawer = useCallback(
    (d: DrawerId | null) => {
      setParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          if (d) next.set("drawer", d);
          else next.delete("drawer");
          return next;
        },
        { replace: true },
      );
    },
    [setParams],
  );
  const toggleDrawer = (d: DrawerId) => setDrawer(drawer === d ? null : d);
  const closeDrawer = useCallback(() => setDrawer(null), [setDrawer]);
  useEffect(() => {
    if (params.get("run") && !drawer) setDrawer("agent");
  }, []);
  const showRun = useCallback(
    (id: string, force = false) => {
      setSelectedRun(id);
      if (force || drawerRef.current === null) setDrawer("agent");
    },
    [setDrawer],
  );

  const stage = view?.state.stage ?? null;
  const seenStage = useRef<Stage | null>(null);
  useEffect(() => {
    if (!stage) return;
    const prev = seenStage.current;
    seenStage.current = stage;
    if (prev === stage) return;
    if (prev === null && params.get("tab")) return;
    setTab(tabForStage(stage));
  }, [stage]);

  const loadPrompt = useCallback(
    async (kind: DocKind) => {
      const stage =
        kind === "research" ? "research" : kind === "plan" ? "plan" : null;
      if (!stage) return;
      try {
        const p = await api.prompt(key, stage);
        setPrompts((m) => ({ ...m, [kind]: p.prompt }));
      } catch {
        // the empty state still shows the path
      }
    },
    [key],
  );

  const loadDoc = useCallback(
    async (kind: DocKind) => {
      try {
        const d = await api.doc(key, kind);
        setDocs((m) => ({ ...m, [kind]: d }));
      } catch {
        setDocs((m) => ({ ...m, [kind]: null }));
        try {
          const p = await api.docPath(key, kind);
          setPaths((m) => ({ ...m, [kind]: p.path }));
        } catch {
          // ignore
        }
        void loadPrompt(kind);
      }
    },
    [key, loadPrompt],
  );

  const loadReviewPrompt = useCallback(() => {
    api
      .prompt(key, "review")
      .then((r) => setReviewPrompt(r.prompt))
      .catch(() => setReviewPrompt(null));
  }, [key]);

  const reloadPrompts = useCallback(() => {
    setDocs((m) => {
      for (const k of Object.keys(m) as DocKind[])
        if (m[k] === null) void loadPrompt(k);
      return m;
    });
    loadReviewPrompt();
  }, [loadPrompt, loadReviewPrompt]);

  const loadAll = useCallback(async () => {
    try {
      const v = await api.session(key);
      setView(v);
      setError(null);
      const c = await api.comments(key);
      setComments(c.comments);
      loadReviewPrompt();
      await Promise.all(v.docs.map((d) => loadDoc(d.kind)));
    } catch (e) {
      setError((e as Error).message);
    }
  }, [key, loadDoc, loadReviewPrompt]);

  useEffect(() => {
    void loadAll();
  }, [loadAll]);

  useEffect(() => {
    const waiting = view?.runs.some((r) => r.status === "waiting")
      ? "● "
      : view?.runs.some((r) => r.status === "idle")
        ? "○ "
        : "";
    document.title = view
      ? `${waiting}${view.session.title} · plantool`
      : "plantool";
  }, [view]);

  useEffect(() => {
    if (!selectedRun) return;
    api
      .runEvents(key, selectedRun)
      .then((r) =>
        setRunLines((m) => ({
          ...m,
          [selectedRun]: r.events.map((e) => ({
            seq: e.seq,
            at: e.at,
            event: e.event as RunLine["event"],
          })),
        })),
      )
      .catch(() => {});
  }, [key, selectedRun]);

  const onEvent = useCallback(
    (e: LiveEvent) => {
      if (
        [
          "stage-changed",
          "run-started",
          "run-updated",
          "run-ended",
          "run-removed",
        ].includes(e.type)
      )
        void api
          .session(key)
          .then(setView)
          .catch(() => {});
      switch (e.type) {
        case "comment-added":
          setComments((c) =>
            c.some((x) => x.id === e.comment.id) ? c : [...c, e.comment],
          );
          if (e.comment.kind === "agent")
            toast(
              `${e.comment.author} commented on ${e.comment.doc}:${e.comment.anchor.line}`,
            );
          break;
        case "comments-added":
          setComments((c) => [
            ...c,
            ...e.comments.filter((n) => !c.some((x) => x.id === n.id)),
          ]);
          toast(`${e.comments.length} comments added`);
          break;
        case "comments-since":
          setComments((c) => [
            ...c,
            ...e.comments.filter((n) => !c.some((x) => x.id === n.id)),
          ]);
          break;
        case "comment-updated":
          setComments((c) =>
            c.map((x) => (x.id === e.comment.id ? e.comment : x)),
          );
          break;
        case "comment-removed":
          setComments((c) => c.filter((x) => !e.ids.includes(x.id)));
          break;
        case "thread-resolution":
          setComments((c) =>
            c.map((x) =>
              e.ids.includes(x.id) || (x.parent && e.ids.includes(x.parent))
                ? { ...x, resolved: e.resolved }
                : x,
            ),
          );
          break;
        case "doc-refreshed":
          void loadDoc(e.kind);
          void api
            .session(key)
            .then(setView)
            .catch(() => {});
          void api
            .comments(key)
            .then((c) => setComments(c.comments))
            .catch(() => {});
          toast(
            `${e.kind}.md updated${e.outdated ? ` · ${e.outdated} comment(s) outdated` : ""}`,
          );
          break;
        case "stage-changed":
          setView((v) =>
            v ? { ...v, state: { ...v.state, stage: e.to } } : v,
          );
          toast(`stage: ${e.to}`);
          loadReviewPrompt();
          break;
        case "session-updated":
          void api
            .session(key)
            .then(setView)
            .catch(() => {});
          setView((v) => (v ? { ...v, session: e.session } : v));
          reloadPrompts();
          break;
        case "session-removed":
          nav("/", { replace: true });
          break;
        case "run-started":
        case "run-updated":
        case "run-ended": {
          const prev = viewRef.current?.runs.find((r) => r.id === e.run.id);
          setView((v) => (v ? { ...v, runs: upsertRun(v.runs, e.run) } : v));
          if (e.type === "run-started") showRun(e.run.id);
          if (
            e.type === "run-updated" &&
            prev &&
            prev.status !== e.run.status
          ) {
            if (e.run.status === "waiting")
              toast("The agent needs your answer", "warn");
            else if (e.run.status === "idle" && prev.status === "running")
              toast("The agent finished its turn; your move");
          }
          if (e.type === "run-ended") {
            setChangesNonce((n) => n + 1);
            void api
              .comments(key)
              .then((r) => setComments(r.comments))
              .catch(() => {});
          }
          if (e.type === "run-ended")
            toast(
              `run ${e.run.status}${e.run.error ? `: ${e.run.error}` : ""}`,
              e.run.status === "failed" ? "error" : "info",
            );
          break;
        }
        case "run-removed":
          setView((v) =>
            v ? { ...v, runs: v.runs.filter((r) => r.id !== e.id) } : v,
          );
          setRunLines((m) => {
            const { [e.id]: _gone, ...rest } = m;
            return rest;
          });
          setSelectedRun((s) => (s === e.id ? null : s));
          break;
        case "run-event":
          setRunLines((m) => {
            const prev = m[e.run_id] ?? [];
            if (prev.length && prev[prev.length - 1].seq >= e.seq) return m;
            return {
              ...m,
              [e.run_id]: [...prev, { seq: e.seq, at: e.at, event: e.event }],
            };
          });
          break;
        case "commit-progress": {
          const commit = e.commit;
          const running =
            commit.phase === "staging" || commit.phase === "hooks";
          setView((v) => (v ? { ...v, commit: running ? commit : null } : v));
          break;
        }
        case "changes-opened":
          setView((v) =>
            v ? { ...v, state: { ...v.state, review: e.review } } : v,
          );
          setChangesNonce((n) => n + 1);
          break;
        case "navigate": {
          const t = e.target;
          if (t.tab) setTab(t.tab);
          else if (t.doc) setTab(t.doc);
          if (t.comment) setHighlight(t.comment);
          if (t.line) setTarget({ line: t.line, nonce: Date.now() });
          break;
        }
        default:
          break;
      }
    },
    [key, loadDoc, reloadPrompts, loadReviewPrompt, toast, showRun],
  );

  const { connected } = useLive(view ? key : null, onEvent, loadAll);

  const actions: ThreadActions = useMemo(
    () => ({
      promote: async (root) => {
        await api.promoteBlocker(key, root.id);
      },
      reply: async (parent, body) => {
        const r = await api.addComment(key, {
          doc: parent.doc,
          parent: parent.id,
          body,
        });
        setComments((c) => [
          ...c,
          ...r.comments.filter((n) => !c.some((x) => x.id === n.id)),
        ]);
      },
      resolve: async (root, resolved) => {
        await api.resolve(key, [root.id], resolved);
      },
      edit: async (c, body) => {
        await api.editComment(key, c.id, body);
      },
      remove: async (c) => {
        if (
          !(await confirm("Delete this comment? This cannot be undone.", true))
        )
          return;
        await api.removeComments(key, [c.id]);
      },
    }),
    [key, confirm],
  );

  const onAdd = async (
    kind: DocKind,
    line: number,
    body: string,
    type?: import("../types").CommentType,
    scope?: import("../types").AnchorScope,
  ) => {
    const r = await api.addComment(key, { doc: kind, line, body, type, scope });
    setComments((c) => [
      ...c,
      ...r.comments.filter((n) => !c.some((x) => x.id === n.id)),
    ]);
  };

  const onStage = async (s: Stage, overrideReason?: string) => {
    setBusy(true);
    try {
      await api.setStage(
        key,
        s,
        overrideReason,
        s === "approved" ? docs.plan?.sha : undefined,
      );
    } catch (e) {
      toast((e as Error).message, "error");
    } finally {
      setBusy(false);
    }
  };

  const onBrief = async (brief: string | null) => {
    setBusy(true);
    try {
      const r = await api.setBrief(key, brief);
      setView((v) => (v ? { ...v, session: r.session } : v));
      reloadPrompts();
    } catch (e) {
      toast((e as Error).message, "error");
      throw e;
    } finally {
      setBusy(false);
    }
  };

  const onSendToRun = async (text: string) => {
    const live = view?.runs.find(
      (r) =>
        r.status === "starting" ||
        r.status === "running" ||
        r.status === "waiting" ||
        r.status === "idle",
    );
    if (!live) {
      toast(
        "No run is live; start one or paste the prompt into your agent.",
        "error",
      );
      return;
    }
    setBusy(true);
    try {
      await api.runInput(key, live.id, { text });
      setSelectedRun(live.id);
      toast("Sent to the running agent");
    } catch (e) {
      toast((e as Error).message, "error");
    } finally {
      setBusy(false);
    }
  };

  const onContinueMilestone = async (milestoneRun: Run) => {
    if (milestoneRun.status !== "idle") {
      setDialog({
        stage: "implement",
        resumeId: milestoneRun.provider_session_id
          ? milestoneRun.id
          : undefined,
        initialMode: "step-by-step",
      });
      return;
    }
    setBusy(true);
    try {
      await api.runInput(key, milestoneRun.id, {
        text: "I reviewed the completed milestone. Continue with exactly the next unchecked plan task, then pause for review again. If no tasks remain, run the final checks and move to implementation-review.",
      });
      showRun(milestoneRun.id);
    } catch (e) {
      toast((e as Error).message, "error");
    } finally {
      setBusy(false);
    }
  };

  const onCancelCommit = () =>
    void api.cancelCommit(key).catch((e: Error) => toast(e.message, "error"));

  const onApproveMilestone = async (
    milestoneRun: Run,
    commit: boolean,
    message: string,
  ) => {
    setBusy(true);
    try {
      const r = await api.approveMilestone(
        key,
        milestoneRun.id,
        commit,
        message,
      );
      showRun(milestoneRun.id);
      setChangesNonce((n) => n + 1);
      toast(
        r.committed
          ? `Milestone committed as ${r.sha.slice(0, 12)}; the agent is continuing`
          : "Milestone approved; the agent is continuing",
      );
    } catch (e) {
      setChangesNonce((n) => n + 1);
      toast((e as Error).message, "error");
    } finally {
      setBusy(false);
    }
  };

  const onDelete = async (removeWorktree: boolean) => {
    setBusy(true);
    try {
      await api.removeSession(key, removeWorktree);
      nav("/", { replace: true });
    } catch (e) {
      toast((e as Error).message, "error");
    } finally {
      setBusy(false);
    }
  };

  const onRemoveRun = async (id: string) => {
    try {
      await api.removeRun(key, id);
      setView((v) =>
        v ? { ...v, runs: v.runs.filter((r) => r.id !== id) } : v,
      );
      setSelectedRun((s) => (s === id ? null : s));
    } catch (e) {
      toast((e as Error).message, "error");
    }
  };

  const onJump = (c: Comment) => {
    setTab(c.anchor.code ? "changes" : c.doc);
    setHighlight(c.id);
    setTarget({ line: c.anchor.line, nonce: Date.now() });
    setDrawer("comments");
  };

  const onOpenThread = (c: Comment) => {
    setHighlight(c.id);
    setDrawer("comments");
  };

  const stepComment = (dir: 1 | -1) => {
    const k = docKindOf(tab);
    const scoped = comments.filter(
      (c) => !c.parent && !c.resolved && (k ? c.doc === k && !c.anchor.code : !!c.anchor.code),
    );
    const next = stepThread(documentOrder(scoped), highlight, dir);
    if (next) onJump(next);
  };

  useShortcuts({
    n: () => stepComment(1),
    p: () => stepComment(-1),
    c: () => toggleDrawer("comments"),
    "g i": () => nav("/"),
    "1": () => setTab("research"),
    "2": () => setTab("plan"),
    "3": () => setTab("changes"),
  });

  const onOpenChanges = () => {
    setTab("changes");
    setChangesNonce((n) => n + 1);
  };

  if (error && !view) {
    return (
      <div className="page">
        <header className="topbar">
          <Link to="/" className="brand">
            plantool
          </Link>
        </header>
        <main className="list">
          <p className="error">{error}</p>
          <button className="btn" type="button" onClick={() => void loadAll()}>
            Try again
          </button>
        </main>
      </div>
    );
  }
  if (!view) return <div className="page muted">Loading…</div>;

  const kind = docKindOf(tab);
  const run: Run | null = selectedRun
    ? (view.runs.find((r) => r.id === selectedRun) ?? null)
    : null;
  const liveRun =
    [...view.runs]
      .filter(isLive)
      .sort((a, b) => b.started_at.localeCompare(a.started_at))[0] ?? null;
  const latestRun =
    [...view.runs].sort((a, b) => b.started_at.localeCompare(a.started_at))[0] ??
    null;
  const agentState = liveRun
    ? RUN_WORD[liveRun.status]
    : latestRun
      ? RUN_WORD[latestRun.status]
      : "no runs";
  const openCount = openThreads(comments).length;
  const startRun = (
    stage: RunStage,
    request?: {
      resumeId?: string;
      initialMode?: "step-by-step";
      initialPrompt?: string;
    },
  ) => setDialog({ stage, ...request });
  const onRefreshPr = () =>
    void api
      .refreshPr(key)
      .then((result) =>
        setView((current) =>
          current ? { ...current, session: result.session } : current,
        ),
      )
      .catch((e: Error) => toast(e.message, "error"));

  return (
    <div className={cn("session", drawer && "with-drawer")}>
      <SessionSpine
        view={view}
        comments={comments}
        activeTab={tab}
        planDoc={docs.plan ?? null}
        prompt={prompts.research ?? null}
        busy={busy}
        onStartRun={startRun}
        onStage={onStage}
        onJump={onJump}
        onNavigate={(line) => {
          setTab("plan");
          setTarget({ line, nonce: Date.now() });
        }}
        onAskAgent={() => setAskOpen(true)}
        onPr={() => setPrOpen(true)}
        onOpenRun={(id) => showRun(id, true)}
        onOpenChanges={onOpenChanges}
        onContinueMilestone={(r) => void onContinueMilestone(r)}
        onApproveMilestone={(r, commit, message) =>
          void onApproveMilestone(r, commit, message)
        }
        onCancelCommit={onCancelCommit}
        onDetails={() => setDrawer("details")}
        onSettings={() => setSettingsOpen(true)}
        onTab={setTab}
        waiting={waiting}
      />
      <main className="canvas" id="session-content">
        <div className="canvas-head" ref={observeHead}>
          <nav className="canvas-tabs" aria-label="Session documents">
            {(["research", "plan", "changes", "files"] as const).map((t) => (
              <button
                key={t}
                type="button"
                className={cn(tab === t && "active")}
                aria-current={tab === t ? "page" : undefined}
                onClick={() => setTab(t)}
              >
                {t === "research" ? "Research" : t === "plan" ? "Plan" : t === "files" ? "Files" : "Changes"}
              </button>
            ))}
          </nav>
          <span className="spacer" />
          {kind && (
            <div className="seg" aria-label="Document display">
              {VIEW_MODES.map(([m, label]) => (
                <button
                  key={m}
                  type="button"
                  className={mode === m ? "active" : ""}
                  aria-pressed={mode === m}
                  onClick={() => setMode(m)}
                >
                  {label}
                </button>
              ))}
            </div>
          )}
          <button
            type="button"
            className={cn("drawer-toggle", drawer === "comments" && "active")}
            aria-expanded={drawer === "comments"}
            aria-controls="drawer-comments"
            onClick={() => toggleDrawer("comments")}
          >
            Comments <span className="num">{openCount}</span>
          </button>
          <button
            type="button"
            className={cn("drawer-toggle", drawer === "agent" && "active")}
            aria-expanded={drawer === "agent"}
            aria-controls="drawer-agent"
            onClick={() => toggleDrawer("agent")}
          >
            <span
              className={cn("run-status", (liveRun ?? latestRun)?.status)}
              aria-hidden="true"
            />
            Agent<span className="toggle-state"> · {agentState}</span>
          </button>
          <ReviewPreferences
            mode={kind ? mode : undefined}
            onMode={setMode}
            author={author}
            onAuthor={(name) => {
              setAuthor(name);
              setAuthorName(name);
            }}
          />
        </div>
        <div className={`document-notices mode-${mode}`}>
          {!connected && (
            <p className="banner">
              Disconnected. This view may be stale. Reconnecting to the local
              daemon…
            </p>
          )}
          {view.state.pause_reason && (
            <p className="banner">Checkpoint: {view.state.pause_reason}</p>
          )}
        </div>

        {!(kind && mode === "rendered" && docs[kind]?.headings.some((h) => h.level === 1)) && (
          <h1 className="sr-only">{view.session.title}</h1>
        )}
        <div className="document-surface">
          {kind ? (
            <>
              {kind === "plan" && docs.plan && (
                <PlanRevisions
                  sessionKey={key}
                  sha={docs.plan.sha}
                  content={docs.plan.content}
                />
              )}
              <DocView
                kind={kind}
                doc={docs[kind] ?? null}
                path={
                  paths[kind] ??
                  view.docs.find((d) => d.kind === kind)?.path ??
                  null
                }
                prompt={prompts[kind] ?? null}
                comments={comments.filter(
                  (c) => c.doc === kind && !c.anchor.code,
                )}
                onAdd={onAdd}
                mode={mode}
                target={target}
                highlightComment={highlight}
                onOpenThread={onOpenThread}
                onToggleTask={
                  kind === "plan" && mode === "rendered" && docs.plan
                    ? async (line, checked) => {
                        const sha = docs.plan?.sha;
                        if (!sha) return;
                        try {
                          await api.setTask(key, line, checked, sha);
                          await loadDoc("plan");
                        } catch (e) {
                          toast((e as Error).message, "error");
                          await loadDoc("plan");
                        }
                      }
                    : undefined
                }
                numerals={numerals}
                sessionKey={key}
                onOpenFile={openFile}
              />
            </>
          ) : tab === "files" ? (
            <FilesTab
              sessionKey={key}
              selected={params.get("file")}
              onSelect={openFile}
            />
          ) : (
            <ChangesTab
              key={`${key}:${view.session.worktree ?? view.session.repo.root}:${view.session.base}:${view.session.difftool ?? "auto"}`}
              workspace={view.session.worktree ?? view.session.repo.root}
              highlightComment={highlight}
              sessionKey={key}
              nonce={changesNonce}
              comments={comments}
              actions={actions}
              onAdded={() =>
                void api.comments(key).then((r) => setComments(r.comments))
              }
            />
          )}
        </div>
      </main>
      {drawer && (
        <Drawer id={drawer} onClose={closeDrawer} bare={drawer === "agent"}>
          {drawer === "comments" && (
            <CommentsDrawer
              sessionKey={key}
              comments={comments}
              docs={docs}
              author={author}
              selected={highlight}
              onSelect={onJump}
              liveRun={liveRun}
              reviewPrompt={reviewPrompt}
              onSendToRun={onSendToRun}
              actions={actions}
              busy={busy}
              numerals={numerals}
            />
          )}
          {drawer === "agent" && (
            <AgentDrawer
              sessionKey={key}
              runs={view.runs}
              selectedRun={run?.id ?? null}
              lines={run ? (runLines[run.id] ?? []) : []}
              onSelectRun={setSelectedRun}
              onRemoveRun={onRemoveRun}
              onResume={(r) => startRun(resumeStageFor(r), { resumeId: r.id })}
              onClose={closeDrawer}
            />
          )}
          {drawer === "details" && (
            <DetailsDrawer
              view={view}
              busy={busy}
              onBrief={onBrief}
              onStage={onStage}
              onStartRun={(s) => startRun(s)}
              onRefreshPr={onRefreshPr}
              onUpdated={setView}
              onWorkspace={(session) =>
                setView((current) => (current ? { ...current, session } : current))
              }
              onDelete={onDelete}
            />
          )}
        </Drawer>
      )}
      {confirmation}
      <ShortcutsHelp />
      {dialog && (
        <StartRunDialog
          stage={dialog.stage}
          sessionKey={key}
          runs={view.runs}
          resumeId={dialog.resumeId}
          initialMode={dialog.initialMode}
          initialPrompt={dialog.initialPrompt}
          onClose={() => setDialog(null)}
          onStarted={(id) => showRun(id, true)}
        />
      )}
      {prOpen && (
        <PrDialog
          sessionKey={key}
          commit={view.commit?.scope.kind === "pr" ? view.commit : null}
          onCancelCommit={onCancelCommit}
          onClose={() => setPrOpen(false)}
          onDraftAgent={() => {
            setPrOpen(false);
            setDialog({ stage: "draft-pr" });
          }}
          onSaved={(session) =>
            setView((current) => (current ? { ...current, session } : current))
          }
        />
      )}
      {askOpen && (
        <AskAgentDialog
          sessionKey={key}
          runs={view.runs}
          onClose={() => setAskOpen(false)}
          onStarted={(id) => showRun(id, true)}
        />
      )}
      {settingsOpen && <SettingsDialog onClose={() => setSettingsOpen(false)} />}
      <div className="toasts">
        {toasts.map((t) => (
          <div
            key={t.id}
            role={t.kind === "error" ? "alert" : "status"}
            className={`toast ${t.kind ?? "info"}`}
          >
            {t.text}
            {t.kind !== "info" && (
              <button
                type="button"
                aria-label="Dismiss notification"
                onClick={() =>
                  setToasts((current) =>
                    current.filter((item) => item.id !== t.id),
                  )
                }
              >
                <XIcon size={14} aria-hidden="true" />
              </button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function upsertRun(runs: Run[], r: Run): Run[] {
  const i = runs.findIndex((x) => x.id === r.id);
  if (i < 0) return [...runs, r];
  const next = runs.slice();
  next[i] = r;
  return next;
}
