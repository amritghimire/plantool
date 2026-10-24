"""Pure graph phase routing; approval validity stays in Rust."""
def first(areas):
    return next((role for role in ("backend","frontend","devops") if role in areas),"qa")
def advance(state):
    state=dict(state);index=state.get("phase_index",0)+1
    if index<len(state.get("phases",[])):
        state.update(phase_index=index,areas=state["phases"][index]["areas"]);state["step"]=first(state["areas"])
    else:state["step"]="pr"
    return state
def boundary(state):
    phases=state.get("phases",[])
    if not phases:return {**state,"step":"pr"}
    phase=phases[state.get("phase_index",0)];preset=state.get("preset","minimal")
    return {**state,"step":"milestone"} if preset=="strict" or preset=="standard" and phase["tagged"] else advance(state)
