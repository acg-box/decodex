"""Check persisted model settings with an exact packaged native CLI and a local provider."""

import hashlib
import json
import os
from pathlib import Path
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from native_session import Session


def case(mode, unchanged=False):
    requests = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            requests.append(json.loads(self.rfile.read(int(self.headers["Content-Length"]))))
            serial = len(requests)
            frames = [
                {"type": "response.created", "response": {"id": f"response-{serial}"}},
                {"type": "response.output_item.done", "item": {
                    "type": "message", "id": f"message-{serial}", "role": "assistant",
                    "content": [{"type": "output_text", "text": "Preserved settings."}],
                }},
                {"type": "response.completed", "response": {
                    "id": f"response-{serial}",
                    "usage": {"input_tokens": 10, "output_tokens": 2, "total_tokens": 12},
                }},
            ]
            payload = "".join("data: " + json.dumps(frame) + "\n\n" for frame in frames).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    serving = threading.Thread(target=server.serve_forever, daemon=True)
    serving.start()
    try:
        with tempfile.TemporaryDirectory(prefix="dx-settings-", dir="/tmp") as home:
            config = Path(home, "config.toml")
            base = (
                'model="gpt-5.6-sol"\nmodel_provider="fixture"\n'
                '[features]\nenable_request_compression=false\nfast_mode=false\n'
                '[analytics]\nenabled=false\n'
                '[model_providers.fixture]\nname="fixture"\n'
                f'base_url="http://127.0.0.1:{server.server_port}"\n'
                'wire_api="responses"\nrequires_openai_auth=false\nsupports_websockets=false\n'
            )
            config.write_text(base)
            session = Session(home)
            try:
                started = session.call("thread/start", {
                    "cwd": home, "historyMode": mode, "approvalPolicy": "never", "sandbox": "read-only",
                })["thread"]
                thread = started["id"]
                if not unchanged:
                    session.call("thread/settings/update", {
                        "threadId": thread, "serviceTier": "flex", "summary": "concise",
                    })
                session.call("turn/start", {
                    "threadId": thread, "input": [{"type": "text", "text": "Retain these settings."}],
                })
                session.done()
                if not unchanged:
                    assert requests[-1]["service_tier"] == "flex"
                    assert requests[-1]["reasoning"]["summary"] == "concise"
            finally:
                session.close()

            def checkpoints():
                return [item["payload"] for line in Path(started["path"]).read_text().splitlines()
                        if (item := json.loads(line)).get("type") == "event_msg"
                        and item["payload"].get("type") == "thread_settings_applied"]

            if unchanged:
                before = checkpoints()
                assert before, "The materialized thread must contain settings"
            else:
                # A later global preference must not replace the saved thread selection.
                config.write_text('service_tier="default"\nmodel_reasoning_summary="detailed"\n' + base)
            session = Session(home)
            try:
                resumed = session.call("thread/resume", {"threadId": thread, "excludeTurns": True})
                if unchanged:
                    assert checkpoints() == before, "Cold resume appended redundant settings"
                    assert len(requests) == 1, "Cold resume must not run inference"
                    return {"history_mode": mode, "requests": len(requests),
                            "unchanged_settings_checkpoints": len(before), "resume_did_not_append": True}
                assert resumed["serviceTier"] == "flex", "Cold resume lost the selected service tier"
                session.call("turn/start", {
                    "threadId": thread, "input": [{"type": "text", "text": "Use the retained settings."}],
                })
                session.done()
                assert requests[-1]["service_tier"] == "flex"
                assert requests[-1]["reasoning"]["summary"] == "concise", "Cold resume lost the selected summary"
                assert len(requests) == 2, "The fixture must not replay inference"
            finally:
                session.close()
            return {"history_mode": mode, "requests": len(requests), "tier_and_summary_preserved": True}
    finally:
        server.shutdown()
        server.server_close()
        serving.join(timeout=5)


def main():
    results = [case(mode) for mode in ["legacy", "paginated"]] + [case("legacy", unchanged=True)]
    binary = Path(os.environ["QUALIFIED_CODEX_BINARY"])
    result = {
        "status": "passed", "cases": results, "case_count": len(results),
        "cli_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "source_commit": os.environ["QUALIFIED_CODEX_SOURCE_COMMIT"],
        "scope": "Packaged private-stdio CLI and local provider; no live account or installed-app acceptance.",
    }
    Path(os.environ["DECODEX_QUALIFICATION_OUTPUT"], "thread-settings-native.json").write_text(
        json.dumps(result, indent=2) + "\n"
    )
    print("Cold resume preserved tier and summary in both history modes and skipped unchanged settings checkpoints.")


if __name__ == "__main__":
    main()
