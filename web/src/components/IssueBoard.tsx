import {useState} from "react";
import {Link} from "react-router-dom";
import {api} from "../api";
import {stepLabel,type EngineeringItem} from "../engineering";
import {NextAction} from "./NextAction";
export const boardColumns = [["backlog","Backlog"],["planned","Planning"],["in_progress","In progress"],["review","Review"],["waiting","Waiting"],["done","Done"]] as const;
export function workflowColumn(item:EngineeringItem):string {
  if (["done","closed","merged","declined_closed"].includes(item.step)) return "done";
  if (item.frozen || ["owner_question","prerequisite_wait","escalated"].includes(item.step)) return "waiting";
  if (item.waiting_gates || ["plan_review","reviewer","pr","milestone"].includes(item.step)) return "review";
  if (["backend","frontend","devops","qa","docs","rebase"].includes(item.step)) return "in_progress";
  return ["new","backlog","declined_wait"].includes(item.step)?"backlog":"planned";
}
export function IssueBoard({items,repository,refresh}:{items:EngineeringItem[];repository:string|null;refresh:()=>void}) {
  const [busy,setBusy]=useState("");const [error,setError]=useState("");
  const [draft,setDraft]=useState("");
  const move=async(id:string,column:string|null)=>{setBusy(id);setError("");try{await api.organizeItem(id,column);refresh();}catch(e){setError((e as Error).message);}finally{setBusy("");}};
  return <section aria-label="Issue board"><p className="muted">Drag cards or choose a column to organize your board. Workflow status stays visible; moving a card does not approve, start, pause or complete work.</p>
    {repository&&<form className="board-add" onSubmit={e=>e.preventDefault()}><label>New backlog issue<input value={draft} onChange={e=>setDraft(e.target.value)} placeholder="Describe a small, concrete task"/></label><a className="btn" href={`https://github.com/${repository}/issues/new?title=${encodeURIComponent(draft)}`} target="_blank" rel="noreferrer">Create issue on GitHub →</a><p className="small muted">Your new issue joins PlanTool after intake. GitHub confirms its author and repository permissions.</p></form>}
    {error&&<p role="alert">{error}</p>}
    <div className="issue-board">{boardColumns.map(([key,label])=><section className="sprint-column" aria-label={label} key={key} onDragOver={e=>e.preventDefault()} onDrop={e=>{e.preventDefault();const id=e.dataTransfer.getData("text/plantool-item");if(items.some(i=>i.id===id))void move(id,key);}}>
      <h4>{label} <span className="muted">{items.filter(i=>(i.board_column||workflowColumn(i))===key).length}</span></h4>
      {items.filter(i=>(i.board_column||workflowColumn(i))===key).map(i=><article className="item-card" key={i.id} draggable={!busy} onDragStart={e=>e.dataTransfer.setData("text/plantool-item",i.id)}>
        <span className="small muted">{i.issue?`ISSUE #${i.issue}`:"PROJECT ACTIVITY"}{i.priority==="urgent"?" · Urgent":""}</span>
        <h4><Link to={`/engineering/items/${encodeURIComponent(i.id)}`}>{i.title}</Link></h4>
        <span className={`state-chip ${i.frozen?"attention":""}`}>{i.frozen?"Owner attention required":stepLabel(i.step)}</span>
        {i.board_column&&<p className="small muted">Manual column · Workflow: {stepLabel(i.step)}</p>}
        {i.waiting_gates>0&&<p>{i.waiting_gates} pending or open {i.waiting_gates===1?"gate":"gates"}</p>}
        <NextAction action={i.next_action}/>
        {i.error&&<details><summary>Technical details</summary><p className="error">{i.error}</p></details>}
        <label className="small">Column for {i.title}<select aria-label={`Column for ${i.title}`} disabled={busy===i.id} value={i.board_column||""} onChange={e=>void move(i.id,e.target.value||null)}><option value="">Automatic ({boardColumns.find(([c])=>c===workflowColumn(i))?.[1]})</option>{boardColumns.map(([c,l])=><option key={c} value={c}>{l}</option>)}</select></label>
      </article>)}
    </section>)}</div>
  </section>;
}
