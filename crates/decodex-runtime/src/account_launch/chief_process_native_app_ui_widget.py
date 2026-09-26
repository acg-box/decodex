"""Synthetic local MCP counter for the isolated native socket test."""
import json
import sys

HTML = """<!doctype html><title>Counter fixture</title><h1>Counter fixture</h1><output id="counter">Waiting</output>
<script>
const send = value => parent.postMessage({jsonrpc:'2.0', ...value}, '*');
let requested = false;
addEventListener('message', event => {
  if (event.source !== parent) return;
  const message = event.data;
  if (message.id === 'fixture-init' && message.result) {
    send({method:'ui/notifications/initialized'});
  }
  if (message.method === 'ui/notifications/tool-result' && !requested) {
    requested = true;
    send({id:'fixture-call', method:'tools/call', params:{name:'counter', arguments:{value:42}}});
  }
  if (message.id === 'fixture-call' && message.result?.structuredContent?.value === 42) {
    document.getElementById('counter').textContent = '42';
    send({id:'fixture-counter-42', method:'ping'});
  }
  if (message.method === 'ui/resource-teardown') send({id:message.id, result:{}});
});
send({id:'fixture-init', method:'ui/initialize', params:{protocolVersion:'2026-01-26', appInfo:{name:'Fixture',version:'1'}, appCapabilities:{availableDisplayModes:['inline','fullscreen']}}});
</script>"""

for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    with open(sys.argv[1] + ".requests", "a", encoding="utf-8") as journal:
        journal.write(str(method) + "\n")
    result = None
    if method == "initialize":
        result = {"protocolVersion": request["params"]["protocolVersion"], "capabilities": {"tools": {}, "resources": {}}, "serverInfo": {"name": "fixture", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": [{"name": "counter", "description": "Read a local fixture counter", "inputSchema": {"type": "object", "properties": {"value": {"type": "integer"}}}, "annotations": {"readOnlyHint": True}, "_meta": {"ui": {"resourceUri": "ui://fixture/counter", "visibility": ["model", "app"]}}}]}
    elif method in ("resources/list", "resources/templates/list"):
        result = {"resources" if method == "resources/list" else "resourceTemplates": []}
    elif method == "resources/read":
        result = {"contents": [{"uri": request["params"]["uri"], "mimeType": "text/html;profile=mcp-app", "text": HTML, "_meta": {"ui": {"csp": {}}}}]}
    elif method == "tools/call":
        with open(sys.argv[1], "a", encoding="utf-8") as journal:
            journal.write(json.dumps(request["params"]) + "\n")
        result = {"content": [{"type": "text", "text": "Local counter"}], "structuredContent": {"value": request["params"]["arguments"]["value"]}}
    elif "id" in request:
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "error": {"code": -32601, "message": "Unknown fixture method"}}), flush=True)
        continue
    if result is not None:
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
