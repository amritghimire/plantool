import json
import httpx
import pytest
from plantool_worker.app import create_app, VERSIONS, Store

async def fake_daemon(request):
    if request.url.path.endswith("health"): return httpx.Response(200,json=VERSIONS)
    return httpx.Response(200,json={"attempt_id":"a1","generation":1})

@pytest.mark.asyncio
async def test_version_auth_replay_restart_and_stale_checkpoint(tmp_path):
    token="test-token-"*5
    app=create_app(tmp_path,token,httpx.MockTransport(fake_daemon))
    headers={"x-plantool-worker-token":token,"x-plantool-contract-version":"5"}
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url="http://localhost",headers=headers) as client:
            assert (await client.get("/internal/v1/health",headers={"x-plantool-worker-token":"forged"})).status_code==403
            assert (await client.get("/internal/v1/health",headers={"x-plantool-contract-version":"99"})).status_code==409
            start={"thread_id":"p:i","graph":"work_item","input":{},"idempotency_key":"start","contract_version":5,"graph_version":4}
            first=(await client.post("/internal/v1/start",json=start)).json()
            assert first["state"]=="interrupted"
            assert (await client.post("/internal/v1/start",json=start)).json()==first
            resume={"thread_id":"p:i","interrupt_id":first["interrupt_id"],"value":{"outcome":"done"},"idempotency_key":"resume","contract_version":5,"graph_version":4}
            done=(await client.post("/internal/v1/resume",json=resume)).json()
            assert done["state"]=="completed"
            assert (await client.post("/internal/v1/resume",json=resume)).json()==done
            assert (await client.post("/internal/v1/resume",json={**resume,"idempotency_key":"stale"})).status_code==409
    restarted=create_app(tmp_path,token,httpx.MockTransport(fake_daemon))
    async with restarted.router.lifespan_context(restarted):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=restarted),base_url="http://localhost",headers=headers) as client:
            assert (await client.post("/internal/v1/resume",json=resume)).json()==done
            with Store(tmp_path/"worker-state/checkpoints.db").db() as db: db.execute("UPDATE bridge_threads SET graph_version=0")
            assert (await client.get("/internal/v1/status/p:i")).status_code==409

@pytest.mark.asyncio
async def test_recovery_uses_new_checkpoint_epoch_preserves_old_and_replays(tmp_path):
    async def daemon(request):
        if request.url.path.endswith('health'):return httpx.Response(200,json=VERSIONS)
        return httpx.Response(200,json={"state":{},"policy_version":1})
    token='t'*40;app=create_app(tmp_path,token,httpx.MockTransport(daemon))
    headers={"x-plantool-worker-token":token,"x-plantool-contract-version":"5"}
    async with app.router.lifespan_context(app):
        async with httpx.AsyncClient(transport=httpx.ASGITransport(app=app),base_url='http://worker',headers=headers) as client:
            body={"thread_id":"p:1","graph":"work_item","input":{},"idempotency_key":"original",**VERSIONS}
            first=(await client.post('/internal/v1/start',json=body)).json()
            recovered={**body,"idempotency_key":"recovery-1","input":{"workflow":{}}}
            reply=await client.post('/internal/v1/recover',json=recovered)
            assert reply.status_code==200,reply.text
            second=reply.json();assert second['interrupt_id']!=first['interrupt_id']
            assert (await client.post('/internal/v1/recover',json=recovered)).json()==second
            store=Store(tmp_path/'worker-state/checkpoints.db')
            with store.db() as db:
                assert db.execute('SELECT count(*) FROM bridge_history').fetchone()[0]==1
                assert db.execute('SELECT count(DISTINCT thread_id) FROM checkpoints').fetchone()[0]==2
            stale={"thread_id":"p:1","interrupt_id":first['interrupt_id'],"value":{},"idempotency_key":"stale",**VERSIONS}
            assert (await client.post('/internal/v1/resume',json=stale)).status_code==409
