"""Event-driven project graph: every wait is an interrupt, never a sleep loop."""
import asyncio,hashlib
from langgraph.types import interrupt
from .models import Dispatch,RoleResult
async def build_step(daemon,store,state):
    w=dict(state["workflow"]);item=w["item"];step=w["step"]
    identity=hashlib.sha256(f"{item}:{w.get('sequence',0)}:{step}".encode()).hexdigest()
    def waiting():
        with store.db() as db:db.execute("UPDATE bridge_threads SET interrupt_id=? WHERE id=?",(identity,item))
    await asyncio.to_thread(waiting)
    context={"item":item,"thread_id":item,"interrupt_id":identity,"idempotency_key":f"project:{identity}"}
    if step=="discussion":
        from .work_item import build_step as discuss
        return await discuss(daemon,store,state)
    if step in ("project_wait","await_sprint_gate"):
        response=await daemon.post("/internal/v1/project_wait",json={**context,"kind":"project" if step=="project_wait" else "sprint"});response.raise_for_status()
        event=interrupt({"project":item,"interrupt_id":identity})
        next_step="discussion" if event["kind"]=="sprint_planning_due" else "start_items" if event["kind"]=="sprint_decision" else "sprint_plan" if event["kind"]=="plan_ready" and w.get("sprint_id") and event.get("payload",{}).get("planning_due") else "project_wait"
        response=await daemon.post("/internal/v1/project_event_ack",json={"project":item.removesuffix(":project"),"event_id":event["id"],"next_step":next_step,"idempotency_key":f"ack:{event['id']}"});response.raise_for_status()
        if response.json().get("stale"):next_step="project_wait"
    elif step=="sprint_plan":
        request=Dispatch(item=item,role="pm",generation=w["generation"]+1,inputs={"step":step},plan_sha="",policy_version=w["policy_version"],capability="read",thread_id=item,interrupt_id=identity,idempotency_key=context["idempotency_key"])
        response=await daemon.post("/internal/v1/request_dispatch",json=request.model_dump(mode="json"));response.raise_for_status()
        result=RoleResult.model_validate(interrupt({"attempt":response.json(),"interrupt_id":identity}))
        next_step="publish_plans" if result.outcome.value=="done" else "escalated"
    elif step in ("publish_plans","start_items"):
        response=await daemon.post("/internal/v1/"+("project_publish" if step=="publish_plans" else "project_start_items"),json=context);response.raise_for_status()
        next_step=("project_wait" if response.json().get("empty") else "await_sprint_gate") if step=="publish_plans" else None
    else:return {"workflow":w,"result":w}
    response=await daemon.post("/internal/v1/workflow_context",json={"item":item});response.raise_for_status();facts=response.json()
    if next_step is not None and facts["state"]["step"]!=next_step:raise RuntimeError("Project projection mismatch")
    updated={**facts["state"],"policy_version":facts["policy_version"]}
    return {"workflow":updated,"result":updated}
