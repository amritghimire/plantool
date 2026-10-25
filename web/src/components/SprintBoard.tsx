import {useCallback,useState,useEffect} from "react";
import {Link} from "react-router-dom";
import {api,type SprintIssue} from "../api";
import {usePolling} from "../lib/usePolling";
import {stepLabel} from "../engineering";
export function sprintColumn(item:SprintIssue):string {
  if (["merged","closed","done","declined_closed"].includes(item.step)) return "Done";
  if (item.frozen || ["owner_question","prerequisite_wait","milestone","pr"].includes(item.step)) return "Waiting";
  return item.membership==="started"?"In progress":"Planned";
}
export function SprintBoard({project}:{project:string}){
  const load=useCallback(()=>api.sprints(project),[project]);
  const {data,error,refresh}=usePolling(load);
  const [chosen,setChosen]=useState("");const [busy,setBusy]=useState(false);const [failure,setFailure]=useState("");const [message,setMessage]=useState("");
  useEffect(()=>setMessage(""),[data]);
  const week=chosen||data?.current_week;
  const sprint=data?.sprints.find(s=>s.week===week);
  const current=data?.sprints.find(s=>s.week===data.current_week);
  const trigger=async()=>{setBusy(true);setFailure("");try{const result=await api.planSprint(project,crypto.randomUUID());setMessage(result.message);setChosen(result.week);refresh();}catch(e){setFailure((e as Error).message);}finally{setBusy(false);}};
  return <section className="sprint-section" aria-label="Sprint board"><header className="section-heading"><div><h3>Sprints</h3><p>Plan the week and follow its issues.</p></div>{data?.planning?.state==="blocked"?<Link className="btn primary" to={`/engineering/items/${encodeURIComponent(data.planning.scheduler)}`}>Review paused planning</Link>:<button className="btn primary" disabled={busy||!data||data.planning_queued} onClick={()=>void trigger()}>{busy?"Queueing…":data?.planning?.state==="restoring"?"Restoring planning…":data?.planning?.state==="running"?"Planning is running":data?.planning?.state==="waiting"?"Waiting for a model":data?.planning_queued?"Planning is queued":data?.planning?.state==="stalled"?"Resume planning with passkey":current?"Replan this week with passkey":"Plan this week with passkey"}</button>}</header>
    {data?.planning && <p role="status">{data.planning.message}</p>}
    {data&&<><label> Sprint <select value={week} onChange={e=>setChosen(e.target.value)}>{!current&&<option value={data.current_week}>{data.current_week} · Not planned</option>}{data.sprints.map(s=><option key={s.id} value={s.week}>{s.week} · {s.state}</option>)}</select></label><p className="small muted">{data.backlog_count} open issues outside a sprint. Planning can prepare these issues. Changes still need the usual approval; approved work keeps going.</p>
    {sprint?.issue&&<a href={`https://github.com/${data.repository}/issues/${sprint.issue}`} target="_blank" rel="noreferrer">Review sprint on GitHub →</a>}
    {!sprint?<p>No sprint has been planned for this week. Start planning above.</p>:!sprint.items.length?<p>No issues selected yet. Prepared plans will be considered as they become ready.</p>:<div className="sprint-board">{["Planned","In progress","Waiting","Done"].map(column=>{const items=sprint.items.filter(i=>sprintColumn(i)===column);return <section className="sprint-column" key={column} aria-label={column}><h4>{column} <span className="muted">{items.length}</span></h4>{items.map(i=><article className="item-card" key={i.id}><span className="small muted">{i.issue?`#${i.issue}`:"Project work"} · {i.estimate}</span><h4><Link to={`/engineering/items/${encodeURIComponent(i.id)}`}>{i.title}</Link></h4><p>{i.frozen?"Needs attention":stepLabel(i.step)}</p>{i.issue&&<a href={`https://github.com/${data.repository}/issues/${i.issue}`} target="_blank" rel="noreferrer">Issue</a>}{i.pr&&<> · <a href={`https://github.com/${data.repository}/pull/${i.pr}`} target="_blank" rel="noreferrer">PR #{i.pr}</a></>}</article>)}</section>;})}</div>}</>}
    {(message||failure||error)&&<p role={failure||error?"alert":"status"}>{failure||error||message}</p>}
  </section>;
}
