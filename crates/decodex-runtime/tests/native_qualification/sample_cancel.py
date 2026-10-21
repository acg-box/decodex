"""Qualify exact-request cancellation and late completion against an explicit native CLI."""
import hashlib
import json
import os
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from native_session import Session


def reply(session, identity):
    deadline = time.monotonic() + 30
    while True:
        for index, frame in enumerate(session.events):
            if frame.get("id") == identity:
                return session.events.pop(index)
        frame = session.q.get(timeout=max(0.01, deadline - time.monotonic()))
        assert isinstance(frame, dict), "Native server closed before the original reply"
        if frame.get("id") == identity:
            return frame
        session.events.append(frame)
        assert time.monotonic() < deadline, "Original sampling reply timed out"


def case(mode, completed_first):
    requests = []
    started = threading.Event()
    release = threading.Event()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            requests.append(json.loads(self.rfile.read(int(self.headers["Content-Length"]))))
            index = len(requests)
            if index == 2:
                started.set()
                assert release.wait(20), "Fixture was not released"
            response_id = "sample-fixture-" + str(index)
            frames = [
                {"type": "response.created", "response": {"id": response_id}},
                {"type": "response.output_item.done", "item": {
                    "type": "message", "id": "message-" + str(index), "role": "assistant",
                    "content": [{"type": "output_text", "text": "Preserved fixture answer."}],
                }},
                {"type": "response.completed", "response": {
                    "id": response_id, "usage": {"input_tokens": 100, "output_tokens": 3, "total_tokens": 103},
                }},
            ]
            data = "".join("event: " + f["type"] + "\ndata: " + json.dumps(f) + "\n\n" for f in frames).encode()
            try:
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
            except (BrokenPipeError, ConnectionResetError):
                pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix="decodex-sample-cancel-", dir="/tmp") as home:
            Path(home, "config.toml").write_text(
                'model="gpt-5.6-sol"\nmodel_provider="fixture"\n'
                '[features]\nenable_request_compression=false\n'
                '[analytics]\nenabled=false\n'
                f'[model_providers.fixture]\nname="fixture"\nbase_url="http://127.0.0.1:{server.server_port}"\n'
                'wire_api="responses"\nrequires_openai_auth=false\nsupports_websockets=false\n'
                'stream_max_retries=0\nrequest_max_retries=0\n'
            )
            session = Session(home)
            try:
                parent = session.call("thread/start", {
                    "cwd": home, "historyMode": mode, "approvalPolicy": "never", "sandbox": "read-only",
                })["thread"]["id"]
                session.call("turn/start", {"threadId": parent, "input": [{"type": "text", "text": "Parent context."}]})
                session.done()

                def history():
                    if mode == "legacy":
                        return session.call("thread/read", {"threadId": parent, "includeTurns": True})["thread"]["turns"]
                    return session.call("thread/turns/list", {"threadId": parent, "limit": 100})["data"]

                before = history()
                assert len(before) == 1
                turn = before[0]["id"]
                session.serial += 1
                original = session.serial
                session.send({"id": original, "method": "thread/sample", "params": {
                    "threadId": parent, "expectedTurnId": turn, "prompt": "A separate explanation?",
                }})
                assert started.wait(10), "Sampling did not reach the controlled provider"
                assert history() == before, "Parent must stay responsive and unchanged while sampling"
                assert session.call("thread/sample/cancel", {"requestId": original + 10000}) == {}
                assert not any(f.get("id") == original for f in session.events), "A foreign identity must leave sampling running"
                if completed_first:
                    release.set()
                    original_reply = reply(session, original)
                    assert original_reply["result"]["text"] == "Preserved fixture answer."
                    assert session.call("thread/sample/cancel", {"requestId": original}) == {}
                else:
                    assert session.call("thread/sample/cancel", {"requestId": original}) == {}
                    original_reply = reply(session, original)
                    assert original_reply["error"]["code"] == -32603
                    assert original_reply["error"]["message"] == "sample canceled"
                assert session.call("thread/sample/cancel", {"requestId": original}) == {}
                assert history() == before, "Cancellation must not append to or interrupt the parent"
                assert session.call("thread/loaded/list", {})["data"] == [parent], "The temporary child must be reclaimed"
                assert len(requests) == 2, "One parent and one sample, with no inference replay"
                return {"history_mode": mode, "completed_before_cancel": completed_first,
                        "requests": len(requests), "parent_preserved": True, "temporary_child_reclaimed": True,
                        "foreign_and_repeated_cancel_safe": True}
            finally:
                release.set()
                session.close()
    finally:
        release.set()
        server.shutdown()
        server.server_close()


def main():
    results = [case(mode, completed_first) for mode in ["legacy", "paginated"] for completed_first in [False, True]]
    binary = Path(os.environ["QUALIFIED_CODEX_BINARY"])
    result = {"status": "passed", "cases": results, "case_count": len(results),
              "cli_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "scope": "Exact packaged stdio native RPC and loopback provider; no real account, live provider or installed-app acceptance."}
    Path(os.environ["DECODEX_QUALIFICATION_OUTPUT"], "sample-cancel-native.json").write_text(json.dumps(result, indent=2) + "\n")
    print("Native cancellation passed in legacy and paginated history: exact identity, cleanup, completed-answer race, parent preservation and no replay.")


if __name__ == "__main__":
    main()
