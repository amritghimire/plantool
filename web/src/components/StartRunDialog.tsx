import { useEffect, useState } from "react";
import { api } from "../api";
import { PERMISSION_MODES, type ImplementationMode, type PermissionMode, type Run } from "../types";
import { relTime } from "../lib/format";
import { providerLabel, savedAgentSetting, saveAgentSettings } from "../lib/agents";
import { ProviderSetup } from "./ProviderSetup";

export type RunStage = "research" | "plan" | "implement" | "critique" | "assist" | "draft-pr";
const STAGE_TEXT: Record<RunStage, { label: string; hint: string }> = {
  research: { label: "Research", hint: "The agent reads the repository and writes research.md for review." },
  plan: { label: "Plan", hint: "The agent turns the brief and research into plan.md for your approval." },
  implement: { label: "Implement", hint: "The agent follows the approved plan and updates the code." },
  critique: { label: "Review document", hint: "The agent reads the current document and leaves comments for you to review." },
  assist: { label: "Ask for help", hint: "The agent answers your request without moving the session to another stage." },
  "draft-pr": { label: "Draft PR", hint: "The agent prepares PR text from the implementation." },
};
import { CopyButton } from "./CopyButton";

interface ProviderInfo {
  id: string;
  available: boolean;
  version?: string;
  error?: string;
  models: { id: string; label: string }[];
}

