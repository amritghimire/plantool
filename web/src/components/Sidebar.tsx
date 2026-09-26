import { useEffect, useState } from "react";
import { STAGES, STAGE_LABEL, type Comment, type DocKind, type DocSummary, type Run, type SessionView, type Stage } from "../types";
import { relTime, shortPath } from "../lib/format";

export interface SidebarProps {
  view: SessionView;
  comments: Comment[];
  activeTab: string;
  onTab: (t: string) => void;
  onStage: (s: Stage) => Promise<void>;
  onBrief: (brief: string | null) => Promise<void>;
  onDelete: (removeWorktree: boolean) => Promise<void>;
  onJump: (c: Comment) => void;
  onStartRun: (stage: "research" | "plan" | "implement") => void;
  onOpenChanges: () => void;
  onSelectRun: (id: string) => void;
  onRemoveRun: (id: string) => Promise<void>;
  selectedRun: string | null;
  busy: boolean;
}

export function Sidebar(p: SidebarProps) {
  const stage = p.view.state.stage;
  const idx = STAGES.indexOf(stage);
  const openThreads = p.comments.filter((c) => !c.parent && !c.resolved).sort((a, b) => a.doc.localeCompare(b.doc) || a.anchor.line - b.anchor.line);
  const openHuman = openThreads.filter((c) => c.kind === "human").length;
  const plan = p.view.docs.find((d) => d.kind === "plan");
  const research = p.view.docs.find((d) => d.kind === "research");
  const s = p.view.session;

  const finished = p.view.runs.filter((r) => !isLive(r));
  const confirm = (msg: string) => openHuman === 0 || window.confirm(msg);
  const skipToBuild = () => window.confirm("Skip planning? The stage becomes approved. The agent will write a short ticket list (todo items) to plan.md from the brief, then implement ticket by ticket.");

  return (
    <aside className="sidebar">
      <div className="side-block">
        <div className="side-title">{s.title}</div>
        <div className="muted small">{p.view.key}</div>
        <div className="muted small" title={`repository: ${s.repo.root}`}>
          {shortPath(s.repo.root)} · {s.repo.branch} → {s.base}
        </div>
        {s.worktree && (
          <div className="muted small" title={`worktree: ${s.worktree}`}>
            worktree {shortPath(s.worktree)}
          </div>
        )}
        <div className="muted small" title={`${s.created_at}${s.created_in ? ` in ${s.created_in}` : ""}`}>
          created {relTime(s.created_at)}
          {s.created_in && s.created_in !== s.repo.root ? ` from ${shortPath(s.created_in)}` : ""}
        </div>
      </div>

      <Brief brief={s.brief ?? null} onSave={p.onBrief} busy={p.busy} />

      <ol className="stepper">
        {STAGES.map((st, i) => (
          <li key={st} className={i < idx ? "done" : i === idx ? "current" : ""}>
            <span className="dot" />
            <span>{STAGE_LABEL[st]}</span>
          </li>
        ))}
      </ol>

      <div className="side-block actions">
        {stage === "new" && (
          <>
            <button className="btn primary" onClick={() => p.onStartRun("research")} disabled={p.busy} type="button">
              Start research
            </button>
            <div className="skip-row muted small">
              skip:
              <button className="link" onClick={() => p.onStartRun("plan")} disabled={p.busy} type="button">
                research → plan
              </button>
              <button className="link" onClick={() => skipToBuild() && void p.onStage("approved")} disabled={p.busy} type="button">
                planning → build
              </button>
            </div>
          </>
        )}
        {(stage === "researching" || stage === "research-review") && (
          <>
            <button className="btn primary" onClick={() => p.onStartRun("plan")} disabled={p.busy} type="button">
              Plan it
            </button>
            <div className="skip-row muted small">
              skip:
              <button className="link" onClick={() => skipToBuild() && void p.onStage("approved")} disabled={p.busy} type="button">
                planning → build
              </button>
            </div>
          </>
        )}
        {(stage === "planning" || stage === "plan-review") && (
          <>
            <button
              className="btn primary"
              disabled={p.busy || !plan?.exists}
              onClick={() => confirm(`${openHuman} of your comments are still open. Approve anyway?`) && void p.onStage("approved")}
              type="button"
            >
              Approve plan
            </button>
            <button className="btn ghost" onClick={() => p.onStartRun("plan")} disabled={p.busy} type="button">
              {plan?.exists ? "Revise with agent" : "Start planning"}
            </button>
          </>
        )}
        {stage === "approved" && (
          <>
            <button className="btn primary" onClick={() => p.onStartRun("implement")} disabled={p.busy} type="button">
              Start implementation
            </button>
            {!plan?.exists && <div className="muted small">No plan: the agent writes the tickets (todo items) to plan.md first, then works through them.</div>}
          </>
        )}
        {(stage === "implementing" || stage === "implementation-review") && (
          <>
            <button className="btn ghost" onClick={p.onOpenChanges} disabled={p.busy} type="button">
              Review changes
            </button>
            <button
              className="btn primary"
              disabled={p.busy}
              onClick={() => confirm(`${openHuman} of your comments are still open. Accept anyway?`) && void p.onStage("done")}
              type="button"
            >
              Accept implementation
            </button>
          </>
        )}
        <label className="muted small stage-select">
          set stage
          <select value={stage} onChange={(e) => void p.onStage(e.target.value as Stage)} disabled={p.busy}>
            {STAGES.map((st) => (
              <option key={st} value={st}>
                {STAGE_LABEL[st]}
              </option>
            ))}
          </select>
        </label>
      </div>

      <nav className="side-block tabs">
        <DocTab label="Research" doc={research} active={p.activeTab === "research"} onClick={() => p.onTab("research")} />
        <DocTab label="Plan" doc={plan} active={p.activeTab === "plan"} onClick={() => p.onTab("plan")} />
        <button className={`tab ${p.activeTab === "changes" ? "active" : ""}`} onClick={() => p.onTab("changes")} type="button">
          Changes
        </button>
      </nav>

      {plan?.exists && plan.progress.length > 0 && (
        <div className="side-block">
          <div className="side-heading">Progress</div>
          {plan.progress.map((ph) => (
            <div key={ph.name} className="progress">
              <div className="progress-label">
                <span>{ph.name}</span>
                <span className="muted">
                  {ph.done}/{ph.total}
                </span>
              </div>
              <div className="bar">
                <div className="fill" style={{ width: `${ph.total ? (100 * ph.done) / ph.total : 0}%` }} />
              </div>
            </div>
          ))}
        </div>
      )}

      <div className="side-block">
        <div className="side-heading">
          Open comments <span className="badge">{openThreads.length}</span>
        </div>
        {openThreads.length === 0 && <div className="muted small">Click + next to any block to comment.</div>}
        <ul className="comment-list">
          {openThreads.map((c) => (
            <li key={c.id} onClick={() => p.onJump(c)}>
              <span className={`kind kind-${c.kind}`}>{c.kind}</span>
              <span className="muted small">
                {c.doc}:{c.anchor.line}
              </span>
              <span className="preview">{c.body.split("\n")[0]}</span>
            </li>
          ))}
        </ul>
      </div>

      <div className="side-block">
        <div className="side-heading">
          Runs
          {finished.length > 1 && (
            <button className="link" onClick={() => window.confirm(`Clear ${finished.length} finished runs and their transcripts?`) && finished.forEach((r) => void p.onRemoveRun(r.id))} type="button">
              clear finished
            </button>
          )}
        </div>
        {p.view.runs.length === 0 && <div className="muted small">No hosted runs yet.</div>}
        <ul className="run-list">
          {[...p.view.runs].reverse().map((r) => (
            <li key={r.id} className={p.selectedRun === r.id ? "active" : ""} onClick={() => p.onSelectRun(r.id)}>
              <span className={`run-status ${r.status}`} />
              <span>
                {r.provider} · {r.stage}
              </span>
              <span className="spacer" />
              <span className="muted small">{relTime(r.started_at)}</span>
              {!isLive(r) && (
                <button
                  className="run-remove"
                  title="Clear this run and its transcript"
                  onClick={(e) => {
                    e.stopPropagation();
                    if (window.confirm("Clear this run and its transcript?")) void p.onRemoveRun(r.id);
                  }}
                  type="button"
                >
                  ×
                </button>
              )}
            </li>
          ))}
        </ul>
      </div>

      <div className="side-block danger-zone">
        <button
          className="link danger"
          disabled={p.busy}
          onClick={() => {
            const wt = s.worktree ? window.confirm(`Also remove the git worktree at ${s.worktree}? (Cancel keeps it.)`) : false;
            if (window.confirm(`Drop session ${p.view.key}? Its documents, comments and run transcripts are deleted. This cannot be undone.`)) void p.onDelete(wt);
          }}
          type="button"
        >
          {stage === "done" ? "Drop this finished session" : "Drop session"}
        </button>
      </div>
    </aside>
  );
}

