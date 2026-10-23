"""Authenticated Unix-socket bridge. Graphs deliberately stop at a Phase 2 interrupt."""
from __future__ import annotations
import asyncio
from contextlib import asynccontextmanager
import hashlib
import hmac
import json
import os
from pathlib import Path
import sqlite3
from typing import Any, TypedDict
from uuid import uuid4
import httpx
from fastapi import FastAPI, HTTPException, Request
from langgraph.checkpoint.sqlite.aio import AsyncSqliteSaver
from langgraph.graph import StateGraph, START, END
from langgraph.types import Command, interrupt
from .models import Start, Resume, Cancel, Dispatch

VERSIONS = json.loads(Path(__file__).with_name("versions.json").read_text())

class State(TypedDict, total=False):
    input: dict[str, Any]
    result: Any

class Store:
    def __init__(self, path: Path):
        self.path = path
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        with self.db() as db:
            db.executescript("""
            CREATE TABLE IF NOT EXISTS bridge_threads(id TEXT PRIMARY KEY, graph_version INTEGER NOT NULL, state TEXT NOT NULL, interrupt_id TEXT, result TEXT);
            CREATE TABLE IF NOT EXISTS bridge_inbox(id TEXT PRIMARY KEY, digest TEXT NOT NULL, result TEXT);
            """)
    def db(self):
        db = sqlite3.connect(self.path, timeout=5)
        db.execute("PRAGMA journal_mode=WAL")
        return db
    def thread(self, name):
        with self.db() as db:
            row = db.execute("SELECT graph_version,state,interrupt_id,result FROM bridge_threads WHERE id=?", (name,)).fetchone()
        if row and row[0] != VERSIONS["graph_version"]:
            raise HTTPException(409, "Checkpoint graph_version requires a migration; refusing resume")
        if not row:
            with self.db() as db:
                exists = db.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name='checkpoints'").fetchone()
                if exists and db.execute("SELECT 1 FROM checkpoints WHERE thread_id=? LIMIT 1",(name,)).fetchone():
                    raise HTTPException(409,"Checkpoint has no graph_version; migration required")
        return None if not row else {"thread_id":name,"graph_version":row[0],"state":row[1],"interrupt_id":row[2],"result":json.loads(row[3]) if row[3] else None}
    def receipt(self, key, body):
        digest = hashlib.sha256(json.dumps(body, sort_keys=True).encode()).hexdigest()
        with self.db() as db:
            row = db.execute("SELECT digest,result FROM bridge_inbox WHERE id=?", (key,)).fetchone()
            if row and row[0] != digest:
                raise HTTPException(409, "Idempotency key reused for different input")
            db.execute("INSERT OR IGNORE INTO bridge_inbox(id,digest) VALUES(?,?)", (key,digest))
        return json.loads(row[1]) if row and row[1] else None
    def save(self, thread, state, interrupt_id, result, key):
        value = {"thread_id":thread,"graph_version":VERSIONS["graph_version"],"state":state,"interrupt_id":interrupt_id,"result":result}
        with self.db() as db:
            db.execute("INSERT INTO bridge_threads VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET state=excluded.state,interrupt_id=excluded.interrupt_id,result=excluded.result", (thread,VERSIONS["graph_version"],state,interrupt_id,json.dumps(result)))
            db.execute("UPDATE bridge_inbox SET result=? WHERE id=?", (json.dumps(value),key))
        return value

