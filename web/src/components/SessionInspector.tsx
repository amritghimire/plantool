import type { ReactNode } from "react";
import {
  CheckCircle,
  Circle,
  GitBranch,
  ChatCircleText,
  ClockCounterClockwise,
} from "@phosphor-icons/react";
import type { SessionView } from "../types";
import { relTime } from "../lib/format";

export function SessionInspector({
  view,
  reviewPanel,
  activityPanel,
}: {
  view: SessionView;
  reviewPanel: ReactNode;
  activityPanel: ReactNode;
}) {
  const plan = view.docs.find((doc) => doc.kind === "plan");
  const phases = plan?.progress ?? [];
  const done = phases.reduce((n, phase) => n + phase.done, 0);
  const total = phases.reduce((n, phase) => n + phase.total, 0);
  return (
    <aside className="session-inspector" aria-label="Session overview">
      <header className="inspector-title">Session overview</header>
      <section className="inspector-section">
        <h2>The brief</h2>
        <p>
          {view.session.brief ||
            "No brief yet. Add the goal and constraints in session details."}
        </p>
      </section>
      {total > 0 && (
        <section className="inspector-section">
          <h2>
            Plan progress{" "}
            <span>
              {done}/{total}
            </span>
          </h2>
          <div className="bar">
            <div
              className="fill"
              style={{ width: `${(100 * done) / total}%` }}
            />
          </div>
          <ul className="inspector-phases">
            {phases.map((phase) => (
              <li key={phase.name}>
                {phase.done === phase.total ? (
                  <CheckCircle size={17} aria-hidden="true" />
                ) : (
                  <Circle size={17} aria-hidden="true" />
                )}
                <span>{phase.name}</span>
                <small>
                  {phase.done}/{phase.total}
                </small>
              </li>
            ))}
          </ul>
          <p className="small muted">
            Checkbox progress is reported by the agent.
          </p>
        </section>
      )}
      <section className="inspector-section">
        <h2>
          <ChatCircleText size={17} aria-hidden="true" /> Review feedback
        </h2>
        <p>
          {view.open_comments
            ? `${view.open_comments} open comment${view.open_comments === 1 ? "" : "s"}. Read the conversation in the session panel.`
            : "No open comments. Select a paragraph or a code line to leave feedback."}
        </p>
        {reviewPanel}
      </section>
      <section className="inspector-section">
        <h2>
          <GitBranch size={17} aria-hidden="true" /> Workspace
        </h2>
        <dl className="inspector-context">
          <div>
            <dt>Branch</dt>
            <dd>{view.workspace_branch || view.session.repo.branch}</dd>
          </div>
          <div>
            <dt>Base</dt>
            <dd>{view.session.base}</dd>
          </div>
          <div>
            <dt>Review tool</dt>
            <dd>{view.session.difftool ?? "Auto detect"}</dd>
          </div>
        </dl>
      </section>
      {view.state.activity?.length ? (
        <section className="inspector-section">
          <h2>
            <ClockCounterClockwise size={17} aria-hidden="true" /> Recent
            activity
          </h2>
          <ol className="inspector-activity">
            {view.state.activity
              .slice(-4)
              .reverse()
              .map((entry) => (
                <li key={entry.seq}>
                  <p>{entry.summary}</p>
                  <time dateTime={entry.at}>{relTime(entry.at)}</time>
                </li>
              ))}
          </ol>
        </section>
      ) : null}
      {activityPanel}
    </aside>
  );
}
