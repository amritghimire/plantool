import { useEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api } from "../api";
import { clearDraft, getDraft, setDraft } from "../lib/drafts";
import { providerLabel } from "../lib/agents";
import { PERMISSION_MODES, type PermissionMode, type Run, type RunEvent } from "../types";

export interface RunLine {
  seq: number;
  event: RunEvent;
  at?: string;
}

interface Activity {
  id: string;
  kind: string;
  title: string;
  detail?: string;
  output: string;
  status: "running" | "completed" | "failed" | "denied";
}

interface Message {
  id: string;
  role: "user" | "assistant";
  content: string;
}

type Item = { t: "msg"; m: Message } | { t: "act"; a: Activity } | { t: "status"; label: string; detail?: string };
interface Turn { id: string; start: number; end: number | null; completed: boolean; duration?: string }

interface Pending {
  request_id: string;
  kind: "permission" | "input";
  title: string;
  detail?: string;
  options?: { id: string; label: string }[];
  questions?: { id: string; text: string; options?: string[] }[];
}

export type Phase = { kind: "starting" | "working" | "watching" | "waiting" | "idle" | "stopped" | "failed"; title: string; detail?: string };

export function phaseOf(run: Run, items: Item[], pending: Pending[]): Phase {
  const lastRunning = [...items].reverse().find((i): i is { t: "act"; a: Activity } => i.t === "act" && i.a.status === "running");
  switch (run.status) {
    case "starting":
      return { kind: "starting", title: "Starting the agent…" };
    case "waiting":
      return { kind: "waiting", title: pending.length > 1 ? `Needs your answer (${pending.length} requests below)` : "Needs your answer below" };
    case "running": {
      const a = lastRunning?.a;
      if (a && `${a.title} ${a.detail ?? ""}`.includes("session watch")) {
        return { kind: "watching", title: "Waiting for your comments", detail: "The agent is blocked in `plantool session watch`. Comment on the document, reply on a thread, or send it a message; it wakes up on the next event." };
      }
      return { kind: "working", title: a ? `Working · ${a.title}` : "Working…" };
    }
    case "idle":
      if (run.milestone_pending) return { kind: "idle", title: "Milestone ready for review", detail: "If difftool is available, new comments reach this run. Approve the milestone in the sidebar when the review is done." };
      return { kind: "idle", title: "Turn finished. Your move.", detail: "Comment on the document, use “Send comments to the running agent”, or type below." };
    case "failed":
      return { kind: "failed", title: "Failed", detail: run.error ?? undefined };
    default:
      return { kind: "stopped", title: "Stopped", detail: run.provider_session_id ? "Resume to continue with the same context." : run.error ?? undefined };
  }
}

