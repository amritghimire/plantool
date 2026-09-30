import { useEffect, useState } from "react";
import type { CommitJob } from "../types";

const SHOWN_LINES = 8;

export function CommitProgress({ job, label, onCancel }: { job: CommitJob; label: string; onCancel: () => void }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  const seconds = Math.max(0, Math.round((now - Date.parse(job.started_at)) / 1000));
  const lines = job.lines.slice(-SHOWN_LINES);
  return <div className="commit-progress" role="status" aria-label={label}>
    <div className="commit-progress-head">
      <span className="run-status running" />
      <span>{label}</span>
      <span className="muted small">{job.phase === "staging" ? "staging" : "running hooks"} · {seconds}s</span>
      <span className="spacer" />
      <button className="link" type="button" onClick={onCancel}>Cancel commit</button>
    </div>
    {lines.length > 0 && <pre className="commit-progress-log">{lines.join("\n")}</pre>}
  </div>;
}
