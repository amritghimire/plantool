from plantool_worker.work_item import route, after_plan

def test_roles_gates_skips_and_defect_owner():
    state={"step":"research","areas":["frontend"],"owners":{"T1":"frontend"}}
    for expected in ["plan","plan_review","plan_gate"]:
        state=route(state,{"outcome":"done"});assert state["step"]==expected
    state=after_plan(state,True);assert state["step"]=="frontend"
    state=route(state,{"outcome":"done"});assert state["step"]=="qa"
    state=route(state,{"outcome":"needs_fix","findings":[{"scope":"in","task_key":"T1","severity":"medium"}]});assert state["step"]=="frontend"

def test_retries_supervisor_and_freeze():
    state={"step":"qa","approved":True,"owners":{"T1":"backend"}}
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

def test_prerequisite_wait_has_no_implementation_authority():
    state={"step":"supervisor","supervisor_rounds":1,"approved":False}
    updated=route(state,{"outcome":"done","details":{"action":"wait","task_key":"pr:3"}})
    assert updated["step"]=="prerequisite_wait"
    assert updated["approved"] is False

def test_identical_discussion_moves_to_proposal_without_claiming_agreement():
    from plantool_worker.discussion import advance
    state={"step":"discussion","discussion":{"round":1,"after":"plan","previous_body":"same update"}}
    result={"outcome":"done","details":{"body":"Same  update","decision":None}}
    updated=advance(state,result)
    assert updated["step"]=="plan"
    assert "no new information" in updated["discussion_summary"]
    assert not updated.get("approved",False)


def test_plan_review_observations_wait_for_approval_and_rework_cannot_start_coding():
    state={"step":"plan_review","approved":False,"owners":{"T1":"backend"}}
    observation={"outcome":"done","findings":[{"scope":"in","task_key":"T1","severity":"medium","title":"Regression risk is acknowledged and mitigated"}]}
    assert route(state,observation)["step"]=="plan_gate"
    assert route(state,{**observation,"outcome":"needs_fix"})["step"]=="plan"
    supervisor={**state,"step":"supervisor"}
    assert route(supervisor,{"outcome":"done","details":{"action":"retry","owner":"backend"}})["step"]=="plan"


def test_completed_retry_recommendations_normalize_role_names_without_owner_review():
    for owner,expected in [("Docs","plan"),("CTO","research"),("cto","research"),("PM","plan")]:
        state={"step":"supervisor","approved":False,"supervisor_rounds":10}
        result={"outcome":"done","details":{"action":"retry","owner":owner}}
        assert route(state,result)["step"]==expected
        assert not route(state,result).get("frozen")
    assert route({"step":"supervisor","approved":True,"supervisor_rounds":10},{"outcome":"done","details":{"action":"retry","owner":"Docs"}})["step"]=="docs"


def test_scope_replan_still_prepares_a_new_plan_after_old_supervisor_rounds():
    state={"step":"supervisor","approved":True,"supervisor_rounds":10}
    result={"outcome":"done","details":{"action":"replan","reason":"Propose a smaller scope"}}
    next_state=route(state,result)
    assert next_state["step"]=="plan"
    assert next_state["approved"] is False
    assert not next_state.get("frozen")
