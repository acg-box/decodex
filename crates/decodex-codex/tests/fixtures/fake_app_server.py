#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import sys
import time

if sys.argv[1] == "--version-hang":
    time.sleep(60)
    raise SystemExit(0)

if sys.argv[1] == "--version-oversized":
    sys.stdout.write("x" * 8192)
    raise SystemExit(0)

if sys.argv[1] == "--version":
    print("fake-codex 1")
    raise SystemExit(0)

if sys.argv[1] == "generate-json-schema":
    output = Path(sys.argv[-1])
    output.mkdir(parents=True, exist_ok=True)
    if "--orphan-pid" in sys.argv:
        index = sys.argv.index("--orphan-pid")
        child = subprocess.Popen([sys.executable, "-c", "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(60)"])
        Path(sys.argv[index + 1]).write_text(str(child.pid))
    requests = ["initialize", "account/read", "thread/start", "thread/list", "thread/search", "thread/read", "thread/resume", "thread/name/set", "turn/start", "account/rateLimits/read", "collaborationMode/list", "thread/archive"]
    notifications = ["thread/started", "turn/started", "item/started", "item/completed", "turn/completed"]
    if "--missing-required" in sys.argv:
        requests.remove("thread/list")
    if "--missing-optional-methods" in sys.argv:
        requests.remove("thread/read")
        requests.remove("thread/search")
        requests.remove("thread/archive")

    missing_optional = "--missing-optional" in sys.argv
    malformed_optional = "--malformed-optional" in sys.argv

    def method_schema(methods):
        return {"oneOf": [{"properties": {"method": {"enum": [method]}}} for method in methods]}

    (output / "ClientRequest.json").write_text(json.dumps(method_schema(requests)))
    (output / "ServerNotification.json").write_text(json.dumps(method_schema(notifications)))
    collaboration = {
        "definitions": {
            "CollabAgentState": {"type": "object"},
            "CollabAgentTool": {"type": "string", "enum": ["spawnAgent", "sendInput", "resumeAgent", "wait", "closeAgent"]},
            "CollabAgentToolCallStatus": {"type": "string", "enum": ["inProgress", "completed", "failed"]},
            "SubAgentActivityKind": {"type": "string", "enum": ["started", "interacted", "interrupted"]},
            "Thread": {
                "properties": {
                    "parentThreadId": {"type": ["string", "null"]},
                    "agentNickname": {"type": ["string", "null"]},
                    "agentRole": {"type": ["string", "null"]},
                }
            },
            "ThreadItem": {
                "oneOf": [
                    {
                        "type": "object",
                        "required": ["agentsStates", "id", "receiverThreadIds", "senderThreadId", "status", "tool", "type"],
                        "properties": {
                            "agentsStates": {"type": "object", "additionalProperties": {"$ref": "#/definitions/CollabAgentState"}},
                            "id": {"type": "string"},
                            "receiverThreadIds": {"type": "array", "items": {"type": "string"}},
                            "senderThreadId": {"type": "string"},
                            "status": {"allOf": [{"$ref": "#/definitions/CollabAgentToolCallStatus"}]},
                            "tool": {"allOf": [{"$ref": "#/definitions/CollabAgentTool"}]},
                            "type": {"enum": ["collabAgentToolCall"]},
                        },
                    },
                    {
                        "type": "object",
                        "required": ["agentPath", "agentThreadId", "id", "kind", "type"],
                        "properties": {
                            "agentPath": {"type": "string"},
                            "agentThreadId": {"type": "string"},
                            "id": {"type": "string"},
                            "kind": {"$ref": "#/definitions/SubAgentActivityKind"},
                            "type": {"enum": ["subAgentActivity"]},
                        },
                    },
                ]
            },
        }
    }
    (output / "codex_app_server_protocol.v2.schemas.json").write_text(json.dumps({"schema": "fake"}))
    (output / "v2").mkdir()
    if "--false-collaboration" in sys.argv:
        collaboration = {"description": "collabAgentToolCall parentThreadId agentNickname agentRole subAgentActivity"}
    collaboration_output = "{" if malformed_optional else json.dumps(collaboration)
    if "--oversized-schema" in sys.argv:
        collaboration_output += " " * (17 * 1024 * 1024)
    if not missing_optional:
        (output / "v2/ThreadReadResponse.json").write_text(collaboration_output)
        history = {
            "properties": {
                "historyMode": {
                    "anyOf": [
                        {"$ref": "#/definitions/ThreadHistoryMode"},
                        {"type": "null"},
                    ]
                }
            },
            "definitions": {
                "ThreadHistoryMode": {
                    "type": "string",
                    "enum": ["legacy", "paginated"],
                }
            },
        }
        (output / "v2/ThreadStartParams.json").write_text(
            "{" if malformed_optional else json.dumps(history)
        )
    if "--too-many-files" in sys.argv:
        for index in range(513):
            (output / f"extra-{index}.json").write_text("{}")
    if "--schema-symlink" in sys.argv:
        (output / "schema-link.json").symlink_to(output / "ClientRequest.json")
    if "--preflight-fail" in sys.argv:
        raise SystemExit(19)
    raise SystemExit(0)

