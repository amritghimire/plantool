from plantool_worker.work_item import route, after_plan

def test_roles_gates_skips_and_defect_owner():
    state={"step":"research","areas":["frontend"],"owners":{"T1":"frontend"}}
    for expected in ["plan","plan_review","plan_gate"]:
        state=route(state,{"outcome":"done"});assert state["step"]==expected
    state=after_plan(state,True);assert state["step"]=="frontend"
    state=route(state,{"outcome":"done"});assert state["step"]=="qa"
    state=route(state,{"outcome":"needs_fix","findings":[{"scope":"in","task_key":"T1","severity":"medium"}]});assert state["step"]=="frontend"

def test_retries_supervisor_and_freeze():
    state={"step":"qa","owners":{"T1":"backend"}}
    defect={"outcome":"needs_fix","findings":[{"scope":"in","task_key":"T1","severity":"low"}]}
    for _ in range(3): state=route(state,defect);assert state["step"]=="backend"
    state=route(state,defect);assert state["step"]=="supervisor"
    state=route(state,{"outcome":"done","details":{"action":"escalate"}});assert state["frozen"]
