import { revisionDiff } from "../lib/revisionDiff";
import { useEffect, useState } from "react";
import { api } from "../api";
import type { DocResponse } from "../types";

export function PlanRevisions({ sessionKey, sha, content }: { sessionKey: string; sha: string; content: string }) {
  const [revisions, setRevisions] = useState<{ approved: DocResponse | null; viewed: DocResponse | null; change: string | null; pending: boolean } | null>(null);
  const [compare, setCompare] = useState<"approved" | "viewed" | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    void api.planRevisions(sessionKey).then((r) => { if (!cancelled) setRevisions(r); }).catch((e: Error) => { if (!cancelled) setError(e.message); });
    return () => { cancelled = true; };
  }, [sessionKey, sha]);
  const compares = (["approved", "viewed"] as const).filter((key) => revisions?.[key]);
  return <section className="plan-revisions" aria-label="Plan revisions">
    {error && <p role="alert">{error}</p>}
    {revisions?.change === "small-edit" && <p className="revision-note">Small edits since approval. Task scope is unchanged.</p>}
    <div className="revision-actions">
      {compares.map((key) => <button className="link" type="button" key={key} aria-pressed={compare === key} onClick={() => setCompare(compare === key ? null : key)}>Compare with {key === "viewed" ? "last viewed" : "approved"} revision</button>)}
      <button className="link" type="button" onClick={() => void api.markViewed(sessionKey, "plan", sha).then(() => setRevisions((r) => r ? { ...r, viewed: null } : r)).catch((e: Error) => setError(e.message))}>Mark this revision reviewed</button>
    </div>
    {compare && revisions?.[compare] && <div className="revision-comparison"><h4>{compare === "viewed" ? "Last viewed" : "Approved"} plan</h4><p className="muted">− removed · + added · unmarked lines are unchanged</p><pre>{revisionDiff(revisions[compare]?.content ?? "", content)}</pre></div>}
  </section>;
}