function Brief({ brief, onSave, busy }: { brief: string | null; onSave: (b: string | null) => Promise<void>; busy: boolean }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(brief ?? "");
  useEffect(() => {
    if (!editing) setDraft(brief ?? "");
  }, [brief, editing]);
  const save = async () => {
    try {
      await onSave(draft.trim() ? draft : null);
      setEditing(false);
    } catch {
      // the page shows the error
    }
  };
  return (
    <div className="side-block brief">
      <div className="side-heading">
        Brief
        {!editing && (
          <button className="link" onClick={() => setEditing(true)} type="button">
            {brief ? "edit" : "add"}
          </button>
        )}
      </div>
      {editing ? (
        <>
          <textarea
            rows={5}
            value={draft}
            autoFocus
            placeholder="What should the agent research or build? Context, links, constraints. It is rendered into every stage prompt."
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if ((e.metaKey || e.ctrlKey) && e.key === "Enter") void save();
              if (e.key === "Escape") setEditing(false);
            }}
          />
          <div className="composer-actions">
            <span className="spacer" />
            <button className="btn ghost small" onClick={() => setEditing(false)} type="button">
              Cancel
            </button>
            <button className="btn primary small" disabled={busy} onClick={() => void save()} type="button">
              Save
            </button>
          </div>
        </>
      ) : brief ? (
        <div className="brief-text">{brief}</div>
      ) : (
        <div className="muted small">Nothing yet. Tell the agent what to research or build; it goes into every stage prompt.</div>
      )}
    </div>
  );
}

function isLive(r: Run): boolean {
  return r.status === "starting" || r.status === "running" || r.status === "waiting" || r.status === "idle";
}

function DocTab({ label, doc, active, onClick }: { label: string; doc: DocSummary | undefined; active: boolean; onClick: () => void }) {
  return (
    <button className={`tab ${active ? "active" : ""}`} onClick={onClick} type="button">
      {label}
      {doc && !doc.exists && <span className="muted small"> · waiting</span>}
    </button>
  );
}

export function docKindOf(tab: string): DocKind | null {
  return tab === "research" || tab === "plan" ? tab : null;
}

export type { Run };