def create_app(home: Path, token: str, transport=None):
    locks: dict[str, asyncio.Lock] = {}
    store = Store(home/"worker-state/checkpoints.db")
    @asynccontextmanager
    async def lifespan(app):
        async with httpx.AsyncClient(transport=transport or httpx.AsyncHTTPTransport(uds=str(home/"run/daemon.sock")), base_url="http://localhost", headers={"x-plantool-run-token":token,"x-plantool-contract-version":str(VERSIONS["contract_version"])}, timeout=60) as daemon:
            # Both processes must agree before a checkpoint or model is touched.
            health = await daemon.get("/internal/v1/health")
            health.raise_for_status()
            if health.json() != VERSIONS:
                raise RuntimeError("Daemon contract_version/graph_version mismatch")
            async with AsyncSqliteSaver.from_conn_string(str(store.path)) as saver:
                async def bridge(state: State):
                    payload = state.get("input", {})
                    dispatched = None
                    if payload.get("dispatch"):
                        request = Dispatch.model_validate(payload["dispatch"])
                        response = await daemon.post("/internal/v1/request_dispatch", json=request.model_dump(mode="json"))
                        response.raise_for_status()
                        dispatched = response.json()
                    # The Phase 3 role graph replaces this interrupt. No model call,
                    # GitHub action or implied approval occurs in this stub.
                    return {"result":interrupt({"phase":2,"dispatch":dispatched})}
                graph = StateGraph(State)
                graph.add_node("bridge",bridge)
                graph.add_edge(START,"bridge")
                graph.add_edge("bridge",END)
                app.state.graph = graph.compile(checkpointer=saver)
                yield
    app = FastAPI(lifespan=lifespan, docs_url=None, redoc_url=None, openapi_url=None)
    @app.middleware("http")
    async def authenticate(request: Request, next):
        from starlette.responses import JSONResponse
        if not token or not hmac.compare_digest(request.headers.get("x-plantool-worker-token", ""), token):
            return JSONResponse({"detail":"Worker credential required"},status_code=403)
        if request.headers.get("x-plantool-contract-version") != str(VERSIONS["contract_version"]):
            return JSONResponse({"detail":"Contract version mismatch"},status_code=409)
        return await next(request)
    def versions(request):
        if request.contract_version != VERSIONS["contract_version"] or request.graph_version != VERSIONS["graph_version"]:
            raise HTTPException(409, "Contract/graph version mismatch")
    def lock(thread):
        if not thread or len(thread)>256 or any(c in thread for c in ("\x00","\n","\r")):
            raise HTTPException(400,"Invalid thread_id")
        return locks.setdefault(thread,asyncio.Lock())
    @app.get("/internal/v1/health")
    async def health(): return VERSIONS
    @app.get("/internal/v1/status/{thread:path}")
    async def status(thread: str):
        value = store.thread(thread)
        if value is None: raise HTTPException(404,"Unknown thread")
        return value
    @app.post("/internal/v1/start")
    async def start(request: Start):
        versions(request)
        async with lock(request.thread_id):
            receipt = store.receipt(request.idempotency_key,request.model_dump(mode="json"))
            if receipt is not None: return receipt
            current = store.thread(request.thread_id)
            if current and current["state"] == "starting":
                return store.save(request.thread_id,"recovery_needed",None,{"error":"Start was interrupted before its checkpoint was acknowledged"},request.idempotency_key)
            if current: return store.save(request.thread_id,current["state"],current["interrupt_id"],current["result"],request.idempotency_key)
            identifier = str(uuid4())
            payload = dict(request.input or {})
            if payload.get("dispatch"):
                payload["dispatch"] = {**payload["dispatch"],"thread_id":request.thread_id,"interrupt_id":identifier}
            # Record identity before dispatch; a retry replays the same request.
            with store.db() as db:
                db.execute("INSERT INTO bridge_threads VALUES(?,?,'starting',?,NULL)",(request.thread_id,VERSIONS["graph_version"],identifier))
            await app.state.graph.ainvoke({"input":payload},{"configurable":{"thread_id":request.thread_id}})
            return store.save(request.thread_id,"interrupted",identifier,None,request.idempotency_key)
    @app.post("/internal/v1/resume")
    async def resume(request: Resume):
        versions(request)
        async with lock(request.thread_id):
            receipt = store.receipt(request.idempotency_key,request.model_dump(mode="json"))
            if receipt is not None: return receipt
            current = store.thread(request.thread_id)
            if not current or current["state"] != "interrupted" or current["interrupt_id"] != request.interrupt_id:
                raise HTTPException(409,"Stale interrupt or cancelled graph")
            snapshot = await app.state.graph.aget_state({"configurable":{"thread_id":request.thread_id}})
            if not snapshot.next and "result" in snapshot.values:
                return store.save(request.thread_id,"completed",None,snapshot.values["result"],request.idempotency_key)
            result = await app.state.graph.ainvoke(Command(resume=request.value),{"configurable":{"thread_id":request.thread_id}})
            return store.save(request.thread_id,"completed",None,result.get("result"),request.idempotency_key)
    @app.post("/internal/v1/cancel")
    async def cancel(request: Cancel):
        async with lock(request.thread_id):
            receipt = store.receipt(request.idempotency_key,request.model_dump(mode="json"))
            if receipt is not None: return receipt
            store.thread(request.thread_id)
            return store.save(request.thread_id,"cancelled",None,None,request.idempotency_key)
    return app

# Token is read from the inherited descriptor, never argv or an environment value.
def runtime_app():
    fd = int(os.environ["PLANTOOL_WORKER_TOKEN_FD"])
    with os.fdopen(fd, "r") as stream: token = stream.readline(1024).strip()
    if len(token)<32: raise RuntimeError("Missing inherited worker credential")
    return create_app(Path(os.environ["PLANTOOL_HOME"]),token)
