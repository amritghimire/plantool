import { revisionDiff } from "../lib/revisionDiff";
import { useEffect, useState } from "react";
import { api } from "../api";
import type { DocResponse } from "../types";

export function PlanRevisions({ sessionKey, sha, content, onAccept }: { sessionKey: string; sha: string; content: string; onAccept: () => void }) {
  const [revisions, setRevisions] = useState<{ approved: DocResponse | null; viewed: DocResponse | null; change: string | null; pending: boolean } | null>(null);
  const [compare, setCompare] = useState<"approved" | "viewed" | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    void api.planRevisions(sessionKey).then((r) => { if (!cancelled) setRevisions(r); }).catch((e: Error) => { if (!cancelled) setError(e.message); });
    return () => { cancelled = true; };
  }, [sessionKey, sha]);
  return <section className="plan-revisions" aria-label="Plan revisions">
    {error && <p role="alert">{error}</p>}
    {revisions?.pending && <p className="banner">Plan scope changed. Review the revision before starting another milestone. <button className="btn" onClick={onAccept} type="button">Accept plan revision</button></p>}
    {revisions?.change === "small-edit" && <p className="banner">The plan has small edits since approval. Task scope is unchanged.</p>}
    {(["approved", "viewed"] as const).map((key) => revisions?.[key] && <button className="btn ghost small" type="button" key={key} onClick={() => setCompare(compare === key ? null : key)}>Compare with {key === "viewed" ? "last viewed" : "approved"} revision</button>)}
    <button className="btn ghost small" type="button" onClick={() => void api.markViewed(sessionKey, "plan", sha).then(() => setRevisions((r) => r ? { ...r, viewed: null } : r)).catch((e: Error) => setError(e.message))}>Mark this revision reviewed</button>
    {compare && revisions?.[compare] && <div className="revision-comparison"><h4>{compare === "viewed" ? "Last viewed" : "Approved"} plan</h4><p className="muted">− removed · + added · unmarked lines are unchanged</p><pre>{revisionDiff(revisions[compare]?.content ?? "", content)}</pre></div>}
  </section>;
}