export function projectRun(lines: RunLine[]) {
  const items: Item[] = [];
  const acts = new Map<string, Activity>();
  const msgs = new Map<string, Message>();
  const pending = new Map<string, Pending>();
  const turns: Turn[] = [];
  let streaming: Message | null = null;
  let lastStreamed: Message | null = null;
  for (const { event: e, at } of lines) {
    switch (e.type) {
      case "message": {
        const m = msgs.get(e.id);
        // The final message for text that was already streamed: adopt the streamed item instead of repeating it.
        const streamed = streaming ?? (lastStreamed && lastStreamed.content.trim() === e.content.trim() ? lastStreamed : null);
        if (m) m.content = e.content;
        else if (e.role === "assistant" && streamed) {
          streamed.id = e.id;
          streamed.content = e.content;
          msgs.set(e.id, streamed);
          lastStreamed = null;
        } else {
          const nm = { id: e.id, role: e.role, content: e.content };
          msgs.set(e.id, nm);
          items.push({ t: "msg", m: nm });
        }
        if (e.role === "assistant") streaming = null;
        break;
      }
      case "text-delta": {
        if (!streaming) {
          streaming = { id: `stream-${items.length}`, role: "assistant", content: "" };
          items.push({ t: "msg", m: streaming });
          lastStreamed = streaming;
        }
        streaming.content += e.delta;
        break;
      }
      case "activity-start": {
        const a: Activity = { id: e.id, kind: e.kind, title: e.title, detail: e.detail, output: "", status: "running" };
        acts.set(e.id, a);
        items.push({ t: "act", a });
        streaming = null;
        break;
      }
      case "activity-output": {
        const a = acts.get(e.id);
        if (a) a.output += e.delta;
        break;
      }
      case "activity-complete": {
        const a = acts.get(e.id);
        if (a) {
          a.status = e.status;
          if (e.output) a.output = e.output;
        }
        break;
      }
      case "permission":
        pending.set(e.request_id, { request_id: e.request_id, kind: "permission", title: e.title, detail: e.detail, options: e.options });
        break;
      case "input-request":
        pending.set(e.request_id, { request_id: e.request_id, kind: "input", title: e.title, questions: e.questions });
        break;
      case "request-resolved":
        pending.delete(e.request_id);
        break;
      case "turn-started":
        streaming = null;
        turns.push({ id: e.turn_id, start: items.length, end: null, completed: false, duration: at });
        break;
      case "turn-completed":
        streaming = null;
        if (e.status !== "completed") items.push({ t: "status", label: `turn ${e.status}`, detail: e.error });
        {
          const turn = [...turns].reverse().find((t) => t.id === e.turn_id && t.end === null);
          if (turn) {
            turn.end = items.length;
            turn.completed = true;
            if (at && turn.duration) turn.duration = `${Math.max(0, Math.round((Date.parse(at) - Date.parse(turn.duration)) / 1000))}s`;
            else turn.duration = undefined;
          }
        }
        break;
      case "status":
        items.push({ t: "status", label: e.label, detail: e.detail });
        break;
      default:
        break;
    }
  }
  return { items, pending: [...pending.values()], turns };
}

