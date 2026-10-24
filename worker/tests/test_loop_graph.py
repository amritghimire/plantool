"""Real checkpointed loop nodes, fake daemon and no model calls."""
import copy,json
import httpx,pytest
from plantool_worker.app import create_app,VERSIONS
from plantool_worker.work_item import route

@pytest.mark.asyncio
async def test_scan_roles_are_serial_read_only_and_replayed_resumes_do_not_duplicate(tmp_path):
    item="p:scan:week";facts={"item":item,"step":"scan_devops","sequence":0,"generation":0,"plan_sha":"","frozen":False}
    calls={};order=[]
    async def daemon(request):
        path=request.url.path;body=json.loads(request.content or b'{}')
        if path.endswith('/health'):return httpx.Response(200,json=VERSIONS)
        if path.endswith('/workflow_context'):return httpx.Response(200,json={"state":facts,"policy_version":1})
        if path.endswith('/request_dispatch'):
            assert body["capability"]=="read" and body["plan_sha"]==""
            if body["idempotency_key"] not in calls:order.append(body["role"]);calls[body["idempotency_key"]]=body
            return httpx.Response(200,json={"attempt_id":f'a{body["generation"]}',"generation":body["generation"]})
        raise AssertionError(path)
    app=create_app(tmp_path,"token",httpx.MockTransport(daemon));headers={"x-plantool-worker-token":"token","x-plantool-contract-version":str(VERSIONS["contract_version"])}
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url="http://localhost",headers=headers) as client:
            status=(await client.post('/internal/v1/start',json={"thread_id":item,"graph":"work_item","input":{"workflow":copy.deepcopy(facts)},"idempotency_key":"start",**VERSIONS})).json()
            for index,role in enumerate(("devops","qa","pm")):
                details={"kind":"routes","findings":[{"title":"Add regression test","body":"Static TODO evidence","route":"backlog","reason":"Routine coverage"}]} if role=="pm" else {"kind":"scan","summary":"Observed default-branch evidence; unavailable sources recorded"}
                result={"item":item,"role":role,"attempt_id":f'a{index+1}',"generation":index+1,"outcome":"done","summary":"Scan turn","tasks_done":[],"artifacts":[],"checks":[],"findings":[],"next":None,"review_guide":None,"details":details}
                facts=route(facts,result);facts.update(generation=index+1,sequence=index+1)
                payload={"thread_id":item,"interrupt_id":status["interrupt_id"],"value":result,"idempotency_key":f'resume:{index}',**VERSIONS}
                response=await client.post('/internal/v1/resume',json=payload);assert response.status_code==200,response.text;status=response.json()
                assert (await client.post('/internal/v1/resume',json=payload)).json()==status
            assert status["state"]=="completed" and order==["devops","qa","pm"] and len(calls)==3

@pytest.mark.asyncio
@pytest.mark.parametrize("disposition,waiting,next_step",[("fits","backlog","research"),("decline","declined_wait","triage")])
async def test_triage_wait_requires_daemon_wake_and_never_releases_coding(tmp_path,disposition,waiting,next_step):
    item="p:1";facts={"item":item,"step":"triage","sequence":0,"generation":0,"plan_sha":"","frozen":False,"urgent":False}
    dispatches=[];waits=[];seen=set()
    async def daemon(request):
        path=request.url.path;body=json.loads(request.content or b'{}')
        if path.endswith('/health'):return httpx.Response(200,json=VERSIONS)
        if path.endswith('/workflow_context'):return httpx.Response(200,json={"state":facts,"policy_version":1})
        if path.endswith('/request_dispatch'):
            assert body["capability"]=="read"
            if body["idempotency_key"] not in seen:dispatches.append(body["role"]);seen.add(body["idempotency_key"])
            return httpx.Response(200,json={"attempt_id":f'a{body["generation"]}',"generation":body["generation"]})
        if path.endswith('/loop_wait'):waits.append(body);return httpx.Response(200,json={"waiting":True})
        raise AssertionError(path)
    app=create_app(tmp_path,"token",httpx.MockTransport(daemon));headers={"x-plantool-worker-token":"token","x-plantool-contract-version":str(VERSIONS["contract_version"])}
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url="http://localhost",headers=headers) as client:
            status=(await client.post('/internal/v1/start',json={"thread_id":item,"graph":"work_item","input":{"workflow":copy.deepcopy(facts)},"idempotency_key":"start",**VERSIONS})).json()
            result={"item":item,"role":"pm","attempt_id":"a1","generation":1,"outcome":"done","summary":"Project fit","tasks_done":[],"artifacts":[],"checks":[],"findings":[],"next":None,"review_guide":None,"details":{"kind":"triage","disposition":disposition,"reason":"Project scope","duplicate_of":None}}
            facts=route(facts,result);facts.update(generation=1,sequence=1)
            status=(await client.post('/internal/v1/resume',json={"thread_id":item,"interrupt_id":status["interrupt_id"],"value":result,"idempotency_key":"triaged",**VERSIONS})).json()
            assert status["state"]=="interrupted" and len(waits)==1 and waits[0]["kind"]==waiting and dispatches==["pm"]
            facts.update(step=next_step,sequence=2)
            response=await client.post('/internal/v1/resume',json={"thread_id":item,"interrupt_id":status["interrupt_id"],"value":{"event":"owner-or-schedule","step":next_step},"idempotency_key":"wake",**VERSIONS});assert response.status_code==200,response.text
            assert response.json()["state"]=="interrupted" and dispatches==["pm","cto" if next_step=="research" else "pm"]
