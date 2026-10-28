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

@pytest.mark.asyncio
async def test_authoritative_workflow_advance_survives_role_handoff(tmp_path):
    original={"item":"p:stale-infra","step":"backend","generation":0,"sequence":7,"plan_sha":"a"*64,"policy_version":1,"areas":["backend"],"owners":{"T1":"backend"},"retries":{},"supervisor_rounds":0,"frozen":False,"approved":True,"infra_changed":True,"affected_files":["playground/text.py"],"acceptance_criteria":["slug works"]}
    facts=copy.deepcopy(original);contexts=0;roles=[]
    async def daemon(request):
        nonlocal contexts
        path=request.url.path
        if path.endswith('/health'):return httpx.Response(200,json=VERSIONS)
        data=json.loads(request.content)
        if path.endswith('/workflow_context'):
            contexts+=1
            if contexts>1:
                facts.update(step='devops',infra_changed=False,generation=1,sequence=8)
            return httpx.Response(200,json={"state":facts,"policy_version":1})
        if path.endswith('/request_dispatch'):
            roles.append(data['role'])
            return httpx.Response(200,json={"attempt_id":"a"+str(data['generation']),"generation":data['generation']})
        raise AssertionError(path)
    app=create_app(tmp_path,'t'*40,httpx.MockTransport(daemon))
    headers={"x-plantool-worker-token":'t'*40,"x-plantool-contract-version":str(VERSIONS['contract_version'])}
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url='http://worker',headers=headers) as client:
            started=await client.post('/internal/v1/start',json={"thread_id":"p:stale-infra","graph":"work_item","input":{"workflow":original},"idempotency_key":"start",**VERSIONS})
            assert started.status_code==200,started.text
            value={"item":"p:stale-infra","role":"backend","attempt_id":"a1","generation":1,"outcome":"done","summary":"Implementation accepted","tasks_done":["T1"],"artifacts":[],"checks":[],"findings":[],"next":None,"review_guide":None,"details":None}
            resumed=await client.post('/internal/v1/resume',json={"thread_id":"p:stale-infra","interrupt_id":started.json()['interrupt_id'],"value":value,"idempotency_key":"resume",**VERSIONS})
            assert resumed.status_code==200,resumed.text
            assert resumed.json()['state']=='interrupted'
            # Re-entering the interrupted node reuses its dispatch key; it
            # does not create a second model attempt before adopting DevOps.
            assert roles==['backend','backend','devops']

@pytest.mark.asyncio
async def test_llm_role_scope_and_cached_result_use_no_real_provider(tmp_path):
    calls=[]
    async def daemon(request):
        if request.url.path.endswith('/health'):return httpx.Response(200,json=VERSIONS)
        assert request.url.path=='/v1/chat/completions'
        data=json.loads(request.content);calls.append(data)
        claim={"item":"forged","role":"pm","attempt_id":"wrong","generation":999,"outcome":"done","summary":"Read relevant files","tasks_done":[],"artifacts":[],"checks":[],"findings":[],"next":None,"review_guide":None,"details":{"kind":"research","markdown":"Architecture evidence"}}
        return httpx.Response(200,json={"choices":[{"message":{"content":json.dumps(claim)}}]})
    app=create_app(tmp_path,'t'*40,httpx.MockTransport(daemon));headers={"x-plantool-worker-token":'t'*40,"x-plantool-contract-version":str(VERSIONS['contract_version'])}
    payload={"request":{"item":"p:1","role":"cto","generation":1,"inputs":{"step":"research"},"plan_sha":"","policy_version":1,"capability":"read","idempotency_key":"dispatch:1","thread_id":"p:1","interrupt_id":"i1"},"attempt_id":"a1","prompt":"Research only"}
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url='http://worker',headers=headers) as client:
            one=await client.post('/internal/v1/role',json=payload);assert one.status_code==200,one.text
            two=await client.post('/internal/v1/role',json=payload);assert one.json()==two.json()
            assert one.json()['item']=='p:1' and one.json()['role']=='cto' and one.json()['attempt_id']=='a1' and one.json()['generation']==1
            assert len(calls)==1 and calls[0]['plantool']['attempt_id']=='a1'

