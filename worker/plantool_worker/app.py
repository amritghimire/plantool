"""Authenticated Unix-socket bridge. Graphs deliberately stop at a Phase 2 interrupt."""
from __future__ import annotations
import asyncio
from contextlib import asynccontextmanager
import hashlib
import hmac
import json
import os
import sys
import traceback
from pathlib import Path
import sqlite3
import ssl
from typing import Any, TypedDict
from uuid import uuid4
import httpx
from fastapi import FastAPI, HTTPException, Request
from langgraph.checkpoint.sqlite.aio import AsyncSqliteSaver
from langgraph.graph import StateGraph, START, END
from langgraph.types import Command, interrupt
from .models import Start, Resume, Cancel, Dispatch, RoleCall, RoleResult

VERSIONS = json.loads(Path(__file__).with_name("versions.json").read_text())

class State(TypedDict, total=False):
    input: dict[str, Any]
    result: Any
    workflow: dict[str, Any]

class Store:
    def __init__(self, path: Path):
        self.path = path
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        path.parent.chmod(0o700)
        with self.db() as db:
            db.executescript("""
            CREATE TABLE IF NOT EXISTS bridge_threads(id TEXT PRIMARY KEY, graph_version INTEGER NOT NULL, state TEXT NOT NULL, interrupt_id TEXT, result TEXT);
            CREATE TABLE IF NOT EXISTS bridge_epochs(id TEXT PRIMARY KEY, epoch TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS bridge_history(id TEXT, epoch TEXT, snapshot TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS bridge_inbox(id TEXT PRIMARY KEY, digest TEXT NOT NULL, result TEXT);
            """)
    def db(self):
        db = sqlite3.connect(self.path, timeout=5)
        self.path.chmod(0o600)
        db.execute("PRAGMA journal_mode=WAL")
        return db
    def config(self, name):
        with self.db() as db:
            row = db.execute("SELECT epoch FROM bridge_epochs WHERE id=?", (name,)).fetchone()
        return {"configurable":{"thread_id":name if not row else name+"~"+row[0]}}
    def thread(self, name):
        with self.db() as db:
            row = db.execute("SELECT graph_version,state,interrupt_id,result FROM bridge_threads WHERE id=?", (name,)).fetchone()
        if row and row[0] != VERSIONS["graph_version"]:
            raise HTTPException(409, "Checkpoint graph_version requires a migration; refusing resume")
        if not row:
            with self.db() as db:
                exists = db.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name='checkpoints'").fetchone()
                if exists and db.execute("SELECT 1 FROM checkpoints WHERE thread_id=? LIMIT 1",(self.config(name)["configurable"]["thread_id"],)).fetchone():
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

DETAIL_KINDS={"triage":"triage","scan_devops":"scan","scan_qa":"scan","scan_pm":"routes","route_finding":"routes","research":"research","plan":"plan","reviewer":"security","docs":"docs","supervisor":"strategy","discussion":"discussion","sprint_plan":"sprint"}

def parse_role_content(content):
    """Accept a complete JSON object, optionally wrapped in one Markdown fence.

    Never extract a guessed object from prose or treat reasoning as a result.
    """
    if not isinstance(content,str):
        raise ValueError("Role response must contain JSON text")
    text=content.strip()
    if text.startswith("```json\n") or text.startswith("```\n"):
        lines=text.splitlines()
        if lines[-1].strip()!="```":raise ValueError("Incomplete JSON fence")
        text="\n".join(lines[1:-1])
    data=json.loads(text)
    if not isinstance(data,dict):raise ValueError("Role result must be an object")
    return data

