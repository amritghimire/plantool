import {useState} from "react";
import {api} from "../api";
const PROVIDERS=["openai","openrouter","ollama-cloud","opencode-zen","ollama"];
export function ModelSettings({expanded=false}:{expanded?:boolean}={}){
 const [settings,setSettings]=useState<Awaited<ReturnType<typeof api.modelSettings>>|null>(null);const [yaml,setYaml]=useState("");const [project,setProject]=useState("");const [allowed,setAllowed]=useState<string[]>(PROVIDERS);const [busy,setBusy]=useState(false);const [message,setMessage]=useState("");
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
 {settings?.routes.map(r=><p key={r.route}>{r.route}: {r.available?r.model:`unavailable — ${r.error??"not verified"}`}</p>)}
 {message&&<p role="status">{message}</p>}</details>
}
