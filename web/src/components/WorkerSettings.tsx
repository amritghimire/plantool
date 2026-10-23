import {useState} from "react";
import {api} from "../api";
export function WorkerSettings(){
 const [worker,setWorker]=useState<{available:boolean;message:string}|null>(null);const [busy,setBusy]=useState(false);const [error,setError]=useState("");
 const load=async()=>{try{const value=await api.workerStatus();setWorker(value.worker)}catch(e){setError((e as Error).message)}};
 const repair=async()=>{setBusy(true);setError("");try{const value=await api.repairWorker();setWorker(value.worker)}catch(e){setError((e as Error).message);setWorker({available:false,message:"Engineering OS unavailable"})}finally{setBusy(false)}};
 return <details className="disclosure settings-group form-stack" onToggle={e=>{if(e.currentTarget.open&&!worker)void load()}}><summary>Engineering OS</summary>
 <p>{worker?.message??"Engineering OS unavailable until setup. Personal mode works independently."}</p><p>Repair installs the locked Python worker and verifies its contract version. Engineering OS requires macOS sandbox-exec.</p>
 <button className="btn" disabled={busy} onClick={()=>void repair()}>{busy?"Repairing…":"Repair with passkey"}</button>{error&&<p role="alert">{error}</p>}</details>
}
