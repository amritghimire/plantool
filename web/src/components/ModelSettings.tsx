import {useState} from "react";
import {api, type ProviderDiagnostic} from "../api";
const PROVIDERS=["openai","openrouter","ollama-cloud","opencode-zen","ollama"];
export function ModelSettings({expanded=false}:{expanded?:boolean}={}){
 const [settings,setSettings]=useState<Awaited<ReturnType<typeof api.modelSettings>>|null>(null);const [yaml,setYaml]=useState("");const [project,setProject]=useState("");const [allowed,setAllowed]=useState<string[]>(PROVIDERS);const [busy,setBusy]=useState(false);const [message,setMessage]=useState("");
 const [diagnostic,setDiagnostic]=useState<ProviderDiagnostic|null>(null);
 const [testing,setTesting]=useState("");
 const test=async(route:string)=>{setBusy(true);setTesting(route);setMessage("Testing synthetic provider execution; this may take several minutes.");try{const result=await api.diagnoseModel(route);setDiagnostic(result);setMessage(result.ok?"All live capability checks passed. Real role execution still requires valid task evidence.":"Live check failed. See the failing stage and suggested next step below.");await load()}catch(error){setMessage((error as Error).message)}finally{setBusy(false);setTesting("")}};
 const load=async()=>{try{const value=await api.modelSettings();setSettings(value);setYaml(value.yaml)}catch(e){setMessage((e as Error).message)}};
 const action=async(verify:boolean)=>{setBusy(true);setMessage("");try{if(verify){await api.verifyModels();await load();setMessage("Live model catalogs checked; unavailable slugs were not substituted.")}else{await api.saveModels({yaml,project:project||null,allowed_providers:project?allowed:null});setMessage("Saved. Resume execution, then verify the routes before starting a new attempt.")}}catch(e){setMessage((e as Error).message)}finally{setBusy(false)}};
 return <details open={expanded?true:undefined} className="disclosure settings-group form-stack" onToggle={e=>{if(e.currentTarget.open&&!settings)void load()}}><summary>Model routes and providers</summary>
 <p>One model runs at a time. Use Stop All before changing chains or allowed providers; saving requires a fresh passkey. OpenRouter and Zen keys are read only from macOS Keychain services plantool/openrouter and plantool/opencode-zen. Ollama local/cloud use the signed-in local Ollama service without a PlanTool API key.</p>
 <p>Review OpenRouter’s setting to exclude providers that train on prompts, and ChatGPT’s “Improve the model for everyone” setting.</p>
 <label>Project provider policy<select value={project} onChange={e=>{setProject(e.target.value);setAllowed(settings?.projects.find(p=>p.id===e.target.value)?.allowed_providers??PROVIDERS)}}><option value="">Edit global chains only</option>{settings?.projects.map(p=><option key={p.id} value={p.id}>{p.checkout} ({p.visibility??"visibility unknown"})</option>)}</select></label>
 {project&&<fieldset><legend>Allowed providers</legend>{PROVIDERS.map(p=><label key={p}><input type="checkbox" checked={allowed.includes(p)} onChange={e=>setAllowed(e.target.checked?[...allowed,p]:allowed.filter(v=>v!==p))}/>{p}</label>)}</fieldset>}
 <label>Fallback chains (YAML)<textarea rows={14} value={yaml} onChange={e=>setYaml(e.target.value)}/></label>
 <button className="btn" disabled={busy||!settings} onClick={()=>void action(false)}>Save chains with passkey</button>
 <button className="btn" disabled={busy||!settings} onClick={()=>void action(true)}>Verify routes with passkey</button>
 <section aria-label="Provider diagnostics">
 <h3>Live provider checks</h3>
 <p>Catalog verification proves only that an ID exists. HTTP tests use up to three requests; a Codex diagnostic turn may use several model/tool calls. Every explicit live test consumes account quota. It uses synthetic data, never your project files. Resume execution first and wait for the global model lease to be free. Codex uses its signed-in CLI, not an OpenAI API key.</p>
 {settings?.routes.map(r=><div key={r.route}><p>{r.route}: {r.available?r.model:`unavailable — ${r.error??"not verified"}`}</p>
 <button className="btn" disabled={busy} onClick={()=>void test(r.route)}>{testing===r.route?"Testing…":`Test ${r.route} with passkey`}</button>
 {r.diagnostic&&<button className="btn" onClick={()=>setDiagnostic(r.diagnostic!)}>Show last result for {r.route}</button>}</div>)}
 {diagnostic&&<div role="status"><h4>{diagnostic.route}: {diagnostic.ok?"Passed":"Failed"}</h4><p>{diagnostic.provider} · {diagnostic.model} · {new Date(diagnostic.checked_at*1000).toLocaleString()}</p>
 {diagnostic.stages.map((s,i)=><div key={i}><strong>{s.stage}: {s.ok?"passed":"failed"}</strong><p>{s.message}{s.http_status?` (HTTP ${s.http_status})`:""}</p>{s.next_step&&<p>Next: {s.next_step}</p>}{s.retry_at&&<p>Retry after {new Date(s.retry_at*1000).toLocaleString()}</p>}</div>)}
 <button className="btn" onClick={async()=>{try{await navigator.clipboard.writeText(JSON.stringify(diagnostic,null,2));setMessage("Copied redacted diagnostics. No credentials, provider bodies or project prompts included.")}catch{setMessage("Clipboard unavailable. Select and copy the redacted JSON below.")}}}>Copy redacted diagnostics</button>
 <details><summary>Redacted diagnostic JSON</summary><pre>{JSON.stringify(diagnostic,null,2)}</pre></details></div>}
 <details><summary>Provider troubleshooting guide</summary>
 <p>Codex: run <code>codex login status</code> on the daemon host. Install/sign in to the same Codex CLI used for Personal. A catalog result does not prove subscription execution. No OpenAI API key is required.</p>
 <p>Local Ollama: run <code>ollama list</code>, pull the exact configured model, then use <code>ollama run MODEL 'Reply with OK only'</code> and <code>ollama ps</code>. Confirm the daemon can reach localhost:11434. Large models may load slowly.</p>
 <p>Ollama Cloud: use <code>ollama signin</code>, pull the exact cloud model reference, then test it through the local Ollama service. A :cloud model runs remotely and account quotas still apply. PlanTool does not require a separate Ollama API key.</p>
 <p>OpenRouter: store a generic-password item with service <code>plantool/openrouter</code> in the daemon user's Keychain. Verify the exact live model ID. HTTP 429 means quota/rate limiting; 404 may mean a missing upstream backend even when its catalog ID exists.</p>
 <p>OpenCode Zen: check the service <code>plantool/opencode-zen</code> and current catalog. An auto route may resolve to a different available model; the diagnostic shows the actual resolved ID.</p>
 <p>For every provider: basic chat, tool calls and structured JSON are separate capabilities. Failed diagnostics do not change chains, approve work or require worker Repair. Share only the redacted diagnostic JSON; never paste keys, authentication headers, raw prompts or private provider logs.</p>
 </details></section>
 {message&&<p role="status">{message}</p>}</details>
}