export function RunPanel({ sessionKey, run, lines, onClose, onResume }: { sessionKey: string; run: Run | null; lines: RunLine[]; onClose: () => void; onResume?: (run: Run) => void }) {
  const draftKey = run ? `run:${run.id}` : "";
  const [input, setInput] = useState(() => draftKey ? getDraft(draftKey) : "");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  useEffect(() => { setInput(draftKey ? getDraft(draftKey) : ""); }, [draftKey]);
  const { items, pending, turns } = useMemo(() => projectRun(lines), [lines]);
  const segments = useMemo(() => {
    if (!turns.length) return [{ id: "transcript", items, completed: !run || !["starting", "running", "waiting", "idle"].includes(run.status), duration: undefined }];
    const groups: { id: string; items: Item[]; completed: boolean; duration?: string }[] = [];
    let cursor = 0;
    for (const turn of turns) {
      if (turn.start > cursor) groups.push({ id: `before-${cursor}`, items: items.slice(cursor, turn.start), completed: false });
      const end = turn.end ?? items.length;
      groups.push({ id: turn.id, items: items.slice(turn.start, end), completed: turn.completed, duration: turn.duration });
      cursor = end;
    }
    if (cursor < items.length) groups.push({ id: `after-${cursor}`, items: items.slice(cursor), completed: false });
    return groups;
  }, [items, turns, run?.status]);
  const phase = run ? phaseOf(run, items, pending) : null;
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 160;
    if (nearBottom) el.scrollTop = el.scrollHeight;
  }, [lines.length]);
  if (!run) return null;
  const send = async (body: unknown): Promise<boolean> => {
    setBusy(true);
    setErr(null);
    try {
      await api.runInput(sessionKey, run.id, body);
      return true;
    } catch (e) {
      setErr((e as Error).message);
      return false;
    } finally {
      setBusy(false);
    }
  };
  const sendText = async () => {
    const text = input.trim();
    if (!text || busy) return;
    if (await send({ text })) {
      setInput("");
      clearDraft(draftKey);
    }
  };
  const live = run.status === "running" || run.status === "starting" || run.status === "waiting" || run.status === "idle";
  return (
    <section className="run-panel">
      <header className="run-head">
        <span className={`run-status ${run.status}`} />
        <strong>
          {providerLabel(run.provider)} · {run.task ?? run.stage}
        </strong>
        <span className="muted small">{run.model ?? ""}</span>
        <span className="spacer" />
        <span className="muted small">{run.status}</span>
        {live ? (
          <select
            className="permission-select"
            title={PERMISSION_MODES.find((m) => m.id === (run.permission_mode ?? "ask"))?.hint}
            value={run.permission_mode ?? "ask"}
            disabled={busy}
            onChange={(e) => void send({ permission_mode: e.target.value as PermissionMode })}
          >
            {PERMISSION_MODES.map((m) => (
              <option key={m.id} value={m.id}>
                {m.label}
              </option>
            ))}
          </select>
        ) : (
          run.permission_mode && run.permission_mode !== "ask" && <span className="muted small">{PERMISSION_MODES.find((m) => m.id === run.permission_mode)?.label}</span>
        )}
        {live && (
          <button className="btn ghost" onClick={() => void api.stopRun(sessionKey, run.id).catch((e: Error) => setErr(e.message))} type="button">
            Stop
          </button>
        )}
        {!live && run.provider_session_id && onResume && (
          <button className="btn primary small" onClick={() => onResume(run)} type="button" title="Start a new run that continues this one's session">
            Resume
          </button>
        )}
        <button className="btn ghost" onClick={onClose} type="button" title="Hide">
          ×
        </button>
      </header>
      {phase && (
        <div className={`run-phase ${phase.kind}`} role="status">
          <span className={`run-status ${run.status}`} />
          <div>
            <div className="run-phase-title">{phase.title}</div>
            {phase.detail && <div className="muted small">{phase.detail}</div>}
          </div>
        </div>
      )}
      <div className="run-scroll" ref={scroller}>
        {segments.map((segment) => <TurnView key={segment.id} items={segment.items} completed={segment.completed} duration={segment.duration} />)}
        {items.length === 0 && <div className="muted small">Waiting for the agent…</div>}
      </div>
      {pending.map((pd) => (
        <div key={pd.request_id} className="permission">
          <div className="permission-title">{pd.title}</div>
          {pd.detail && <pre className="permission-detail">{pd.detail}</pre>}
          {pd.kind === "permission" && (
            <div className="composer-actions">
              {(pd.options?.length ? pd.options : [{ id: "allow", label: "Allow" }, { id: "deny", label: "Deny" }]).map((o) => (
                <button key={o.id} className={`btn ${o.id.startsWith("allow") ? "primary" : "ghost"}`} disabled={busy} onClick={() => void send({ permission: { request_id: pd.request_id, decision: o.id } })} type="button">
                  {o.label}
                </button>
              ))}
              {(run.permission_mode ?? "ask") !== "allow-all" && (
                <button className="btn ghost" disabled={busy} title="Answer this and every later request with allow" onClick={() => void send({ permission_mode: "allow-all" })} type="button">
                  Allow all
                </button>
              )}
            </div>
          )}
          {pd.kind === "input" && <InputAnswer pd={pd} busy={busy} onAnswer={(answers) => void send({ input: { request_id: pd.request_id, answers } })} />}
        </div>
      ))}
      {err && <div className="error">{err}</div>}
      {live && (
        <div className="run-input">
          <textarea
            rows={2}
            value={input}
            placeholder="Tell the agent something… (⌘↩)"
            onChange={(e) => { setInput(e.target.value); setDraft(draftKey, e.target.value); }}
            onKeyDown={(e) => {
              if ((e.metaKey || e.ctrlKey) && e.key === "Enter" && input.trim()) {
                void sendText();
              }
            }}
          />
          <button
            className="btn primary"
            disabled={busy || !input.trim()}
            onClick={() => void sendText()}
            type="button"
          >
            Send
          </button>
        </div>
      )}
    </section>
  );
}

