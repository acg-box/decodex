"""Isolated stdio MCP fixture. All captured values are synthetic test data."""
import json
import sys
from pathlib import Path

record = Path(sys.argv[1])
state = {"replies": []}
pending = None


def send(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method == "initialize":
        state["capabilities"] = request["params"]["capabilities"]
        record.write_text(json.dumps(state))
        send({"id": request["id"], "result": {
            "protocolVersion": request["params"]["protocolVersion"],
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "form-fixture", "version": "1"},
        }})
    elif method == "tools/list":
        send({"id": request["id"], "result": {"tools": [{
            "name": "form_fixture", "description": "Ask for an explicit test value.",
            "inputSchema": {"type": "object", "properties": {}},
            "annotations": {"readOnlyHint": True},
        }]}})
    elif method == "tools/call":
        pending = request["id"]
        send({"id": "form-request", "method": "openai/elicitation/create", "params": {
            "mode": "form", "message": "Select the fixture value",
            "_meta": {"fixture/source": "native-mcp"},
            "requestedSchema": {"type": "object", "properties": {
                "answer": {"type": "string", "oneOf": [
                    {"const": "wire-value", "title": "Display label"}
                ]}
            }, "required": ["answer"]},
        }})
    elif method is None and request.get("id") == "form-request":
        state["replies"].append(request)
        record.write_text(json.dumps(state))
        send({"id": pending, "result": {"content": [{
            "type": "text", "text": "Fixture form action: " + request.get("result", {}).get("action", "error")
        }]}})
    elif "id" in request:
        send({"id": request["id"], "error": {"code": -32601, "message": "Unknown fixture method"}})
