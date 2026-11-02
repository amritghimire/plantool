import { MagnifyingGlassIcon, ArrowUpRightIcon, CaretRightIcon } from "@phosphor-icons/react";
import { cn } from "../lib/cn";
import { CodeReview } from "./CodeReview";
import { Thread, type ThreadActions } from "./Thread";
import type { Comment } from "../types";
import { Fragment, useEffect, useRef, useState } from "react";
import { driftRows } from "../lib/drift";
import { api } from "../api";
import type { ChangeScope, ChangesResponse, FileDiff } from "../types";

type DiffState =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ready"; diff: FileDiff };

export function ChangesTab({
  sessionKey,
  nonce,
  workspace,
  comments = [],
  actions,
  onAdded,
  highlightComment,
}: {
  sessionKey: string;
  nonce: number;
  workspace?: string;
  comments?: Comment[];
  actions?: ThreadActions;
  onAdded?: () => void;
  highlightComment?: string | null;
}) {
  const [fileQuery, setFileQuery] = useState("");
  const [data, setData] = useState<ChangesResponse | null>(null);
  const [scope, setScope] = useState<ChangeScope | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const generation = useRef(0);
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [diffs, setDiffs] = useState<Record<string, DiffState>>({});
  useEffect(() => {
    let active = true;
    const version = ++generation.current;
    setLoading(true);
    setErr(null);
    api
      .changes(sessionKey, scope ?? undefined)
      .then((r) => {
        if (!active || generation.current !== version) return;
        setData(r);
        setLoading(false);
        setDiffs({});
        setOpen(new Set());
      })
      .catch((e: Error) => {
        if (active) setErr(e.message);
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
      generation.current++;
    };
  }, [sessionKey, nonce, scope]);
  useEffect(() => {
    if (data?.review?.tool !== "difftool") return;
    let active = true;
    const timer = window.setInterval(() => {
      void api
        .changes(sessionKey, scope ?? undefined)
        .then((result) => {
          if (active) setData(result);
        })
        .catch(() => {});
    }, 5000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [sessionKey, scope, data?.review?.tool]);
  useEffect(() => {
    if (!highlightComment || !data) return;
    document
      .getElementById(`c-${highlightComment}`)
      ?.scrollIntoView?.({ block: "center", behavior: "auto" });
  }, [highlightComment, data]);
  const openReview = async () => {
    const version = generation.current;
    setBusy(true);
    setErr(null);
    try {
      const r = await api.openChanges(sessionKey, scope ?? undefined);
      if (generation.current === version) setData(r);
    } catch (e) {
      if (generation.current === version) setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const pickScope = (s: ChangeScope) => {
    if (s === (scope ?? data?.scope)) return;
    generation.current++;
    setOpen(new Set());
    setDiffs({});
    setScope(s);
  };
  const toggle = (path: string) => {
    const next = new Set(open);
    if (next.has(path)) {
      next.delete(path);
      setOpen(next);
      return;
    }
    next.add(path);
    setOpen(next);
    if (diffs[path]) return;
    const version = generation.current;
    setDiffs((d) => ({ ...d, [path]: { kind: "loading" } }));
    api
      .changesFile(sessionKey, path, data?.scope)
      .then((diff) => {
        if (generation.current === version)
          setDiffs((d) => ({ ...d, [path]: { kind: "ready", diff } }));
      })
      .catch((e: Error) => {
        if (generation.current === version)
          setDiffs((d) => ({
            ...d,
            [path]: { kind: "error", message: e.message },
          }));
      });
  };
  if (err && !data)
    return (
      <div className="doc-empty">
        <p className="error" role="alert">
          {err}
        </p>
      </div>
    );
  if (!data)
    return (
      <div className="diff-loading" role="status">
        Loading comparison…
        <div />
        <div />
        <div />
      </div>
    );
  const total = changeTotals(data);
  const drift = data.drift?.checked
    ? driftRows(data.drift.planned_files, data.stat.map((f) => f.path))
    : null;
  const query = fileQuery.toLowerCase();
  const files = data.stat.filter((file) => file.path.toLowerCase().includes(query));
  const difftoolComments =
    data.review?.tool === "difftool" && data.difftool_open_comments !== undefined
      ? data.difftool_open_comments === null
        ? "Couldn't be read from difftool"
        : data.difftool_open_comments === 0
          ? "None yet. They appear here when you leave them in difftool."
          : `${data.difftool_open_comments} open. Resolve them in difftool.`
      : null;
  return (
    <div className="changes" aria-busy={loading}>
      {loading && (
        <p className="small muted" role="status">
          Updating comparison…
        </p>
      )}
      <div className="changes-head">
        <p className="changes-count">
          <strong className="num">{data.stat.length}</strong> file
          {data.stat.length === 1 ? "" : "s"} changed {data.label}
          <span className="added"> +{total.added}</span>
          <span className="deleted"> −{total.deleted}</span>
          {data.review?.tool === "difftool" && (
            <span className="muted"> · includes new files</span>
          )}
        </p>
        <span className="review-tool">
          {data.tool === "difftool"
            ? "difftool"
            : data.tool === "git-difftool"
              ? "git difftool"
              : "Built-in diff"}
        </span>
        <button
          className="btn"
          onClick={() =>
            data.tool === "none"
              ? data.stat[0] && toggle(data.stat[0].path)
              : void openReview()
          }
          disabled={
            busy || loading || (data.tool === "none" && !data.stat.length)
          }
          type="button"
        >
          {data.tool === "difftool"
            ? "Review in difftool"
            : data.tool === "none"
              ? "Use built-in diff"
              : "Open in git difftool"}
          {data.tool !== "none" && <ArrowUpRightIcon size={15} aria-hidden="true" />}
        </button>
      </div>
      <dl className="review-context" aria-label="Comparison context">
        {workspace && (
          <div>
            <dt>Workspace</dt>
            <dd className="ref" title={workspace}>
              {workspace}
            </dd>
          </div>
        )}
        <div>
          <dt>Comparing</dt>
          <dd>
            <code className="ref" title={data.base}>
              {data.base}
            </code>
            <span aria-hidden="true"> → </span>working tree
          </dd>
        </div>
        <div>
          <dt>Scope</dt>
          <dd>
            {data.step_available ? (
              <div className="seg" role="tablist" aria-label="Change scope">
                <button
                  className={data.scope === "step" ? "active" : ""}
                  onClick={() => pickScope("step")}
                  type="button"
                  role="tab"
                  disabled={busy}
                  aria-selected={data.scope === "step"}
                >
                  This milestone
                </button>
                <button
                  className={data.scope === "all" ? "active" : ""}
                  onClick={() => pickScope("all")}
                  type="button"
                  role="tab"
                  disabled={busy}
                  aria-selected={data.scope === "all"}
                >
                  Whole implementation
                </button>
              </div>
            ) : data.scope === "step" ? (
              "Current milestone"
            ) : (
              "Whole implementation"
            )}
          </dd>
        </div>
        {difftoolComments && (
          <div>
            <dt>Difftool comments</dt>
            <dd>{difftoolComments}</dd>
          </div>
        )}
      </dl>
      {data.review?.tool === "git-difftool" && (
        <p className="muted small">git difftool was launched on this machine.</p>
      )}
      {err && (
        <p className="error" role="alert">
          {err}
        </p>
      )}
      {data.message && <p className="muted">{data.message}</p>}
      {actions && comments.some((c) => !c.parent && c.anchor.code) && (
        <section aria-label="Code review comments">
          <h2>Code review comments</h2>
          {comments
            .filter((c) => !c.parent && c.anchor.code)
            .map((root) => (
              <Thread
                key={root.id}
                root={root}
                replies={comments.filter((c) => c.parent === root.id)}
                actions={actions}
                highlighted={root.id === highlightComment}
              />
            ))}
        </section>
      )}
      {data.drift && (
        <details className="disclosure changes-section">
          <summary>
            Plan and code ·{" "}
            {data.drift.checked ? "heuristic drift check" : "drift not checked"}
            {drift && (
              <span className="muted">
                {" "}
                · {drift.outside.length} outside the plan
              </span>
            )}
            {data.drift.plan_changed && (
              <span className="muted"> · plan changed after approval</span>
            )}
          </summary>
          {!data.drift.checked && (
            <p>
              The plan has no parseable affected-files table. Add one to check
              changed files against the plan.
            </p>
          )}
          {data.drift.ticks_without_diff && (
            <p>
              Tasks are checked but this diff is empty. Verify the work or its
              base.
            </p>
          )}
          {data.drift.plan_changed && (
            <p>
              The plan changed after approval. Review its revision comparison.
            </p>
          )}
          {drift && (
            <table className="drift-table">
              <tbody>
                {(
                  [
                    ["Outside the plan", drift.outside],
                    ["Planned and touched", drift.touched],
                    ["Planned, not touched", drift.untouched],
                  ] as const
                ).map(([label, paths]) => (
                  <tr key={label}>
                    <th scope="row">{label}</th>
                    <td className="num">{paths.length}</td>
                    <td>
                      {paths.length > 0 && (
                        <details>
                          <summary>
                            {paths.length === 1 ? "Show file" : "Show files"}
                          </summary>
                          <ul className="path-list">
                            {paths.map((path) => (
                              <li key={path}>{path}</li>
                            ))}
                          </ul>
                        </details>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <p className="muted small">
            These signals help find drift. They do not prove a task is complete.
          </p>
        </details>
      )}
      {data.milestones && data.milestones.length > 0 && (
        <details className="disclosure changes-section">
          <summary>Planned milestones and completed tasks</summary>
          {data.milestones.map((m) => (
            <section key={m.key} className="milestone-tasks">
              <p className="milestone-name">
                {m.key} <span className="muted">· {m.status}</span>
              </p>
              {m.completed_tasks.length > 0 && (
                <ul>
                  {m.completed_tasks.map((task) => (
                    <li key={task}>{task}</li>
                  ))}
                </ul>
              )}
            </section>
          ))}
        </details>
      )}
      <section className="changes-files" aria-label="Changed files">
        <div className="changes-files-head">
          <h2>
            Files <span className="num muted">{data.stat.length}</span>
          </h2>
          {data.stat.length > 0 && (
            <label className="file-search">
              <MagnifyingGlassIcon size={15} aria-hidden="true" />
              <input
                aria-label="Filter changed files"
                placeholder="Filter files…"
                value={fileQuery}
                onChange={(event) => setFileQuery(event.target.value)}
              />
            </label>
          )}
        </div>
        {!data.stat.length && (
          <p className="muted">No changes yet. They appear here as your agent works.</p>
        )}
        {fileQuery && !files.length && (
          <div className="file-list-empty">
            <p>No matching files.</p>
            <button className="link" type="button" onClick={() => setFileQuery("")}>
              Clear file filter
            </button>
          </div>
        )}
        {files.length > 0 && (
          <table className="stat">
            <thead>
              <tr>
                <th scope="col">Path</th>
                <th scope="col">Added</th>
                <th scope="col">Removed</th>
              </tr>
            </thead>
            <tbody>
              {files.map((file) => {
                const isOpen = open.has(file.path);
                const state = diffs[file.path];
                return (
                  <Fragment key={file.path}>
                    <tr className={cn("file", isOpen && "open")}>
                      <td>
                        <button
                          type="button"
                          className="file-toggle"
                          aria-expanded={isOpen}
                          onClick={() => toggle(file.path)}
                          disabled={loading}
                        >
                          <span className="sr-only">{isOpen ? "Collapse " : "Expand "}</span>
                          <CaretRightIcon className="chev" size={13} aria-hidden="true" />
                          <span className="file-path">{file.path}</span>
                        </button>
                      </td>
                      <td className="added">+{file.added}</td>
                      <td className="deleted">−{file.deleted}</td>
                    </tr>
                    {isOpen && (
                      <tr className="diff">
                        <td colSpan={3}>
                          {!state || state.kind === "loading" ? (
                            <div className="diff-loading" role="status">
                              Loading diff…
                              <div />
                              <div />
                              <div />
                            </div>
                          ) : state.kind === "error" ? (
                            <p className="error file-diff" role="alert">
                              {state.message}
                            </p>
                          ) : (
                            <DiffBlock
                              diff={state.diff}
                              review={
                                actions && onAdded
                                  ? { sessionKey, comments, actions, onAdded }
                                  : undefined
                              }
                            />
                          )}
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        )}
      </section>
    </div>
  );
}

export function changeTotals(data: ChangesResponse) {
  return data.stat.reduce(
    (a, f) => ({ added: a.added + f.added, deleted: a.deleted + f.deleted }),
    { added: 0, deleted: 0 },
  );
}

function lineClass(l: string): string {
  if (
    l.startsWith("+++") ||
    l.startsWith("---") ||
    l.startsWith("diff ") ||
    l.startsWith("index ") ||
    l.startsWith("new file") ||
    l.startsWith("deleted file")
  )
    return "dl meta";
  if (l.startsWith("@@")) return "dl hunk";
  if (l.startsWith("+")) return "dl add";
  if (l.startsWith("-")) return "dl del";
  return "dl";
}

export function DiffBlock({
  diff,
  review,
}: {
  diff: FileDiff;
  review?: {
    sessionKey: string;
    comments: Comment[];
    actions: ThreadActions;
    onAdded: () => void;
  };
}) {
  const lines = diff.diff.replace(/\n$/, "").split("\n");
  if (diff.diff.trim() === "")
    return (
      <div className="muted small file-diff">
        No textual diff (binary or unchanged).
      </div>
    );
  let newLine = 0;
  return (
    <div className="file-diff">
      {lines.map((l, i) => {
        const hunk = l.match(/^@@ -\d+(?:,\d+)? \+(\d+)/);
        if (hunk) newLine = Number(hunk[1]);
        const commentable =
          !hunk &&
          (l.startsWith(" ") || (l.startsWith("+") && !l.startsWith("+++")));
        const line = commentable ? newLine++ : null;
        return (
          <div className="diff-line" key={i}>
            <span className="diff-line-number" aria-hidden="true">
              {line ?? ""}
            </span>
            <span className={lineClass(l)} style={{ whiteSpace: "pre" }}>
              {l || " "}
            </span>
            {review && line !== null && line > 0 && (
              <CodeReview
                {...review}
                path={diff.path}
                line={line}
                context={l.slice(1)}
              />
            )}
          </div>
        );
      })}
      {diff.truncated && <span className="dl meta">… diff truncated</span>}
    </div>
  );
}
