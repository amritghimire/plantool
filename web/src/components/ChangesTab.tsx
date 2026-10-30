import {
  Code,
  FileCode,
  MagnifyingGlass,
  ArrowUpRight,
  CaretRight,
} from "@phosphor-icons/react";
import { cn } from "../lib/cn";
import { CodeReview } from "./CodeReview";
import { Thread, type ThreadActions } from "./Thread";
import type { Comment } from "../types";
import { useEffect, useRef, useState } from "react";
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
  const total = data.stat.reduce(
    (a, f) => ({ added: a.added + f.added, deleted: a.deleted + f.deleted }),
    { added: 0, deleted: 0 },
  );
  return (
    <div className="changes" aria-busy={loading}>
      {loading && (
        <p className="small muted" role="status">
          Updating comparison…
        </p>
      )}
      <div className="changes-head button-row">
        <div>
          <strong>{data.stat.length}</strong> file
          {data.stat.length === 1 ? "" : "s"} changed {data.label}
          <span className="added"> +{total.added}</span>
          <span className="deleted"> −{total.deleted}</span>
        </div>
        <span className="spacer" />
        <span className="review-tool">
          {data.tool === "difftool"
            ? "difftool"
            : data.tool === "git-difftool"
              ? "git difftool"
              : "Built-in diff"}
        </span>
        <button
          className="btn primary"
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
          <ArrowUpRight size={15} aria-hidden="true" />
        </button>
      </div>
      <dl className="review-context" aria-label="Comparison context">
        {workspace && (
          <div>
            <dt>Workspace</dt>
            <dd title={workspace}>{workspace}</dd>
          </div>
        )}
        <div>
          <dt>Comparing</dt>
          <dd>
            <code title={data.base}>{data.base}</code>
            <span aria-hidden="true"> → </span>working tree
          </dd>
        </div>
        <div>
          <dt>Scope</dt>
          <dd>
            {data.scope === "step"
              ? "Current milestone"
              : "Whole implementation"}
          </dd>
        </div>
      </dl>
      {data.review?.tool === "difftool" &&
        data.difftool_open_comments !== undefined && (
          <p className="muted">
            Difftool comments (read only):{" "}
            {data.difftool_open_comments === null
              ? "not available"
              : `${data.difftool_open_comments} open`}
            . Resolve them in difftool.
          </p>
        )}
      {data.step_available && (
        <div className="changes-scope button-row">
          <span className="muted small">show</span>
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
        </div>
      )}
      {data.review?.tool === "difftool" && (
        <p className="banner">
          <a href={data.review.url} target="_blank" rel="noreferrer noopener">
            Open this review in difftool ↗
          </a>
          <span className="muted small"> · includes new files</span>
        </p>
      )}
      {data.review?.tool === "git-difftool" && (
        <p className="banner">git difftool was launched on this machine.</p>
      )}
      {err && (
        <p className="error" role="alert">
          {err}
        </p>
      )}
      {data.message && <p className="muted">{data.message}</p>}
      {actions && comments.some((c) => !c.parent && c.anchor.code) && (
        <section aria-label="Code review comments">
          <h3>Code review comments</h3>
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
        <details
          className="banner disclosure"
          open={
            !data.drift.checked ||
            data.drift.outside_files.length > 0 ||
            data.drift.ticks_without_diff ||
            data.drift.plan_changed
          }
        >
          <summary>
            Plan and code ·{" "}
            {data.drift.checked ? "heuristic drift check" : "drift not checked"}
          </summary>
          {!data.drift.checked && (
            <p>
              The plan has no parseable affected-files table. Add one to check
              changed files against the plan.
            </p>
          )}
          {data.drift.checked && data.drift.outside_files.length > 0 && (
            <p>Outside planned files: {data.drift.outside_files.join(", ")}</p>
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
          <p>
            Planned files: {data.drift.planned_files.join(", ") || "Not listed"}
          </p>
          <p className="muted">
            These signals help find drift. They do not prove a task is complete.
          </p>
        </details>
      )}
      {data.milestones && (
        <details className="disclosure">
          <summary>Planned milestones and completed tasks</summary>
          {data.milestones.map((m) => (
            <section key={m.key}>
              <strong>
                {m.key} · {m.status}
              </strong>
              <ul>
                {m.completed_tasks.map((task) => (
                  <li key={task}>{task}</li>
                ))}
              </ul>
            </section>
          ))}
        </details>
      )}
      <div className="review-files-layout">
        <nav className="review-file-list" aria-label="Changed files">
          <div className="file-list-title">
            Files <span>{data.stat.length}</span>
          </div>
          <label className="file-search">
            <MagnifyingGlass size={15} aria-hidden="true" />
            <input
              aria-label="Filter changed files"
              placeholder="Filter files…"
              value={fileQuery}
              onChange={(event) => setFileQuery(event.target.value)}
            />
          </label>
          {data.stat
            .filter((file) =>
              file.path.toLowerCase().includes(fileQuery.toLowerCase()),
            )
            .map((file) => (
              <button
                key={file.path}
                type="button"
                className={cn("review-file", open.has(file.path) && "selected")}
                aria-expanded={open.has(file.path)}
                onClick={() => toggle(file.path)}
                disabled={loading}
              >
                <span className="sr-only">
                  {open.has(file.path) ? "Collapse " : "Expand "}
                </span>
                <FileCode size={16} aria-hidden="true" />
                <span>{file.path}</span>
                <small aria-hidden="true">
                  <span className="added">+{file.added}</span>
                  <span className="deleted">−{file.deleted}</span>
                </small>
              </button>
            ))}
          {fileQuery &&
            !data.stat.some((file) =>
              file.path.toLowerCase().includes(fileQuery.toLowerCase()),
            ) && (
              <div className="file-list-empty">
                <p>No matching files.</p>
                <button
                  className="link"
                  type="button"
                  onClick={() => setFileQuery("")}
                >
                  Clear file filter
                </button>
              </div>
            )}
        </nav>
        <section className="review-file-content" aria-label="File diffs">
          {!open.size && (
            <div className="diff-empty">
              <Code size={32} aria-hidden="true" />
              <h2>
                {data.stat.length
                  ? "Choose a file to review"
                  : "No changes yet"}
              </h2>
              <p>
                {data.stat.length
                  ? "Inspect each change and leave feedback directly on a line."
                  : "Changes will appear here as your agent works."}
              </p>
              {data.stat[0] && (
                <button
                  className="btn"
                  type="button"
                  onClick={() => toggle(data.stat[0].path)}
                  disabled={loading}
                >
                  Review first file
                  <CaretRight size={15} aria-hidden="true" />
                </button>
              )}
            </div>
          )}
          {data.stat
            .filter((file) => open.has(file.path))
            .map((file) => {
              const state = diffs[file.path];
              return (
                <article className="review-file-panel" key={file.path}>
                  <header>
                    <FileCode size={16} aria-hidden="true" />
                    <span>{file.path}</span>
                    <span className="spacer" />
                    <small className="added">+{file.added}</small>
                    <small className="deleted">−{file.deleted}</small>
                  </header>
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
                </article>
              );
            })}
        </section>
      </div>
    </div>
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