assert sys.argv[1] == "serve"
mode = sys.argv[2]
if mode in ("mark-spawn", "schema-missing"):
    Path(sys.argv[3]).write_text("spawned")
if mode in ("orphan-exit", "orphan-stubborn", "orphan-error", "orphan-timeout"):
    child = subprocess.Popen([sys.executable, "-c", "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(60)"])
    Path(sys.argv[3]).write_text(str(child.pid))
    if mode == "orphan-exit":
        raise SystemExit(0)
    if mode == "orphan-error":
        print("not-json", flush=True)
        time.sleep(60)
        raise SystemExit(0)
    if mode == "orphan-timeout":
        for _ in sys.stdin:
            time.sleep(60)
        raise SystemExit(0)
    import signal
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    time.sleep(60)
    raise SystemExit(0)
if mode == "crash":
    print('{"accessToken":"sk-this-must-never-escape"}', file=sys.stderr)
    raise SystemExit(17)

account_reads = 0
for line in sys.stdin:
    message = json.loads(line)
    if mode == "hang":
        time.sleep(60)
    method = message.get("method")
    if method == "initialize":
        if mode == "oversized-frame":
            sys.stdout.write("{" + ("x" * (1024 * 1024 + 1)))
            sys.stdout.flush()
            time.sleep(60)
        if mode == "queue-overflow":
            for index in range(100):
                print(json.dumps({"jsonrpc": "2.0", "method": "unrelated", "params": {"index": index}}), flush=True)
        assert os.environ.get("OPENAI_API_KEY") is None
        result = {
            "codexHome": "/tmp/other-codex-home" if mode == "home-mismatch" else "/tmp/fake-codex-home",
            "platformFamily": "unix",
            "platformOs": "test",
            "userAgent": "fake-codex/1",
        }
    elif method == "account/read":
        account_reads += 1
        email = "changed@example.test" if mode == "account-switch" and account_reads > 1 else "private@example.test"
        account = None if mode == "account-none" else {"type": "chatgpt", "email": email}
        result = {"account": account, "requiresOpenaiAuth": True}
    elif method == "thread/list":
        assert message["params"]["useStateDbOnly"] is True
        count = 101 if mode == "oversized-thread-list" else 1
        result = {
            "data": [{"id": f"00000000-0000-4000-8000-{index:012d}", "archived": False, "parentThreadId": None} for index in range(1, count + 1)],
            "nextCursor": None,
        }
    elif method == "thread/read" and mode != "optional-unsupported":
        assert message["params"]["includeTurns"] is False
        result = {
            "thread": {
                "id": message["params"]["threadId"],
                "archived": False,
                "parentThreadId": None,
            }
        }
    elif method == "thread/search" and mode != "optional-unsupported":
        assert message["params"]["limit"] <= 10
        assert message["params"]["searchTerm"].startswith("decodex-capability-probe-")
        result = {"data": [], "nextCursor": None}
    elif method == "initialized":
        continue
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": message["id"], "error": {"code": -32601, "message": "unsupported"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": message["id"], "result": result}), flush=True)