@pytest.mark.asyncio
@pytest.mark.parametrize("failure", ["http", "invalid_json"])
async def test_role_errors_preserve_safe_cause_without_leaking_prompt(tmp_path, capsys, failure):
    async def daemon(request):
        if request.url.path.endswith('/health'):return httpx.Response(200,json=VERSIONS)
        if failure=="http":return httpx.Response(429,json={"error":"SECRET-PROVIDER-BODY"})
        return httpx.Response(200,json={"choices":[{"message":{"content":"SECRET-INVALID-OUTPUT"}}]})
    token='t'*40;app=create_app(tmp_path,token,httpx.MockTransport(daemon))
    payload={"request":{"item":"p:1","role":"pm","generation":1,"inputs":{"step":"triage"},"plan_sha":"","policy_version":1,"capability":"read","idempotency_key":"d","thread_id":"p:1","interrupt_id":"i"},"attempt_id":"a1","prompt":"SECRET-PROMPT"}
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url='http://worker',headers={"x-plantool-worker-token":token,"x-plantool-contract-version":"5"}) as client:
            reply=await client.post('/internal/v1/role',json=payload)
            assert reply.status_code==502
            assert reply.json()['detail']['kind']==('upstream_http' if failure=='http' else 'invalid_role_result')
            assert (await client.get('/internal/v1/health')).status_code==200
    output=capsys.readouterr().err+reply.text
    assert 'SECRET' not in output and token not in output
    assert '429' in output if failure=='http' else 'JSONDecodeError' in output

@pytest.mark.asyncio
@pytest.mark.parametrize('status',[404,429,500])
async def test_upstream_failures_are_not_hidden_as_generic_500(tmp_path,status):
    async def daemon(request):
        if request.url.path.endswith('health'):return httpx.Response(200,json=VERSIONS)
        return httpx.Response(status,json={'error':{'message':'private provider details'}})
    app=create_app(tmp_path,'t'*40,httpx.MockTransport(daemon))
    payload={"request":{"item":"p:1","role":"pm","generation":1,"inputs":{"step":"triage"},"plan_sha":"","policy_version":1,"capability":"read","idempotency_key":"d","thread_id":"p:1","interrupt_id":"i"},"attempt_id":"a1","prompt":"triage"}
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url='http://worker',headers={'x-plantool-worker-token':'t'*40,'x-plantool-contract-version':'5'}) as client:
            response=await client.post('/internal/v1/role',json=payload)
            assert response.status_code==502
            assert response.json()['detail']['upstream_status']==status
            assert 'private provider details' not in response.text

@pytest.mark.asyncio
@pytest.mark.parametrize('project',[False,True])
async def test_failed_read_role_fallback_creates_fresh_generation_interrupt(tmp_path,project):
    step='sprint_plan' if project else 'triage'
    facts={'item':'p:1','step':step,'project':project,'sequence':0,'generation':0,'plan_sha':'','frozen':False}
    original=dict(facts);requests=[]
    async def daemon(request):
        path=request.url.path
        if path.endswith('health'):return httpx.Response(200,json=VERSIONS)
        if path.endswith('workflow_context'):return httpx.Response(200,json={'state':facts,'policy_version':1})
        if path.endswith('request_dispatch'):
            data=json.loads(request.content)
            existing=next((r for r in requests if r['idempotency_key']==data['idempotency_key']),None)
            if existing is None: requests.append(data)
            else: assert existing==data
            return httpx.Response(200,json={'attempt_id':'a'+str(data['generation']),'generation':data['generation']})
        raise AssertionError(path)
    app=create_app(tmp_path,'t'*40,httpx.MockTransport(daemon))
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url='http://worker',headers={'x-plantool-worker-token':'t'*40,'x-plantool-contract-version':'5'}) as client:
            first=await client.post('/internal/v1/start',json={'thread_id':'p:1','graph':'project' if project else 'work_item','input':{'workflow':original},'idempotency_key':'start',**VERSIONS})
            assert first.status_code==200,first.text
            facts.update(sequence=1,generation=1,route_retry=True)
            failed={'item':'p:1','role':'pm','attempt_id':'a1','generation':1,'outcome':'failed','summary':'upstream 429','tasks_done':[],'artifacts':[],'checks':[],'findings':[],'next':None,'review_guide':None,'details':None}
            reply=await client.post('/internal/v1/resume',json={'thread_id':'p:1','interrupt_id':first.json()['interrupt_id'],'value':failed,'idempotency_key':'resume',**VERSIONS})
            assert reply.status_code==200,reply.text
            assert reply.json()['state']=='interrupted'
            assert [r['generation'] for r in requests]==[1,2]
            assert requests[0]['interrupt_id']!=requests[1]['interrupt_id']

