import { useEffect, useState } from "react";
import { api } from "../api";
import { STAGES, STAGE_LABEL, type SessionView, type Stage } from "../types";
import { relTime, shortPath, workspacePath } from "../lib/format";
import { useConfirmation } from "../lib/confirmation";
import { runAnyStage } from "../lib/notice";
import type { RunStage } from "./StartRunDialog";
import { CopyButton } from "./CopyButton";
import { WherePanel } from "./WherePanel";
import { ArchivePanel } from "./ArchivePanel";
import { AwaySummary } from "./AwaySummary";
import { DropSessionDialog } from "./DropSessionDialog";

export function DetailsDrawer({
  view,
  busy,
  onBrief,
  onStage,
  onStartRun,
  onRefreshPr,
  onUpdated,
  onWorkspace,
  onDelete,
}: {
  view: SessionView;
  busy: boolean;
  onBrief: (brief: string | null) => Promise<void>;
  onStage: (s: Stage) => Promise<void>;
  onStartRun: (stage: RunStage) => void;
  onRefreshPr: () => void;
  onUpdated: (view: SessionView) => void;
  onWorkspace: (session: SessionView["session"]) => void;
  onDelete: (removeWorktree: boolean) => Promise<void>;
}) {
  const { confirm, dialog } = useConfirmation();
  const s = view.session;
  const stage = view.state.stage;
  const [deleting, setDeleting] = useState(false);
  const [workspaceCheck, setWorkspaceCheck] = useState<{
    missing: boolean;
    candidates: { root: string; branch: string }[];
  } | null>(null);
  const [workspaceError, setWorkspaceError] = useState<string | null>(null);
  const phases = view.docs.find((d) => d.kind === "plan")?.progress ?? [];
  const activity = (view.state.activity ?? []).slice(-8).reverse();
  const setStage = async (to: Stage) => {
    if (to === stage) return;
    if (
      await confirm(
        `Set the stage from ${STAGE_LABEL[stage]} to ${STAGE_LABEL[to]}? This skips the step's normal flow.`,
        false,
        {
          title: "Set the stage",
          confirmLabel: `Set to ${STAGE_LABEL[to]}`,
          fields: [
            { label: "From", value: STAGE_LABEL[stage] },
            { label: "To", value: STAGE_LABEL[to] },
          ],
        },
      )
    )
      await onStage(to);
  };
  return (
    <div className="details-drawer">
      <section className="details-section">
        <p className="details-key">{view.key}</p>
        {s.pull_request && (
          <p className="small">
            PR{" "}
            <a href={s.pull_request.url} target="_blank" rel="noreferrer">
              #{s.pull_request.number}
            </a>{" "}
            · {s.pull_request.state.toLowerCase()}
            {s.pull_request.draft ? " draft" : ""}{" "}
            <button className="link" type="button" onClick={onRefreshPr}>
              Refresh
            </button>{" "}
            <span className="muted">checked {relTime(s.pull_request.updated_at)}</span>
          </p>
        )}
      </section>

      <section className="details-section">
        <Brief brief={s.brief ?? null} onSave={onBrief} busy={busy} />
      </section>

      <section className="details-section">
        <h3>Workspace</h3>
        {s.worktree && view.workspace_branch === null && (
          <p className="error small">
            Workspace missing, detached, or no longer in this repository. Refresh
            to choose a valid worktree.
          </p>
        )}
        <div className="workspace-detail" title={workspacePath(s)}>
          <span className="workspace-line">
            <span className="workspace-path">{workspacePath(s)}</span>
            <CopyButton
              text={workspacePath(s)}
              label="Copy"
              className="btn ghost small workspace-copy"
            />
          </span>
          <button
            className="link small"
            type="button"
            onClick={() => {
              setWorkspaceError(null);
              void api
                .workspaceCandidates(view.key)
                .then(setWorkspaceCheck)
                .catch((e: Error) => setWorkspaceError(e.message));
            }}
          >
            Refresh workspace
          </button>
        </div>
        {workspaceCheck?.missing && (
          <p className="error small">
            Recorded workspace is missing or no longer belongs to this repository.
            New runs are blocked until you choose a valid workspace.
          </p>
        )}
        {workspaceCheck?.candidates
          .filter((candidate) => candidate.root !== s.worktree)
          .map((candidate) => (
            <button
              key={candidate.root}
              className="link small"
              type="button"
              onClick={async () => {
                if (
                  !(await confirm(
                    `Use ${candidate.root} on branch ${candidate.branch} for this session?`,
                  ))
                )
                  return;
                void api
                  .adoptWorkspace(view.key, candidate.root)
                  .then((result) => {
                    onWorkspace(result.session);
                    setWorkspaceCheck(null);
                  })
                  .catch((e: Error) => setWorkspaceError(e.message));
              }}
            >
              Use {shortPath(candidate.root)} · {candidate.branch}
            </button>
          ))}
        {workspaceCheck &&
          !workspaceCheck.missing &&
          workspaceCheck.candidates.length === 0 && (
            <p className="muted small">No matching worktree found.</p>
          )}
        {workspaceError && <p className="error small">{workspaceError}</p>}
        <dl className="details-fields">
          <div>
            <dt>Repository</dt>
            <dd title={s.repo.root}>{shortPath(s.repo.root)}</dd>
          </div>
          <div>
            <dt>Branch</dt>
            <dd>
              {view.workspace_branch || s.repo.branch} → {s.base}
            </dd>
          </div>
          <div>
            <dt>Review tool</dt>
            <dd>{s.difftool ?? "auto"}</dd>
          </div>
          <div>
            <dt>Created</dt>
            <dd title={`${s.created_at}${s.created_in ? ` in ${s.created_in}` : ""}`}>
              {relTime(s.created_at)}
              {s.created_in && s.created_in !== s.repo.root
                ? ` from ${shortPath(s.created_in)}`
                : ""}
            </dd>
          </div>
        </dl>
        <WherePanel
          key={`${view.key}:${s.base}:${s.worktree}:${s.difftool}:${view.workspace_branch}:${JSON.stringify(s.pause_rule)}`}
          view={view}
          onUpdated={onUpdated}
        />
      </section>

      {phases.length > 0 && (
        <section className="details-section">
          <h3>Build progress</h3>
          <ul className="details-phases">
            {phases.map((p) => (
              <li key={p.name}>
                <span>{p.name}</span>
                <span className="num">
                  {p.done} / {p.total}
                </span>
              </li>
            ))}
          </ul>
          <p className="muted small">Checkbox progress is reported by the agent.</p>
        </section>
      )}

      <section className="details-section">
        <h3>History</h3>
        <AwaySummary sessionKey={view.key} state={view.state} />
        {activity.length ? (
          <ol className="details-activity">
            {activity.map((a) => (
              <li key={a.seq}>
                <span>{a.summary}</span>
                <time dateTime={a.at}>{relTime(a.at)}</time>
              </li>
            ))}
          </ol>
        ) : (
          <p className="muted small">No recorded activity yet.</p>
        )}
      </section>

      <section className="details-section">
        <ArchivePanel sessionKey={view.key} repo={s.repo.root} />
      </section>

      <section className="details-section">
        <details className="disclosure advanced-controls">
          <summary>Advanced</summary>
          <button
            className="link"
            onClick={() => onStartRun(runAnyStage(stage))}
            disabled={busy}
            type="button"
          >
            Run any stage…
          </button>
          <label className="stage-select">
            Set stage
            <select
              value={stage}
              onChange={(e) => void setStage(e.target.value as Stage)}
              disabled={busy}
            >
              {STAGES.map((st) => (
                <option key={st} value={st}>
                  {STAGE_LABEL[st]}
                </option>
              ))}
            </select>
          </label>
        </details>
      </section>

      <section className="details-section danger-zone">
        <button
          className="link danger"
          disabled={busy}
          onClick={() => setDeleting(true)}
          type="button"
        >
          {stage === "done" ? "Drop this finished session" : "Drop session"}
        </button>
      </section>
      {dialog}
      {deleting && (
        <DropSessionDialog
          view={view}
          busy={busy}
          onClose={() => setDeleting(false)}
          onDelete={onDelete}
        />
      )}
    </div>
  );
}

function Brief({
  brief,
  onSave,
  busy,
}: {
  brief: string | null;
  onSave: (b: string | null) => Promise<void>;
  busy: boolean;
}) {
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
    <div className="brief">
      <h3>
        Brief
        {!editing && (
          <button className="link" onClick={() => setEditing(true)} type="button">
            {brief ? "Edit" : "Add"}
          </button>
        )}
      </h3>
      {editing ? (
        <>
          <textarea
            rows={6}
            value={draft}
            autoFocus
            aria-label="Brief"
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
            <button className="btn small" disabled={busy} onClick={() => void save()} type="button">
              Save
            </button>
          </div>
        </>
      ) : brief ? (
        <div className="brief-text">{brief}</div>
      ) : (
        <p className="muted small">
          Nothing yet. Tell the agent what to research or build; it goes into every
          stage prompt.
        </p>
      )}
    </div>
  );
}
