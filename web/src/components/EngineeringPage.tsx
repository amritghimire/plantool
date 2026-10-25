import { useCallback, useState } from "react";
import { Link, useParams, useSearchParams } from "react-router-dom";
import { api } from "../api";
import { stepLabel } from "../engineering";
import { usePolling } from "../lib/usePolling";
import { RecoveryControls } from "./RecoveryControls";
import { GithubSettings } from "./GithubSettings";
export function EngineeringPage() {
  const { data, error, refresh, updated } = usePolling(api.engineering);
  const status = usePolling(api.workerStatus);
  const [params, setParams] = useSearchParams();
  const [query, setQuery] = useState("");
  const selected = params.get("project");
  const project =
    data?.projects.find((p) => p.id === selected) ??
    (!selected ? data?.projects[0] : undefined);
  const adding = params.get("add") === "1";
  const items =
    data?.items.filter(
      (i) =>
        i.project === project?.id &&
        (!query ||
          `${i.title} ${i.step} ${i.issue ?? ""}`
            .toLowerCase()
            .includes(query.toLowerCase())),
    ) ?? [];
  const active = data?.active.filter((a) => a.project === project?.id) ?? [];
  const worker = status.data?.worker;
  const attention = items.filter(
    (i) => i.frozen || i.error || i.waiting_gates > 0,
  );
  return (
    <main className="engineering-page">
      <aside className="page-sidebar">
        <div className="eyebrow">ENGINEERING OS</div>
        <h1>Projects</h1>
        <p className="muted">One team, across your repositories.</p>
        <nav aria-label="Projects">
          {data?.projects.map((p) => (
            <button
              type="button"
              key={p.id}
              aria-current={p.id === project?.id ? "page" : undefined}
              onClick={() => setParams({ project: p.id })}
            >
              {p.name ?? p.id}
              <span className="small muted">
                {p.visibility ?? "Repository"}
              </span>
            </button>
          ))}
        </nav>
        <button
          className="btn"
          onClick={() => setParams({ ...Object.fromEntries(params), add: "1" })}
        >
          Add project
        </button>
        <Link to="/setup/engineering">Resume guided setup</Link>
        <Link to="/settings/github">GitHub connections & approvers</Link>
      </aside>
      <section className="engineering-content">
        <header className="page-heading">
          <div>
            <div className="eyebrow">PROJECT WORKSPACE</div>
            <h2>
              {adding
                ? "Add a project"
                : (project?.name ?? "Welcome to Engineering OS")}
            </h2>
            <p className="muted">
              {adding
                ? "Reuse a GitHub App installation or connect another one. Each repository has its own checkout and policies."
                : "See what is running, what happened, and what needs you."}
            </p>
          </div>
          <button
            className="btn"
            onClick={() => {
              refresh();
              status.refresh();
            }}
          >
            Refresh activity
          </button>
        </header>
        {(error || status.error) && (
          <p role="alert" className="error">
            {error || status.error}
          </p>
        )}
        {adding ? (
          <>
            <GithubSettings
              mode="projects"
              onConfigured={() => {
                setParams({});
                refresh();
              }}
            />
            <button className="btn ghost" onClick={() => setParams({})}>
              Back to projects
            </button>
          </>
        ) : !data ? (
          <p role="status">Loading projects…</p>
        ) : !project ? (
          <div className="onboarding-card">
            <h3>
              {selected
                ? "Project not found"
                : "Your engineering workspace starts here"}
            </h3>
            <p>
              Connect repositories as individual projects. The worker, model
              lease and approvers are shared; schedules and policies belong to
              each project.
            </p>
            <ol>
              <li>
                <Link to="/setup/engineering">
                  Start or resume guided setup
                </Link>
              </li>
              <li>
                <Link to="/settings/worker">Set up the worker</Link>
              </li>
              <li>
                <Link to="/settings/models">Verify model routes</Link>
              </li>
              <li>
                <Link to="/settings/projects">
                  Choose project policies and schedules
                </Link>
              </li>
            </ol>
          </div>
        ) : (
          <>
            <div className="status-grid">
              <article className="status-card">
                <span className="eyebrow">WORKER SERVICE · GLOBAL</span>
                <h3>
                  <span
                    className={`status-dot ${worker?.available ? "ok" : "warn"}`}
                  />
                  {worker
                    ? worker.available
                      ? "Ready"
                      : "Needs setup or repair"
                    : "Checking…"}
                </h3>
                <p>
                  {worker?.message ?? "Checking the shared workflow worker."}
                </p>
                {!worker?.available && (
                  <Link className="btn" to="/settings/worker">
                    Set up or repair worker
                  </Link>
                )}
              </article>
              <article className="status-card">
                <span className="eyebrow">THIS PROJECT</span>
                <h3>
                  {active.length} active{" "}
                  {active.length === 1 ? "attempt" : "attempts"}
                </h3>
                <p>
                  Roles waiting, queued or executing. One model runs at a time
                  across all projects.
                </p>
                <p className="small">
                  Scheduler: {stepLabel(project.scheduler ?? "not started")}
                </p>
              </article>
              <article className="status-card">
                <span className="eyebrow">NEXT ACTIONS</span>
                <h3>
                  {attention.length}{" "}
                  {attention.length === 1 ? "item needs" : "items need"}{" "}
                  attention
                </h3>
                <p>
                  Inspect waiting gates or frozen items below. GitHub remains
                  the approval surface.
                </p>
                <Link
                  to={`/workspace?project=${encodeURIComponent(project.id)}`}
                >
                  Open team workspace →
                </Link>
              </article>
            </div>
            {project.preflight && (project.preflight.warnings.length || project.preflight.advisories?.length) ? (
              <aside className="next-action">
                <strong>GitHub protection guidance</strong>
                <p className="small muted">Advisory only: this does not block planning or execution. GitHub still enforces configured rules at merge.</p>
                {[...project.preflight.warnings,...(project.preflight.advisories ?? [])].map((w) => (
                  <p key={w}>{w}</p>
                ))}
                <Link to="/settings/github">Review GitHub preflight</Link>
              </aside>
            ) : null}
            {project.scheduler === "escalated" && <RecoveryControls item={`${project.id}:project`} onRecovered={refresh} />}
            {project.error && (
              <p role="alert" className="error">
                GitHub sync: {project.error}
              </p>
            )}
            <div className="project-meta">
              <span>
                {project.visibility} · {project.branch ?? "Default branch"}
              </span>
              <span>
                Last GitHub sync:{" "}
                {project.synced_at
                  ? new Date(project.synced_at).toLocaleString()
                  : "Not yet"}
              </span>
              <details>
                <summary>Project checkout</summary>
                <code>{project.checkout}</code>
              </details>
              <Link to="/settings/projects">Project policies</Link>
            </div>
            <section aria-label="Active roles">
              <h3>Running and queued</h3>
              {active.length ? (
                active.map((a) => (
                  <Link
                    className="activity-row"
                    key={a.id}
                    to={`/engineering/items/${encodeURIComponent(a.item)}`}
                  >
                    <span className="role-avatar">
                      {a.role.slice(0, 2).toUpperCase()}
                    </span>
                    <span>
                      <strong>{a.role}</strong> · {a.title}
                      <small>
                        {a.model ?? a.route}{" "}
                        {a.provider ? `· ${a.provider}` : ""}
                      </small>
                    </span>
                    <span className="state-chip">{a.state}</span>
                  </Link>
                ))
              ) : (
                <p className="muted">
                  No active attempts in this project. Check work items for
                  backlog, approvals or completed activity.
                </p>
              )}
            </section>
            <section>
              <div className="section-heading">
                <h3>Work items</h3>
                <label>
                  Search work items
                  <input
                    type="search"
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                    placeholder="Title, issue or stage"
                  />
                </label>
              </div>
              {items.length ? (
                <div className="item-grid">
                  {items.map((i) => (
                    <Link
                      className="item-card"
                      key={i.id}
                      to={`/engineering/items/${encodeURIComponent(i.id)}`}
                    >
                      <div className="section-heading">
                        <span className="small muted">
                          {i.issue ? `ISSUE #${i.issue}` : "PROJECT ACTIVITY"}
                        </span>
                        {i.priority === "urgent" && (
                          <span className="state-chip attention">Urgent</span>
                        )}
                      </div>
                      <h4>{i.title}</h4>
                      <span
                        className={`state-chip ${i.frozen ? "attention" : ""}`}
                      >
                        {i.frozen
                          ? "Owner attention required"
                          : stepLabel(i.step)}
                      </span>
                      {i.waiting_gates > 0 && (
                        <p>
                          {i.waiting_gates} pending or open{" "}
                          {i.waiting_gates === 1 ? "gate" : "gates"}
                        </p>
                      )}
                      {i.error && <p className="error">{i.error}</p>}
                    </Link>
                  ))}
                </div>
              ) : (
                <div className="empty-state">
                  <h4>
                    {query
                      ? "No matching work items"
                      : "Ready for your first issue"}
                  </h4>
                  <p>
                    {query
                      ? "Try another title, issue number or stage."
                      : "Create a small issue as an allowlisted owner. PM triage will place it in the project flow."}
                  </p>
                  {!query && project.name && (
                    <a
                      href={`https://github.com/${project.name}/issues/new`}
                      target="_blank"
                      rel="noreferrer"
                    >
                      Create an issue on GitHub →
                    </a>
                  )}
                </div>
              )}
            </section>
          </>
        )}
        {updated && (
          <p className="small muted">
            Activity refreshed {updated.toLocaleTimeString()}
          </p>
        )}
      </section>
    </main>
  );
}
export function EngineeringItemPage() {
  const { id = "" } = useParams();
  const load = useCallback(() => api.engineeringItem(id), [id]);
  const { data, error, refresh } = usePolling(load);
  const [tab, setTab] = useState("attempts");
  if (!data)
    return (
      <main className="engineering-content">
        <Link to="/engineering">← Projects</Link>
        {error ? (
          <p role="alert">{error}</p>
        ) : (
          <p role="status">Loading item activity…</p>
        )}
      </main>
    );
  const { item } = data;
  const repo = data.repository;
  const issueUrl =
    repo && item.issue
      ? `https://github.com/${repo}/issues/${item.issue}`
      : null;
  const prUrl =
    repo && item.pr ? `https://github.com/${repo}/pull/${item.pr}` : null;
  return (
    <main className="item-detail">
      <Link to={`/engineering?project=${encodeURIComponent(item.project)}`}>
        ← Project overview
      </Link>
      <header className="page-heading">
        <div>
          <div className="eyebrow">
            WORK ITEM {item.issue ? `#${item.issue}` : ""}
          </div>
          <h1>{item.title}</h1>
          <p>
            <span className="state-chip">
              {item.frozen
                ? "Frozen · owner attention required"
                : stepLabel(item.step)}
            </span>
          </p>
        </div>
        <button className="btn" onClick={refresh}>
          Refresh item
        </button>
      </header>
      {error && <p role="alert">{error}</p>}
      {item.error && (
        <p role="alert" className="error">
          {item.error}
        </p>
      )}
      <div className="next-action">
        <strong>Next action</strong>
        <p>
          {item.frozen
            ? "Review the failure and recovery state before restarting work. Side effects remain frozen."
            : item.waiting_gates
              ? "Review the current gate and its exact plan hash or commit on GitHub."
              : "Follow the recorded attempts and graph activity below. The scheduler and worker advance eligible work."}
        </p>
        <div className="button-row">
          {issueUrl && (
            <a className="btn" href={issueUrl} target="_blank" rel="noreferrer">
              Open issue
            </a>
          )}
          {prUrl && (
            <a className="btn" href={prUrl} target="_blank" rel="noreferrer">
              Open PR
            </a>
          )}
          <Link
            className="btn"
            to={`/workspace?project=${encodeURIComponent(item.project)}&item=${encodeURIComponent(item.id)}`}
          >
            Open discussion
          </Link>
        </div>
      </div>
      {item.frozen && <RecoveryControls item={item.id} onRecovered={refresh} />}
      <nav className="detail-tabs" aria-label="Item activity">
        {[
          ["attempts", "Role attempts"],
          ["graph", "Graph history"],
          ["gates", "Approval gates"],
        ].map(([key, name]) => (
          <button
            key={key}
            aria-pressed={tab === key}
            onClick={() => setTab(key)}
          >
            {name}
          </button>
        ))}
      </nav>
      {tab === "attempts" ? (
        <section aria-label="Role attempts">
          <p className="muted">
            Latest {data.history_limit} recorded attempts. Token totals are
            reported usage, not estimates.
          </p>
          {data.attempts.length ? (
            data.attempts.map((a) => (
              <article className="attempt-card" key={a.id}>
                <div className="section-heading">
                  <h3>
                    {a.role}{" "}
                    <span className="small muted">Attempt {a.generation}</span>
                  </h3>
                  <span className="state-chip">{a.state}</span>
                </div>
                <p>
                  {a.model ?? a.route}
                  {a.provider ? ` · ${a.provider}` : ""}
                </p>
                <p>{a.summary ?? "No result summary recorded yet."}</p>
                <div className="small muted">
                  {new Date(a.started_at).toLocaleString()}
                  {a.duration_ms !== null
                    ? ` · ${(a.duration_ms / 1000).toFixed(1)}s`
                    : ""}
                  {a.tokens_in !== null
                    ? ` · ${a.tokens_in} input / ${a.tokens_out ?? 0} output tokens`
                    : ""}
                </div>
              </article>
            ))
          ) : (
            <p>No attempts recorded yet.</p>
          )}
        </section>
      ) : tab === "graph" ? (
        <section>
          <h2>Recorded graph history</h2>
          <p className="muted">
            Actual persisted events, newest first. Conditional or skipped roles
            are not shown as completed.
          </p>
          <ol className="graph-timeline">
            {data.events.map((e) => (
              <li key={e.id}>
                <span className="timeline-dot" />
                <strong>
                  {e.step ? stepLabel(e.step) : e.action.replaceAll(".", " ")}
                </strong>
                <time>{new Date(e.at).toLocaleString()}</time>
                <small>{e.action}</small>
              </li>
            ))}
          </ol>
          {!data.events.length && <p>No graph events recorded yet.</p>}
          <details>
            <summary>Commit references</summary>
            <p>
              Base: <code>{data.base}</code>
            </p>
            <p>
              Current head: <code>{data.head}</code>
            </p>
          </details>
        </section>
      ) : (
        <section>
          <h2>Approval gates</h2>
          <p>
            Decisions stay on GitHub and bind to the current revision or head.
            Workspace messages never approve work.
          </p>
          {data.gates.map((g) => {
            const url =
              g.kind === "pr" || g.kind === "milestone" ? prUrl : issueUrl;
            return (
              <article className="attempt-card" key={g.id}>
                <div className="section-heading">
                  <h3>{g.kind}</h3>
                  <span className="state-chip">{g.state}</span>
                </div>
                <p className="digest">{g.sha ?? g.hash}</p>
                {url && (
                  <a
                    href={`${url}${g.comment ? `#issuecomment-${g.comment}` : ""}`}
                    target="_blank"
                    rel="noreferrer"
                  >
                    Review on GitHub →
                  </a>
                )}
              </article>
            );
          })}
          {!data.gates.length && <p>No gates recorded yet.</p>}
        </section>
      )}
    </main>
  );
}