def create_app(home: Path, token: str, transport=None):
    locks: dict[str, asyncio.Lock] = {}
    store = Store(home/"worker-state/checkpoints.db")
    @asynccontextmanager
    async def lifespan(app):
        # This client only speaks HTTP over the daemon Unix socket. HTTPX's default
        # SSL setup reads certifi/cacert.pem even for HTTP, which the worker sandbox
        # correctly denies. A strict context avoids that unused CA-file read without
        # disabling certificate verification. Ignore proxy/CA environment settings.
        daemon_transport = transport if transport is not None else httpx.AsyncHTTPTransport(
            uds=str(home/"run/daemon.sock"),
            verify=ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT),
            trust_env=False,
        )
        async with httpx.AsyncClient(transport=daemon_transport, trust_env=False, base_url="http://localhost", headers={"x-plantool-run-token":token,"x-plantool-contract-version":str(VERSIONS["contract_version"])}, timeout=httpx.Timeout(600, connect=5, pool=5)) as daemon:
            # Both processes must agree before a checkpoint or model is touched.
            health = await daemon.get("/internal/v1/health")
            health.raise_for_status()
            if health.json() != VERSIONS:
                raise RuntimeError("Daemon contract_version/graph_version mismatch")
            async with AsyncSqliteSaver.from_conn_string(str(store.path)) as saver:
                async def bridge(state: State):
                    if state.get("workflow",{}).get("project"):
                        from .project import build_step
                        return await build_step(daemon,store,state)
                    if state.get("workflow"):
                        from .work_item import build_step
                        return await build_step(daemon,store,state)
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
                graph.add_conditional_edges("bridge",lambda s: "bridge" if s.get("workflow") and s["workflow"]["step"] not in ("merged","closed","escalated","owner_question","prerequisite_wait","loop_complete","declined_closed") else END)
                app.state.graph = graph.compile(checkpointer=saver)
                app.state.daemon = daemon
                yield
    app = FastAPI(lifespan=lifespan, docs_url=None, redoc_url=None, openapi_url=None)
    @app.exception_handler(httpx.HTTPStatusError)
    async def broker_error(request, error):
        from starlette.responses import JSONResponse
        status = error.response.status_code
        # A broker rejection belongs to this graph, never the worker installation.
        reason = "Daemon rejected this graph operation; inspect workflow state"
        # Only expose known diagnostic phrases, never arbitrary response contents.
        for known in ("Item is frozen", "Stale dispatch generation", "Role capability mismatch",
                      "Graph step/role mismatch", "Stale policy version", "Another attempt is active",
                      "Dependencies are incomplete", "Approved immutable plan required"):
            if known in error.response.text:
                reason = known
                break
        return JSONResponse(status_code=409 if status in (400,403,404,409,422) else 503,
            content={"detail":{"kind":"daemon_rejected" if status < 500 else "daemon_unavailable",
                "upstream_status":status,"operation":error.request.url.path,"reason":reason}})
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
            receipt = await asyncio.to_thread(store.receipt,request.idempotency_key,request.model_dump(mode="json"))
            if receipt is not None: return receipt
            current = store.thread(request.thread_id)
            if current and current["state"] == "starting":
                return await asyncio.to_thread(store.save,request.thread_id,"recovery_needed",None,{"error":"Start was interrupted before its checkpoint was acknowledged"},request.idempotency_key)
            if current: return await asyncio.to_thread(store.save,request.thread_id,current["state"],current["interrupt_id"],current["result"],request.idempotency_key)
            identifier = str(uuid4())
            payload = dict(request.input or {})
            if payload.get("dispatch"):
                payload["dispatch"] = {**payload["dispatch"],"thread_id":request.thread_id,"interrupt_id":identifier}
            # Record identity before dispatch; a retry replays the same request.
            def record_start():
                with store.db() as db:db.execute("INSERT INTO bridge_threads VALUES(?,?,'starting',?,NULL)",(request.thread_id,VERSIONS["graph_version"],identifier))
            await asyncio.to_thread(record_start)
            initial={"input":payload}
            if payload.get("workflow"):
                context=await app.state.daemon.post("/internal/v1/workflow_context",json={"item":request.thread_id});context.raise_for_status();facts=context.json()
                initial["workflow"]={**payload["workflow"],"policy_version":facts["policy_version"]}
            output=await app.state.graph.ainvoke(initial,store.config(request.thread_id))
            identifier=store.thread(request.thread_id)["interrupt_id"]
            return await asyncio.to_thread(store.save,request.thread_id,"interrupted",identifier,output.get("result"),request.idempotency_key)
    @app.post("/internal/v1/resume")
    async def resume(request: Resume):
        versions(request)
        async with lock(request.thread_id):
            receipt = await asyncio.to_thread(store.receipt,request.idempotency_key,request.model_dump(mode="json"))
            if receipt is not None: return receipt
            current = store.thread(request.thread_id)
            if current and current["state"] == "cancelled":
                # Cancellation only changed bridge metadata; the durable graph interrupt
                # still exists. Reattach only the exact original interrupt, under current
                # daemon authority. This preserves existing gate IDs and owner decisions.
                snapshot = await app.state.graph.aget_state(store.config(request.thread_id))
                exact = bool(snapshot.next) and any(
                    isinstance(signal.value, dict) and signal.value.get("interrupt_id") == request.interrupt_id
                    for task in snapshot.tasks for signal in task.interrupts
                )
                if exact and snapshot.values.get("workflow"):
                    response = await app.state.daemon.post("/internal/v1/workflow_context",json={"item":request.thread_id})
                    response.raise_for_status(); facts = response.json(); workflow = facts["state"]
                    if facts.get("execution_enabled") is True and not workflow.get("frozen") and not workflow.get("checkpoint_uncertain") and not workflow.get("closed_on_github") and workflow.get("step") not in ("closed","merged","escalated","declined_closed","loop_complete"):
                        def reattach():
                            with store.db() as db: db.execute("UPDATE bridge_threads SET state='interrupted',interrupt_id=? WHERE id=? AND state='cancelled'",(request.interrupt_id,request.thread_id))
                        await asyncio.to_thread(reattach)
                        current = store.thread(request.thread_id)
            if not current or current["state"] != "interrupted" or current["interrupt_id"] != request.interrupt_id:
                raise HTTPException(409,"Stale interrupt or cancelled graph")
            snapshot = await app.state.graph.aget_state(store.config(request.thread_id))
            if not snapshot.next and "result" in snapshot.values:
                return await asyncio.to_thread(store.save,request.thread_id,"completed",None,snapshot.values["result"],request.idempotency_key)
            result = await app.state.graph.ainvoke(Command(resume=request.value),store.config(request.thread_id))
            snapshot=await app.state.graph.aget_state(store.config(request.thread_id))
            waiting=bool(snapshot.next)
            identifier=store.thread(request.thread_id)["interrupt_id"] if waiting else None
            return await asyncio.to_thread(store.save,request.thread_id,"interrupted" if waiting else "completed",identifier,result.get("result"),request.idempotency_key)
    @app.post("/internal/v1/recover")
    async def recover(request: Start):
        versions(request)
        receipt = await asyncio.to_thread(store.receipt,request.idempotency_key,request.model_dump(mode="json"))
        if receipt is not None: return receipt
        with store.db() as db:
            epoch = db.execute("SELECT epoch FROM bridge_epochs WHERE id=?", (request.thread_id,)).fetchone()
        if epoch and epoch[0] == request.idempotency_key:
            return await start(request)
        facts = await app.state.daemon.post("/internal/v1/workflow_context", json={"item":request.thread_id})
        facts.raise_for_status()
        current = facts.json()["state"]
        requested = dict(request.input.get("workflow") or {})
        current.pop("head_sha",None); requested.pop("head_sha",None)
        if current.get("frozen") or current != requested:
            raise HTTPException(409, "Recovery scope changed")
        async with lock(request.thread_id):
            with store.db() as db:
                prior = db.execute("SELECT epoch FROM bridge_epochs WHERE id=?", (request.thread_id,)).fetchone()
                if not prior or prior[0] != request.idempotency_key:
                    row = db.execute("SELECT * FROM bridge_threads WHERE id=?", (request.thread_id,)).fetchone()
                    if row:
                        db.execute("INSERT INTO bridge_history VALUES(?,?,?)", (request.thread_id,prior[0] if prior else "original",json.dumps(row)))
                    db.execute("INSERT INTO bridge_epochs VALUES(?,?) ON CONFLICT(id) DO UPDATE SET epoch=excluded.epoch", (request.thread_id,request.idempotency_key))
                    db.execute("DELETE FROM bridge_threads WHERE id=?", (request.thread_id,))
        return await start(request)
    @app.post("/internal/v1/role")
    async def role(request: RoleCall):
        try:
            return await invoke_role(request)
        except HTTPException:
            raise
        except Exception as error:
            status = error.response.status_code if isinstance(error, httpx.HTTPStatusError) else None
            kind = "upstream_http" if status else "invalid_role_result" if isinstance(error, (ValueError, KeyError, IndexError, TypeError)) else "worker_internal"
            detail = {"kind":kind,"upstream_status":status,"exception":type(error).__name__}
            # Do not log str(error), locals, provider response bodies or prompts.
            detail["frames"] = [{"file":Path(f.filename).name,"line":f.lineno,"function":f.name} for f in traceback.extract_tb(error.__traceback__)]
            print(json.dumps({"attempt":request.attempt_id,**detail}),file=sys.stderr,flush=True)
            raise HTTPException(502, detail={k:v for k,v in detail.items() if k != "frames"}) from None
    async def invoke_role(request: RoleCall):
        key="role:"+request.attempt_id
        async with lock(key):
            receipt=await asyncio.to_thread(store.receipt,key,request.model_dump(mode="json"))
            if receipt is not None: return receipt
            expected_kind=DETAIL_KINDS.get(request.request.inputs.get("step"))
            candidates=request.request.inputs.get("prepared_candidates") if request.request.inputs.get("step")=="sprint_plan" else None
            if candidates==[]:
                result=RoleResult.model_validate({"item":request.request.item,"role":request.request.role,"attempt_id":request.attempt_id,"generation":request.request.generation,"outcome":"done","summary":"No prepared sprint candidates; waiting for item plans.","tasks_done":[],"artifacts":[],"checks":[],"findings":[],"details":{"kind":"sprint","items":[],"notes":"No eligible prepared plans. Frozen or unplanned issues remain outside this sprint; continue their own planning/recovery separately."}}).model_dump(mode="json")
                with store.db() as db:db.execute("UPDATE bridge_inbox SET result=? WHERE id=?",(json.dumps(result),key))
                return result
            daemon=app.state.daemon
            messages=[{"role":"system","content":"Use supplied tools when needed. Your final answer must be one RoleResult JSON object, without prose or Markdown. Never approve or publish. The complete JSON Schema follows: " + json.dumps(RoleResult.model_json_schema())},{"role":"user","content":request.prompt}]
            if expected_kind:
                messages.append({"role":"system","content":f"This step requires non-null details with kind={expected_kind}. Use that schema variant, not another role variant. Return all required fields for it."})
            if candidates is not None:
                messages.append({"role":"system","content":"Only these exact prepared sprint candidate IDs may appear in details.items: " + json.dumps([c["item"] for c in candidates]) + ". Known issues and discussion mentions are not eligible unless listed. Return items=[] if none selected."})
            # This is a separate role invocation, not the interrupting workflow node.
            async def call_role(state):
                tools=[{"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":{arg:{"type":"string"}},"required":[arg],"additionalProperties":False}}} for name,arg,description in [("read_repo_file","path","Read a regular default-branch file; never-sent files are denied"),("read_artifact","sha256","Read an item-bound immutable artifact")]]
                tools.append({"type":"function","function":{"name":"request_urgent","description":"Request owner urgency; never promotes or approves","parameters":{"type":"object","properties":{},"additionalProperties":False}}})
                formatting_retry=False
                for index in range(12):
                    response=await daemon.post("/v1/chat/completions",json={"plantool":{"item":request.request.item,"role":request.request.role,"generation":request.request.generation,"attempt_id":request.attempt_id,"idempotency_key":f"{key}:{index}"},"messages":messages,"tools":[] if formatting_retry else tools,"stream":False,"response_format":{"type":"json_schema","json_schema":{"name":"role_result","schema":RoleResult.model_json_schema()}}})
                    response.raise_for_status();message=response.json()["choices"][0]["message"]
                    if formatting_retry and message.get("tool_calls"):raise ValueError("Formatting correction cannot call tools")
                    if message.get("tool_calls"):
                        messages.append(message)
                        for call in message["tool_calls"]:
                            name=call["function"]["name"]
                            if name not in ("read_repo_file","read_artifact","request_urgent"):raise ValueError("Unknown role capability")
                            raw=call["function"]["arguments"]
                            arguments=json.loads(raw) if isinstance(raw,str) else raw
                            if not isinstance(arguments,dict):raise ValueError("Tool arguments must be an object")
                            arguments["item"]=request.request.item
                            if name=="request_urgent":arguments["idempotency_key"]=f"{key}:urgent"
                            reply=await daemon.post(f"/internal/v1/{name}",json=arguments);reply.raise_for_status()
                            messages.append({"role":"tool","tool_call_id":call["id"],"content":json.dumps(reply.json())})
                        continue
                    try:
                        choice=response.json()["choices"][0]
                        if choice.get("finish_reason")=="length":raise ValueError("Truncated role result")
                        data=parse_role_content(message.get("content"))
                        data.update(item=request.request.item,role=request.request.role,attempt_id=request.attempt_id,generation=request.request.generation)
                        if expected_kind and (not isinstance(data.get("details"),dict) or data["details"].get("kind")!=expected_kind):
                            raise ValueError("Wrong step-specific result details")
                        if candidates is not None:
                            allowed={c["item"] for c in candidates}
                            details=data.get("details") or {}
                            if details.get("kind")!="sprint" or any(c.get("item") not in allowed for c in details.get("items",[])):
                                raise ValueError("Sprint choice is not a prepared candidate")
                        return {"result":RoleResult.model_validate(data).model_dump(mode="json")}
                    except (ValueError,TypeError):
                        if formatting_retry or index==11:raise
                        # One same-route correction uses normal proxy accounting. No
                        # tools are supplied, so prior tool effects are not repeated.
                        formatting_retry=True
                        messages.append({"role":"assistant","content":message.get("content") if isinstance(message.get("content"),str) else ""})
                        messages.append({"role":"user","content":"The final response was not a complete valid RoleResult. Return one complete JSON object matching the system schema, using only evidence already obtained. No tools, Markdown, commentary or new claims. Keep the summary concise."})
                raise ValueError("Role tool/call budget exhausted")
            graph=StateGraph(State);graph.add_node("role",call_role);graph.add_edge(START,"role");graph.add_edge("role",END)
            result=(await graph.compile().ainvoke({}))["result"]
            def record_role():
                with store.db() as db:db.execute("UPDATE bridge_inbox SET result=? WHERE id=?",(json.dumps(result),key))
            await asyncio.to_thread(record_role)
            return result
    @app.post("/internal/v1/cancel")
    async def cancel(request: Cancel):
        async with lock(request.thread_id):
            receipt = await asyncio.to_thread(store.receipt,request.idempotency_key,request.model_dump(mode="json"))
            if receipt is not None: return receipt
            current = store.thread(request.thread_id)
            if current and current["state"] in ("completed","cancelled"):
                return await asyncio.to_thread(store.save,request.thread_id,current["state"],current["interrupt_id"],current["result"],request.idempotency_key)
            return await asyncio.to_thread(store.save,request.thread_id,"cancelled",None,None,request.idempotency_key)
    return app

# Token is read from the inherited descriptor, never argv or an environment value.
def runtime_app():
    fd = int(os.environ["PLANTOOL_WORKER_TOKEN_FD"])
    with os.fdopen(fd, "r") as stream: token = stream.readline(1024).strip()
    if len(token)<32: raise RuntimeError("Missing inherited worker credential")
    return create_app(Path(os.environ["PLANTOOL_HOME"]),token)
