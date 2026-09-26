import { useEffect, useState } from "react";
import { api } from "../api";
import { PERMISSION_MODES, type PermissionMode } from "../types";
import { CopyButton } from "./CopyButton";

interface ProviderInfo {
  id: string;
  available: boolean;
  version?: string;
  error?: string;
  models: { id: string; label: string }[];
}

export function StartRunDialog({ stage, sessionKey, onClose, onStarted }: { stage: "research" | "plan" | "implement"; sessionKey: string; onClose: () => void; onStarted: (id: string) => void }) {
  const [providers, setProviders] = useState<ProviderInfo[] | null>(null);
  const [provider, setProvider] = useState<string>(() => {
    try {
      return localStorage.getItem("plantool.provider") || "claude";
    } catch {
      return "claude";
    }
  });
  const [model, setModel] = useState("");
  const [prompt, setPrompt] = useState("");
  const [permission, setPermission] = useState<PermissionMode>(() => {
    try {
      const v = localStorage.getItem("plantool.permission");
      return PERMISSION_MODES.some((m) => m.id === v) ? (v as PermissionMode) : "ask";
    } catch {
      return "ask";
    }
  });
  const [preview, setPreview] = useState<string | null>(null);
  const [worktree, setWorktree] = useState(true);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api.providers().then((r) => setProviders(r.providers)).catch((e: Error) => setErr(e.message));
  }, []);
  useEffect(() => {
    const t = window.setTimeout(() => {
      api.prompt(sessionKey, stage, prompt || undefined).then((r) => setPreview(r.prompt)).catch(() => setPreview(null));
    }, 250);
    return () => window.clearTimeout(t);
  }, [sessionKey, stage, prompt]);
  const current = providers?.find((p) => p.id === provider);
  const start = async () => {
    setBusy(true);
    setErr(null);
    try {
      localStorage.setItem("plantool.provider", provider);
      localStorage.setItem("plantool.permission", permission);
    } catch {
      // ignore
    }
    try {
      const r = await api.startRun(sessionKey, { provider, stage, model: model || undefined, prompt: prompt || undefined, permission_mode: permission, worktree: stage === "implement" ? worktree : undefined });
      onStarted(r.run.id);
      onClose();
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>Start {stage} run</h3>
        <p className="muted small">The agent runs in the session's checkout and writes to the session folder. You can also run it in your own terminal: ask your agent to run <code>plantool skill</code>.</p>
        <label>
          Provider
          <select value={provider} onChange={(e) => { setProvider(e.target.value); setModel(""); }}>
            {(providers ?? [{ id: "claude", available: true, models: [] }, { id: "codex", available: true, models: [] }, { id: "opencode", available: true, models: [] }]).map((p) => (
              <option key={p.id} value={p.id} disabled={!p.available}>
                {p.id}
                {p.version ? ` ${p.version}` : ""}
                {!p.available ? " (not found)" : ""}
              </option>
            ))}
          </select>
        </label>
        {current?.error && <div className="error">{current.error}</div>}
        <label>
          Model
          {current && current.models.length > 0 ? (
            <select value={model} onChange={(e) => setModel(e.target.value)}>
              <option value="">default</option>
              {current.models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.label}
                </option>
              ))}
            </select>
          ) : (
            <input value={model} placeholder="default" onChange={(e) => setModel(e.target.value)} />
          )}
        </label>
        <label>
          Permissions
          <select value={permission} onChange={(e) => setPermission(e.target.value as PermissionMode)}>
            {PERMISSION_MODES.map((m) => (
              <option key={m.id} value={m.id}>
                {m.label}
              </option>
            ))}
          </select>
          <span className="muted small">{PERMISSION_MODES.find((m) => m.id === permission)?.hint}</span>
        </label>
        {stage === "implement" && (
          <label className="check">
            <input type="checkbox" checked={worktree} onChange={(e) => setWorktree(e.target.checked)} />
            <span>
              Work in a git worktree <span className="muted small">(.claude/worktrees/&lt;slug&gt; off the base branch; your checkout stays untouched)</span>
            </span>
          </label>
        )}
        <label>
          Extra instructions
          <textarea rows={3} value={prompt} placeholder="Optional. Appended to the stage prompt." onChange={(e) => setPrompt(e.target.value)} />
        </label>
        {preview && (
          <details className="prompt-preview">
            <summary>
              The prompt the agent gets <CopyButton text={preview} label="copy" className="link" />
            </summary>
            <pre className="prompt-text">{preview}</pre>
          </details>
        )}
        {err && <div className="error">{err}</div>}
        <div className="composer-actions">
          <span className="spacer" />
          <button className="btn ghost" onClick={onClose} type="button">
            Cancel
          </button>
          <button className="btn primary" disabled={busy || (current ? !current.available : false)} onClick={() => void start()} type="button">
            Start
          </button>
        </div>
      </div>
    </div>
  );
}