@pytest.mark.asyncio
@pytest.mark.parametrize('first', ['prose','fenced','wrong_details','wrong_shape'])
async def test_provider_format_correction_is_bounded_validated_and_cached(tmp_path,first):
    calls=[]
    good={"item":"forged","role":"pm","attempt_id":"wrong","generation":999,"outcome":"done","summary":"fits","tasks_done":[],"artifacts":[],"checks":[],"findings":[],"details":{"kind":"triage","disposition":"fits","reason":"Small docs task","duplicate_of":None}}
    async def daemon(request):
        if request.url.path.endswith('/health'):return httpx.Response(200,json=VERSIONS)
        body=json.loads(request.content);calls.append(body)
        assert 'JSON Schema' in body['messages'][0]['content']
        if len(calls)==1:
            content='I recommend this task.' if first=='prose' else '```json\n'+json.dumps(good)+'\n```' if first=='fenced' else json.dumps({**good,'details':[] if first=='wrong_shape' else None})
        else:
            assert body['tools']==[]
            content=json.dumps(good)
        return httpx.Response(200,json={'choices':[{'message':{'content':content},'finish_reason':'stop'}]})
    app=create_app(tmp_path,'t'*40,httpx.MockTransport(daemon))
    payload={'request':{'item':'p:1','role':'pm','generation':1,'inputs':{'step':'triage'},'plan_sha':'','policy_version':1,'capability':'read','idempotency_key':'d','thread_id':'p:1','interrupt_id':'i'},'attempt_id':'a','prompt':'Triage only'}
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url='http://worker',headers={'x-plantool-worker-token':'t'*40,'x-plantool-contract-version':'5'}) as client:
            response=await client.post('/internal/v1/role',json=payload)
            assert response.status_code==200,response.text
            assert response.json()['details']['kind']=='triage'
            assert (await client.post('/internal/v1/role',json=payload)).json()==response.json()
            assert len(calls)==(1 if first=='fenced' else 2)

@pytest.mark.asyncio
async def test_empty_prepared_sprint_does_not_invent_items_or_call_provider(tmp_path):
    async def daemon(request):
        assert request.url.path.endswith('/health')
        return httpx.Response(200,json=VERSIONS)
    app=create_app(tmp_path,'t'*40,httpx.MockTransport(daemon))
    payload={'request':{'item':'p:project','role':'pm','generation':1,'inputs':{'step':'sprint_plan','prepared_candidates':[]},'plan_sha':'','policy_version':1,'capability':'read','idempotency_key':'d','thread_id':'p:project','interrupt_id':'i'},'attempt_id':'a','prompt':'Known issue #1 is frozen'}
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url='http://worker',headers={'x-plantool-worker-token':'t'*40,'x-plantool-contract-version':'5'}) as client:
            response=await client.post('/internal/v1/role',json=payload)
            assert response.status_code==200,response.text
            assert response.json()['details']['items']==[]

@pytest.mark.asyncio
async def test_unfixable_output_stops_after_one_formatting_retry(tmp_path):
    calls=[]
    async def daemon(request):
        if request.url.path.endswith('/health'):return httpx.Response(200,json=VERSIONS)
        calls.append(json.loads(request.content))
        return httpx.Response(200,json={'choices':[{'message':{'content':'Not JSON'},'finish_reason':'length'}]})
    app=create_app(tmp_path,'t'*40,httpx.MockTransport(daemon))
    payload={'request':{'item':'p:1','role':'pm','generation':1,'inputs':{'step':'triage'},'plan_sha':'','policy_version':1,'capability':'read','idempotency_key':'d','thread_id':'p:1','interrupt_id':'i'},'attempt_id':'a','prompt':'Triage only'}
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url='http://worker',headers={'x-plantool-worker-token':'t'*40,'x-plantool-contract-version':'5'}) as client:
            response=await client.post('/internal/v1/role',json=payload)
            assert response.status_code==502
            assert len(calls)==2
            assert calls[0]['plantool']['idempotency_key']!=calls[1]['plantool']['idempotency_key']
            assert (await client.get('/internal/v1/health')).status_code==200
