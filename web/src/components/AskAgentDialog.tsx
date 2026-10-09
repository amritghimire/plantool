import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { providerLabel, savedAgentSetting, saveAgentSettings } from "../lib/agents";
import { PERMISSION_MODES, type PermissionMode, type Run } from "../types";
import { ProviderSetup } from "./ProviderSetup";

type ProviderInfo = { id: string; available: boolean; error?: string; models: { id: string; label: string }[] };
const live = (run: Run) => ["starting", "running", "waiting", "idle"].includes(run.status);

export function AskAgentDialog({ sessionKey, runs, onClose, onStarted }: { sessionKey: string; runs: Run[]; onClose: () => void; onStarted: (id: string) => void }) {
  const active = runs.filter(live);
  const stopped = runs.filter((run) => !live(run) && run.provider_session_id);
  const [action, setAction] = useState<"live" | "resume" | "fresh">(active.length ? "live" : "fresh");
  const [target, setTarget] = useState(active[0]?.id ?? "");
  const [text, setText] = useState("");
  const [files, setFiles] = useState<File[]>([]);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [provider, setProvider] = useState(() => { try { return localStorage.getItem("plantool.provider") || "claude"; } catch { return "claude"; } });
  const [model, setModel] = useState(() => savedAgentSetting(provider, "model"));
  const [effort, setEffort] = useState(() => savedAgentSetting(provider, "effort"));
  const [permission, setPermission] = useState<PermissionMode>(() => { try { return (localStorage.getItem("plantool.permission") as PermissionMode) || "ask"; } catch { return "ask"; } });
  const [busy, setBusy] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
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
    if ((!text.trim() && !files.length) || busy) return;
    setBusy(true);
    setError(null);
    try {
      if (files.some((file) => file.size === 0 || file.size > 10 * 1024 * 1024)) throw new Error("Each attachment must be between 1 byte and 10 MB.");
      const uploaded = await Promise.all(files.map((file) => api.uploadAttachment(sessionKey, file)));
      const request = [text.trim(), ...uploaded.map((file) => `Attached file: ${file.path}`)].filter(Boolean).join("\n\n");
      if (action === "live") {
        if (!chosenRun || !live(chosenRun)) throw new Error("Choose a live run.");
        await api.runInput(sessionKey, chosenRun.id, { text: request });
        onStarted(chosenRun.id);
      } else {
        if (action === "resume" && (!chosenRun || live(chosenRun))) throw new Error("Choose a stopped run.");
        if (action === "fresh" && chosenProvider && !chosenProvider.available) throw new Error(chosenProvider.error || `${provider} is unavailable`);
        const result = await api.startRun(sessionKey, {
          provider: effectiveProvider,
          stage: "assist",
          prompt: request,
          model: action === "resume" ? chosenRun?.model || undefined : model || undefined,
          effort: action === "fresh" ? effort || undefined : undefined,
          resume_run: action === "resume" ? target : undefined,
          permission_mode: action === "resume" ? chosenRun?.permission_mode ?? permission : permission,
        });
        if (action === "fresh") { try { localStorage.setItem("plantool.provider", provider); localStorage.setItem("plantool.permission", permission); } catch { /* storage is optional */ } saveAgentSettings(provider, model, effort); }
        onStarted(result.run.id);
      }
      onClose();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return <div className="modal-backdrop" onClick={() => { if (!busy) onClose(); }}><div className={`modal ask-dialog ${dragging ? "dragging" : ""}`} role="dialog" aria-modal="true" aria-label="Ask agent" onClick={(event) => event.stopPropagation()} onDragOver={(event) => { event.preventDefault(); setDragging(true); }} onDragLeave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node)) setDragging(false); }} onDrop={(event) => { event.preventDefault(); setDragging(false); setFiles((current) => [...current, ...Array.from(event.dataTransfer.files)]); }}>
    <h3>Ask agent</h3>
    <p className="muted small">The agent can use this session's diff, comments, and documents.</p>
    <textarea autoFocus rows={5} value={text} onChange={(event) => setText(event.target.value)} onKeyDown={(event) => { if ((event.metaKey || event.ctrlKey) && event.key === "Enter") void submit(); }} placeholder="Ask about this change…" aria-label="Your request" />
    {files.length > 0 && <div className="attachment-list">{files.map((file, index) => <span className="attachment-chip" key={`${file.name}-${index}`}>{file.name}<button type="button" aria-label={`Remove ${file.name}`} onClick={() => setFiles((current) => current.filter((_, i) => i !== index))}>×</button></span>)}</div>}
    <div className="ask-toolbar"><input ref={fileInput} type="file" multiple hidden onChange={(event) => { setFiles((current) => [...current, ...Array.from(event.target.files ?? [])]); event.target.value = ""; }} /><button className="btn ghost" type="button" onClick={() => fileInput.current?.click()}>＋ Attach files</button><span className="muted small">or drop files here · up to 10 MB each</span></div>
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
    <div className="composer-actions"><span className="muted small">⌘↩ to send</span><span className="spacer" /><button className="btn ghost" type="button" onClick={onClose}>Cancel</button><button className="btn primary" type="button" disabled={busy || (!text.trim() && !files.length) || (action === "fresh" && (chosenProvider?.available === false || (provider === "ollama" && !model)))} onClick={() => void submit()}>{busy ? "Sending…" : action === "live" ? "Send" : action === "resume" ? "Resume and send" : "Start and send"}</button></div>
  </div></div>;
}
