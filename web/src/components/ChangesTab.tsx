import { useEffect, useState } from "react";
import { api } from "../api";
import type { ChangesResponse } from "../types";

export function ChangesTab({ sessionKey, nonce }: { sessionKey: string; nonce: number }) {
  const [data, setData] = useState<ChangesResponse | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    setErr(null);
    api.changes(sessionKey).then(setData).catch((e: Error) => setErr(e.message));
  }, [sessionKey, nonce]);
  const open = async () => {
    setBusy(true);
    setErr(null);
    try {
      const r = await api.openChanges(sessionKey);
      setData(r);
      if (r.review?.tool === "difftool") window.open(r.review.url, "_blank");
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  if (err && !data) return <div className="doc-empty"><p className="error">{err}</p></div>;
  if (!data) return <div className="doc-empty muted">Loading…</div>;
  const total = data.stat.reduce((a, f) => ({ added: a.added + f.added, deleted: a.deleted + f.deleted }), { added: 0, deleted: 0 });
  return (
    <div className="changes">
      <div className="changes-head">
        <div>
          <strong>{data.stat.length}</strong> file{data.stat.length === 1 ? "" : "s"} changed against <code>{data.base}</code>
          <span className="added"> +{total.added}</span>
          <span className="deleted"> −{total.deleted}</span>
        </div>
        <span className="spacer" />
        <span className="muted small">
          {data.tool === "difftool" ? "difftool found" : data.tool === "git-difftool" ? "using git difftool" : "no diff tool found"}
        </span>
        <button className="btn primary" onClick={() => void open()} disabled={busy} type="button">
          {data.tool === "difftool" ? "Review in difftool" : "Open in git difftool"}
        </button>
      </div>
      {data.review?.tool === "difftool" && (
        <p className="banner">
          difftool review: <a href={data.review.url} target="_blank" rel="noreferrer">{data.review.url}</a>
        </p>
      )}
      {data.review?.tool === "git-difftool" && <p className="banner">git difftool was launched on this machine.</p>}
      {err && <p className="error">{err}</p>}
      {data.message && <p className="muted">{data.message}</p>}
      <table className="stat">
        <tbody>
          {data.stat.map((f) => (
            <tr key={f.path}>
              <td className="added">+{f.added}</td>
              <td className="deleted">−{f.deleted}</td>
              <td className="path">{f.path}</td>
            </tr>
          ))}
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
