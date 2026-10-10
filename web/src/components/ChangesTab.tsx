import { CodeReview } from "./CodeReview";
import { Thread, type ThreadActions } from "./Thread";
import type { Comment } from "../types";
import { useEffect, useState } from "react";
import { api } from "../api";
import type { ChangeScope, ChangesResponse, FileDiff } from "../types";

type DiffState = { kind: "loading" } | { kind: "error"; message: string } | { kind: "ready"; diff: FileDiff };

export function ChangesTab({ sessionKey, nonce, comments = [], actions, onAdded, highlightComment }: { sessionKey: string; nonce: number; comments?: Comment[]; actions?: ThreadActions; onAdded?: () => void; highlightComment?: string | null }) {
  const [data, setData] = useState<ChangesResponse | null>(null);
  const [scope, setScope] = useState<ChangeScope | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [diffs, setDiffs] = useState<Record<string, DiffState>>({});
  useEffect(() => {
    setErr(null);
    api
      .changes(sessionKey, scope ?? undefined)
      .then((r) => {
        setData(r);
        setDiffs({});
        setOpen(new Set());
      })
      .catch((e: Error) => setErr(e.message));
  }, [sessionKey, nonce, scope]);
  useEffect(() => {
    if (data?.review?.tool !== "difftool") return;
    const timer = window.setInterval(() => { void api.changes(sessionKey, scope ?? undefined).then(setData).catch(() => {}); }, 5000);
    return () => window.clearInterval(timer);
  }, [sessionKey, scope, data?.review?.tool]);
  useEffect(() => {
    if (!highlightComment || !data) return;
    document.getElementById(`c-${highlightComment}`)?.scrollIntoView?.({ block: "center", behavior: "auto" });
  }, [highlightComment, data]);
  const openReview = async () => {
    setBusy(true);
    setErr(null);
    try {
      const r = await api.openChanges(sessionKey, scope ?? undefined);
      setData(r);
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const pickScope = (s: ChangeScope) => {
    if (s === (scope ?? data?.scope)) return;
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
    setDiffs((d) => ({ ...d, [path]: { kind: "loading" } }));
    api
      .changesFile(sessionKey, path, data?.scope)
      .then((diff) => setDiffs((d) => ({ ...d, [path]: { kind: "ready", diff } })))
      .catch((e: Error) => setDiffs((d) => ({ ...d, [path]: { kind: "error", message: e.message } })));
  };
  if (err && !data) return <div className="doc-empty"><p className="error" role="alert">{err}</p></div>;
  if (!data) return <div className="doc-empty muted">Loading…</div>;
  const total = data.stat.reduce((a, f) => ({ added: a.added + f.added, deleted: a.deleted + f.deleted }), { added: 0, deleted: 0 });
  return (
    <div className="changes">
      <div className="changes-head button-row">
        <div>
          <strong>{data.stat.length}</strong> file{data.stat.length === 1 ? "" : "s"} changed {data.label}
          <span className="added"> +{total.added}</span>
          <span className="deleted"> −{total.deleted}</span>
        </div>
        <span className="spacer" />
        <span className="muted small">
          {data.tool === "difftool" ? "difftool found" : data.tool === "git-difftool" ? "using git difftool" : "no diff tool found"}
        </span>
        <button className="btn primary" onClick={() => void openReview()} disabled={busy} type="button">
          {data.tool === "difftool" ? "Review in difftool" : data.tool === "none" ? "Use built-in diff" : "Open in git difftool"}
        </button>
      </div>
      {data.difftool_open_comments !== undefined && <p className="muted">Difftool comments (read only): {data.difftool_open_comments === null ? "not available" : `${data.difftool_open_comments} open`}. Resolve them in difftool.</p>}
      {data.step_available && (
        <div className="changes-scope button-row">
          <span className="muted small">show</span>
          <div className="seg" role="tablist" aria-label="Change scope">
            <button className={data.scope === "step" ? "active" : ""} onClick={() => pickScope("step")} type="button" role="tab" aria-selected={data.scope === "step"}>
              This milestone
            </button>
            <button className={data.scope === "all" ? "active" : ""} onClick={() => pickScope("all")} type="button" role="tab" aria-selected={data.scope === "all"}>
              Whole implementation
            </button>
          </div>
        </div>
      )}
      {data.review?.tool === "difftool" && (
        <p className="banner">
          difftool review: <a href={data.review.url} target="_blank" rel="noreferrer noopener">{data.review.url}</a>
          <span className="muted small"> · the daemon opens it; if the tab did not appear, use the link</span>
        </p>
      )}
      {data.review?.tool === "git-difftool" && <p className="banner">git difftool was launched on this machine.</p>}
      {err && <p className="error" role="alert">{err}</p>}
      {data.message && <p className="muted">{data.message}</p>}
      {actions && comments.some((c) => !c.parent && c.anchor.code) && <section aria-label="Code review comments"><h3>Code review comments</h3>{comments.filter((c) => !c.parent && c.anchor.code).map((root) => <Thread key={root.id} root={root} replies={comments.filter((c) => c.parent === root.id)} actions={actions} highlighted={root.id === highlightComment} />)}</section>}
      {data.drift && <details className="banner disclosure" open={!data.drift.checked || data.drift.outside_files.length > 0 || data.drift.ticks_without_diff || data.drift.plan_changed}>
        <summary>Plan and code · {data.drift.checked ? "heuristic drift check" : "drift not checked"}</summary>
        {!data.drift.checked && <p>The plan has no parseable affected-files table. Add one to check changed files against the plan.</p>}
        {data.drift.checked && data.drift.outside_files.length > 0 && <p>Outside planned files: {data.drift.outside_files.join(", ")}</p>}
        {data.drift.ticks_without_diff && <p>Tasks are checked but this diff is empty. Verify the work or its base.</p>}
        {data.drift.plan_changed && <p>The plan changed after approval. Review its revision comparison.</p>}
        <p>Planned files: {data.drift.planned_files.join(", ") || "Not listed"}</p>
        <p className="muted">These signals help find drift. They do not prove a task is complete.</p>
      </details>}
      {data.milestones && <details className="disclosure"><summary>Planned milestones and completed tasks</summary>{data.milestones.map((m) => <section key={m.key}><strong>{m.key} · {m.status}</strong><ul>{m.completed_tasks.map((task) => <li key={task}>{task}</li>)}</ul></section>)}</details>}
      <table className="stat">
        <tbody>
          {data.stat.map((f) => {
            const isOpen = open.has(f.path);
            const state = diffs[f.path];
            return [
              <tr key={f.path} className={`file ${isOpen ? "open" : ""}`}>
                <td className="added">+{f.added}</td>
                <td className="deleted">−{f.deleted}</td>
                <td className="path">
                  <button className="file-toggle" onClick={() => toggle(f.path)} type="button" aria-expanded={isOpen} aria-label={`${isOpen ? "Collapse" : "Expand"} ${f.path}`}>
                    <span className="chev" aria-hidden="true">›</span>
                    {f.path}
                  </button>
                </td>
              </tr>,
              isOpen ? (
                <tr key={`${f.path}#diff`} className="diff">
                  <td colSpan={3}>
                    {!state || state.kind === "loading" ? (
                      <div className="muted small file-diff">Loading diff…</div>
                    ) : state.kind === "error" ? (
                      <div className="error small file-diff" role="alert">{state.message}</div>
                    ) : (
                      <DiffBlock diff={state.diff} review={actions && onAdded ? { sessionKey, comments, actions, onAdded } : undefined} />
                    )}
                  </td>
                </tr>
              ) : null,
            ];
          })}
          {data.stat.length === 0 && (
            <tr>
              <td colSpan={3} className="muted">
                No changes yet.
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}

function lineClass(l: string): string {
  if (l.startsWith("+++") || l.startsWith("---") || l.startsWith("diff ") || l.startsWith("index ") || l.startsWith("new file") || l.startsWith("deleted file")) return "dl meta";
  if (l.startsWith("@@")) return "dl hunk";
  if (l.startsWith("+")) return "dl add";
  if (l.startsWith("-")) return "dl del";
  return "dl";
}

export function DiffBlock({ diff, review }: { diff: FileDiff; review?: { sessionKey: string; comments: Comment[]; actions: ThreadActions; onAdded: () => void } }) {
  const lines = diff.diff.replace(/\n$/, "").split("\n");
  if (diff.diff.trim() === "") return <div className="muted small file-diff">No textual diff (binary or unchanged).</div>;
  let newLine = 0;
  return (
    <div className="file-diff">
      {lines.map((l, i) => {
        const hunk = l.match(/^@@ -\d+(?:,\d+)? \+(\d+)/);
        if (hunk) newLine = Number(hunk[1]);
        const commentable = !hunk && (l.startsWith(" ") || (l.startsWith("+") && !l.startsWith("+++")));
        const line = commentable ? newLine++ : null;
        return <div key={i}><span className={lineClass(l)} style={{ whiteSpace: "pre" }}>{l || " "}</span>
          {review && line !== null && line > 0 && <CodeReview {...review} path={diff.path} line={line} context={l.slice(1)} />}
        </div>;
      })}
      {diff.truncated && <span className="dl meta">… diff truncated</span>}
    </div>
  );
}
