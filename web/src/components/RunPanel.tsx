import { useEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api } from "../api";
import type { Run, RunEvent } from "../types";

export interface RunLine {
  seq: number;
  event: RunEvent;
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

interface Pending {
  request_id: string;
  kind: "permission" | "input";
  title: string;
  detail?: string;
  options?: { id: string; label: string }[];
  questions?: { id: string; text: string; options?: string[] }[];
}

export function projectRun(lines: RunLine[]) {
  const items: Item[] = [];
  const acts = new Map<string, Activity>();
  const msgs = new Map<string, Message>();
  const pending = new Map<string, Pending>();
  let streaming: Message | null = null;
  for (const { event: e } of lines) {
    switch (e.type) {
      case "message": {
        const m = msgs.get(e.id);
        if (m) m.content = e.content;
        else {
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
        break;
      case "turn-completed":
        streaming = null;
        if (e.status !== "completed") items.push({ t: "status", label: `turn ${e.status}`, detail: e.error });
        break;
      case "status":
        items.push({ t: "status", label: e.label, detail: e.detail });
        break;
      default:
        break;
    }
  }
  return { items, pending: [...pending.values()] };
}

export function RunPanel({ sessionKey, run, lines, onClose }: { sessionKey: string; run: Run | null; lines: RunLine[]; onClose: () => void }) {
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const { items, pending } = useMemo(() => projectRun(lines), [lines]);
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 160;
    if (nearBottom) el.scrollTop = el.scrollHeight;
  }, [lines.length]);
  if (!run) return null;
  const send = async (body: unknown) => {
    setBusy(true);
    setErr(null);
    try {
      await api.runInput(sessionKey, run.id, body);
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const live = run.status === "running" || run.status === "starting" || run.status === "waiting" || run.status === "idle";
  return (
    <section className="run-panel">
      <header className="run-head">
        <span className={`run-status ${run.status}`} />
        <strong>
          {run.provider} · {run.stage}
        </strong>
        <span className="muted small">{run.model ?? ""}</span>
        <span className="spacer" />
        <span className="muted small">{run.status}</span>
        {live && (
          <button className="btn ghost" onClick={() => void api.stopRun(sessionKey, run.id)} type="button">
            Stop
          </button>
        )}
        <button className="btn ghost" onClick={onClose} type="button" title="Hide">
          ×
        </button>
      </header>
      {run.error && <div className="error">{run.error}</div>}
      <div className="run-scroll" ref={scroller}>
        {items.map((it, i) => {
          if (it.t === "msg") {
            return (
              <div key={it.m.id + i} className={`run-msg ${it.m.role}`}>
                {it.m.role === "assistant" ? <ReactMarkdown remarkPlugins={[remarkGfm]}>{it.m.content}</ReactMarkdown> : <pre>{it.m.content}</pre>}
              </div>
            );
          }
          if (it.t === "act") return <ActivityView key={it.a.id + i} a={it.a} />;
          return (
            <div key={i} className="run-status-line muted small">
              {it.label}
              {it.detail ? ` — ${it.detail}` : ""}
            </div>
          );
        })}
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
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if ((e.metaKey || e.ctrlKey) && e.key === "Enter" && input.trim()) {
                void send({ text: input });
                setInput("");
              }
            }}
          />
          <button
            className="btn primary"
            disabled={busy || !input.trim()}
            onClick={() => {
              void send({ text: input });
              setInput("");
            }}
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
