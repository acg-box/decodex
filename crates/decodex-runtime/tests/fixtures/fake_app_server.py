#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def spawn_ready_descendant(pid_path, sleep_seconds):
    child = subprocess.Popen(
        [
            sys.executable,
            "-c",
            (
                "import os,signal,sys,time; from pathlib import Path; "
                "signal.signal(signal.SIGTERM, signal.SIG_IGN); "
                "Path(sys.argv[1]).write_text(str(os.getpid())); "
                "time.sleep(float(sys.argv[2]))"
            ),
            str(pid_path),
            str(sleep_seconds),
        ]
    )
    deadline = time.monotonic() + 2
    while not pid_path.exists():
        if child.poll() is not None or time.monotonic() >= deadline:
            raise RuntimeError("descendant did not become ready")
        time.sleep(0.01)
    return child


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
    if "--preflight-hang" in sys.argv:
        time.sleep(60)
        raise SystemExit(0)
    output = Path(sys.argv[-1])
    output.mkdir(parents=True, exist_ok=True)
    if "--orphan-pid" in sys.argv:
        index = sys.argv.index("--orphan-pid")
        child = spawn_ready_descendant(Path(sys.argv[index + 1]), "0.25")
    requests = ["initialize", "account/read", "account/login/start", "thread/start", "thread/list", "thread/search", "thread/read", "thread/resume", "thread/name/set", "turn/start", "account/rateLimits/read", "collaborationMode/list", "thread/archive"]
    if "--reset-card" in sys.argv:
        requests.append("account/rateLimitResetCredit/consume")
    server_requests = ["account/chatgptAuthTokens/refresh"]
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
    (output / "ServerRequest.json").write_text(json.dumps(method_schema(server_requests)))
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
    login = {
        "oneOf": [
            {
                "type": "object",
                "required": ["accessToken", "chatgptAccountId"],
                "properties": {
                    "type": {"enum": ["chatgptAuthTokens"]},
                    "accessToken": {"type": "string"},
                    "chatgptAccountId": {"type": "string"},
                    "chatgptPlanType": {"type": ["string", "null"]},
                },
            }
        ]
    }
    refresh_params = {
        "type": "object",
        "required": ["reason"],
        "properties": {
            "reason": {"enum": ["unauthorized"]},
            "previousAccountId": {"type": ["string", "null"]},
        },
    }
    refresh_response = {
        "type": "object",
        "required": ["accessToken", "chatgptAccountId"],
        "properties": {
            "accessToken": {"type": "string"},
            "chatgptAccountId": {"type": "string"},
            "chatgptPlanType": {"type": ["string", "null"]},
        },
    }
    (output / "v2/LoginAccountParams.json").write_text(json.dumps(login))
    refresh_root = output / "v2" if "--nested-refresh-only" in sys.argv else output
    (refresh_root / "ChatgptAuthTokensRefreshParams.json").write_text(
        json.dumps(refresh_params)
    )
    (refresh_root / "ChatgptAuthTokensRefreshResponse.json").write_text(
        json.dumps(refresh_response)
    )
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
if mode in ("mark-spawn", "schema-missing", "nested-refresh-schema"):
    Path(sys.argv[3]).write_text("spawned")
if mode in ("orphan-exit", "orphan-stubborn", "orphan-error", "orphan-timeout"):
    if mode == "orphan-stubborn":
        import signal

        signal.signal(signal.SIGTERM, signal.SIG_IGN)
    descendant_sleep = {
        "orphan-exit": "0.25",
        "orphan-error": "1",
        "orphan-timeout": "7",
    }.get(mode, "60")
    child = spawn_ready_descendant(Path(sys.argv[3]), descendant_sleep)
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
    time.sleep(60)
    raise SystemExit(0)
if mode == "crash":
    print('{"accessToken":"sk-this-must-never-escape"}', file=sys.stderr)
    raise SystemExit(17)

