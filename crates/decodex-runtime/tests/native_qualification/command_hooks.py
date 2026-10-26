"""Check command working directories in native hooks with an exact packaged CLI."""

import json
import os
import shlex
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from native_session import Session


def main():
    requests = []
    with tempfile.TemporaryDirectory(prefix="dx-command-hooks-", dir="/tmp") as temporary:
        home = Path(temporary).resolve()
        project = home / "project"
        nested = project / "nested"
        nested.mkdir(parents=True)
        (nested / "marker.txt").write_text("command-directory-preserved")
        log = home / "hook-events.jsonl"
        script = home / "record_hook.py"
        script.write_text(
            "import json, sys\nfrom pathlib import Path\n"
            "payload = json.load(sys.stdin)\n"
            "payload['process_cwd'] = str(Path.cwd())\n"
            f"with Path({str(log)!r}).open('a') as out:\n"
            "    out.write(json.dumps(payload) + '\\n')\n"
            "if payload['hook_event_name'] == 'PermissionRequest':\n"
            "    print(json.dumps({'hookSpecificOutput': {'hookEventName': 'PermissionRequest', "
            "'decision': {'behavior': 'allow'}}}))\n"
        )
        group = [{"matcher": "^Bash$", "hooks": [{
            "type": "command", "command": f"{shlex.quote(sys.executable)} {shlex.quote(str(script))}",
        }]}]
        (home / "hooks.json").write_text(json.dumps({"hooks": {
            event: group for event in ("PreToolUse", "PermissionRequest", "PostToolUse")
        }}))

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_POST(self):
                requests.append(json.loads(self.rfile.read(int(self.headers["Content-Length"]))))
                serial = len(requests)
                item = ({"type": "function_call", "id": "command-cwd", "call_id": "command-cwd",
                         "name": "exec_command", "arguments": json.dumps({
                             "cmd": "cat marker.txt", "workdir": "nested", "login": False,
                             "sandbox_permissions": "require_escalated", "yield_time_ms": 30000,
                         })} if serial == 1 else {
                             "type": "message", "role": "assistant", "id": "done", "content": [
                                 {"type": "output_text", "text": "Directory checked."}],
                         })
                frames = [
                    {"type": "response.created", "response": {"id": f"response-{serial}"}},
                    {"type": "response.output_item.done", "item": item},
                    {"type": "response.completed", "response": {"id": f"response-{serial}",
                        "usage": {"input_tokens": 20, "output_tokens": 3, "total_tokens": 23}}},
                ]
                data = "".join("data: " + json.dumps(frame) + "\n\n" for frame in frames).encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        serving = threading.Thread(target=server.serve_forever, daemon=True)
        serving.start()
        try:
            config = home / "config.toml"
            config.write_text(
                'model="gpt-5.6-sol"\nmodel_provider="fixture"\n'
                '[features]\nenable_request_compression=false\ncodex_hooks=true\n'
                '[analytics]\nenabled=false\n'
                f'[model_providers.fixture]\nname="fixture"\nbase_url="http://127.0.0.1:{server.server_port}"\n'
                'wire_api="responses"\nrequires_openai_auth=false\nsupports_websockets=false\n'
            )
            session = Session(str(home))
            try:
                entries = session.call("hooks/list", {"cwds": [str(project)]})["data"]
                hooks = [hook for entry in entries for hook in entry["hooks"]]
                assert len(hooks) == 3, "Discover all three fixture hooks before trusting them"
                with config.open("a") as output:
                    for hook in hooks:
                        output.write(f'\n[hooks.state.{json.dumps(hook["key"])}]\n'
                                     f'trusted_hash={json.dumps(hook["currentHash"])}\n')
            finally:
                session.close()
            session = Session(str(home))
            try:
                thread = session.call("thread/start", {
                    "cwd": str(project), "approvalPolicy": "on-request", "sandbox": "read-only",
                })["thread"]["id"]
                session.call("turn/start", {"threadId": thread, "input": [
                    {"type": "text", "text": "Run the controlled command."}]})
                session.done()
            finally:
                session.close()
            events = [json.loads(line) for line in log.read_text().splitlines()]
            assert [event["hook_event_name"] for event in events] == [
                "PreToolUse", "PermissionRequest", "PostToolUse"]
            observed = [event["cwd"] for event in events]
            assert observed == [str(nested)] * 3, f"Hook cwd must match execution: {observed}"
            assert all(event["process_cwd"] == str(project) for event in events)
            assert len(requests) == 2, "The command must run once, without model retries"
            outputs = [item["output"] for item in requests[1]["input"]
                       if item.get("type") == "function_call_output" and item.get("call_id") == "command-cwd"]
            assert len(outputs) == 1 and "command-directory-preserved" in str(outputs[0])
            print(json.dumps({"status": "passed", "hook_count": 3, "model_requests": 2,
                              "source_commit": os.environ["QUALIFIED_CODEX_SOURCE_COMMIT"],
                              "limits": "Isolated local command, hook stdin and hook process cwd; no remote executor."}))
        finally:
            server.shutdown()
            server.server_close()
            serving.join(timeout=5)


if __name__ == "__main__":
    main()
