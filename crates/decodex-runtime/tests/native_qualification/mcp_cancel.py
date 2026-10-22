"""Check native MCP request cancellation and connection reuse with a packaged CLI."""

import hashlib
import json
import os
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


def mcp_server(record):
    def send(identity, result):
        print(json.dumps({"jsonrpc": "2.0", "id": identity, "result": result}), flush=True)

    for line in sys.stdin:
        frame = json.loads(line)
        method = frame.get("method")
        if method in ["initialize", "tools/call", "notifications/cancelled"]:
            with Path(record).open("a") as output:
                output.write(json.dumps(frame) + "\n")
        if method == "initialize":
            send(frame["id"], {"protocolVersion": frame["params"]["protocolVersion"],
                               "capabilities": {"tools": {}}, "serverInfo": {"name": "fixture", "version": "1"}})
        elif method == "tools/list":
            send(frame["id"], {"tools": [
                {"name": name, "description": "Controlled cancellation fixture.",
                 "inputSchema": {"type": "object", "properties": {}}}
                for name in ["wait", "next"]
            ]})
        elif method in ["tools/call", "notifications/cancelled"]:
            if method == "tools/call" and frame["params"]["name"] == "next":
                send(frame["id"], {"content": [{"type": "text", "text": "Connection preserved."}], "isError": False})
        elif "id" in frame:
            send(frame["id"], {})


def wait_for(predicate):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.02)
    raise TimeoutError("Expected controlled MCP event")


def case(mode):
    from native_session import Session
    from finished_sample import finished

    requests = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            requests.append(json.loads(self.rfile.read(int(self.headers["Content-Length"]))))
            serial = len(requests)
            item = ({"type": "function_call", "id": "call-" + str(serial), "call_id": "call-" + str(serial),
                     "namespace": "mcp__fixture", "name": "wait" if serial == 1 else "next", "arguments": "{}"}
                    if serial < 3 else {"type": "message", "role": "assistant", "id": "done", "content": [
                        {"type": "output_text", "text": "Connection preserved."}]})
            frames = [
                {"type": "response.created", "response": {"id": "response-" + str(serial)}},
                {"type": "response.output_item.done", "item": item},
                {"type": "response.completed", "response": {"id": "response-" + str(serial),
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
        with tempfile.TemporaryDirectory(prefix="dx-mcp-cancel-", dir="/tmp") as home:
            record = Path(home, "mcp-events.jsonl")
            Path(home, "config.toml").write_text(
                'model="gpt-5.6-sol"\nmodel_provider="fixture"\n'
                '[features]\nenable_request_compression=false\n'
                '[analytics]\nenabled=false\n'
                f'[model_providers.fixture]\nname="fixture"\nbase_url="http://127.0.0.1:{server.server_port}"\n'
                'wire_api="responses"\nrequires_openai_auth=false\nsupports_websockets=false\n'
                '[mcp_servers.fixture]\ncommand="/usr/bin/python3"\n'
                f'args=[{json.dumps(str(Path(__file__).resolve()))},"--server",{json.dumps(str(record))}]\n'
                'required=true\ndefault_tools_approval_mode="approve"\n'
            )
            session = Session(home)
            try:
                thread = session.call("thread/start", {
                    "cwd": home, "historyMode": mode, "approvalPolicy": "never", "sandbox": "danger-full-access",
                })["thread"]["id"]
                turn = session.call("turn/start", {"threadId": thread, "input": [
                    {"type": "text", "text": "Start the controlled MCP call."}]})["turn"]["id"]

                def events():
                    return [json.loads(line) for line in record.read_text().splitlines()] if record.exists() else []

                call = wait_for(lambda: next((f for f in events() if f.get("method") == "tools/call"), None))
                assert call["params"]["name"] == "wait"
                session.call("turn/interrupt", {"threadId": thread, "turnId": turn})
                finished(session, thread, "interrupted")
                canceled = wait_for(lambda: next((f for f in events() if f.get("method") == "notifications/cancelled"), None))
                assert canceled["params"]["requestId"] == call["id"], "Cancellation must retain the exact request identity"
                session.call("turn/start", {"threadId": thread, "input": [
                    {"type": "text", "text": "Use the same MCP connection again."}]})
                session.done()
                calls = [f for f in events() if f.get("method") == "tools/call"]
                assert [f["params"]["name"] for f in calls] == ["wait", "next"]
                assert calls[1]["id"] != calls[0]["id"]
                assert len([f for f in events() if f.get("method") == "initialize"]) == 1, "MCP must not reconnect"
                assert len([f for f in events() if f.get("method") == "notifications/cancelled"]) == 1
                assert len(requests) == 3, "Cancellation must not replay inference or tool calls"
                return {"history_mode": mode, "requests": len(requests), "exact_request_canceled": True,
                        "connection_reused": True, "initializations": 1,
                        "tool_calls": len(calls), "cancellations": 1}
            finally:
                session.close()
    finally:
        server.shutdown()
        server.server_close()
        serving.join(timeout=5)


def main():
    results = [case(mode) for mode in ["legacy", "paginated"]]
    result = {"status": "passed", "cases": results, "case_count": len(results),
              "cli_sha256": hashlib.sha256(Path(os.environ["QUALIFIED_CODEX_BINARY"]).read_bytes()).hexdigest(),
              "source_commit": os.environ["QUALIFIED_CODEX_SOURCE_COMMIT"],
              "scope": "Exact packaged private-stdio CLI, stdio MCP fixture and loopback provider; no live account or installed app."}
    Path(os.environ["DECODEX_QUALIFICATION_OUTPUT"], "mcp-cancel-native.json").write_text(json.dumps(result, indent=2) + "\n")
    print("Native turn interruption canceled the exact MCP request and preserved the connection in both history modes.")


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--server":
        mcp_server(sys.argv[2])
    else:
        main()
