import { STAGES, STAGE_LABEL, type Comment, type DocKind, type DocSummary, type Run, type SessionView, type Stage } from "../types";
import { relTime, shortPath } from "../lib/format";

export interface SidebarProps {
  view: SessionView;
  comments: Comment[];
  activeTab: string;
  onTab: (t: string) => void;
  onStage: (s: Stage) => Promise<void>;
  onJump: (c: Comment) => void;
  onStartRun: (stage: "research" | "plan" | "implement") => void;
  onOpenChanges: () => void;
  onSelectRun: (id: string) => void;
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

  const confirm = (msg: string) => openHuman === 0 || window.confirm(msg);

  return (
    <aside className="sidebar">
      <div className="side-block">
        <div className="side-title">{s.title}</div>
        <div className="muted small">{p.view.key}</div>
        <div className="muted small" title={s.worktree ?? s.repo.root}>
          {shortPath(s.worktree ?? s.repo.root)} · {s.repo.branch} → {s.base}
        </div>
      </div>

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
          <button className="btn primary" onClick={() => p.onStartRun("research")} disabled={p.busy} type="button">
            Start research
          </button>
        )}
        {(stage === "researching" || stage === "research-review") && (
          <button className="btn primary" onClick={() => p.onStartRun("plan")} disabled={p.busy} type="button">
            Plan it
          </button>
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
          <button className="btn primary" onClick={() => p.onStartRun("implement")} disabled={p.busy} type="button">
            Start implementation
          </button>
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
        <div className="side-heading">Runs</div>
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
            </li>
          ))}
        </ul>
      </div>
    </aside>
  );
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
