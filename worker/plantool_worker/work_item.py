"""Work-item routing. Results are claims; daemon capabilities remain authoritative."""
from __future__ import annotations
from typing import Any

CODING = {"backend", "frontend", "devops", "qa", "reviewer"}
STEPS = {"research":"cto", "plan":"pm", "plan_review":"cto", "backend":"backend", "frontend":"frontend", "devops":"devops", "qa":"qa", "reviewer":"reviewer", "docs":"docs", "supervisor":"supervisor"}

def route(state: dict[str, Any], result: dict[str, Any]) -> dict[str, Any]:
    state = dict(state)
    step = state["step"]
    if state.get("route_retry") and result.get("outcome")!="done": return state
    findings = result.get("findings", [])
    defects = [f for f in findings if f.get("scope") == "in"]
    if step == "supervisor":
        directive = result.get("details") or {}
        rounds = state.get("supervisor_rounds", 0) + 1
        state["supervisor_rounds"] = rounds
        if rounds > 2 or directive.get("action") == "escalate":
            state.update(step="escalated", frozen=True)
        elif directive.get("action") == "replan":
            state.update(step="plan", approved=False)
        elif directive.get("action") == "retry" and directive.get("owner") in {"backend","frontend","devops","qa"}:
            state["step"] = directive["owner"]
        else:
            state.update(step="escalated", frozen=True)
        return state
    if result.get("outcome") != "done" or defects or any(f.get("severity")=="high" for f in findings):
        key = next((f.get("task_key") for f in defects if f.get("task_key")), "item")
        retries = dict(state.get("retries", {}))
        retries[key] = retries.get(key, 0) + 1
        state["retries"] = retries
        owner = state.get("owners", {}).get(key)
        if retries[key] > 3 or any(f.get("severity") == "high" for f in findings) or owner not in {"backend","frontend","devops","qa"}:
            state["step"] = "supervisor"
        else:
            state["step"] = owner
        return state
    if step == "research": state["step"] = "plan"
    elif step == "plan": state["step"] = "plan_review"
    elif step == "plan_review": state["step"] = "plan_gate"
    elif step == "backend": state["step"] = "frontend" if "frontend" in state.get("areas", []) else ("devops" if state.get("infra_changed") else "qa")
    elif step == "frontend": state["step"] = "devops" if state.get("infra_changed") else "qa"
    elif step == "devops": state["step"] = "qa"
    elif step == "qa": state["step"] = "reviewer"
    elif step == "reviewer": state["step"] = "docs"
    elif step == "docs": state["step"] = "pr"
    else: raise ValueError(f"Invalid role step {step}")
    return state

def after_plan(state: dict[str, Any], approved: bool) -> dict[str, Any]:
    state = dict(state)
    state["approved"] = approved
    state["step"] = ("backend" if "backend" in state.get("areas", []) else "frontend" if "frontend" in state.get("areas", []) else "devops" if state.get("infra_changed") else "qa") if approved else "plan"
    return state

async def build_step(daemon, store, state):
    """A node only submits short broker calls and interrupts; it never waits for a role."""
    import hashlib
    from langgraph.types import interrupt
    from .models import Dispatch, RoleResult
    workflow = dict(state["workflow"])
    item = workflow["item"]
    step = workflow["step"]
    identity = hashlib.sha256(f"{item}:{workflow.get('sequence',0)}:{step}".encode()).hexdigest()
    # Stable identity is persisted before external admission, including retries.
    def waiting():
        with store.db() as db:db.execute("UPDATE bridge_threads SET interrupt_id=? WHERE id=?",(identity,item))
    import asyncio
    await asyncio.to_thread(waiting)
    context = {"item":item,"thread_id":item,"interrupt_id":identity,"idempotency_key":f"node:{identity}"}
    if step in STEPS:
        role = STEPS[step]
        request = Dispatch(item=item,role=role,generation=workflow.get("generation",0)+1,
            inputs={"step":step},plan_sha=workflow.get("plan_sha", ""),
            policy_version=workflow["policy_version"],capability="write" if role in {"backend","frontend","devops"} else "tests" if role=="qa" else "read",
            thread_id=item,interrupt_id=identity,idempotency_key=context["idempotency_key"])
        response=await daemon.post("/internal/v1/request_dispatch",json=request.model_dump(mode="json"));response.raise_for_status()
        result=RoleResult.model_validate(interrupt({"attempt":response.json(),"interrupt_id":identity})).model_dump(mode="json")
        facts=await daemon.post("/internal/v1/workflow_context",json={"item":item});facts.raise_for_status();facts=facts.json()
        for key in ["areas","owners","plan_sha","infra_changed","generation","sequence","affected_files","acceptance_criteria","route_retry"]:
            if key in facts["state"]: workflow[key]=facts["state"][key]
        updated=route(workflow,result)
        if step in {"backend","frontend"} and updated["step"]=="qa" and workflow.get("infra_changed"): updated["step"]="devops"
        if updated["step"]!=facts["state"]["step"]: raise RuntimeError("Daemon rejected graph transition")
        updated["policy_version"]=facts["policy_version"]
        updated["summary"]=result["summary"]
    elif step=="plan_gate":
        response=await daemon.post("/internal/v1/request_gate",json={**context,"kind":"plan","target_digest":workflow["plan_sha"],"target_sha":None,"summary":workflow.get("summary"),"discussion_thread_id":None});response.raise_for_status()
        decision=interrupt({"gate":response.json(),"interrupt_id":identity})
        updated=after_plan(workflow,decision.get("decision")=="approved")
        facts=await daemon.post("/internal/v1/workflow_context",json={"item":item});facts.raise_for_status();facts=facts.json();updated["policy_version"]=facts["policy_version"]
        if updated["step"]!=facts["state"]["step"]: raise RuntimeError("Stale gate resume")
    elif step=="pr":
        response=await daemon.post("/internal/v1/workflow_pr",json=context);response.raise_for_status()
        decision=interrupt({"pr":response.json(),"interrupt_id":identity})
        if decision.get("event")=="merged": updated={**workflow,"step":"merged"}
        else:
            facts=await daemon.post("/internal/v1/workflow_context",json={"item":item});facts.raise_for_status();facts=facts.json();updated={**workflow,**facts["state"],"policy_version":facts["policy_version"]}
    else: return {"workflow":workflow,"result":workflow}
    return {"workflow":updated,"result":updated}
