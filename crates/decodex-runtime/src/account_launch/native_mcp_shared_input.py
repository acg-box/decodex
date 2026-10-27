"""Synthetic stateless MCP helper that asks for per-call user input."""
import json
import os
import sys

server_info = {"name": f"sharing-input-fixture-{os.getpid()}", "version": "1"}
for line in sys.stdin:
    request = json.loads(line)
    if "id" not in request:
        continue
    method = request.get("method")
    if method == "server/discover":
        result = {
            "resultType": "complete",
            "supportedVersions": ["2026-07-28"],
            "capabilities": {"tools": {}},
            "_meta": {"io.modelcontextprotocol/serverInfo": server_info},
            "ttlMs": 0,
            "cacheScope": "private",
        }
    elif method == "tools/list":
        result = {
            "resultType": "complete",
            "ttlMs": 0,
            "cacheScope": "private",
            "tools": [
                {
                    "name": "ask",
                    "inputSchema": {"type": "object", "properties": {"label": {"type": "string"}}, "required": ["label"]},
                    "annotations": {"readOnlyHint": True},
                },
                {
                    "name": "identify",
                    "inputSchema": {"type": "object"},
                    "annotations": {"readOnlyHint": True},
                },
            ],
        }
    elif method == "tools/call" and request.get("params", {}).get("name") == "identify":
        result = {
            "resultType": "complete",
            "content": [{"type": "text", "text": str(os.getpid())}],
            "structuredContent": {"pid": os.getpid()},
        }
    elif method == "tools/call":
        params = request.get("params", {})
        label = params.get("arguments", {}).get("label")
        responses = params.get("inputResponses")
        if responses is None:
            result = {
                "resultType": "input_required",
                "inputRequests": {"approval": {
                    "method": "elicitation/create",
                    "params": {
                        "mode": "form",
                        "message": f"Answer for {label}",
                        "_meta": {"codex_requires_user_input": True},
                        "requestedSchema": {
                            "type": "object",
                            "properties": {"answer": {"type": "string"}},
                            "required": ["answer"],
                        },
                    },
                }},
                "requestState": f"state-{label}",
                "_meta": {"io.modelcontextprotocol/serverInfo": server_info},
            }
        else:
            answer = responses["approval"]["content"]["answer"]
            assert params["requestState"] == f"state-{label}"
            result = {
                "resultType": "complete",
                "content": [{"type": "text", "text": answer}],
                "structuredContent": {"pid": os.getpid(), "label": label, "answer": answer},
            }
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
