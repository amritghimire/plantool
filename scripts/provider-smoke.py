#!/usr/bin/env python3
"""Read-only OpenRouter smoke probe. Key stays in memory; never runs a PlanTool graph."""
import json
import subprocess
import sys
import urllib.request
import urllib.error

model = sys.argv[1] if len(sys.argv) > 1 else "google/gemma-4-31b-it:free"
if not model.endswith(":free"):
    raise SystemExit("This probe only accepts explicit :free model IDs.")
try:
    key = subprocess.check_output(["/usr/bin/security", "find-generic-password", "-s", "plantool/openrouter", "-w"], stderr=subprocess.DEVNULL).decode().strip()
except subprocess.CalledProcessError:
    raise SystemExit("Cannot read plantool/openrouter from Keychain. No key was printed.")
for label, extra in [
    ("basic chat", {}),
    ("tools + structured JSON", {
        "tools": [{"type":"function","function":{"name":"read_repo_file","description":"Read a file","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":False}}}],
        "response_format":{"type":"json_schema","json_schema":{"name":"smoke","schema":{"type":"object","properties":{"summary":{"type":"string"}},"required":["summary"],"additionalProperties":False}}},
    }),
]:
    payload={"model":model,"messages":[{"role":"user","content":"Return a JSON object with summary equal to OK. Do not call tools."}],"max_tokens":256,"stream":False,**extra}
    request=urllib.request.Request("https://openrouter.ai/api/v1/chat/completions", data=json.dumps(payload).encode(),headers={"Authorization":"Bearer "+key,"Content-Type":"application/json"})
    try:
        with urllib.request.urlopen(request,timeout=180) as response:
            data=json.load(response)
            print(label+": HTTP "+str(response.status)+"; choices="+str(len(data.get("choices",[]))))
    except urllib.error.HTTPError as error:
        body=error.read(16384).decode(errors="replace").replace(key,"[REDACTED]")
        print(label+": HTTP "+str(error.code)+"; "+body)
    except Exception as error:
        print(label+": transport failure ("+type(error).__name__+")")
