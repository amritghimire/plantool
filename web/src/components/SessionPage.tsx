import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { api, authorName, setAuthorName } from "../api";
import { useLive } from "../lib/live";
import { tabForStage } from "../lib/stage";
import type { Comment, DocKind, DocResponse, LiveEvent, Run, SessionView, Stage } from "../types";
import { ChangesTab } from "./ChangesTab";
import { DocView, type ViewMode } from "./DocView";
import { RunPanel, type RunLine } from "./RunPanel";
import { Sidebar, docKindOf } from "./Sidebar";
import { StartRunDialog, type RunStage } from "./StartRunDialog";
import { ThemeToggle } from "./ThemeToggle";
import { PrDialog } from "./PrDialog";
import { AskAgentDialog } from "./AskAgentDialog";
import type { ThreadActions } from "./Thread";

type Toast = { id: number; text: string; kind?: "info" | "error" | "warn" };

export function SessionPage() {
  const { repo, slug } = useParams();
  const key = `${repo}/${slug}`;
  const [params, setParams] = useSearchParams();
  const nav = useNavigate();
  const [view, setView] = useState<SessionView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [docs, setDocs] = useState<Partial<Record<DocKind, DocResponse | null>>>({});
  const [paths, setPaths] = useState<Partial<Record<DocKind, string>>>({});
  const [prompts, setPrompts] = useState<Partial<Record<DocKind, string>>>({});
  const [reviewPrompt, setReviewPrompt] = useState<string | null>(null);
  const [comments, setComments] = useState<Comment[]>([]);
  const [tab, setTabState] = useState<string>(params.get("tab") ?? "plan");
  const [mode, setModeState] = useState<ViewMode>(() => {
    const m = localStorage.getItem("plantool.mode");
    return m === "source" || m === "slides" ? m : "rendered";
  });
  const setMode = (m: ViewMode) => {
    setModeState(m);
    localStorage.setItem("plantool.mode", m);
  };
  const [showResolved, setShowResolved] = useState(false);
  const [target, setTarget] = useState<{ line: number; nonce: number } | null>(null);
  const [highlight, setHighlight] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [dialog, setDialog] = useState<{ stage: RunStage; resumeId?: string; initialMode?: "step-by-step"; initialPrompt?: string } | null>(null);
  const [askOpen, setAskOpen] = useState(false);
  const [prOpen, setPrOpen] = useState(false);
  const [selectedRun, setSelectedRun] = useState<string | null>(params.get("run"));
  const [runLines, setRunLines] = useState<Record<string, RunLine[]>>({});
  const [changesNonce, setChangesNonce] = useState(0);
  const [author, setAuthor] = useState(authorName());
  const toastId = useRef(0);
  const viewRef = useRef<SessionView | null>(null);
  useEffect(() => {
    viewRef.current = view;
  }, [view]);

  const toast = useCallback((text: string, kind: Toast["kind"] = "info") => {
    const id = ++toastId.current;
    setToasts((t) => [...t, { id, text, kind }]);
    window.setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 4000);
  }, []);

  const setTab = (t: string) => {
    setTabState(t);
    const next = new URLSearchParams(params);
    next.set("tab", t);
    setParams(next, { replace: true });
  };

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
      const stage = kind === "research" ? "research" : kind === "plan" ? "plan" : null;
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
    api.prompt(key, "review").then((r) => setReviewPrompt(r.prompt)).catch(() => setReviewPrompt(null));
  }, [key]);

  const reloadPrompts = useCallback(() => {
    setDocs((m) => {
      for (const k of Object.keys(m) as DocKind[]) if (m[k] === null) void loadPrompt(k);
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
    const waiting = view?.runs.some((r) => r.status === "waiting") ? "● " : view?.runs.some((r) => r.status === "idle") ? "○ " : "";
    document.title = view ? `${waiting}${view.session.title} · plantool` : "plantool";
  }, [view]);

  useEffect(() => {
    if (!selectedRun) return;
    api.runEvents(key, selectedRun).then((r) => setRunLines((m) => ({ ...m, [selectedRun]: r.events.map((e) => ({ seq: e.seq, at: e.at, event: e.event as RunLine["event"] })) }))).catch(() => {});
  }, [key, selectedRun]);

  const onEvent = useCallback(
    (e: LiveEvent) => {
      switch (e.type) {
        case "comment-added":
          setComments((c) => (c.some((x) => x.id === e.comment.id) ? c : [...c, e.comment]));
          if (e.comment.kind === "agent") toast(`${e.comment.author} commented on ${e.comment.doc}:${e.comment.anchor.line}`);
          break;
        case "comments-added":
          setComments((c) => [...c, ...e.comments.filter((n) => !c.some((x) => x.id === n.id))]);
          toast(`${e.comments.length} comments added`);
          break;
        case "comments-since":
          setComments((c) => [...c, ...e.comments.filter((n) => !c.some((x) => x.id === n.id))]);
          break;
        case "comment-updated":
          setComments((c) => c.map((x) => (x.id === e.comment.id ? e.comment : x)));
          break;
        case "comment-removed":
          setComments((c) => c.filter((x) => !e.ids.includes(x.id)));
          break;
        case "thread-resolution":
          setComments((c) => c.map((x) => (e.ids.includes(x.id) || (x.parent && e.ids.includes(x.parent)) ? { ...x, resolved: e.resolved } : x)));
          break;
        case "doc-refreshed":
          void loadDoc(e.kind);
          void api.session(key).then(setView).catch(() => {});
          void api.comments(key).then((c) => setComments(c.comments)).catch(() => {});
          toast(`${e.kind}.md updated${e.outdated ? ` · ${e.outdated} comment(s) outdated` : ""}`);
          break;
        case "stage-changed":
          setView((v) => (v ? { ...v, state: { ...v.state, stage: e.to } } : v));
          toast(`stage: ${e.to}`);
          loadReviewPrompt();
          break;
        case "session-updated":
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
          if (e.type === "run-started") setSelectedRun(e.run.id);
          if (e.type === "run-updated" && prev && prev.status !== e.run.status) {
            if (e.run.status === "waiting") toast("The agent needs your answer", "warn");
            else if (e.run.status === "idle" && prev.status === "running") toast("The agent finished its turn; your move");
          }
          if (e.type === "run-ended") toast(`run ${e.run.status}${e.run.error ? `: ${e.run.error}` : ""}`, e.run.status === "failed" ? "error" : "info");
          break;
        }
        case "run-removed":
          setView((v) => (v ? { ...v, runs: v.runs.filter((r) => r.id !== e.id) } : v));
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
            return { ...m, [e.run_id]: [...prev, { seq: e.seq, at: e.at, event: e.event }] };
          });
          break;
        case "commit-progress": {
          const commit = e.commit;
          const running = commit.phase === "staging" || commit.phase === "hooks";
          setView((v) => (v ? { ...v, commit: running ? commit : null } : v));
          break;
        }
        case "changes-opened":
          setView((v) => (v ? { ...v, state: { ...v.state, review: e.review } } : v));
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
    [key, loadDoc, reloadPrompts, loadReviewPrompt, toast],
  );

  useLive(view ? key : null, onEvent, () => void loadAll());

  const actions: ThreadActions = useMemo(
    () => ({
      reply: async (parent, body) => {
        const r = await api.addComment(key, { doc: parent.doc, parent: parent.id, body });
        setComments((c) => [...c, ...r.comments.filter((n) => !c.some((x) => x.id === n.id))]);
      },
      resolve: async (root, resolved) => {
        await api.resolve(key, [root.id], resolved);
      },
      edit: async (c, body) => {
        await api.editComment(key, c.id, body);
      },
      remove: async (c) => {
        if (!window.confirm("Delete this comment?")) return;
        await api.removeComments(key, [c.id]);
      },
    }),
    [key],
  );

  const onAdd = async (kind: DocKind, line: number, body: string) => {
    const r = await api.addComment(key, { doc: kind, line, body });
    setComments((c) => [...c, ...r.comments.filter((n) => !c.some((x) => x.id === n.id))]);
  };

  const onStage = async (s: Stage) => {
    setBusy(true);
    try {
      await api.setStage(key, s);
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
    const live = view?.runs.find((r) => r.status === "starting" || r.status === "running" || r.status === "waiting" || r.status === "idle");
    if (!live) {
      toast("No run is live; start one or paste the prompt into your agent.", "error");
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
      setDialog({ stage: "implement", resumeId: milestoneRun.provider_session_id ? milestoneRun.id : undefined, initialMode: "step-by-step" });
      return;
    }
    setBusy(true);
    try {
      await api.runInput(key, milestoneRun.id, { text: "I reviewed the completed milestone. Continue with exactly the next unchecked plan ticket, then pause for review again. If no tickets remain, run the final checks and move to implementation-review." });
      setSelectedRun(milestoneRun.id);
    } catch (e) {
      toast((e as Error).message, "error");
    } finally {
      setBusy(false);
    }
  };

  const onCancelCommit = () => void api.cancelCommit(key).catch((e: Error) => toast(e.message, "error"));

  const onApproveMilestone = async (milestoneRun: Run, commit: boolean, message: string) => {
    setBusy(true);
    try {
      const r = await api.approveMilestone(key, milestoneRun.id, commit, message);
      setSelectedRun(milestoneRun.id);
      setChangesNonce((n) => n + 1);
      toast(r.committed ? `Milestone committed as ${r.sha.slice(0, 12)}; the agent is continuing` : "Milestone approved; the agent is continuing");
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
      setView((v) => (v ? { ...v, runs: v.runs.filter((r) => r.id !== id) } : v));
      setSelectedRun((s) => (s === id ? null : s));
    } catch (e) {
      toast((e as Error).message, "error");
    }
  };

  const onJump = (c: Comment) => {
    setTab(c.doc);
    if (c.resolved) setShowResolved(true);
    setHighlight(c.id);
    setTarget({ line: c.anchor.line, nonce: Date.now() });
  };

  const onOpenChanges = () => {
    setTab("changes");
    setChangesNonce((n) => n + 1);
  };

  if (error && !view) {
    return (
      <div className="page">
        <header className="topbar">
          <Link to="/" className="brand">plantool</Link>
        </header>
        <main className="list">
          <p className="error">{error}</p>
          <button className="btn" type="button" onClick={() => void loadAll()}>Try again</button>
        </main>
      </div>
    );
  }
  if (!view) return <div className="page muted">Loading…</div>;

  const kind = docKindOf(tab);
  const run: Run | null = selectedRun ? (view.runs.find((r) => r.id === selectedRun) ?? null) : null;

  return (
    <div className={`session ${run ? "with-run" : ""}`}>
      <header className="topbar">
        <Link to="/" className="brand">plantool</Link>
        <span className="muted">/</span>
        <span className="crumb">{view.session.repo_slug}</span>
        <span className="muted">/</span>
        <span className="crumb">{view.session.slug}</span>
        <span className="spacer" />
        <label className="muted small author">
          as
          <input
            value={author}
            onChange={(e) => {
              setAuthor(e.target.value);
              setAuthorName(e.target.value);
            }}
          />
        </label>
        <label className="muted small">
          <input type="checkbox" checked={showResolved} onChange={(e) => setShowResolved(e.target.checked)} /> resolved
        </label>
        {kind && (
          <div className="seg">
            <button className={mode === "rendered" ? "active" : ""} onClick={() => setMode("rendered")} type="button">
              Rendered
            </button>
            <button className={mode === "slides" ? "active" : ""} onClick={() => setMode("slides")} type="button" title="One heading per slide; ← → to move">
              Slides
            </button>
            <button className={mode === "source" ? "active" : ""} onClick={() => setMode("source")} type="button">
              Source
            </button>
          </div>
        )}
        <ThemeToggle />
      </header>
      <Sidebar
        view={view}
        comments={comments}
        activeTab={tab}
        onTab={setTab}
        onStage={onStage}
        onBrief={onBrief}
        onDelete={onDelete}
        onJump={onJump}
        onStartRun={(s, resumeId) => setDialog({ stage: s, resumeId })}
        onAskAgent={() => setAskOpen(true)}
        onPr={() => setPrOpen(true)}
        onRefreshPr={() => void api.refreshPr(key).then((result) => setView((current) => current ? { ...current, session: result.session } : current)).catch((e: Error) => toast(e.message, "error"))}
        onContinueMilestone={(r) => void onContinueMilestone(r)}
        onApproveMilestone={(r, commit, message) => void onApproveMilestone(r, commit, message)}
        onCancelCommit={onCancelCommit}
        onOpenChanges={onOpenChanges}
        onSelectRun={setSelectedRun}
        onRemoveRun={onRemoveRun}
        onWorkspace={(session) => setView((current) => current ? { ...current, session } : current)}
        actions={actions}
        reviewPrompt={reviewPrompt}
        onSendToRun={onSendToRun}
        selectedRun={selectedRun}
        busy={busy}
      />
      <main className="content">
        {kind ? (
          <DocView
            kind={kind}
            doc={docs[kind] ?? null}
            path={paths[kind] ?? view.docs.find((d) => d.kind === kind)?.path ?? null}
            prompt={prompts[kind] ?? null}
            onStartRun={kind === "research" || kind === "plan" ? () => setDialog({ stage: kind }) : undefined}
            comments={comments.filter((c) => c.doc === kind)}
            actions={actions}
            onAdd={onAdd}
            mode={mode}
            target={target}
            highlightComment={highlight}
            showResolved={showResolved}
          />
        ) : (
          <ChangesTab sessionKey={key} nonce={changesNonce} />
        )}
      </main>
      {run && (
        <RunPanel
          sessionKey={key}
          run={run}
          lines={runLines[run.id] ?? []}
          onClose={() => setSelectedRun(null)}
          onResume={(r) => setDialog({ stage: r.task === "assist" ? "assist" : r.task === "draft-pr" ? "draft-pr" : r.stage === "researching" ? "research" : r.stage === "implementing" || r.stage === "implementation-review" ? "implement" : "plan", resumeId: r.id })}
        />
      )}
      {dialog && <StartRunDialog stage={dialog.stage} sessionKey={key} runs={view.runs} resumeId={dialog.resumeId} initialMode={dialog.initialMode} initialPrompt={dialog.initialPrompt} onClose={() => setDialog(null)} onStarted={(id) => setSelectedRun(id)} />}
      {prOpen && <PrDialog sessionKey={key} commit={view.commit?.scope.kind === "pr" ? view.commit : null} onCancelCommit={onCancelCommit} onClose={() => setPrOpen(false)} onDraftAgent={() => { setPrOpen(false); setDialog({ stage: "draft-pr" }); }} onSaved={(session) => setView((current) => current ? { ...current, session } : current)} />}
      {askOpen && <AskAgentDialog sessionKey={key} runs={view.runs} onClose={() => setAskOpen(false)} onStarted={(id) => setSelectedRun(id)} />}
      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.kind ?? "info"}`}>
            {t.text}
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
