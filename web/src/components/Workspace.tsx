import {useState} from "react";
import {api} from "../api";
export function Workspace(){
 const [data,setData]=useState<Awaited<ReturnType<typeof api.workspace>>|null>(null);
 const [selected,setSelected]=useState("");const [body,setBody]=useState("");const [key,setKey]=useState(()=>crypto.randomUUID());const [busy,setBusy]=useState(false);const [error,setError]=useState("");
 const load=async()=>{try{setData(await api.workspace());setError("")}catch(e){setError((e as Error).message)}};
 const thread=data?.threads.find(t=>t.id===selected);
 const reply=async()=>{if(!thread||!body.trim())return;setBusy(true);try{await api.workspaceReply({thread:thread.id,body,idempotency_key:key});setBody("");setKey(crypto.randomUUID());await load()}catch(e){setError((e as Error).message)}finally{setBusy(false)}};
 return <details className="disclosure settings-group form-stack" onToggle={e=>{if(e.currentTarget.open&&!data)void load()}}><summary>Workspace</summary>
 <p>Project-visible discussions. Owner replies provide context and never approve a plan, sprint or milestone. No direct messages.</p>
 <button className="btn" onClick={()=>void load()}>Refresh workspace</button>
 {data?.projects.map(project=><section key={project.id}><h3>{project.name??project.id}</h3>{data.threads.filter(t=>t.project===project.id).map(t=><button className="btn" key={t.id} onClick={()=>{setSelected(t.id);setBody("");setKey(crypto.randomUUID())}}>{t.channel?`#${t.channel}`:t.title}</button>)}</section>)}
 {thread&&<section aria-label="Discussion thread"><h3>{thread.title}</h3><ol>{thread.messages.map(m=><li key={m.id}><strong>{m.sender}</strong> <time>{m.at}</time><p style={{whiteSpace:"pre-wrap"}}>{m.body}</p></li>)}</ol>
 <label>Owner reply (context only)<textarea value={body} onChange={e=>setBody(e.target.value)} disabled={busy}/></label><button className="btn" disabled={busy||!body.trim()} onClick={()=>void reply()}>{busy?"Sending…":"Send context"}</button></section>}
 {error&&<p role="alert">{error}</p>}</details>
}
