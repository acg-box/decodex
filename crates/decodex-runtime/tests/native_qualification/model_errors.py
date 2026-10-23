"""Verify terminal model errors and their persisted classification on an exact runtime."""

import hashlib
import json
import os
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from native_session import Session


def case(transport):
    requests = []
    message = "The selected fixture model is unavailable."
    error = {"code": "model_not_found", "message": message}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            self.rfile.read(int(self.headers["Content-Length"]))
            requests.append(self.path)
            if transport == "http":
                payload = json.dumps({"error": error}).encode()
                self.send_response(404)
                self.send_header("Content-Type", "application/json")
                self.send_header("Retry-After", "0")
            else:
                event = ({"type": "error", "error": error} if transport == "sse_error"
                         else {"type": "response.failed", "response": {"id": "failed", "error": error}})
                payload = ("event: " + event["type"] + "\ndata: " + json.dumps(event) + "\n\n").encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix="dx-model-errors-") as home:
            Path(home, "config.toml").write_text(
                'model="gpt-5.6-sol"\nmodel_provider="fixture"\n'
                '[features]\nenable_request_compression=false\n'
                '[model_providers.fixture]\nname="Fixture"\n'
                f'base_url="http://127.0.0.1:{server.server_port}"\n'
                'wire_api="responses"\nrequires_openai_auth=false\nsupports_websockets=false\n'
                'stream_max_retries=2\nrequest_max_retries=2\n'
            )
            session = Session(home)
            errors = []
            try:
                tid = session.call("thread/start", {"cwd": home, "historyMode": "paginated",
                                   "approvalPolicy": "never", "sandbox": "read-only"})["thread"]["id"]
                session.call("turn/start", {"threadId": tid, "input": [{"type": "text", "text": "Run the fixture."}]})
                deadline = time.monotonic() + 30
                while True:
                    event = session.events.pop(0) if session.events else session.q.get(
                        timeout=max(0.01, deadline - time.monotonic()))
                    if event.get("method") == "error":
                        errors.append(event["params"])
                    if event.get("method") == "turn/completed":
                        live = event["params"]["turn"]
                        break
                    if "id" in event and "method" in event:
                        raise AssertionError("Unexpected client interaction: " + event["method"])
                stored = session.call("thread/read", {"threadId": tid, "includeTurns": True})["thread"]
            finally:
                session.close()
            cold = Session(home)
            try:
                recovered = cold.call("thread/read", {"threadId": tid, "includeTurns": True})["thread"]
            finally:
                cold.close()
            result = {"transport": transport, "request_count": len(requests), "live": live["error"],
                      "status": live["status"], "retrying_errors": sum(e.get("willRetry") is True for e in errors),
                      "stored": stored["turns"][-1]["error"], "cold": recovered["turns"][-1]["error"]}
            expected = {"message": message, "codexErrorInfo": "modelNotFound"}
            result["passed"] = (len(requests) == 1 and live["status"] == "failed" and bool(errors)
                                and all(e.get("willRetry") is False for e in errors)
                                and all(result[key].get("message") == expected["message"]
                                        and result[key].get("codexErrorInfo") == expected["codexErrorInfo"]
                                        for key in ("live", "stored", "cold")))
            return result
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    binary = Path(os.environ["QUALIFIED_CODEX_BINARY"])
    result = {"source_commit": os.environ["QUALIFIED_CODEX_SOURCE_COMMIT"],
              "cli_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "cases": [case(transport) for transport in ("http", "sse_error", "sse_failed")]}
    Path(os.environ["DECODEX_QUALIFICATION_OUTPUT"], "model-errors-native.json").write_text(
        json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    assert all(case["passed"] for case in result["cases"]), "Model errors must be terminal and persist unchanged."
