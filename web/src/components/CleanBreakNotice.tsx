import { useEffect, useState } from "react";
import { api } from "../api";
export function CleanBreakNotice() {
  const [backup,setBackup]=useState<string|null>(null);
  const [error,setError]=useState("");
  useEffect(()=>{let active=true;void api.cleanBreakNotice().then(r=>{if(active)setBackup(r.backup)}).catch((e:Error)=>{if(active)setError(e.message)});return()=>{active=false}},[]);
  if(!backup)return error?<p role="alert">{error}</p>:null;
  return <aside role="status" className="card"><p>PlanTool 0.1 starts with a fresh database. Your previous data was backed up to <code>{backup}</code>. Old sessions were not imported.</p><button className="btn" onClick={()=>{void api.dismissCleanBreakNotice().then(()=>setBackup(null)).catch((e:Error)=>setError(e.message))}}>Dismiss backup notice</button>{error&&<p role="alert">{error}</p>}</aside>;
}
