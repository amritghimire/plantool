import { useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api } from "../api";
import type { EngineeringAttempt } from "../engineering";
import type { RunEvent } from "../types";
import { projectRun, type RunLine } from "./RunPanel";

export function AttemptTranscript({
  item,
  attempt,
}: {
  item: string;
  attempt: EngineeringAttempt;
}) {
  const [events, setEvents] = useState<RunLine[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const eventsRef = useRef<RunLine[] | null>(null);
  const busyRef = useRef(false);
  const load = async (refresh = false) => {
    if ((!refresh && eventsRef.current !== null) || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError("");
    try {
      const transcript = await api.engineeringTranscript(item, attempt.id);
      const next = transcript.events.map((entry) => ({
        seq: entry.seq,
        event: entry.event as RunEvent,
      }));
      eventsRef.current = next;
      setEvents(next);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };
  const loadRef = useRef(load);
  loadRef.current = load;
  const active = ["queued", "starting", "running", "waiting", "processing"].includes(attempt.state);
  useEffect(() => {
    if (!open) return;
    void loadRef.current();
    if (!active) return;
    const timer = window.setInterval(() => void loadRef.current(true), 2000);
    return () => window.clearInterval(timer);
  }, [open, active]);
  const projected = events ? projectRun(events) : null;
  return (
    <details
      id={`attempt-transcript-${attempt.id}`}
      className="attempt-transcript"
      onToggle={(event) => {
        setOpen(event.currentTarget.open);
      }}
    >
      <summary>{busy ? "Loading transcript…" : "View AI transcript"}</summary>
      {error && <p role="alert">{error}</p>}
      {projected && !events?.length && (active
        ? <p role="status">No provider events have arrived yet. This transcript refreshes while the attempt runs.</p>
        : <p>No provider events were retained for this attempt. Its recorded summary and token usage are still available.</p>)}
      {projected?.items.map((entry, index) =>
        entry.t === "msg" ? (
          <article className="transcript-message" key={`${entry.m.id}-${index}`}>
            <strong>{entry.m.role === "assistant" ? "Agent" : entry.m.role}</strong>
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{entry.m.content}</ReactMarkdown>
          </article>
        ) : entry.t === "act" ? (
          <details className="transcript-activity" key={entry.a.id}>
            <summary>
              {entry.a.title} · {entry.a.status}
            </summary>
            {entry.a.detail && <p>{entry.a.detail}</p>}
            {entry.a.output && <pre>{entry.a.output}</pre>}
          </details>
        ) : (
          <p className="transcript-status" key={`${entry.label}-${index}`}>
            {entry.label}
            {entry.detail ? `: ${entry.detail}` : ""}
          </p>
        ),
      )}
      {!!projected?.pending.length && (
        <section aria-label="Unanswered requests">
          <strong>Requests recorded during this attempt</strong>
          {projected.pending.map((entry) => (
            <p key={entry.request_id}>
              {entry.title}
              {entry.detail ? `: ${entry.detail}` : ""}
            </p>
          ))}
        </section>
      )}
      {events?.some(({ event }) => event.type === "raw") && (
        <details>
          <summary>Technical provider events</summary>
          {events
            .filter(({ event }) => event.type === "raw")
            .map(({ seq, event }) => (
              <pre key={seq}>{event.type === "raw" ? event.line : ""}</pre>
            ))}
        </details>
      )}
    </details>
  );
}
