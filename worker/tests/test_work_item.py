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

def test_strict_research_and_phase_stops_never_become_owner_approvals():
    from plantool_worker.work_item import route,after_plan
    from plantool_worker.phases import advance
    state={"step":"research","preset":"strict","discuss_enabled":True}
    done={"outcome":"done","findings":[]}
    assert route(state,done)["step"]=="research_gate"
    state.update(phases=[{"areas":["backend"],"tagged":False},{"areas":["frontend"],"tagged":False}])
    state=after_plan(state,True);assert state["step"]=="backend"
    state["step"]="docs";state=route(state,done);assert state["step"]=="milestone"
    state=advance(state);assert state["step"]=="frontend" and state["phase_index"]==1
    state["step"]="docs";state=route(state,done);assert state["step"]=="milestone"
    assert advance(state)["step"]=="pr"

def test_failed_triage_retries_route_before_escalating():
    state={"step":"triage","route_retry":True}
    assert route(state,{"outcome":"failed"})['step']=='triage'
    state['route_retry']=False
    assert route(state,{"outcome":"failed"})['frozen'] is True


def test_clarification_waits_for_owner_without_freezing_or_implementation():
    state = {"step":"supervisor", "supervisor_rounds":2, "approved":False}
    updated = route(state, {"outcome":"done", "details":{"action":"clarify", "reason":"Should scope remain documentation-only?"}})
    assert updated["step"] == "owner_question"
    assert not updated.get("frozen", False)
    assert updated["approved"] is False
    assert updated["owner_question"]["question"] == "Should scope remain documentation-only?"
