"""Keep unsampled native agent mail through compaction and cold continuation."""

import hashlib
import json
import os
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from native_session import Session


def qualify(history_mode, remote):
    requests = []
    mail = {
        "type": "agent_message",
        "id": "agent_pending-result",
        "author": "/root/worker",
        "recipient": "/root",
        "content": [{"type": "input_text", "text": "Keep the worker's release blocker."}],
    }

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            serial = len(requests)
            requests.append(body)
            compact = serial in (1, 2, 4)
            text = f"MAIL_SUMMARY_{serial}" if compact else "Fixture reply."
            item = (
                {"type": "compaction", "encrypted_content": text}
                if compact and remote
                else {
                    "type": "message", "role": "assistant", "id": f"reply-{serial}",
                    "content": [{"type": "output_text", "text": text}],
                }
            )
            response = f"mail-{serial}"
            frames = [
                {"type": "response.created", "response": {"id": response}},
                {"type": "response.output_item.done", "output_index": 0, "item": item},
                {"type": "response.completed", "response": {"id": response}},
            ]
            payload = "".join(
                "event: " + frame["type"] + "\ndata: " + json.dumps(frame) + "\n\n"
                for frame in frames
            ).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix="dx-mail-", dir="/tmp") as directory:
            home = str(Path(directory).resolve())
            provider = "OpenAI" if remote else "Fixture"
            Path(home, "config.toml").write_text(
                'model="gpt-5.6-sol"\nmodel_provider="fixture"\n'
                'model_auto_compact_token_limit=2000000000\n'
                '[features]\nenable_request_compression=false\ntoken_budget=false\n'
                '[model_providers.fixture]\n'
                f'name="{provider}"\nbase_url="http://127.0.0.1:{server.server_port}"\n'
                'wire_api="responses"\nrequires_openai_auth=false\nsupports_websockets=false\n'
            )
            session = Session(home)
            try:
                tid = session.call("thread/start", {
                    "cwd": home, "historyMode": history_mode,
                    "approvalPolicy": "never", "sandbox": "read-only",
                })["thread"]["id"]
                session.call("turn/start", {
                    "threadId": tid, "input": [{"type": "text", "text": "Plan the release."}],
                })
                session.done()
                session.call("thread/inject_items", {"threadId": tid, "items": [mail]})
                session.call("thread/compact/start", {"threadId": tid})
                session.done()
            finally:
                session.close()

            cold = Session(home)
            try:
                cold.call("thread/resume", {"threadId": tid, "cwd": home})
                cold.call("thread/compact/start", {"threadId": tid})
                cold.done()
                cold.call("turn/start", {
                    "threadId": tid,
                    "input": [{"type": "text", "text": "Continue after the worker result."}],
                })
                cold.done()
                cold.call("thread/compact/start", {"threadId": tid})
                cold.done()
                cold.call("turn/start", {
                    "threadId": tid,
                    "input": [{"type": "text", "text": "Continue after acknowledged mail."}],
                })
                cold.done()
            finally:
                cold.close()

            inputs = [body["input"] for body in requests]
            pending = [item for item in inputs[3] if item["type"] == "agent_message"]
            exact = [{key: item.get(key) for key in mail} for item in pending]
            summary = next((i for i, item in enumerate(inputs[3])
                            if "MAIL_SUMMARY_2" in json.dumps(item)), None)
            mail_position = next((i for i, item in enumerate(inputs[3])
                                  if item["type"] == "agent_message"), None)
            caller = next((i for i, item in enumerate(inputs[3])
                           if "Continue after the worker result." in json.dumps(item)), None)
            result = {
                "history_mode": history_mode, "compaction": "remote_v2" if remote else "local",
                "requests": len(requests), "pending_mail": exact,
                "summary_before_mail_before_caller": (
                    summary is not None and mail_position is not None and caller is not None
                    and summary < mail_position < caller
                ),
                "acknowledged_mail_removed": not any(
                    item["type"] == "agent_message" for item in inputs[5]
                ),
                "compaction_route_verified": all(
                    any(item["type"] == "compaction_trigger" for item in inputs[i]) == remote
                    for i in (1, 2, 4)
                ),
            }
            result["passed"] = (
                len(requests) == 6 and exact == [mail]
                and result["summary_before_mail_before_caller"]
                and result["acknowledged_mail_removed"]
                and result["compaction_route_verified"]
            )
            return result
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    result = {
        "source_commit": os.environ["QUALIFIED_CODEX_SOURCE_COMMIT"],
        "cli_sha256": hashlib.sha256(Path(os.environ["QUALIFIED_CODEX_BINARY"]).read_bytes()).hexdigest(),
        "cases": [qualify(mode, remote) for mode in ("legacy", "paginated")
                  for remote in (False, True)],
        "limits": "Loopback provider. Idle injected mail and repeated manual compaction; no mid-turn preemption or real-provider acceptance.",
    }
    result["passed"] = all(case["passed"] for case in result["cases"])
    Path(os.environ["DECODEX_QUALIFICATION_OUTPUT"], "pending-mail-native.json").write_text(
        json.dumps(result, indent=2) + "\n"
    )
    print(json.dumps(result, indent=2))
    assert result["passed"], "Unsampled native mail must survive compaction and cold continuation."
