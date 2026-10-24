import {useState} from "react";
import {api} from "../api";
type Data=Awaited<ReturnType<typeof api.loop>>;
export function LoopSettings({expanded=false}:{expanded?:boolean}={}){
 const [data,setData]=useState<Data|null>(null);const [project,setProject]=useState<Data["projects"][number]|null>(null);const [error,setError]=useState("");const [busy,setBusy]=useState(false);
 const load=async()=>{try{const d=await api.loop();setData(d);setProject(p=>d.projects.find(v=>v.id===p?.id)??null);setError("")}catch(e){setError((e as Error).message)}};
 const act=async(f:()=>Promise<unknown>)=>{setBusy(true);try{await f();await load()}catch(e){setError((e as Error).message)}finally{setBusy(false)}};
 return <details open={expanded?true:undefined} className="disclosure settings-group form-stack" onToggle={e=>{if(e.currentTarget.open&&!data)void load()}}><summary>Continuous loop</summary>
 <p>Weekly scans inspect the default branch before the next sprint. Scan times use the project timezone. Use Stop All before changing settings.</p><button className="btn" onClick={()=>void load()}>Refresh loop</button>
 <label>Loop project<select value={project?.id??""} onChange={e=>setProject(data?.projects.find(p=>p.id===e.target.value)??null)}><option value="">Select a project</option>{data?.projects.map(p=><option key={p.id} value={p.id}>{p.name??p.id}</option>)}</select></label>
 {project&&<><label>Scan day<select value={project.scan_day} onChange={e=>setProject({...project,scan_day:Number(e.target.value)})}>{["Monday","Tuesday","Wednesday","Thursday","Friday","Saturday","Sunday"].map((d,i)=><option key={d} value={i}>{d}</option>)}</select></label><label>Local scan time<input type="time" value={project.scan_time} onChange={e=>setProject({...project,scan_time:e.target.value})}/></label><label><input type="checkbox" checked={project.nix_enabled} onChange={e=>setProject({...project,nix_enabled:e.target.checked})}/>Use Nix for agent build and test environments when flake.nix exists</label><p>Nix remains optional and does not replace the sandbox. Enabling it requires Nix on this machine.</p><button className="btn" disabled={busy} onClick={()=>void act(()=>api.saveLoop({project:project.id,version:project.version,scan_day:project.scan_day,scan_time:project.scan_time,nix_enabled:project.nix_enabled}))}>Save loop settings with passkey</button></>}
 <h4>Urgency requests</h4><p>Requests are notifications, never authorization. Promote on GitHub with /plantool urgent or an allowlisted plantool:urgent label.</p>{data?.notifications.map(n=><p key={n.id}>{n.body}</p>)}
 <h4>Declined items</h4>{data?.declines.map(d=><div key={d.item}><p>{d.title}: {d.reason}</p><p>Clock reset: {d.reset_at}{d.active?"":" · Automatic close disabled"}</p><button className="btn" disabled={busy} onClick={()=>void act(()=>api.overrideDecline(d.item))}>Return to triage with passkey</button></div>)}
 {error&&<p role="alert">{error}</p>}</details>
}
