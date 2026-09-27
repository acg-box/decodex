"""Hold a synthetic MCP call until the test admits a second turn."""
import json
import sys
import time
from pathlib import Path

gate = Path(sys.argv[1])
for line in sys.stdin:
    request = json.loads(line)
    if "id" not in request:
        continue
    method = request.get("method")
    if method == "initialize":
        result = {"protocolVersion": request["params"]["protocolVersion"],
                  "serverInfo": {"name": "delayed-fixture", "version": "1"},
                  "capabilities": {"tools": {}}}
    elif method == "tools/list":
        result = {"tools": [{"name": "hold", "description": "Wait for the test.",
                             "inputSchema": {"type": "object", "properties": {}},
                             "annotations": {"readOnlyHint": True}}]}
    elif method == "tools/call":
        with gate.with_suffix(".metadata").open("a") as recorded:
            recorded.write(json.dumps(request["params"].get("_meta", {})) + "\n")
        gate.with_suffix(".started").touch()
        deadline = time.monotonic() + 20
        while not gate.exists() and time.monotonic() < deadline:
            time.sleep(0.01)
        result = {"content": [{"type": "text", "text": "delayed MCP output"}],
                  "isError": not gate.exists()}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