export function StartRunDialog({ stage: initialStage, sessionKey, runs, resumeId, initialMode, initialPrompt = "", onClose, onStarted }: { stage: RunStage; sessionKey: string; runs: Run[]; resumeId?: string; initialMode?: ImplementationMode; initialPrompt?: string; onClose: () => void; onStarted: (id: string) => void }) {
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
  const [model, setModel] = useState(() => resumeTarget?.model || savedAgentSetting(provider, "model"));
  const [effort, setEffort] = useState(() => savedAgentSetting(provider, "effort"));
  const [prompt, setPrompt] = useState(initialPrompt);
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
  const [handoff, setHandoff] = useState<"stop" | "keep" | null>(null);
  const [liveRuns, setLiveRuns] = useState<Run[]>(runs.filter(isLive));
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => { if (event.key === "Escape" && !busy) onClose(); };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [busy, onClose]);
  useEffect(() => {
    api.providers().then((r) => setProviders(r.providers)).catch((e: Error) => setErr(e.message));
  }, []);
  useEffect(() => {
    const t = window.setTimeout(() => {
      const mode = resume ? runs.find((r) => r.id === resume)?.implementation_mode ?? implementationMode : implementationMode;
      api.prompt(sessionKey, resume && mode !== "step-by-step" && stage !== "assist" && stage !== "draft-pr" ? "resume" : stage, prompt || undefined, stage === "implement" ? mode : undefined, resume || undefined).then((r) => setPreview(r.prompt)).catch(() => setPreview(null));
    }, 250);
    return () => window.clearTimeout(t);
  }, [sessionKey, stage, prompt, resume, implementationMode, runs]);
  useEffect(() => {
    if (resume && !resumable.some((r) => r.id === resume)) setResume("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [provider]);
  const [milestones, setMilestones] = useState<{ key: string; status: string }[]>([]);
  const [milestone, setMilestone] = useState("");
  const [customPause, setCustomPause] = useState<string | null>(null);
  useEffect(() => { if (stage !== "implement") return; void api.session(sessionKey).then((v) => { setMilestones(v.state?.milestones ?? []); if (v.session.pause_rule?.mode === "plain-language") setCustomPause(v.session.pause_rule.rule ?? ""); }).catch(() => {}); }, [sessionKey, stage]);
  const current = providers?.find((p) => p.id === provider);
  useEffect(() => { if (provider === "ollama" && current && !current.models.some((item) => item.id === model)) setModel(current.models[0]?.id ?? ""); }, [provider, current, model]);
  const start = async () => {
    setBusy(true);
    setErr(null);
    try {
      localStorage.setItem("plantool.provider", provider);
      localStorage.setItem("plantool.permission", permission);
      saveAgentSettings(provider, model, effort);
    } catch {
      // ignore
    }
    try {
      const latest = await api.session(sessionKey);
      const active = latest.runs.filter(isLive);
      setLiveRuns(active);
      if (active.length && !handoff) {
        setErr("Choose what to do with the live run before starting another.");
        return;
      }
      if (active.length && handoff === "stop") {
        for (const run of active) await api.stopRun(sessionKey, run.id);
        const deadline = Date.now() + 20_000;
        while (Date.now() < deadline) {
          const refreshed = await api.session(sessionKey);
          if (active.every((run) => refreshed.runs.some((item) => item.id === run.id && ["stopped", "failed"].includes(item.status)))) break;
          await new Promise((resolve) => window.setTimeout(resolve, 300));
        }
        const final = await api.session(sessionKey);
        if (!active.every((run) => final.runs.some((item) => item.id === run.id && ["stopped", "failed"].includes(item.status)))) throw new Error("The previous run has not ended. Nothing new was started.");
      }
      const r = await api.startRun(sessionKey, { milestone: milestone || undefined, pause_rule: stage === "implement" ? customPause !== null ? { mode: "plain-language", rule: customPause } : { mode: implementationMode === "step-by-step" ? "every-milestone" : "no-pauses" } : undefined, provider, stage, model: model || undefined, effort: effort || undefined, prompt: prompt || undefined, permission_mode: permission === "ask" && ["copilot", "ollama"].includes(provider) ? "accept-edits" : permission, worktree: stage === "implement" ? worktree : undefined, resume_run: resume || undefined, implementation_mode: stage === "implement" ? implementationMode : undefined });
      onStarted(r.run.id);
      onClose();
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="modal-backdrop" onClick={() => { if (!busy) onClose(); }}>
      <div className="modal form-stack" role="dialog" aria-modal="true" aria-label={resume ? "Resume run" : STAGE_TEXT[stage].label} onClick={(e) => e.stopPropagation()}>
        <h3>{resume ? "Resume run" : STAGE_TEXT[stage].label}</h3>
        <p className="muted small">{STAGE_TEXT[stage].hint}</p>
        <label>
          Stage
          <select value={stage} disabled={!!resumeTarget} onChange={(e) => setStage(e.target.value as RunStage)}>
            {(Object.entries(STAGE_TEXT) as [RunStage, { label: string; hint: string }][]).map(([id, value]) => <option key={id} value={id}>{value.label}</option>)}
          </select>
        </label>
        <label>
          Provider
          <select value={provider} disabled={!!resumeTarget} onChange={(e) => { setProvider(e.target.value); setModel(savedAgentSetting(e.target.value, "model")); setEffort(savedAgentSetting(e.target.value, "effort")); }}>
            {(providers ?? [{ id: "claude", available: true, models: [] }, { id: "codex", available: true, models: [] }, { id: "copilot", available: true, models: [] }, { id: "ollama", available: true, models: [] }, { id: "opencode", available: true, models: [] }]).map((p) => (
              <option key={p.id} value={p.id} disabled={!p.available}>
                {providerLabel(p.id)}
                {p.version ? ` ${p.version}` : ""}
                {!p.available ? " (not found)" : ""}
              </option>
            ))}
          </select>
        </label>
        {providers && <ProviderSetup providers={providers} onRecheck={() => void api.providers().then((r) => setProviders(r.providers)).catch((e: Error) => setErr(e.message))} />}
        {current?.error && <div className="error" role="alert">{current.error}</div>}
        {resumable.length > 0 && (
          <label>
            Continue from
            <select value={resume} disabled={!!resumeTarget} onChange={(e) => {
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
              {provider !== "ollama" && <option value="">default</option>}
              {current.models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.label}
                </option>
              ))}
            </select>
          ) : (
            <input value={model} placeholder={provider === "ollama" ? "No installed model found" : "default"} disabled={provider === "ollama"} onChange={(e) => setModel(e.target.value)} />
          )}
        </label>
        {(provider === "claude" || provider === "codex") && <fieldset className="effort-picker"><legend>Reasoning effort</legend><div className="seg effort-options">
          {(provider === "claude" ? ["", "low", "medium", "high", "max"] : ["", "low", "medium", "high", "xhigh"]).map((value) => <button key={value} type="button" className={effort === value ? "active" : ""} onClick={() => setEffort(value)}>{value === "" ? "Auto" : value === "xhigh" ? "X-high" : value[0].toUpperCase() + value.slice(1)}</button>)}
        </div></fieldset>}
        <label>
          Permissions
          <select value={permission === "ask" && ["copilot", "ollama"].includes(provider) ? "accept-edits" : permission} onChange={(e) => setPermission(e.target.value as PermissionMode)}>
            {PERMISSION_MODES.filter((m) => m.id !== "ask" || !["copilot", "ollama"].includes(provider)).map((m) => (
              <option key={m.id} value={m.id}>
                {m.label}
              </option>
            ))}
          </select>
          <span className="muted small">{["copilot", "ollama"].includes(provider) && permission === "ask" ? "This mode can read files. Choose Auto to let it edit files or run commands." : PERMISSION_MODES.find((m) => m.id === permission)?.hint}</span>
        </label>
        {provider === "ollama" && stage !== "assist" && permission !== "auto" && permission !== "allow-all" && <div className="banner small">Choose Auto or Allow all to run this stage. OpenCode needs command access to update the session.</div>}
        {stage === "implement" && customPause !== null && <label>Checkpoint rule<textarea value={customPause} onChange={(e) => setCustomPause(e.target.value)} /><button type="button" className="btn ghost" onClick={() => setCustomPause(null)}>Use implementation pace instead</button></label>}
        {stage === "implement" && milestones.length > 0 && !resume && <label>Milestone <select value={milestone} onChange={(e) => setMilestone(e.target.value)}><option value="">Next pending phase</option>{milestones.filter((m) => m.status === "pending").map((m) => <option key={m.key} value={m.key}>{m.key}</option>)}</select></label>}
        {stage === "implement" && (
          <label>
            Implementation pace
            <select value={implementationMode} disabled={!!resume} onChange={(e) => setImplementationMode(e.target.value as ImplementationMode)}>
              <option value="all-at-once">All at once</option>
              <option value="step-by-step">Step by step</option>
            </select>
            <span className="muted small">{implementationMode === "step-by-step" ? "One plan phase per run. Review and optionally commit each milestone before continuing." : "Work through the full plan, then review the whole change."}</span>
          </label>
        )}
        {stage === "implement" && (
          <label className="check check-row">
            <input type="checkbox" checked={worktree} onChange={(e) => setWorktree(e.target.checked)} />
            <span>
              Work in a git worktree <span className="muted small">(.worktrees/&lt;slug&gt; or git config plantool.worktreeDir, off the base branch; your checkout stays untouched)</span>
            </span>
          </label>
        )}
        <label>
          {stage === "assist" ? "Your request" : "Extra instructions"}
          <textarea rows={3} value={prompt} placeholder={stage === "assist" ? "What should the agent do?" : "Optional. Appended to the stage prompt."} onChange={(e) => setPrompt(e.target.value)} />
        </label>
        {liveRuns.length > 0 && <fieldset className="handoff-options"><legend>Another run is live: {liveRuns.map((run) => `${run.provider} ${run.task ?? run.stage}`).join(", ")}</legend>
          <label><input type="radio" name="handoff" checked={handoff === "stop"} onChange={() => setHandoff("stop")} /> Stop and start</label>
          <label><input type="radio" name="handoff" checked={handoff === "keep"} onChange={() => setHandoff("keep")} /> Keep running and start</label>
        </fieldset>}
        {preview && (
          <details className="prompt-preview disclosure">
            <summary>
              The prompt the agent gets <CopyButton text={preview} label="copy" className="link" />
            </summary>
            <pre className="prompt-text">{preview}</pre>
          </details>
        )}
        {err && <div className="error" role="alert">{err}</div>}
        <div className="composer-actions button-row dialog-actions">
          <span className="spacer" />
          <button className="btn ghost" onClick={onClose} type="button">
            Cancel
          </button>
          {providers && !providers.some((p) => p.available) && preview && <CopyButton text={preview} label="Copy prompt for your terminal agent" className="btn primary" />}
          <button className="btn primary" disabled={busy || (stage === "assist" && !prompt.trim()) || (liveRuns.length > 0 && !handoff) || (current ? !current.available : false) || (provider === "ollama" && (!model || (stage !== "assist" && permission !== "auto" && permission !== "allow-all")))} onClick={() => void start()} type="button">
            {busy ? "Starting…" : resume ? "Resume run" : `Start ${STAGE_TEXT[stage].label.toLowerCase()}`}
          </button>
        </div>
      </div>
    </div>
  );
}

function isLive(run: Run): boolean {
  return ["starting", "running", "waiting", "idle"].includes(run.status);
}
