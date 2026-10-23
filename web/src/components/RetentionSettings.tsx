import {useState} from "react";
import {api} from "../api";
export function RetentionSettings(){
 const [loaded,setLoaded]=useState(false);const [busy,setBusy]=useState(false);const [days,setDays]=useState(30);const [evidence,setEvidence]=useState(90);const [gb,setGb]=useState(20);const [message,setMessage]=useState("");
 const load=async()=>{setBusy(true);try{const policy=await api.retentionPolicy();setDays(policy.transcript_days);setEvidence(policy.evidence_days);setGb(policy.disk_cap_bytes/1024**3);setLoaded(true)}catch(e){setMessage((e as Error).message)}finally{setBusy(false)}};
 const save=async()=>{setBusy(true);setMessage("");try{await api.saveRetentionPolicy({transcript_days:days,evidence_days:evidence,disk_cap_bytes:Math.round(gb*1024**3)});setMessage("Retention settings saved.")}catch(e){setMessage((e as Error).message)}finally{setBusy(false)}};
 return <details className="disclosure settings-group form-stack" onToggle={e=>{if(e.currentTarget.open&&!loaded&&!busy)void load()}}><summary>Retention and disk use</summary>
 <p>Transcripts are kept after a session closes; evidence is kept after merge. Permanent records, pinned runs and failed/recovery evidence are preserved. The disk cap removes the oldest unpinned transcripts first.</p>
 <label>Transcript retention (days)<input type="number" min="0" max="36500" value={days} onChange={e=>setDays(Number(e.target.value))}/></label>
 <label>Evidence retention (days after merge)<input type="number" min="0" max="36500" value={evidence} onChange={e=>setEvidence(Number(e.target.value))}/></label>
 <label>Disk cap (GiB)<input type="number" min="0.001" step="0.001" value={gb} onChange={e=>setGb(Number(e.target.value))}/></label>
 <button className="btn" disabled={busy||!loaded} onClick={()=>void save()}>Save retention with passkey</button>{message&&<p role="status">{message}</p>}
 </details>;
}
