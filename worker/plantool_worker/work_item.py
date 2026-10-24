"""Work-item routing. Results are claims; daemon capabilities remain authoritative."""
from __future__ import annotations
from typing import Any

CODING = {"backend", "frontend", "devops", "qa", "reviewer"}
STEPS = {"research":"cto", "plan":"pm", "plan_review":"cto", "backend":"backend", "frontend":"frontend", "devops":"devops", "qa":"qa", "reviewer":"reviewer", "docs":"docs", "supervisor":"supervisor"}

def route(state: dict[str, Any], result: dict[str, Any]) -> dict[str, Any]:
    state = dict(state)
    step = state["step"]
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
    if result.get("outcome") != "done" or defects:
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
