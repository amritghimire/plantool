import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { providerLabel, savedAgentSetting, saveAgentSettings } from "../lib/agents";
import { PERMISSION_MODES, type PermissionMode, type Run } from "../types";
import { AttachmentComposer, useAttachmentDraft } from "./AttachmentComposer";
import { ProviderSetup } from "./ProviderSetup";

type ProviderInfo = { id: string; available: boolean; error?: string; models: { id: string; label: string }[] };
const live = (run: Run) => ["starting", "running", "waiting", "idle"].includes(run.status);

export function AskAgentDialog({ sessionKey, runs, onClose, onStarted }: { sessionKey: string; runs: Run[]; onClose: () => void; onStarted: (id: string) => void }) {
  const active = runs.filter(live);
  const stopped = runs.filter((run) => !live(run) && run.provider_session_id);
  const [action, setAction] = useState<"live" | "resume" | "fresh">(active.length ? "live" : "fresh");
  const [target, setTarget] = useState(active[0]?.id ?? "");
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [provider, setProvider] = useState(() => { try { return localStorage.getItem("plantool.provider") || "claude"; } catch { return "claude"; } });
  const [model, setModel] = useState(() => savedAgentSetting(provider, "model"));
  const [effort, setEffort] = useState(() => savedAgentSetting(provider, "effort"));
  const [permission, setPermission] = useState<PermissionMode>(() => { try { return (localStorage.getItem("plantool.permission") as PermissionMode) || "ask"; } catch { return "ask"; } });
  const composer = useAttachmentDraft(sessionKey, `ask:${action}:${action === "fresh" ? provider : target}`);
  const busy = composer.draft.sending;
  const [error, setError] = useState<string | null>(null);
  const destination = `${sessionKey}:${action}:${target}:${provider}`;
  const currentDestination = useRef(destination);
  currentDestination.current = destination;
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => { if (event.key === "Escape" && !busy) onClose(); };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [busy, onClose]);
  useEffect(() => { api.providers().then((result) => setProviders(result.providers)).catch((e: Error) => setError(e.message)); }, []);
  const chosenRun = runs.find((run) => run.id === target);
  const chosenProvider = providers.find((item) => item.id === provider);
  useEffect(() => { if (provider === "ollama" && chosenProvider && !chosenProvider.models.some((item) => item.id === model)) setModel(chosenProvider.models[0]?.id ?? ""); }, [provider, chosenProvider, model]);
  const effectiveProvider = action === "resume" ? chosenRun?.provider ?? provider : provider;
  const submit = async () => {
    const submittedDestination = destination;
    let started: string | undefined;
    const complete = await composer.send(async (request) => {
      if (action === "live") {
        if (!chosenRun || !live(chosenRun)) throw new Error("Choose a live run.");
        await api.runInput(sessionKey, chosenRun.id, { text: request });
        started = chosenRun.id;
      } else {
        if (action === "resume" && (!chosenRun || live(chosenRun))) throw new Error("Choose a stopped run.");
        if (action === "fresh" && chosenProvider && !chosenProvider.available) throw new Error(chosenProvider.error || `${provider} is unavailable`);
        const result = await api.startRun(sessionKey, {
          provider: effectiveProvider, stage: "assist", prompt: request,
          model: action === "resume" ? chosenRun?.model || undefined : model || undefined,
          effort: action === "fresh" ? effort || undefined : undefined,
          resume_run: action === "resume" ? target : undefined,
          permission_mode: action === "resume" ? chosenRun?.permission_mode ?? permission : permission,
        });
        if (action === "fresh") { try { localStorage.setItem("plantool.provider", provider); localStorage.setItem("plantool.permission", permission); } catch { /* storage is optional */ } saveAgentSettings(provider, model, effort); }
        started = result.run.id;
      }
    });
    if (complete && started && currentDestination.current === submittedDestination) { onStarted(started); onClose(); }
  };
  return <div className="modal-backdrop" onClick={() => { if (!busy) onClose(); }}><div className="modal form-stack ask-dialog" role="dialog" aria-modal="true" aria-label="Ask agent" onClick={(event) => event.stopPropagation()}>
    <h3>Ask agent</h3>
    <p className="muted small">The agent can use this session's diff, comments, and documents.</p>
    <AttachmentComposer composer={composer} autoFocus rows={5} label="Your request" placeholder="Ask about this change…" onSend={() => void submit()} />
    <fieldset className="ask-destination"><legend>Send to</legend>
      {active.length > 0 && <label><input type="radio" checked={action === "live"} onChange={() => { setAction("live"); setTarget(active[0].id); }} /> Live run</label>}
      {stopped.length > 0 && <label><input type="radio" checked={action === "resume"} onChange={() => { setAction("resume"); setTarget(stopped[0].id); }} /> Resume a run</label>}
      <label><input type="radio" checked={action === "fresh"} onChange={() => { setAction("fresh"); setTarget(""); }} /> New run</label>
    </fieldset>
    {action !== "fresh" && <select aria-label="Run" value={target} onChange={(event) => setTarget(event.target.value)}>{(action === "live" ? active : stopped).map((run) => <option key={run.id} value={run.id}>{run.provider} · {run.task ?? run.stage} · {run.status}</option>)}</select>}
    {action === "fresh" && <div className="ask-settings"><label>Agent<select value={provider} onChange={(event) => { setProvider(event.target.value); setModel(savedAgentSetting(event.target.value, "model")); setEffort(savedAgentSetting(event.target.value, "effort")); }}>{(providers.length ? providers : [{ id: "claude", available: true, models: [] }, { id: "codex", available: true, models: [] }, { id: "copilot", available: true, models: [] }, { id: "ollama", available: true, models: [] }]).map((item) => <option key={item.id} value={item.id} disabled={!item.available}>{providerLabel(item.id)}{item.available ? "" : " (unavailable)"}</option>)}</select></label><label>Model{chosenProvider?.models.length ? <select value={model} onChange={(event) => setModel(event.target.value)}>{provider !== "ollama" && <option value="">Default</option>}{chosenProvider.models.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}</select> : <input value={model} placeholder={provider === "ollama" ? "Choose an installed model" : "Default model"} disabled={provider === "ollama"} onChange={(event) => setModel(event.target.value)} />}</label></div>}
    {action === "fresh" && providers.length > 0 && <ProviderSetup providers={providers} />}
    {action === "fresh" && (provider === "claude" || provider === "codex") && <fieldset className="effort-picker"><legend>Reasoning effort</legend><div className="seg effort-options">{["", "low", "medium", "high", provider === "claude" ? "max" : "xhigh"].map((value) => <button key={value} type="button" className={effort === value ? "active" : ""} onClick={() => setEffort(value)}>{value ? value === "xhigh" ? "X-high" : value[0].toUpperCase() + value.slice(1) : "Auto"}</button>)}</div></fieldset>}
    {action === "fresh" && <label>Permissions<select value={permission} onChange={(event) => setPermission(event.target.value as PermissionMode)}>{PERMISSION_MODES.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}</select><span className="muted small">{["copilot", "ollama"].includes(provider) && permission === "ask" ? "This mode can read files. Choose Auto to let it edit files or run commands." : PERMISSION_MODES.find((item) => item.id === permission)?.hint}</span></label>}
    {action === "fresh" && chosenProvider?.error && <p className="error">{chosenProvider.error}</p>}
    {error && <p className="error" role="alert">{error}</p>}
    <div className="composer-actions button-row dialog-actions"><span className="muted small">⌘↩ to send</span><span className="spacer" /><button className="btn ghost" type="button" onClick={onClose}>Cancel</button><button className="btn primary" type="button" disabled={!composer.canSend || (action === "fresh" && (chosenProvider?.available === false || (provider === "ollama" && !model)))} onClick={() => void submit()}>{busy ? "Sending…" : action === "live" ? "Send" : action === "resume" ? "Resume and send" : "Start and send"}</button></div>
  </div></div>;
}
