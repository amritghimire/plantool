"""Real process death/restart with a fake daemon and no provider calls."""
import asyncio
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import httpx
import pytest
from plantool_worker.app import VERSIONS

SCRIPT = r'''
import json,os,sqlite3,sys
from pathlib import Path
import httpx,uvicorn
from plantool_worker.app import create_app,VERSIONS
home=Path(sys.argv[1]);token=os.fdopen(0).readline().strip()
async def daemon(request):
    if request.url.path.endswith("health"):return httpx.Response(200,json=VERSIONS)
    body=json.loads(request.content)
    with sqlite3.connect(home/"fake-effects.db") as db:
        db.execute("CREATE TABLE IF NOT EXISTS effects(id TEXT PRIMARY KEY)")
        db.execute("INSERT OR IGNORE INTO effects VALUES(?)",(body["idempotency_key"],))
    return httpx.Response(200,json={"attempt_id":"a1","generation":1})
uvicorn.run(create_app(home,token,httpx.MockTransport(daemon)),uds=sys.argv[2],log_level="error")
'''

@pytest.mark.asyncio
async def test_killed_worker_resumes_checkpoint_once_and_never_repeats_dispatch_effect(tmp_path):
    token="fake-worker-token-"*4
    socket=tmp_path/"worker.sock"
    headers={"x-plantool-worker-token":token,"x-plantool-contract-version":"1"}
    processes=[]
    def launch():
        if socket.exists():socket.unlink()
        env={"PATH":str(Path(sys.executable).parent),"PYTHONPATH":str(Path(__file__).parents[1]),"HOME":str(tmp_path),"PLANTOOL_WORKER_TOKEN_FD":"0"}
        assert all(token not in value for value in env.values())
        process=subprocess.Popen([sys.executable,"-c",SCRIPT,str(tmp_path),str(socket)],stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,env=env)
        processes.append(process)
        process.stdin.write((token+"\n").encode());process.stdin.close()
        return process
    async def ready():
        for _ in range(100):
            try:
                async with httpx.AsyncClient(transport=httpx.AsyncHTTPTransport(uds=str(socket)),base_url="http://localhost",headers=headers) as client:
                    if (await client.get("/internal/v1/health")).status_code==200:return
            except httpx.TransportError:pass
            await asyncio.sleep(.03)
        pytest.fail("Fake worker did not start")
    try:
        process=launch();await ready()
        async with httpx.AsyncClient(transport=httpx.AsyncHTTPTransport(uds=str(socket)),base_url="http://localhost",headers=headers) as client:
            dispatch={"item":"p/i","role":"backend","generation":1,"inputs":{},"plan_sha":"a"*64,"policy_version":0,"capability":"write","idempotency_key":"one-dispatch","thread_id":"p:i","interrupt_id":"pending"}
            start={"thread_id":"p:i","graph":"work_item","input":{"dispatch":dispatch},"idempotency_key":"start",**VERSIONS}
            response=await client.post("/internal/v1/start",json=start);assert response.status_code==200
            first=response.json();assert first["state"]=="interrupted"
        process.kill();process.wait(timeout=5)
        launch();await ready()
        async with httpx.AsyncClient(transport=httpx.AsyncHTTPTransport(uds=str(socket)),base_url="http://localhost",headers=headers) as client:
            resume={"thread_id":"p:i","interrupt_id":first["interrupt_id"],"value":{"outcome":"done"},"idempotency_key":"resume",**VERSIONS}
            response=await client.post("/internal/v1/resume",json=resume);assert response.status_code==200
            done=response.json();assert done["state"]=="completed"
            assert (await client.post("/internal/v1/resume",json=resume)).json()==done
        with sqlite3.connect(tmp_path/"fake-effects.db") as db:assert db.execute("SELECT count(*) FROM effects").fetchone()[0]==1
    finally:
        for process in processes:
            if process.poll() is None:process.kill()
            process.wait(timeout=5)
