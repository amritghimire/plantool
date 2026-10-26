"""A cancelled scheduler must be restored with a new wait identity, not replayed as success."""
import copy
import httpx
import pytest
from plantool_worker.app import create_app, VERSIONS

@pytest.mark.asyncio
async def test_cancelled_project_restores_wait_and_can_start_planning(tmp_path):
    item="p:project"
    facts={"item":item,"project":True,"step":"project_wait","sequence":0,"generation":0,"plan_sha":"","frozen":False,"policy_version":1}
    waits=[];admissions=[]
    async def daemon(request):
        import json
        if request.url.path.endswith("health"):return httpx.Response(200,json=VERSIONS)
        body=json.loads(request.content or b"{}")
        if request.url.path.endswith("workflow_context"):return httpx.Response(200,json={"state":facts,"policy_version":1})
        if request.url.path.endswith("project_wait"):
            waits.append(body["interrupt_id"]);return httpx.Response(200,json={"waiting":True})
        if request.url.path.endswith("project_event_ack"):
            facts.update(step="discussion",sequence=facts["sequence"]+1,discussion={"id":"topic","thread":"sprint","speaker":"pm","round":0,"participants":[],"after":"sprint_plan"})
            return httpx.Response(200,json={"accepted":True})
        if request.url.path.endswith("request_dispatch"):
            admissions.append(body);return httpx.Response(200,json={"attempt_id":"a1","generation":1})
        raise AssertionError(request.url.path)
    app=create_app(tmp_path,"t"*40,httpx.MockTransport(daemon))
    headers={"x-plantool-worker-token":"t"*40,"x-plantool-contract-version":str(VERSIONS["contract_version"])}
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url="http://worker",headers=headers) as client:
            first={"thread_id":item,"graph":"project","input":{"workflow":copy.deepcopy(facts)},"idempotency_key":"start",**VERSIONS}
            started=(await client.post("/internal/v1/start",json=first)).json()
            await client.post("/internal/v1/cancel",json={"thread_id":item,"idempotency_key":"daemon-stop"})
            assert (await client.get(f"/internal/v1/status/{item}")).json()["state"]=="cancelled"
            stale=await client.post("/internal/v1/resume",json={"thread_id":item,"interrupt_id":started["interrupt_id"],"value":{},"idempotency_key":"old-wake",**VERSIONS})
            assert stale.status_code==409
            facts["sequence"]+=1
            recovered=await client.post("/internal/v1/recover",json={**first,"input":{"workflow":copy.deepcopy(facts)},"idempotency_key":"automatic-restore"})
            assert recovered.status_code==200,recovered.text
            assert len(waits)==2 and waits[0]!=waits[1]
            result=await client.post("/internal/v1/resume",json={"thread_id":item,"interrupt_id":recovered.json()["interrupt_id"],"value":{"id":"due","kind":"sprint_planning_due","payload":{}},"idempotency_key":"fresh-wake",**VERSIONS})
            assert result.status_code==200,result.text
            assert len(admissions)==1 and admissions[0]["role"]=="pm" and admissions[0]["capability"]=="read"

@pytest.mark.asyncio
async def test_approved_gate_reattaches_cancelled_interrupt_without_creating_new_gate(tmp_path):
    facts={"item":"p:1","step":"plan_gate","sequence":1,"generation":0,"plan_sha":"a"*64,"areas":["backend"],"approved":False,"frozen":False,"policy_version":1}
    gates={};admissions=[];enabled=False
    async def daemon(request):
        import json
        if request.url.path.endswith("health"):return httpx.Response(200,json=VERSIONS)
        body=json.loads(request.content or b"{}")
        if request.url.path.endswith("workflow_context"):return httpx.Response(200,json={"state":facts,"policy_version":1,"execution_enabled":enabled})
        if request.url.path.endswith("request_gate"):
            gates.setdefault(body["idempotency_key"],body);return httpx.Response(200,json={"gate_id":"original-gate"})
        if request.url.path.endswith("request_dispatch"):
            admissions.append(body);return httpx.Response(200,json={"attempt_id":"a1","generation":1})
        raise AssertionError(request.url.path)
    headers={"x-plantool-worker-token":"t"*40,"x-plantool-contract-version":str(VERSIONS["contract_version"])}
    app=create_app(tmp_path,"t"*40,httpx.MockTransport(daemon))
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url="http://worker",headers=headers) as client:
            started=(await client.post("/internal/v1/start",json={"thread_id":"p:1","graph":"work_item","input":{"workflow":copy.deepcopy(facts)},"idempotency_key":"start",**VERSIONS})).json()
            await client.post("/internal/v1/cancel",json={"thread_id":"p:1","idempotency_key":"stop"})
            resume={"thread_id":"p:1","interrupt_id":started["interrupt_id"],"value":{"decision":"approved"},"idempotency_key":"approved-receipt",**VERSIONS}
            assert (await client.post("/internal/v1/resume",json=resume)).status_code==409
    # Recreate the worker against its real persisted checkpoints and a fresh credential.
    app=create_app(tmp_path,"u"*40,httpx.MockTransport(daemon))
    headers["x-plantool-worker-token"]="u"*40
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url="http://worker",headers=headers) as client:
            assert (await client.get("/internal/v1/status/p:1")).json()["state"]=="cancelled"
            enabled=True;facts.update(step="backend",approved=True,sequence=2)
            facts["frozen"]=True
            assert (await client.post("/internal/v1/resume",json=resume)).status_code==409
            facts["frozen"]=False;facts["closed_on_github"]=True
            assert (await client.post("/internal/v1/resume",json=resume)).status_code==409
            facts["closed_on_github"]=False
            reply=await client.post("/internal/v1/resume",json=resume)
            assert reply.status_code==200,reply.text
            assert len(gates)==1 and len(admissions)==1
            assert admissions[0]["plan_sha"]=="a"*64
            repeated=await client.post("/internal/v1/resume",json=resume)
            assert repeated.status_code==200 and len(admissions)==1
