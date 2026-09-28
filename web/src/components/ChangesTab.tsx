import { useEffect, useState } from "react";
import { api } from "../api";
import type { ChangeScope, ChangesResponse, FileDiff } from "../types";

type DiffState = { kind: "loading" } | { kind: "error"; message: string } | { kind: "ready"; diff: FileDiff };

export function ChangesTab({ sessionKey, nonce }: { sessionKey: string; nonce: number }) {
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
      })
      .catch((e: Error) => setErr(e.message));
  }, [sessionKey, nonce, scope]);
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
  if (err && !data) return <div className="doc-empty"><p className="error">{err}</p></div>;
  if (!data) return <div className="doc-empty muted">Loading…</div>;
  const total = data.stat.reduce((a, f) => ({ added: a.added + f.added, deleted: a.deleted + f.deleted }), { added: 0, deleted: 0 });
  return (
    <div className="changes">
      <div className="changes-head">
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
          {data.tool === "difftool" ? "Review in difftool" : "Open in git difftool"}
        </button>
      </div>
      {data.step_available && (
        <div className="changes-scope">
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
      {err && <p className="error">{err}</p>}
      {data.message && <p className="muted">{data.message}</p>}
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
                      <div className="error small file-diff">{state.message}</div>
                    ) : (
                      <DiffBlock diff={state.diff} />
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

export function DiffBlock({ diff }: { diff: FileDiff }) {
  const lines = diff.diff.replace(/\n$/, "").split("\n");
  if (diff.diff.trim() === "") return <div className="muted small file-diff">No textual diff (binary or unchanged).</div>;
  return (
    <pre className="file-diff">
      {lines.map((l, i) => (
        <span key={i} className={lineClass(l)}>
          {l || " "}
        </span>
      ))}
      {diff.truncated && <span className="dl meta">… diff truncated</span>}
    </pre>
  );
}
