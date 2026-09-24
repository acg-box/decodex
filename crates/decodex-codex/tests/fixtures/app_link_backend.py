"""Loopback-only hosted Apps and model fixture; no external credentials or side effects."""
import base64
import json
import time
from datetime import datetime, timezone
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

AUDIT = Path(sys.argv[1])
# Synthetic bearer material is accepted only by this loopback fixture.
claims = {"exp": int(time.time()) + 3600, "email": "fixture@example.invalid",
          "https://api.openai.com/auth": {"chatgpt_account_id": "isolated-app-account",
           "chatgpt_plan_type": "plus", "chatgpt_user_id": "isolated-user"}}
def encode(value):
    return base64.urlsafe_b64encode(json.dumps(value).encode()).decode().rstrip("=")
token = encode({"alg": "none"}) + "." + encode(claims) + ".fixture"
AUDIT.parent.joinpath("auth.json").write_text(json.dumps({"auth_mode": "chatgpt", "tokens": {
    "id_token": token, "access_token": token, "refresh_token": "isolated-unused",
    "account_id": "isolated-app-account"}, "last_refresh": datetime.now(timezone.utc).isoformat()}))
LINKS = ["work", "work", "personal", "work", "work", "work", "personal"]
model_calls = 0
TOOL = {
    "name": "calendar_create_event",
    "description": "Create an isolated fixture event",
    "inputSchema": {"type": "object", "properties": {
        "title": {"type": "string"}, "link_id": {"type": "string", "enum": ["work", "personal"]}},
        "required": ["title", "link_id"]},
    "annotations": {"readOnlyHint": False, "destructiveHint": False, "openWorldHint": False},
    "_meta": {"connector_id": "calendar", "link_id": "work", "connector_name": "Calendar",
              "_codex_apps": {"resource_uri": "connector://calendar/tools/calendar_create_event",
                              "contains_mcp_source": True, "connector_id": "calendar",
                              "requires_explicit_link_id": True}},
}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def reply(self, status, value):
        data = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        self.reply(200, {"items": [], "connectors": [], "data": [], "models": []})

    def do_POST(self):
        global model_calls
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or "{}")
        method = body.get("method")
        if self.path.endswith("/responses"):
            guardian = body.get("client_metadata", {}).get("x-openai-subagent") == "guardian"
            serial = model_calls
            if guardian:
                with AUDIT.open("a") as output:
                    output.write(json.dumps({"guardian": True}) + "\n")
                item = {"type": "message", "id": "guardian-review", "role": "assistant",
                        "content": [{"type": "output_text", "text": json.dumps({
                            "risk_level": "low", "user_authorization": "high", "outcome": "allow",
                            "rationale": "This is a local isolated fixture action."})}]}
            else:
                model_calls += 1
            if not guardian and serial % 2 == 0:
                link = LINKS[serial // 2]
                code = ('const entry = ALL_TOOLS.find(t => t.name.includes("calendar") && '
                        't.name.includes("create")); text(await tools[entry.name]('
                        + json.dumps({"title": "Isolated fixture", "link_id": link}) + '));')
                item = {"type": "custom_tool_call", "id": f"item-{serial}", "call_id": f"call-{serial}",
                        "name": "exec", "namespace": "functions", "input": code}
            elif not guardian:
                item = {"type": "message", "id": f"message-{serial}", "role": "assistant",
                        "content": [{"type": "output_text", "text": "Done"}]}
            response_id = f"guardian-{serial}" if guardian else f"response-{serial}"
            frames = [{"type": "response.created", "response": {"id": response_id}},
                      {"type": "response.output_item.done", "item": item},
                      {"type": "response.completed", "response": {"id": response_id,
                       "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}}}]
            data = "".join("event: " + e["type"] + "\ndata: " + json.dumps(e) + "\n\n" for e in frames).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        if method == "initialize":
            result = {"protocolVersion": body["params"]["protocolVersion"],
                      "capabilities": {"tools": {"listChanged": True}},
                      "serverInfo": {"name": "isolated-app-links", "version": "1"}}
        elif method == "notifications/initialized":
            return self.reply(202, {})
        elif method == "tools/list":
            result = {"tools": [TOOL]}
        elif method == "tools/call":
            with AUDIT.open("a") as output:
                output.write(json.dumps(body["params"]["arguments"]) + "\n")
            result = {"content": [{"type": "text", "text": "Isolated fixture action completed"}]}
        elif method == "resources/list":
            result = {"resources": []}
        elif method == "resources/templates/list":
            result = {"resourceTemplates": []}
        else:
            return self.reply(200, {"items": [], "connectors": [], "data": []})
        self.reply(200, {"jsonrpc": "2.0", "id": body["id"], "result": result})


server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
print(server.server_port, flush=True)
server.serve_forever()
