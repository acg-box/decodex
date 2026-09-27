"""Synthetic empty-schema human-input MCP request; never accepts real credentials."""
import json
import sys
from pathlib import Path

record = Path(sys.argv[1])
marker = json.loads(sys.argv[2])
pending = None
for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method == "initialize":
        result = {"protocolVersion": request["params"]["protocolVersion"],
                  "capabilities": {"tools": {}}, "serverInfo": {"name": "child-input", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": [{"name": "ask", "inputSchema": {"type": "object", "properties": {}},
                             "annotations": {"readOnlyHint": True}}]}
    elif method == "tools/call":
        pending = request["id"]
        print(json.dumps({"jsonrpc": "2.0", "id": "input", "method": "elicitation/create", "params": {
            "mode": "form", "message": "Synthetic input", "_meta": marker,
            "requestedSchema": {"type": "object", "properties": {}}}}), flush=True)
        continue
    elif method is None and request.get("id") == "input":
        record.write_text(json.dumps(request))
        print(json.dumps({"jsonrpc": "2.0", "id": pending, "result": {
            "content": [{"type": "text", "text": json.dumps(request)}]}}), flush=True)
        continue
    elif "id" not in request:
        continue
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
