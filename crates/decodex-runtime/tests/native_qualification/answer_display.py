"""Qualify native answer display decisions through completion and cold item history."""

import hashlib
import json
import os
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from native_session import Session


def qualify():
    displays = [
        {"version": 1, "mode": "show", "trustedContacts": "skip"},
        {"version": 1, "mode": "hide", "replacement": {"kind": "wellbeingPromptHelpline"}},
        {"version": 1, "mode": "block", "reason": {"kind": "cyber", "trustedAccess": False}},
        {"version": 2, "mode": "show", "trustedContacts": "skip"},
    ]
    requests = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            self.rfile.read(int(self.headers["Content-Length"]))
            index = len(requests)
            requests.append(self.path)
            message = {"type": "message", "id": f"answer-{index}", "role": "assistant",
                       "phase": "final_answer", "content": [{"type": "output_text", "text": "Fixture answer."}]}
            frames = [
                {"type": "response.created", "response": {"id": f"response-{index}"}},
                {"type": "response.metadata", "metadata": {"openai_chatgpt_moderation_metadata": {
                    "final_answer_moderation": {"item_id": message["id"], "moderated": True, "display": displays[index]}}}},
                {"type": "response.output_item.added", "output_index": 0, "item": {**message, "content": []}},
                {"type": "response.output_text.delta", "output_index": 0, "content_index": 0, "delta": "Fixture answer."},
                {"type": "response.output_item.done", "output_index": 0, "item": message},
                {"type": "response.completed", "response": {"id": f"response-{index}"}},
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
        with tempfile.TemporaryDirectory(prefix="dx-answer-display-", dir="/tmp") as home:
            Path(home, "config.toml").write_text(
                'model="gpt-5.6-sol"\nmodel_provider="fixture"\n'
                '[features]\nenable_request_compression=false\n'
                '[model_providers.fixture]\nname="Fixture"\n'
                f'base_url="http://127.0.0.1:{server.server_port}"\n'
                'wire_api="responses"\nrequires_openai_auth=false\nsupports_websockets=false\n'
            )
            session = Session(home)
            saved = []
            try:
                for display in displays:
                    thread = session.call("thread/start", {"cwd": home, "historyMode": "paginated",
                                          "approvalPolicy": "never", "sandbox": "read-only"})["thread"]["id"]
                    session.call("turn/start", {"threadId": thread, "input": [{"type": "text", "text": "Reply once."}]})
                    moderation = None
                    while True:
                        event = session.events.pop(0) if session.events else session.q.get(timeout=30)
                        if event.get("method") == "error":
                            raise AssertionError(event)
                        if event.get("method") == "item/completed" and event["params"]["item"]["type"] == "agentMessage":
                            moderation = event["params"]["item"]["moderation"]
                        if event.get("method") == "turn/completed":
                            assert event["params"]["turn"]["status"] == "completed", event
                            break
                    expected = {"moderated": True, "display": display if display["version"] == 1 else None}
                    assert moderation == expected, (moderation, expected)
                    saved.append((thread, expected))
            finally:
                session.close()
            cold = Session(home)
            try:
                for thread, expected in saved:
                    page = cold.call("thread/items/list", {"threadId": thread, "limit": 20})
                    answers = [row["item"] for row in page["data"] if row["item"]["type"] == "agentMessage"]
                    assert len(answers) == 1 and answers[0]["moderation"] == expected, answers
            finally:
                cold.close()
            assert len(requests) == len(displays)
            return {"passed": True, "scenarios": len(saved), "requests": len(requests),
                    "modes": ["show", "hide", "block", "unsupported"], "live_and_cold_flags_match": True}
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    result = {"source_commit": os.environ["QUALIFIED_CODEX_SOURCE_COMMIT"],
              "cli_sha256": hashlib.sha256(Path(os.environ["QUALIFIED_CODEX_BINARY"]).read_bytes()).hexdigest(),
              **qualify()}
    Path(os.environ["DECODEX_QUALIFICATION_OUTPUT"], "answer-display-native.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
