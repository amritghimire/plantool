"""A real checkpointed LangGraph drives a fake capability broker, never models."""
import copy,json
import httpx,pytest
from plantool_worker.app import create_app,VERSIONS
from plantool_worker.work_item import route,after_plan

@pytest.mark.asyncio
async def test_every_role_dispatch_interrupt_resume_and_both_gates(tmp_path):
    original={"item":"p:1","step":"research","generation":0,"sequence":0,"plan_sha":"","areas":[],"owners":{},"retries":{},"supervisor_rounds":0,"frozen":False}
    facts=copy.deepcopy(original);seen={};order=[]
    async def daemon(request):
        nonlocal facts
        path=request.url.path
        if path.endswith('/health'):return httpx.Response(200,json=VERSIONS)
        data=json.loads(request.content)
        if path.endswith('/workflow_context'):return httpx.Response(200,json={"state":facts,"policy_version":1})
        if path.endswith('/request_dispatch'):
            key=data['idempotency_key']
            if key not in seen:seen[key]=data;order.append(data['role'])
            else:assert seen[key]==data
            return httpx.Response(200,json={"attempt_id":"a"+str(data['generation']),"generation":data['generation']})
        if path.endswith('/request_gate'):return httpx.Response(200,json={"gate_id":"plan"})
        if path.endswith('/workflow_pr'):return httpx.Response(200,json={"intent":"pr"})
        raise AssertionError(path)
    app=create_app(tmp_path,'t'*40,httpx.MockTransport(daemon))
    headers={"x-plantool-worker-token":'t'*40,"x-plantool-contract-version":str(VERSIONS['contract_version'])}
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url='http://worker',headers=headers) as client:
            status=(await client.post('/internal/v1/start',json={"thread_id":"p:1","graph":"work_item","input":{"workflow":original},"idempotency_key":"start",**VERSIONS})).json()
            for index in range(10):
                step=facts['step']
                if step=='plan_gate':
                    facts=after_plan(facts,True);value={"decision":"approved"}
                elif step=='pr':
                    facts['step']='merged';value={"event":"merged"}
                else:
                    role=seen[next(reversed(seen))]['role'];generation=facts['generation']+1
                    value={"item":"p:1","role":role,"attempt_id":"a"+str(generation),"generation":generation,"outcome":"done","summary":"fake result","tasks_done":[],"artifacts":[],"checks":[],"findings":[],"next":None,"review_guide":None,"details":None}
                    if step=='plan':facts.update(areas=['backend'],owners={'T1':'backend'},plan_sha='a'*64)
                    facts=route(facts,value);facts.update(generation=generation,sequence=facts['sequence']+1)
                payload={"thread_id":"p:1","interrupt_id":status['interrupt_id'],"value":value,"idempotency_key":f'resume:{index}',**VERSIONS}
                response=await client.post('/internal/v1/resume',json=payload)
                assert response.status_code==200,response.text
                status=response.json()
                replay=await client.post('/internal/v1/resume',json=payload);assert replay.json()==status
                if status['state']=='completed':break
            assert facts['step']=='merged'
            assert order==['cto','pm','cto','backend','qa','reviewer','docs']
            assert len(seen)==7
