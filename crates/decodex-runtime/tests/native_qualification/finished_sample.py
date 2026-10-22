"""Qualify sampling after failed or interrupted turns with an exact packaged CLI."""

import hashlib
import json
import os
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from native_session import Session


def finished(session, thread, status):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        frame = session.events.pop(0) if session.events else session.q.get(
            timeout=max(0.01, deadline - time.monotonic())
        )
        assert isinstance(frame, dict), "Native server closed before turn completion"
        if frame.get("method") == "turn/completed" and frame["params"]["threadId"] == thread:
            turn = frame["params"]["turn"]
            assert turn["status"] == status, turn
            return turn["id"]
        assert not ("id" in frame and "method" in frame), frame
    raise TimeoutError("Expected finished source turn")


def case(mode, status):
    requests = []
    started = threading.Event()
    release = threading.Event()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            requests.append(json.loads(self.rfile.read(int(self.headers["Content-Length"]))))
            serial = len(requests)
            if serial == 1 and status == "interrupted":
                started.set()
                assert release.wait(20), "Interrupted fixture was not released"
            response = "finished-sample-" + str(serial)
            frames = [{"type": "response.created", "response": {"id": response}}]
            if serial == 1 and status == "failed":
                frames.append({"type": "response.failed", "response": {
                    "id": response, "status": "failed", "error": {
                        "code": "invalid_prompt", "message": "Controlled fixture failure.",
                    },
                }})
            else:
                frames.extend([
                    {"type": "response.output_item.done", "item": {
                        "type": "message", "id": "message-" + str(serial), "role": "assistant",
                        "content": [{"type": "output_text", "text": "Separate fixture explanation."}],
                    }},
                    {"type": "response.completed", "response": {
                        "id": response, "usage": {"input_tokens": 20, "output_tokens": 3, "total_tokens": 23},
                    }},
                ])
            payload = "".join("data: " + json.dumps(frame) + "\n\n" for frame in frames).encode()
            try:
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)
            except (BrokenPipeError, ConnectionResetError):
                pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    serving = threading.Thread(target=server.serve_forever, daemon=True)
    serving.start()
    try:
        with tempfile.TemporaryDirectory(prefix="dx-finished-sample-", dir="/tmp") as home:
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
                turn = session.call("turn/start", {
                    "threadId": parent, "input": [{"type": "text", "text": "Controlled source turn."}],
                })["turn"]["id"]
                if status == "interrupted":
                    assert started.wait(10), "Source turn did not reach the fixture"
                    try:
                        session.call("thread/sample", {
                            "threadId": parent, "expectedTurnId": turn, "prompt": "Reject an active source.",
                        })
                    except RuntimeError as error:
                        assert "-32600" in str(error), error
                    else:
                        raise AssertionError("Sampling must reject an active source")
                    session.call("turn/interrupt", {"threadId": parent, "turnId": turn})
                assert finished(session, parent, status) == turn
                release.set()

                def history():
                    if mode == "legacy":
                        return session.call("thread/read", {"threadId": parent, "includeTurns": True})["thread"]["turns"]
                    return session.call("thread/turns/list", {"threadId": parent, "limit": 100})["data"]

                before = history()
                assert len(before) == 1 and before[0]["status"] == status
                try:
                    session.call("thread/sample", {
                        "threadId": parent, "expectedTurnId": "stale-turn", "prompt": "Reject a stale source.",
                    })
                except RuntimeError as error:
                    assert "-32600" in str(error), error
                else:
                    raise AssertionError("Sampling must reject a stale source identity")
                sampled = session.call("thread/sample", {
                    "threadId": parent, "expectedTurnId": turn, "prompt": "Explain this finished source.",
                })
                assert sampled["sourceTurnId"] == turn
                assert sampled["text"] == "Separate fixture explanation."
                assert history() == before, "Sampling changed the parent history"
                assert session.call("thread/loaded/list", {})["data"] == [parent]
                assert len(requests) == 2, "Source and sample must each run exactly once"
                return {"history_mode": mode, "source_status": status, "requests": len(requests),
                        "parent_preserved": True, "temporary_child_reclaimed": True,
                        "stale_source_rejected": True,
                        "active_source_check": "passed" if status == "interrupted" else "not applicable"}
            finally:
                release.set()
                session.close()
    finally:
        release.set()
        server.shutdown()
        server.server_close()
        serving.join(timeout=5)


def main():
    results = [case(mode, status) for mode in ["legacy", "paginated"] for status in ["failed", "interrupted"]]
    result = {"status": "passed", "cases": results, "case_count": len(results),
              "cli_sha256": hashlib.sha256(Path(os.environ["QUALIFIED_CODEX_BINARY"]).read_bytes()).hexdigest(),
              "source_commit": os.environ["QUALIFIED_CODEX_SOURCE_COMMIT"],
              "scope": "Exact packaged private-stdio CLI and loopback provider; no live account or installed-app acceptance."}
    Path(os.environ["DECODEX_QUALIFICATION_OUTPUT"], "finished-sample-native.json").write_text(
        json.dumps(result, indent=2) + "\n"
    )
    print("Sampling passed after failed and interrupted turns in legacy and paginated history.")


if __name__ == "__main__":
    main()