function ActivityView({ a }: { a: Activity }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={`activity ${a.status}`}>
      <div className="activity-head" onClick={() => setOpen((o) => !o)}>
        <span className="activity-kind">{a.kind}</span>
        <span className="activity-title">{a.title}</span>
        <span className="spacer" />
        <span className="muted small">{a.status}</span>
      </div>
      {open && (a.detail || a.output) && <pre className="activity-body">{[a.detail, a.output].filter(Boolean).join("\n\n")}</pre>}
    </div>
  );
}

function ItemView({ item }: { item: Item }) {
  if (item.t === "msg") return <div className={`run-msg ${item.m.role}`}>{item.m.role === "assistant" ? <ReactMarkdown remarkPlugins={[remarkGfm]}>{item.m.content}</ReactMarkdown> : <pre>{item.m.content}</pre>}</div>;
  if (item.t === "act") return <ActivityView a={item.a} />;
  return <div className="run-status-line muted small">{item.label}{item.detail ? ` — ${item.detail}` : ""}</div>;
}

function ItemList({ items }: { items: Item[] }) {
  const groups: Item[][] = [];
  for (const item of items) {
    const last = groups.at(-1);
    if (item.t === "act" && item.a.status === "completed" && last?.every((entry) => entry.t === "act" && entry.a.status === "completed")) last.push(item);
    else groups.push([item]);
  }
  return <>{groups.map((group, index) => group.length > 1 ? <details key={index} className="activity-group"><summary>{group.length} completed activities</summary>{group.map((item, i) => <ItemView key={i} item={item} />)}</details> : <ItemView key={index} item={group[0]} />)}</>;
}

function TurnView({ items, completed, duration }: { items: Item[]; completed: boolean; duration?: string }) {
  if (!completed) return <ItemList items={items} />;
  let finalIndex = -1;
  items.forEach((item, index) => { if (item.t === "msg" && item.m.role === "assistant") finalIndex = index; });
  const routine = items.filter((item, index) => index !== finalIndex && !(item.t === "msg" && item.m.role === "user") && !(item.t === "act" && ["failed", "denied"].includes(item.a.status)) && item.t !== "status");
  const visible = items.filter((item, index) => index === finalIndex || (item.t === "msg" && item.m.role === "user") || (item.t === "act" && ["failed", "denied"].includes(item.a.status)) || item.t === "status");
  return <div className="run-turn">{routine.length > 0 && <details className="turn-work"><summary>Finished turn{duration ? ` · ${duration}` : ""} · {routine.length} work items</summary><ItemList items={routine} /></details>}<ItemList items={visible} /></div>;
}

function InputAnswer({ pd, busy, onAnswer }: { pd: Pending; busy: boolean; onAnswer: (answers: Record<string, string>) => void }) {
  const [answers, setAnswers] = useState<Record<string, string>>({});
  return (
    <div>
      {pd.questions?.map((q) => (
        <label key={q.id} className="question">
          <div>{q.text}</div>
          {q.options?.length ? (
            <select value={answers[q.id] ?? ""} onChange={(e) => setAnswers({ ...answers, [q.id]: e.target.value })}>
              <option value="">choose…</option>
              {q.options.map((o) => (
                <option key={o} value={o}>
                  {o}
                </option>
              ))}
            </select>
          ) : (
            <input value={answers[q.id] ?? ""} onChange={(e) => setAnswers({ ...answers, [q.id]: e.target.value })} />
          )}
        </label>
      ))}
      <div className="composer-actions">
        <span className="spacer" />
        <button className="btn primary" disabled={busy} onClick={() => onAnswer(answers)} type="button">
          Answer
        </button>
      </div>
    </div>
  );
}
