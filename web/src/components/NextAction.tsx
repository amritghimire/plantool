import {Link} from "react-router-dom";
import type {EngineeringItem} from "../engineering";
export function NextAction({action}:{action:EngineeringItem["next_action"]}){
  if(!action)return null;
  const minutes=action.retry_at?Math.max(0,Math.ceil((action.retry_at*1000-Date.now())/60000)):0;
  return <section className="next-action" aria-label="Next action"><strong>Next action</strong><p>{action.kind==="retry"?minutes>0?`Retrying in ${minutes} ${minutes===1?"minute":"minutes"}. You do not need to restart it.`:"Retrying when a model is available.":action.message}</p>{action.retry_at&&<p className="small muted">Next check: {new Date(action.retry_at*1000).toLocaleString()}</p>}{action.kind==="retry"&&action.detail&&<details><summary>Why it is waiting</summary><p>{action.detail}</p></details>}{action.href&&action.button&&(action.href.startsWith("https://github.com/")?<a className="btn" href={action.href} target="_blank" rel="noreferrer">{action.button}</a>:<Link className="btn" to={action.href}>{action.button}</Link>)}</section>;
}
