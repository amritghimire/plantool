import copy
import httpx
import pytest
from plantool_worker.app import create_app,VERSIONS
from plantool_worker.discussion import advance

@pytest.mark.asyncio
async def test_project_checkpoints_event_turns_free_sizing_gate_and_return_to_wait(tmp_path):
    item="p:project"
    facts={"item":item,"project":True,"step":"project_wait","sequence":0,"generation":0,"plan_sha":"","frozen":False,"discuss_enabled":True,"policy_version":1}
    admitted={};calls=[]
    async def daemon(request):
        nonlocal facts
        path=request.url.path;body=__import__('json').loads(request.content or b'{}');calls.append(path)
        if path.endswith('/health'):return httpx.Response(200,json=VERSIONS)
        if path.endswith('/workflow_context'):return httpx.Response(200,json={"state":facts,"policy_version":1})
        if path.endswith('/project_wait'):return httpx.Response(200,json={"waiting":True})
        if path.endswith('/project_event_ack'):
            facts["step"]=body["next_step"];facts["sequence"]+=1
            if facts["step"]=="discussion":facts["discussion"]={"id":"topic","thread":"sprint-thread","speaker":"pm","round":0,"participants":[],"after":"sprint_plan"}
            return httpx.Response(200,json={"accepted":True})
        if path.endswith('/request_dispatch'):
            assert body["capability"]=="read"
            admitted.setdefault(body["idempotency_key"],{"attempt_id":f'a{len(admitted)}',"generation":body["generation"]})
            return httpx.Response(200,json=admitted[body["idempotency_key"]])
        if path.endswith('/project_publish'):
            facts["step"]="await_sprint_gate";facts["sequence"]+=1
            return httpx.Response(200,json={"snapshot":"immutable"})
        if path.endswith('/project_start_items'):
            facts["step"]="project_wait";facts["sequence"]+=1
            return httpx.Response(200,json={"released":True})
        raise AssertionError(path)
    app=create_app(tmp_path,"token",httpx.MockTransport(daemon));headers={"x-plantool-worker-token":"token","x-plantool-contract-version":str(VERSIONS["contract_version"])}
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url="http://localhost",headers=headers) as client:
            response=await client.post('/internal/v1/start',json={"thread_id":item,"graph":"project","input":{"workflow":copy.deepcopy(facts)},"idempotency_key":"start",**VERSIONS});assert response.status_code==200;status=response.json()
            async def resume(value,key):
                response=await client.post('/internal/v1/resume',json={"thread_id":item,"interrupt_id":status["interrupt_id"],"value":value,"idempotency_key":key,**VERSIONS});assert response.status_code==200,response.text;return response.json()
            status=await resume({"id":"due","kind":"sprint_planning_due","payload":{"week":"2026-W43"}},"due")
            for index in range(4):
                role=facts["discussion"]["speaker"]
                result={"item":item,"role":role,"attempt_id":f'a{index}',"generation":index+1,"outcome":"done","summary":"Inspected candidate risk","tasks_done":[],"artifacts":[],"checks":[],"findings":[],"next":None,"review_guide":None,"details":{"kind":"discussion","body":"@CTO inspect contract","decision":"Three prepared items fit" if index==3 else None}}
                facts=advance(facts,result);facts.update(generation=index+1,sequence=facts["sequence"]+1)
                status=await resume(result,f'turn:{index}')
            assert facts["step"]=="sprint_plan"
            result={"item":item,"role":"pm","attempt_id":"a4","generation":5,"outcome":"done","summary":"PM sizes freely","tasks_done":[],"artifacts":[],"checks":[],"findings":[],"next":None,"review_guide":None,"details":{"kind":"sprint","items":[{"item":f'p:{n}',"estimate":"bounded change"} for n in (1,2,3)],"notes":"Three prepared candidates"}}
            facts.update(step="publish_plans",generation=5,sequence=facts["sequence"]+1)
            status=await resume(result,"sprint-plan")
            assert status["state"]=="interrupted" and facts["step"]=="await_sprint_gate"
            status=await resume({"id":"approved","kind":"sprint_decision","payload":{}},"approval")
            assert status["state"]=="interrupted" and facts["step"]=="project_wait"
            assert len(admitted)==5
            assert calls.count('/internal/v1/project_start_items')==1
