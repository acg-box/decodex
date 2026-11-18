"""Check that a native parent keeps acknowledged child mail after interruption."""

import json
import os
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from native_session import Session


def qualify():
    requests = []
    sampling = threading.Event()
    release = threading.Event()
    mail = {
        "type": "agent_message",
        "id": "agent_interrupted-result",
        "author": "/root/worker",
        "recipient": "/root",
        "content": [{"type": "input_text", "text": "Message Type: FINAL_ANSWER\nThe worker finished."}],
    }

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            requests.append(body["input"])
            response_id = f"mail-{len(requests)}"
            frame = {"type": "response.completed", "response": {"id": response_id}}
            payload = f"event: response.completed\ndata: {json.dumps(frame)}\n\n".encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            if len(requests) == 1:
                sampling.set()
                release.wait(35)
            try:
                self.wfile.write(payload)
            except (BrokenPipeError, ConnectionResetError):
                pass

    with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
        serving = threading.Thread(target=server.serve_forever, daemon=True)
        serving.start()
        try:
            with tempfile.TemporaryDirectory(prefix="dx-interrupted-mail-", dir="/tmp") as directory:
                home = str(Path(directory).resolve())
                Path(home, "config.toml").write_text(
                    'model="gpt-5.6-sol"\nmodel_provider="fixture"\n'
                    'agent_interrupt_message_enabled=false\n'
                    '[features]\ninstant_interrupt=false\nenable_request_compression=false\n'
                    '[analytics]\nenabled=false\n'
                    '[model_providers.fixture]\nname="Fixture"\n'
                    f'base_url="http://127.0.0.1:{server.server_port}"\n'
                    'wire_api="responses"\nrequires_openai_auth=false\nsupports_websockets=false\n'
                )
                session = Session(home)
                try:
                    thread = session.call("thread/start", {
                        "cwd": home, "historyMode": "paginated",
                        "approvalPolicy": "never", "sandbox": "read-only",
                    })["thread"]
                    turn = session.call("turn/start", {
                        "threadId": thread["id"],
                        "input": [{"type": "text", "text": "Start work."}],
                    })["turn"]["id"]
                    assert sampling.wait(10), "Parent did not reach the model fixture"
                    session.call("thread/inject_items", {"threadId": thread["id"], "items": [mail]})
                    session.call("turn/interrupt", {"threadId": thread["id"], "turnId": turn})
                    while True:
                        event = session.events.pop(0) if session.events else session.q.get(timeout=10)
                        if isinstance(event, dict) and event.get("method") == "turn/completed":
                            assert event["params"]["threadId"] == thread["id"], event
                            assert event["params"]["turn"]["id"] == turn, event
                            assert event["params"]["turn"]["status"] == "interrupted", event
                            break
                        assert isinstance(event, dict) and event.get("method") != "error", event

                    history = map(json.loads, Path(thread["path"]).read_text().splitlines())
                    recorded = [
                        {key: event["payload"].get(key) for key in mail}
                        for event in history if event["type"] == "response_item"
                        and event["payload"]["type"] == "agent_message"
                    ]
                    assert recorded == [mail], "Acknowledged child mail was not durable at turn completion"
                finally:
                    release.set()
                    session.close()

                cold = Session(home)
                try:
                    cold.call("thread/resume", {"threadId": thread["id"], "cwd": home})
                    cold.call("turn/start", {
                        "threadId": thread["id"],
                        "input": [{"type": "text", "text": "Continue work."}],
                    })
                    cold.done()
                finally:
                    cold.close()
        finally:
            release.set()
            server.shutdown()
            serving.join()

    sampled = [
        [{key: item.get(key) for key in mail} for item in request if item["type"] == "agent_message"]
        for request in requests
    ]
    assert sampled == [[], [mail]], sampled


if __name__ == "__main__":
    qualify()
    print(json.dumps({
        "status": "passed",
        "source_commit": os.environ["QUALIFIED_CODEX_SOURCE_COMMIT"],
        "cases": ["acknowledged child result flushed on interrupt and sampled once after cold resume"],
    }))
