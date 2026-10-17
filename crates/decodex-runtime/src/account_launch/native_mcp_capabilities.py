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
        second = (request.get("params") or {}).get("cursor") == "second"
        result = {"tools": [], "_meta": {
            "fixture/catalog": {"page": 2 if second else 1, "opaque": [1, "first", None]}
        }}
        if not second:
            result["nextCursor"] = "second"
        response = {"result": result}
    else:
        response = {"result": {}}
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], **response}), flush=True)