account_reads = 0
login_count = 0
callback_serviced = False
exact_thread_id = "thread:XY-1317/non-uuid_Case-Sensitive._~:@+$,;=[]{}()!%&'*? #"
exact_thread = {
    "id": exact_thread_id,
    "archived": False,
    "parentThreadId": None,
    "createdAt": 1784073600,
    "name": "Decodex XY-1317 exact reconciliation",
    "cwd": "/tmp/xy-1317-repository",
    "threadSource": "decodex.xy1317.fixture",
}
exact_thread_reads = 0
reset_card_consumed = False
rate_limit_reads = 0
for line in sys.stdin:
    message = json.loads(line)
    assert "jsonrpc" not in message
    if mode == "hang":
        time.sleep(60)
    method = message.get("method")
    if method == "initialize":
        if mode in ("server-request", "server-request-id-collision"):
            server_request_id = (
                message["id"] if mode == "server-request-id-collision" else 90_001
            )
            print(json.dumps({
                "id": server_request_id,
                "method": "fixture/server/request",
                "params": {},
            }), flush=True)
            reply_line = sys.stdin.readline()
            assert reply_line
            reply = json.loads(reply_line)
            assert "jsonrpc" not in reply
            assert reply == {
                "id": server_request_id,
                "error": {
                    "code": -32601,
                    "message": "account-bound adapter does not service this request",
                },
            }
        if mode == "escaped-error":
            print(json.dumps({"id": message["id"], "error": {"code": -32000, "message": "secret\\quoted"}}), flush=True)
            continue
        if mode == "oversized-frame":
            sys.stdout.write("{" + ("x" * (1024 * 1024 + 1)))
            sys.stdout.flush()
            time.sleep(60)
        if mode == "queue-overflow":
            for index in range(100):
                print(json.dumps({"method": "unrelated", "params": {"index": index}}), flush=True)
        # Apple's /usr/bin/python3 launcher adds only these toolchain/runtime
        # variables after exec; the parent projection itself is HOME/PATH-only.
        assert set(os.environ).issubset({
            "CPATH",
            "HOME",
            "LC_CTYPE",
            "LIBRARY_PATH",
            "MANPATH",
            "PATH",
            "PYTHONNOUSERSITE",
            "SDKROOT",
            "__CF_USER_TEXT_ENCODING",
        })
        assert Path(os.environ["HOME"]).is_absolute()
        assert os.environ.get("PATH") == "/usr/bin:/bin:/usr/sbin:/sbin"
        result = {
            "codexHome": "/tmp/other-codex-home" if mode == "home-mismatch" else str(Path(os.environ["HOME"]) / ".codex"),
            "platformFamily": "unix",
            "platformOs": "test",
            "userAgent": {"late": "wrong"} if mode == "late-typed-error" else ("fake\"codex/1" if mode == "escaped-success" else "fake-codex/1"),
        }
    elif method == "account/read":
        account_reads += 1
        email = (
            "changed@example.test"
            if mode == "account-switch" and account_reads > 1
            else "decodex-callback-capability-probe.invalid"
            if mode == "callback-probe" and not callback_serviced
            else "private@example.test"
        )
        account = None if mode == "account-none" else {"type": "chatgpt", "email": email}
        result = {"account": account, "requiresOpenaiAuth": True}
    elif method == "account/login/start":
        login_count += 1
        assert message["params"]["type"] == "chatgptAuthTokens"
        if mode == "callback-probe":
            assert login_count == 1
            assert message["params"]["accessToken"] != "synthetic-successor-token"
            assert message["params"]["accessToken"].count(".") == 2
            assert message["params"]["chatgptAccountId"] == "callback-provider-account"
            assert message["params"]["chatgptPlanType"] == "business"
            assert not callback_serviced
            print(json.dumps({
                "id": message["id"],
                "method": "account/chatgptAuthTokens/refresh",
                "params": {
                    "reason": "unauthorized",
                    "previousAccountId": "callback-provider-account",
                },
            }), flush=True)
            reply_line = sys.stdin.readline()
            assert reply_line
            reply = json.loads(reply_line)
            assert reply == {
                "id": message["id"],
                "result": {
                    "accessToken": "synthetic-successor-token",
                    "chatgptAccountId": "callback-provider-account",
                    "chatgptPlanType": "business",
                },
            }
            callback_serviced = True
        else:
            assert message["params"]["accessToken"] == "synthetic-nonsecret-sentinel"
            assert message["params"]["chatgptAccountId"] == "synthetic-provider-sentinel"
        result = (
            {"type": "chatgptAuthTokens", "unexpected": "synthetic-nonsecret-sentinel"}
            if mode == "login-extra"
            else {"type": "chatgpt"}
            if mode == "login-wrong-type"
            else {}
            if mode == "login-missing-type"
            else {"type": "chatgptAuthTokens"}
        )
    elif method == "account/rateLimits/read" and mode in (
        "reset-card",
        "reset-card-partial-first",
        "callback-probe",
    ):
        rate_limit_reads += 1
        assert "params" in message
        assert message["params"] is None
        if mode == "callback-probe":
            assert callback_serviced
        credits = [] if reset_card_consumed else [{
            "id": "fixture-reset-credit",
            "grantedAt": 1700000000,
            "expiresAt": 1700003600,
            "resetType": "codexRateLimits",
            "status": "available",
            "title": "fixture reset card",
            "description": None,
        }]
        result = {
            "rateLimits": {},
            "rateLimitResetCredits": {
                "availableCount": len(credits),
                "credits": (
                    None
                    if mode == "reset-card-partial-first" and rate_limit_reads == 1
                    else credits
                ),
            },
        }
    elif method == "account/rateLimitResetCredit/consume" and mode == "reset-card":
        assert message["params"] == {
            "creditId": "fixture-reset-credit",
            "idempotencyKey": "fixture-reset-operation",
        }
        reset_card_consumed = True
        result = {"outcome": "reset"}
    elif method == "thread/list":
        if message["params"].get("searchTerm", "").startswith(
            "decodex-capability-probe-no-match-"
        ):
            assert set(message["params"]) == {"limit", "searchTerm", "useStateDbOnly"}
            assert message["params"]["useStateDbOnly"] is True
            assert message["params"]["limit"] <= 100
            count = 0 if mode == "optional-unsupported" else 1
            if mode == "oversized-thread-list":
                count = 101
            result = {
                "data": [
                    {
                        "id": f"00000000-0000-4000-8000-{index:012d}",
                        "archived": False,
                        "parentThreadId": None,
                    }
                    for index in range(1, count + 1)
                ],
                "nextCursor": None,
            }
        elif "searchTerm" in message["params"]:
            assert set(message["params"]) == {"archived", "limit", "searchTerm"}
            assert message["params"]["searchTerm"] == exact_thread["name"]
            assert message["params"]["limit"] <= 100
            matches_archive = message["params"]["archived"] == exact_thread["archived"]
            data = [dict(exact_thread)] if matches_archive else []
            if mode == "exact-malformed-list":
                data = [{**exact_thread, "createdAt": "not-a-timestamp"}]
            result = {"data": data, "nextCursor": None}
        else:
            assert message["params"]["useStateDbOnly"] is True
            count = 101 if mode == "oversized-thread-list" else 1
            result = {
                "data": [{"id": f"00000000-0000-4000-8000-{index:012d}", "archived": False, "parentThreadId": None} for index in range(1, count + 1)],
                "nextCursor": None,
            }
    elif method == "thread/read" and mode != "optional-unsupported":
        if message["params"]["includeTurns"]:
            exact_thread_reads += 1
            if mode == "exact-missing-post-archive-read" and exact_thread_reads > 1:
                print(json.dumps({"id": message["id"]}), flush=True)
                continue
            if mode == "exact-oversized-read":
                sys.stdout.write("{" + ("x" * (1024 * 1024 + 1)))
                sys.stdout.flush()
                time.sleep(60)
            readback = dict(exact_thread)
            if mode == "exact-mismatched-id" or (mode == "exact-mismatched-post-archive-read" and exact_thread_reads > 1):
                readback["id"] = "thread:different"
            if mode == "exact-malformed-read":
                readback["cwd"] = {"not": "text"}
            result = {"thread": readback}
        else:
            result = {
                "thread": {
                    "id": message["params"]["threadId"],
                    "archived": False,
                    "parentThreadId": None,
                }
            }
    elif method == "thread/archive":
        assert message["params"] == {"threadId": exact_thread_id}
        if mode == "exact-unsupported-archive":
            print(json.dumps({"id": message["id"], "error": {"code": -32601, "message": "unsupported"}}), flush=True)
            continue
        if mode not in ("exact-ambiguous-unapplied", "exact-contradictory-readback"):
            exact_thread["archived"] = True
        if mode in ("exact-ambiguous-unapplied", "exact-drop-after-apply"):
            continue
        result = {}
    elif method == "thread/search" and mode != "optional-unsupported":
        assert message["params"]["limit"] <= 10
        assert message["params"]["searchTerm"].startswith("decodex-capability-probe-")
        result = {"data": [], "nextCursor": None}
    elif method == "initialized":
        continue
    else:
        print(json.dumps({"id": message["id"], "error": {"code": -32601, "message": "unsupported"}}), flush=True)
        continue
    if mode == "exact-wrong-correlation" and method == "thread/list" and "searchTerm" in message["params"]:
        print(json.dumps({"id": message["id"] + 1, "result": result}), flush=True)
    elif mode == "exact-missing-result" and method == "thread/list" and "searchTerm" in message["params"]:
        print(json.dumps({"id": message["id"]}), flush=True)
    else:
        response = {"id": message["id"], "result": result}
        if mode == "legacy-jsonrpc":
            response["jsonrpc"] = "2.0"
        elif mode == "wrong-jsonrpc":
            response["jsonrpc"] = "1.0"
        elif mode == "null-jsonrpc":
            response["jsonrpc"] = None
        print(json.dumps(response), flush=True)

if mode == "callback-probe":
    assert login_count == 1
    assert callback_serviced
