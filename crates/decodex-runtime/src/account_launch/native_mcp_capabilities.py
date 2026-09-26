"""Synthetic MCP status fixture; no network or credentials."""
import json
import sys

mode = sys.argv[1]
for line in sys.stdin:
    request = json.loads(line)
    if "id" not in request:
        continue
    method = request.get("method")
    if (method == "initialize" and mode == "init-error") or (
        method == "tools/list" and mode == "tools-error"
    ):
        response = {"error": {"code": -32603, "message": "Synthetic discovery failure"}}
    elif method == "initialize":
        response = {"result": {
            "protocolVersion": request["params"]["protocolVersion"],
            "serverInfo": {"name": "capability-fixture", "version": "1"},
            "capabilities": {"tools": {}, "extensions": {
                "openai/settings": {"readTool": "settings.read", "updateTool": "settings.update"}
            }},
        }}
    elif method == "tools/list":
        response = {"result": {"tools": []}}
    else:
        response = {"result": {}}
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], **response}), flush=True)
