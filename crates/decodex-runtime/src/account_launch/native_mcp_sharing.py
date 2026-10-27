"""Local MCP fixture that reports the helper process serving each call."""

import json
import os
import sys


mode = sys.argv[1]
server_info = {"name": f"sharing-fixture-{os.getpid()}", "version": "1"}
for line in sys.stdin:
    request = json.loads(line)
    if "id" not in request:
        continue
    method = request.get("method")
    if method == "server/discover" and mode == "modern":
        result = {
            "resultType": "complete",
            "supportedVersions": ["2026-07-28"],
            "capabilities": {"tools": {}},
            "_meta": {"io.modelcontextprotocol/serverInfo": server_info},
            "ttlMs": 0,
            "cacheScope": "private",
        }
    elif method == "server/discover":
        print(json.dumps({
            "jsonrpc": "2.0", "id": request["id"],
            "error": {"code": -32601, "message": "Method not found"},
        }), flush=True)
        continue
    elif method == "initialize":
        result = {
            "protocolVersion": request["params"]["protocolVersion"],
            "capabilities": {"tools": {}},
            "serverInfo": server_info,
        }
    elif method == "tools/list":
        result = {"tools": [{
            "name": "identify", "inputSchema": {"type": "object"},
            "annotations": {"readOnlyHint": True},
        }]}
        if mode == "modern":
            result.update({"resultType": "complete", "ttlMs": 0, "cacheScope": "private"})
    elif method == "tools/call":
        result = {
            "content": [{"type": "text", "text": str(os.getpid())}],
            "structuredContent": {"pid": os.getpid()},
        }
        if mode == "modern":
            result["resultType"] = "complete"
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
