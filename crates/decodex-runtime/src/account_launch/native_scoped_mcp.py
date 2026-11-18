"""Serve a synthetic MCP registration for each requested environment on loopback."""

import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if "id" not in request:
            self.send_response(202)
            self.end_headers()
            return

        result = {}
        if request["method"] == "initialize":
            result = {
                "protocolVersion": request["params"]["protocolVersion"],
                "serverInfo": {"name": self.path.lstrip("/"), "version": "1"},
                "capabilities": {"tools": {}},
            }
        elif request["method"] == "tools/list":
            result = {"tools": [{"name": "lookup", "inputSchema": {"type": "object"}}]}

        body = json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        pass


with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
    print(server.server_port, flush=True)
    server.serve_forever()
