import { useState } from "react";
import { api } from "../api";
import type { SessionView } from "../types";

export function WherePanel({ view, onUpdated }: { view: SessionView; onUpdated: (v: SessionView) => void }) {
  const [base, setBase] = useState(view.session.base);
  const [branch, setBranch] = useState(view.workspace_branch ?? "");
  const [workspace, setWorkspace] = useState(view.session.worktree ?? view.session.repo.root);
  const [tool, setTool] = useState(view.session.difftool ?? "auto");
  const [pauseMode, setPauseMode] = useState(view.session.pause_rule?.mode ?? "no-pauses");
  const [pauseText, setPauseText] = useState(view.session.pause_rule?.rule ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const apply = async () => {
    setBusy(true); setError(null);
    try {
      const result = await api.setContext(view.key, {
        base: base !== view.session.base ? base : undefined,
        branch: branch !== view.workspace_branch ? branch : undefined,
        workspace: workspace !== (view.session.worktree ?? view.session.repo.root) ? workspace : undefined,
        difftool: tool,
        pause_rule: { mode: pauseMode, rule: pauseMode === "plain-language" ? pauseText : undefined },
      });
      onUpdated(result.view);
    } catch (e) { setError((e as Error).message); } finally { setBusy(false); }
  };
  return <><details className="side-block disclosure context-form"><summary>Where · workspace, branch, base and diff tool</summary>
    <label>Workspace <input value={workspace} onChange={(e) => setWorkspace(e.target.value)} /></label>
    <label>Branch <input value={branch} onChange={(e) => setBranch(e.target.value)} /></label>
    <label>Base <input value={base} onChange={(e) => setBase(e.target.value)} /></label>
    <label>Diff tool <select value={tool} onChange={(e) => setTool(e.target.value)}>{["auto", "built-in", "difftool", "git-difftool"].map((t) => <option key={t} value={t}>{t}</option>)}</select></label>
    <label>Pause rule <select value={pauseMode} onChange={(e) => setPauseMode(e.target.value as typeof pauseMode)}><option value="no-pauses">No pauses</option><option value="every-milestone">Every milestone</option><option value="plain-language">When this rule applies</option></select></label>
    {pauseMode === "plain-language" && <label>Checkpoint rule <textarea value={pauseText} onChange={(e) => setPauseText(e.target.value)} placeholder="Pause before changing a public API or when an assumption fails." /></label>}
    <p>Diff window: {view.session.base} → {base}…working tree. A live agent will stop and resume in the selected workspace.</p>
    <button type="button" className="btn primary" disabled={busy || !base.trim() || !workspace.trim()} onClick={() => void apply()}>Apply context change</button>
    <button type="button" className="btn ghost" onClick={() => void api.session(view.key).then(onUpdated).catch((e: Error) => setError(e.message))}>Re-check</button>
  </details>
    {error && <p className="error" role="alert">{error}</p>}
    {(view.state.proposals ?? []).filter((p) => p.kind !== "plan-revision").map((p) => <div className="banner" key={p.id}><strong>Agent proposes {p.kind}</strong><p>{p.old} → {p.new}</p><p>{p.reason}</p>
      <button type="button" className="btn" onClick={() => void api.contextProposal(view.key, p.id, false).then((r) => onUpdated(r.view)).catch((e: Error) => setError(e.message))}>Apply proposal</button>
      <button type="button" className="btn ghost" onClick={() => void api.contextProposal(view.key, p.id, true).then((r) => onUpdated(r.view)).catch((e: Error) => setError(e.message))}>Dismiss</button>
    </div>)}
  </>;
}
