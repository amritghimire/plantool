import { useState } from "react";
import { ArrowClockwiseIcon, CaretDownIcon, CaretRightIcon, XIcon } from "@phosphor-icons/react";
import type { Run } from "../types";
import { cn } from "../lib/cn";
import { relTime } from "../lib/format";
import { useConfirmation } from "../lib/confirmation";
import { isLive, RUN_WORD } from "../lib/notice";
import { durationBars, formatDuration, stopReason } from "../lib/runs";
import { DrawerClose } from "./Drawer";
import { RunPanel, type RunLine } from "./RunPanel";

export function AgentDrawer({
  sessionKey,
  runs,
  selectedRun,
  lines,
  onSelectRun,
  onRemoveRun,
  onResume,
  onClose,
}: {
  sessionKey: string;
  runs: Run[];
  selectedRun: string | null;
  lines: RunLine[];
  onSelectRun: (id: string | null) => void;
  onRemoveRun: (id: string) => Promise<void>;
  onResume: (run: Run) => void;
  onClose: () => void;
}) {
  const { confirm, dialog } = useConfirmation();
  const run = selectedRun ? (runs.find((r) => r.id === selectedRun) ?? null) : null;
  const newest = [...runs].sort((a, b) => b.started_at.localeCompare(a.started_at));
  const finished = newest.filter((r) => !isLive(r));
  const active = newest.filter(isLive);
  const finishedKey = `plantool.finished.${sessionKey}`;
  const [showFinished, setShowFinished] = useState(() => {
    try {
      const stored = localStorage.getItem(finishedKey);
      return stored ? stored === "open" : !runs.some(isLive);
    } catch {
      return false;
    }
  });
  const toggleFinished = () =>
    setShowFinished((o) => {
      try {
        localStorage.setItem(finishedKey, o ? "closed" : "open");
      } catch {
        /* optional preference */
      }
      return !o;
    });
  const bars = durationBars(runs, Date.now());
  const row = (r: Run) => {
    const live = isLive(r);
    const bar = bars.get(r.id);
    const reason = stopReason(r);
    return (
      <li key={r.id} className={cn("run-row", selectedRun === r.id && "active")}>
        <button className="run-select" onClick={() => onSelectRun(r.id)} type="button">
          <span className="sr-only">Open </span>
          <span className="run-line">
            <span className={`run-status ${r.status}`} aria-hidden="true" />
            <span className="run-name">
              {r.provider} <span aria-hidden="true">· </span>
              {r.task ?? r.stage}
            </span>
            <span className="sr-only"> run, </span>
            <span className={`run-word ${r.status}`}>{RUN_WORD[r.status]}</span>
            <span className="run-started" aria-hidden="true">
              {relTime(r.started_at)}
            </span>
          </span>
          <span className="run-bar-line">
            <span className="run-bar" aria-hidden="true">
              <span
                className={cn("run-bar-fill", live ? "live" : r.status)}
                style={{ width: `${bar?.pct ?? 0}%` }}
              />
            </span>
            <span className="run-duration">
              <span className="sr-only">, ran </span>
              {formatDuration(bar?.ms ?? 0)}
              {live && <span className="run-live"> live</span>}
            </span>
          </span>
          {reason && (
            <span className={cn("run-reason", r.status === "failed" && "failed")}>
              {reason}
            </span>
          )}
        </button>
        {!live && (
          <span className="run-actions">
            {r.provider_session_id && (
              <button
                className="run-remove"
                title="Resume this run: the agent keeps its context and picks up where it stopped"
                onClick={() => onResume(r)}
                type="button"
                aria-label="Resume run"
              >
                <ArrowClockwiseIcon size={15} aria-hidden="true" />
              </button>
            )}
            <button
              className="run-remove"
              title="Clear this run and its transcript"
              onClick={async () => {
                if (await confirm("Clear this run and its transcript? This cannot be undone.", true))
                  await onRemoveRun(r.id);
              }}
              type="button"
              aria-label="Clear run transcript"
            >
              <XIcon size={15} aria-hidden="true" />
            </button>
          </span>
        )}
      </li>
    );
  };
  if (run)
    return (
      <div className="agent-drawer with-run">
        <RunPanel
          sessionKey={sessionKey}
          run={run}
          lines={lines}
          onBack={() => onSelectRun(null)}
          onClose={onClose}
          onResume={onResume}
        />
      </div>
    );
  const failed = finished.filter((r) => r.status === "failed").length;
  return (
    <div className="agent-drawer">
      <header className="drawer-head">
        <h2 id="drawer-agent-title">Agent runs</h2>
        <DrawerClose id="agent" onClose={onClose} />
      </header>
      <div className="agent-runs">
        {runs.length === 0 && <p className="muted small">No hosted runs yet.</p>}
        {active.length > 0 && <ul className="run-list">{active.map(row)}</ul>}
        {finished.length > 0 && (
          <div className="finished-head">
            <button
              className="link finished-toggle"
              type="button"
              aria-expanded={showFinished}
              onClick={toggleFinished}
            >
              {showFinished ? (
                <CaretDownIcon size={14} aria-hidden="true" />
              ) : (
                <CaretRightIcon size={14} aria-hidden="true" />
              )}
              Finished runs <span className="badge">{finished.length}</span>
            </button>
            {failed > 0 && <span className="run-word failed">{failed} failed</span>}
            <span className="spacer" />
            {finished.length > 1 && (
              <button
                className="link"
                onClick={async () => {
                  if (
                    await confirm(
                      `Clear ${finished.length} finished runs and their transcripts? This cannot be undone.`,
                      true,
                    )
                  )
                    await Promise.all(finished.map((r) => onRemoveRun(r.id)));
                }}
                type="button"
              >
                Clear finished
              </button>
            )}
          </div>
        )}
        {showFinished && finished.length > 0 && (
          <ul className="run-list">{finished.map(row)}</ul>
        )}
      </div>
      {dialog}
    </div>
  );
}
