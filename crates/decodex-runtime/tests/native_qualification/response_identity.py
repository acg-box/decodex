"""Verify provider output without IDs keeps one identity through streaming and storage."""

import hashlib
import json
import os
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from native_session import Session


def qualify():
    requests = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            self.rfile.read(int(self.headers["Content-Length"]))
            requests.append(self.path)
            item = {"type": "message", "role": "assistant", "content": []}
            frames = [
                {"type": "response.created", "response": {"id": "identity-fixture"}},
                {"type": "response.output_item.added", "output_index": 0, "item": item},
                {"type": "response.output_text.delta", "output_index": 0, "content_index": 0, "delta": "Stable fixture reply."},
                {"type": "response.output_item.done", "output_index": 0, "item": {
                    **item, "content": [{"type": "output_text", "text": "Stable fixture reply."}]}},
                {"type": "response.completed", "response": {"id": "identity-fixture"}},
            ]
            payload = "".join("event: " + frame["type"] + "\ndata: " + json.dumps(frame) + "\n\n"
                              for frame in frames).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix="dx-response-identity-") as home:
            Path(home, "config.toml").write_text(
                'model="gpt-5.6-sol"\nmodel_provider="fixture"\n'
                '[features]\nenable_request_compression=false\n'
                '[model_providers.fixture]\nname="Fixture"\n'
                f'base_url="http://127.0.0.1:{server.server_port}"\n'
                'wire_api="responses"\nrequires_openai_auth=false\nsupports_websockets=false\n'
            )
            session = Session(home)
            started, completed = [], []
            try:
                tid = session.call("thread/start", {"cwd": home, "historyMode": "paginated",
                                   "approvalPolicy": "never", "sandbox": "read-only"})["thread"]["id"]
                session.call("turn/start", {"threadId": tid, "input": [{"type": "text", "text": "Reply once."}]})
                while True:
                    event = session.events.pop(0) if session.events else session.q.get(timeout=30)
                    if event.get("method") == "error":
                        raise AssertionError(event)
                    if event.get("method") in ("item/started", "item/completed"):
                        item = event["params"]["item"]
                        if item["type"] == "agentMessage":
                            (started if event["method"] == "item/started" else completed).append(item["id"])
                    if event.get("method") == "turn/completed":
                        assert event["params"]["turn"]["status"] == "completed", event
                        break
                stored = session.call("thread/read", {"threadId": tid, "includeTurns": True})["thread"]
            finally:
                session.close()
            cold = Session(home)
            try:
                recovered = cold.call("thread/read", {"threadId": tid, "includeTurns": True})["thread"]
            finally:
                cold.close()
            identities = lambda thread: [item["id"] for turn in thread["turns"] for item in turn["items"]
                                         if item["type"] == "agentMessage"]
            result = {"requests": len(requests), "started": started, "completed": completed,
                      "stored": identities(stored), "cold": identities(recovered)}
            result["passed"] = (len(requests) == 1 and len(started) == 1 and bool(started[0])
                                and started == completed == result["stored"] == result["cold"])
            return result
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    result = {"source_commit": os.environ["QUALIFIED_CODEX_SOURCE_COMMIT"],
              "cli_sha256": hashlib.sha256(Path(os.environ["QUALIFIED_CODEX_BINARY"]).read_bytes()).hexdigest(),
              **qualify()}
    Path(os.environ["DECODEX_QUALIFICATION_OUTPUT"], "response-identity-native.json").write_text(
        json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    assert result["passed"], "Provider output must keep one nonempty item ID."
