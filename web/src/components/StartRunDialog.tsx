import { useEffect, useState } from "react";
import { api } from "../api";
import { PERMISSION_MODES, type ImplementationMode, type PermissionMode, type Run } from "../types";
import { relTime } from "../lib/format";

export type RunStage = "research" | "plan" | "implement" | "critique";
import { CopyButton } from "./CopyButton";

interface ProviderInfo {
  id: string;
  available: boolean;
  version?: string;
  error?: string;
  models: { id: string; label: string }[];
}

export function StartRunDialog({ stage: initialStage, sessionKey, runs, resumeId, initialMode, onClose, onStarted }: { stage: RunStage; sessionKey: string; runs: Run[]; resumeId?: string; initialMode?: ImplementationMode; onClose: () => void; onStarted: (id: string) => void }) {
  const resumeTarget = resumeId ? runs.find((r) => r.id === resumeId) : undefined;
  const [stage, setStage] = useState<RunStage>(initialStage);
  const [providers, setProviders] = useState<ProviderInfo[] | null>(null);
  const [provider, setProvider] = useState<string>(() => {
    if (resumeTarget) return resumeTarget.provider;
    try {
      return localStorage.getItem("plantool.provider") || "claude";
    } catch {
      return "claude";
    }
  });
  const resumable = [...runs].filter((r) => r.provider === provider && r.provider_session_id && !(r.status === "starting" || r.status === "running" || r.status === "waiting" || r.status === "idle")).sort((a, b) => b.started_at.localeCompare(a.started_at));
  const [resume, setResume] = useState<string>(resumeId ?? "");
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
  const [implementationMode, setImplementationMode] = useState<ImplementationMode>(resumeTarget?.implementation_mode ?? initialMode ?? "all-at-once");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api.providers().then((r) => setProviders(r.providers)).catch((e: Error) => setErr(e.message));
  }, []);
  useEffect(() => {
    const t = window.setTimeout(() => {
      const mode = resume ? runs.find((r) => r.id === resume)?.implementation_mode ?? implementationMode : implementationMode;
      api.prompt(sessionKey, resume && mode !== "step-by-step" ? "resume" : stage, prompt || undefined, stage === "implement" ? mode : undefined, resume || undefined).then((r) => setPreview(r.prompt)).catch(() => setPreview(null));
    }, 250);
    return () => window.clearTimeout(t);
  }, [sessionKey, stage, prompt, resume, implementationMode, runs]);
  useEffect(() => {
    if (resume && !resumable.some((r) => r.id === resume)) setResume("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [provider]);
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
      const r = await api.startRun(sessionKey, { provider, stage, model: model || undefined, prompt: prompt || undefined, permission_mode: permission, worktree: stage === "implement" ? worktree : undefined, resume_run: resume || undefined, implementation_mode: stage === "implement" ? implementationMode : undefined });
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
        <h3>{resume ? "Resume run" : `Start ${stage} run`}</h3>
        <p className="muted small">The agent runs in the session's checkout and writes to the session folder. You can also run it in your own terminal: ask your agent to run <code>plantool skill</code>.</p>
        <label>
          Stage
          <select value={stage} onChange={(e) => setStage(e.target.value as RunStage)}>
            <option value="research">research</option>
            <option value="plan">plan</option>
            <option value="implement">implement (needs an approved plan)</option>
            <option value="critique">critique: review the document and post findings as comments</option>
          </select>
        </label>
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
        {resumable.length > 0 && (
          <label>
            Continue from
            <select value={resume} onChange={(e) => {
              const id = e.target.value;
              setResume(id);
              const previous = runs.find((r) => r.id === id);
              if (previous?.implementation_mode) setImplementationMode(previous.implementation_mode);
            }}>
              <option value="">a fresh session (full stage prompt)</option>
              {resumable.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.provider} · {r.stage} · {relTime(r.started_at)} · {r.status}
                </option>
              ))}
            </select>
            <span className="muted small">Resuming keeps everything that run already read and wrote; the agent is told to pick up where it left off and act on new comments.</span>
          </label>
        )}
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
          <label>
            Implementation pace
            <select value={implementationMode} disabled={!!resume} onChange={(e) => setImplementationMode(e.target.value as ImplementationMode)}>
              <option value="all-at-once">All at once</option>
              <option value="step-by-step">Step by step</option>
            </select>
            <span className="muted small">{implementationMode === "step-by-step" ? "One plan ticket per turn. Review and commit each milestone before continuing." : "Work through the full plan, then review the whole change."}</span>
          </label>
        )}
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
