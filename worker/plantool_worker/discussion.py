"""Bounded, serial discussion routing; conclusions never approve a gate."""
import re
ROLES={"cto","pm","backend","frontend","devops","qa","reviewer","docs","supervisor"}
def advance(state,result):
    state={**state,"discussion":dict(state["discussion"])}
    if result["outcome"]!="done":return {**state,"step":"escalated","frozen":True}
    turn=state["discussion"];round=turn["round"]
    if round>=3:
        state["step"]=turn["after"];state["discussion_summary"]=result["details"]["decision"]
    else:
        parties=list(turn["participants"])
        if result["role"] not in parties:parties.append(result["role"])
        mentions=list(dict.fromkeys(m.lower() for m in re.findall(r"@([a-z]+)\b",result["details"]["body"],re.I) if m.lower() in ROLES))
        turn.update(round=round+1,participants=parties,speaker=("supervisor" if "pm" in parties else "pm") if round+1>=3 else mentions[0] if len(mentions)==1 else "pm")
    return state
