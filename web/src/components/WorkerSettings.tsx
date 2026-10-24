import {useState} from "react";
import {api} from "../api";
export function WorkerSettings({expanded=false,showItems=true}:{expanded?:boolean;showItems?:boolean}={}){
 const [items,setItems]=useState<NonNullable<Awaited<ReturnType<typeof api.workerStatus>>["items"]>>([]);
 const [worker,setWorker]=useState<{available:boolean;message:string}|null>(null);const [busy,setBusy]=useState(false);const [error,setError]=useState("");
 const load=async()=>{try{const value=await api.workerStatus();setWorker(value.worker);setItems(value.items??[])}catch(e){setError((e as Error).message)}};
 const repair=async()=>{setBusy(true);setError("");try{const value=await api.repairWorker();setWorker(value.worker)}catch(e){setError((e as Error).message);setWorker({available:false,message:"Engineering OS unavailable"})}finally{setBusy(false)}};
 return <details open={expanded?true:undefined} className="disclosure settings-group form-stack" onToggle={e=>{if(e.currentTarget.open&&!worker)void load()}}><summary>Engineering OS</summary>
 <p>{worker?.message??"Engineering OS unavailable until setup. Personal mode works independently."}</p><p>Repair installs the locked Python worker and verifies its contract version. Engineering OS requires macOS sandbox-exec.</p>
 {showItems&&items.map(item=><p key={item.item}>{item.title}: {item.state.frozen?"Owner attention required; side effects frozen":item.state.step}. {item.error??item.summary}</p>)}
 <button className="btn" disabled={busy} onClick={()=>void load()}>{showItems?"Refresh work items":"Refresh worker status"}</button>
 <button className="btn" disabled={busy} onClick={()=>void repair()}>{busy?"Repairing…":"Repair with passkey"}</button>{error&&<p role="alert">{error}</p>}</details>
}
