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
        params = {
            "mode": "form", "message": "Select the fixture value",
            "_meta": {"fixture/source": "native-mcp"},
            "requestedSchema": True if sys.argv[2] == "true" else {"type": "object", "properties": {
                "answer": {"type": "string", "oneOf": [
                    {"const": "wire-value", "title": "Display label"}
                ]}
            }, "required": ["answer"]},
        }
        if len(sys.argv) > 3 and sys.argv[3] == "true":
            params = {"mode": "openai/userVerification", "title": "Verify fixture",
                      "description": "Synthetic unsupported request", "challenge": "AQID"}
        method = "openai/elicitation/create"
        if len(sys.argv) > 4:
            method = "elicitation/create"
            params["_meta"].update({"codex_request_type": "approval_request",
                                  "codex_approval_kind": "mcp_tool_call", "tool_name": "form_fixture"})
            if sys.argv[4] == "url":
                params.pop("requestedSchema")
                params.update({"mode": "url", "url": "https://example.test/approval",
                               "elicitationId": "fixture-url"})
            else:
                params["requestedSchema"] = {"type": "object", "properties": {
                    "answer": {"type": "string"}}, "required": ["answer"]}
        send({"id": "form-request", "method": method, "params": params})
    elif method is None and request.get("id") == "form-request":
        state["replies"].append(request)
        record.write_text(json.dumps(state))
        send({"id": pending, "result": {"content": [{
            "type": "text", "text": "Fixture form action: " + request.get("result", {}).get("action", "error")
        }]}})
    elif "id" in request:
        send({"id": request["id"], "error": {"code": -32601, "message": "Unknown fixture method"}})
