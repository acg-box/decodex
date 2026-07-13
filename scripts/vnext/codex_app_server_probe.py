#!/usr/bin/env python3
"""Run redacted XY-1262 probes against the real Codex app-server.

The probe intentionally uses the normal shared Codex home. Stored account tokens are
read only long enough to perform process-scoped ``account/login/start`` requests and
are never written to stdout, stderr, or the receipt. The normal ``auth.json`` is
hashed before and after each run so credential-state mutation is observable.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import select
import subprocess
import tempfile
import time
from typing import Any


PROBE_SCHEMA = "decodex/vnext-codex-proof/1"
CLIENT_INFO = {"name": "decodex-xy-1262-probe", "version": "1"}
DEFAULT_TIMEOUT = 90.0


class ProtocolError(RuntimeError):
    pass


class AppServer:
    def __init__(self, codex: str, cwd: Path) -> None:
        self.process = subprocess.Popen(
            [codex, "app-server", "--stdio"],
            cwd=cwd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self.next_id = 1
        self.notifications: list[dict[str, Any]] = []
        self.capability_observations: list[dict[str, Any]] = []
        self.initialize()

    def send(self, message: dict[str, Any]) -> None:
        if self.process.stdin is None:
            raise ProtocolError("app-server stdin is unavailable")
        self.process.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
        self.process.stdin.flush()

    def receive(self, timeout: float = DEFAULT_TIMEOUT) -> dict[str, Any]:
        if self.process.stdout is None:
            raise ProtocolError("app-server stdout is unavailable")
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            remaining = max(0.0, deadline - time.monotonic())
            ready, _, _ = select.select([self.process.stdout], [], [], remaining)
            if not ready:
                break
            line = self.process.stdout.readline()
            if not line:
                raise ProtocolError(f"app-server exited with {self.process.poll()}")
            try:
                return json.loads(line)
            except json.JSONDecodeError as error:
                raise ProtocolError("app-server emitted non-JSON stdout") from error
        raise ProtocolError("timed out waiting for app-server message")

    def request(
        self, method: str, params: Any | None = None, timeout: float = DEFAULT_TIMEOUT
    ) -> Any:
        request_id = self.next_id
        self.next_id += 1
        message: dict[str, Any] = {"jsonrpc": "2.0", "id": request_id, "method": method}
        if params is not None:
            message["params"] = params
        self.send(message)
        while True:
            incoming = self.receive(timeout)
            if incoming.get("id") == request_id:
                if "error" in incoming:
                    error = incoming["error"]
                    raise ProtocolError(
                        f"{method} failed: code={error.get('code')} message={error.get('message')}"
                    )
                return incoming.get("result")
            if "method" in incoming and "id" not in incoming:
                self.notifications.append(incoming)
                continue
            if "method" in incoming and "id" in incoming:
                self.send(
                    {
                        "jsonrpc": "2.0",
                        "id": incoming["id"],
                        "error": {
                            "code": -32601,
                            "message": "XY-1262 probe does not service server requests",
                        },
                    }
                )

    def wait_notification(
        self, method: str, predicate=lambda _params: True, timeout: float = DEFAULT_TIMEOUT
    ) -> dict[str, Any]:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for index, notification in enumerate(self.notifications):
                if notification.get("method") == method and predicate(
                    notification.get("params", {})
                ):
                    return self.notifications.pop(index)
            incoming = self.receive(max(0.1, deadline - time.monotonic()))
            if "method" in incoming and "id" not in incoming:
                self.notifications.append(incoming)
            elif "method" in incoming and "id" in incoming:
                self.send(
                    {
                        "jsonrpc": "2.0",
                        "id": incoming["id"],
                        "error": {
                            "code": -32601,
                            "message": "XY-1262 probe does not service server requests",
                        },
                    }
                )
        raise ProtocolError(f"timed out waiting for notification {method}")

    def initialize(self) -> None:
        self.initialize_result = self.request(
            "initialize",
            {
                "clientInfo": CLIENT_INFO,
                "capabilities": {"experimentalApi": True},
            },
        )
        self.send({"jsonrpc": "2.0", "method": "initialized"})

    def login(self, account: dict[str, str]) -> None:
        self.request(
            "account/login/start",
            {
                "type": "chatgptAuthTokens",
                "accessToken": account["access_token"],
                "chatgptAccountId": account["account_id"],
                "chatgptPlanType": account.get("plan_type"),
            },
        )

    def close(self, crash: bool = False) -> None:
        if self.process.poll() is not None:
            return
        if crash:
            self.process.kill()
        else:
            self.process.terminate()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=10)


def sha256_file(path: Path) -> str | None:
    if not path.exists():
        return None
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_canonical_json(path: Path) -> str:
    value = json.loads(path.read_text())
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def read_accounts(path: Path) -> list[dict[str, Any]]:
    accounts = []
    with path.open(encoding="utf-8") as handle:
        for line in handle:
            if line.strip():
                accounts.append(json.loads(line))
    return accounts


def account_login(accounts: list[dict[str, Any]], selector: str) -> dict[str, str]:
    for account in accounts:
        tokens = account.get("tokens") or {}
        candidates = {
            account.get("email"),
            tokens.get("email"),
            tokens.get("account_id"),
        }
        if selector not in candidates:
            continue
        access_token = tokens.get("access_token")
        account_id = tokens.get("account_id")
        if not access_token or not account_id:
            raise ProtocolError("selected account lacks usable tokens")
        return {
            "access_token": access_token,
            "account_id": account_id,
            "plan_type": tokens.get("plan_type"),
        }
    raise ProtocolError("account selector was not found")


def thread_id(result: dict[str, Any]) -> str:
    return result["thread"]["id"]


def start_named_thread(server: AppServer, cwd: Path, name: str) -> str:
    params = {
        "cwd": str(cwd),
        "ephemeral": False,
        "historyMode": "paginated",
        "developerInstructions": (
            "This is a read-only Decodex XY-1262 protocol probe. Do not call tools, "
            "change files, or create side effects. Reply only with the requested marker."
        ),
        "threadSource": "decodex.xy1262.probe",
    }
    try:
        result = server.request("thread/start", params)
        server.capability_observations.append(
            {"capability": "paginated_threads", "result": "supported"}
        )
    except ProtocolError as error:
        if "paginated_threads is not supported yet" not in str(error):
            raise
        server.capability_observations.append(
            {
                "capability": "paginated_threads",
                "result": "schema_advertised_live_rejected",
                "error_code": -32601,
            }
        )
        params["historyMode"] = "legacy"
        result = server.request("thread/start", params)
    identifier = thread_id(result)
    server.request("thread/name/set", {"threadId": identifier, "name": name})
    return identifier


def run_turn(server: AppServer, identifier: str, prompt: str) -> dict[str, Any]:
    response = server.request(
        "turn/start",
        {
            "threadId": identifier,
            "input": [{"type": "text", "text": prompt}],
        },
    )
    turn_identifier = response["turn"]["id"]
    deadline = time.monotonic() + 45.0
    while time.monotonic() < deadline:
        for index, notification in enumerate(server.notifications):
            method = notification.get("method")
            params = notification.get("params", {})
            if params.get("threadId") != identifier:
                continue
            if method == "turn/completed" and params.get("turn", {}).get("id") == turn_identifier:
                server.notifications.pop(index)
                return params["turn"]
            if method == "error" and params.get("turnId") in (None, turn_identifier):
                server.notifications.pop(index)
                error = params.get("error", {})
                info = error.get("codexErrorInfo")
                if isinstance(info, dict):
                    info = info.get("type")
                raise ProtocolError(f"turn failed: codex_error_info={info or 'unknown'}")
        incoming = server.receive(max(0.1, deadline - time.monotonic()))
        if "method" in incoming and "id" not in incoming:
            server.notifications.append(incoming)
        elif "method" in incoming and "id" in incoming:
            server.send(
                {
                    "jsonrpc": "2.0",
                    "id": incoming["id"],
                    "error": {
                        "code": -32601,
                        "message": "XY-1262 probe does not service server requests",
                    },
                }
            )
    raise ProtocolError("timed out waiting for turn terminal event")


def rate_limit_windows(server: AppServer) -> list[dict[str, Any]]:
    result = server.request("account/rateLimits/read")
    buckets = result.get("rateLimitsByLimitId") or {"legacy": result.get("rateLimits", {})}
    windows = []
    for limit_id, bucket in sorted(buckets.items()):
        for field in ("primary", "secondary"):
            window = bucket.get(field)
            if not window:
                continue
            windows.append(
                {
                    "limit_id": limit_id,
                    "source_field": field,
                    "duration_minutes": window.get("windowDurationMins"),
                    "used_percent": window.get("usedPercent"),
                    "resets_at": window.get("resetsAt"),
                }
            )
    return windows


def summarize_thread(thread: dict[str, Any]) -> dict[str, Any]:
    turns = thread.get("turns", [])
    return {
        "id": thread.get("id"),
        "name": thread.get("name"),
        "cwd": thread.get("cwd"),
        "ephemeral": thread.get("ephemeral"),
        "source": thread.get("source"),
        "thread_source": thread.get("threadSource"),
        "history_mode": thread.get("historyMode"),
        "parent_thread_id": thread.get("parentThreadId"),
        "turn_count": len(turns),
        "turn_item_types": sorted(
            {
                item.get("type", "unknown")
                for turn in turns
                for item in turn.get("items", [])
            }
        ),
    }


def live_probe(args: argparse.Namespace) -> dict[str, Any]:
    codex_home = Path(args.codex_home).expanduser().resolve()
    auth_path = codex_home / "auth.json"
    accounts_path = Path(args.accounts_file).expanduser().resolve()
    accounts = read_accounts(accounts_path)
    account_a = account_login(accounts, args.account_a)
    account_b = account_login(accounts, args.account_b)
    if account_a["account_id"] == account_b["account_id"]:
        raise ProtocolError("account A and account B resolve to the same account")

    receipt: dict[str, Any] = {
        "schema": PROBE_SCHEMA,
        "observed_at_unix": int(time.time()),
        "codex_cli": subprocess.check_output(
            [args.codex, "--version"], text=True
        ).strip(),
        "repository_head": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=args.cwd, text=True
        ).strip(),
        "shared_home": "~/.codex",
        "per_run_codex_home": False,
        "account_aliases": ["A", "B"],
        "experiments": {},
    }

    name = f"Decodex XY-1262 shared-home proof {int(time.time())}"
    server_a = AppServer(args.codex, args.cwd)
    try:
        server_a.login(account_a)
        identifier = start_named_thread(server_a, args.cwd, name)
        run_turn(server_a, identifier, "Reply exactly XY1262_ACCOUNT_A_DURABLE.")
        receipt["initialize"] = server_a.initialize_result
        receipt["capability_negotiation"] = server_a.capability_observations
        receipt["account_a_windows"] = rate_limit_windows(server_a)
        receipt["experiments"]["account_a_created"] = True
    finally:
        server_a.close(crash=True)

    server_b = AppServer(args.codex, args.cwd)
    try:
        server_b.login(account_b)
        listed = server_b.request(
            "thread/list",
            {"cwd": str(args.cwd), "searchTerm": name, "limit": 20},
        )
        cwd_threads = server_b.request(
            "thread/list", {"cwd": str(args.cwd), "limit": 100}
        ).get("data", [])
        searched = server_b.request("thread/search", {"searchTerm": name, "limit": 20})
        read_before = server_b.request(
            "thread/read", {"threadId": identifier, "includeTurns": True}
        )["thread"]
        resumed = server_b.request("thread/resume", {"threadId": identifier})["thread"]
        receipt["account_b_windows"] = rate_limit_windows(server_b)
        try:
            run_turn(server_b, identifier, "Reply exactly XY1262_ACCOUNT_B_CONTINUED.")
            receipt["experiments"]["cross_account_turn_continuation"] = True
        except ProtocolError as error:
            receipt["experiments"]["cross_account_turn_continuation"] = {
                "result": "failed",
                "error_class": type(error).__name__,
                "error": str(error),
            }
        read_after = server_b.request(
            "thread/read", {"threadId": identifier, "includeTurns": True}
        )["thread"]
        receipt["experiments"]["restart_list_match"] = any(
            item.get("id") == identifier for item in listed.get("data", [])
        )
        receipt["experiments"]["restart_search_match"] = any(
            item.get("id") == identifier for item in searched.get("data", [])
        )
        receipt["experiments"]["ownership_isolation"] = {
            "listed_thread_count_for_cwd": len(cwd_threads),
            "decodex_mapped_count": 1,
            "ignored_non_decodex_count": sum(
                item.get("id") != identifier for item in cwd_threads
            ),
            "mapping_source": "thread_id_returned_by_decodex_thread_start_only",
        }
        receipt["experiments"]["cross_account_same_thread_resume"] = (
            resumed.get("id") == identifier
        )
        receipt["experiments"]["read_before"] = summarize_thread(read_before)
        receipt["experiments"]["read_after"] = summarize_thread(read_after)
    finally:
        server_b.close(crash=True)

    # A process-scoped bad token proves the auth-failed boundary without changing the
    # account pool or normal auth.json. Resume may load local history; a real turn must
    # fail authentication before any fallback session is created.
    auth_failed = AppServer(args.codex, args.cwd)
    try:
        try:
            auth_failed.login(
                {
                    "access_token": "xy1262-invalid-process-token",
                    "account_id": "xy1262-invalid-process-account",
                }
            )
            receipt["experiments"]["auth_failed_boundary"] = "unexpected_login_success"
        except ProtocolError as error:
            receipt["experiments"]["auth_failed_boundary"] = {
                "result": "login_rejected_before_resume_or_turn",
                "error_class": type(error).__name__,
                "error_code": -32603,
            }
    finally:
        auth_failed.close(crash=True)

    fallback = AppServer(args.codex, args.cwd)
    try:
        fallback.login(account_a)
        fallback_name = f"{name} context-pack fallback"
        fallback_id = start_named_thread(fallback, args.cwd, fallback_name)
        context_pack = (
            "Context Pack v1: prior_runtime_session="
            + identifier
            + "; durable_marker=XY1262_ACCOUNT_A_DURABLE; "
            "possible_side_effects=none; reconciliation=thread/read plus repository HEAD. "
            "Reply exactly XY1262_CONTEXT_PACK_CONTINUED."
        )
        run_turn(fallback, fallback_id, context_pack)
        fallback_read = fallback.request(
            "thread/read", {"threadId": fallback_id, "includeTurns": True}
        )["thread"]
        fallback.request("thread/archive", {"threadId": fallback_id})
        archived = fallback.request(
            "thread/list", {"archived": True, "searchTerm": fallback_name, "limit": 20}
        )
        archive_visible = any(
            item.get("id") == fallback_id for item in archived.get("data", [])
        )
        fallback.request("thread/unarchive", {"threadId": fallback_id})
        receipt["experiments"]["context_pack_fallback"] = {
            "new_runtime_session": fallback_id != identifier,
            "thread": summarize_thread(fallback_read),
            "side_effect_reconciliation": "none_declared_and_repository_head_pinned",
            "explicit_archive_round_trip": archive_visible,
        }
    finally:
        fallback.close(crash=False)

    if sha256_file(auth_path) != args.auth_sha256_before:
        raise ProtocolError("normal Codex auth state changed during the process-scoped probe")
    receipt["auth_state_unchanged"] = True
    receipt["credentials_emitted"] = False
    return receipt


def schema_receipt(args: argparse.Namespace) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="xy1262-schema-") as directory:
        out = Path(directory)
        subprocess.run(
            [
                args.codex,
                "app-server",
                "generate-json-schema",
                "--experimental",
                "--out",
                str(out),
            ],
            check=True,
        )
        client_request = json.loads((out / "ClientRequest.json").read_text())
        server_notification = json.loads((out / "ServerNotification.json").read_text())

        def methods(schema: dict[str, Any]) -> list[str]:
            found = []
            for variant in schema.get("oneOf", []):
                values = variant.get("properties", {}).get("method", {}).get("enum", [])
                found.extend(value for value in values if isinstance(value, str))
            return sorted(found)

        request_methods = methods(client_request)
        notification_methods = methods(server_notification)
        collaboration_schema = (out / "v2/ThreadReadResponse.json").read_text()
        required_requests = [
            "initialize",
            "thread/start",
            "thread/list",
            "thread/search",
            "thread/read",
            "thread/resume",
            "thread/name/set",
            "turn/start",
            "account/rateLimits/read",
            "collaborationMode/list",
        ]
        required_notifications = [
            "thread/started",
            "turn/started",
            "item/started",
            "item/completed",
            "turn/completed",
        ]
        missing = sorted(
            set(required_requests) - set(request_methods)
            | set(required_notifications) - set(notification_methods)
        )
        if missing:
            raise ProtocolError(f"generated schema lacks required methods: {missing}")
        collaboration_markers = [
            "collabAgentToolCall",
            "parentThreadId",
            "agentNickname",
            "agentRole",
            "subAgentActivity",
        ]
        missing_collaboration = sorted(
            marker for marker in collaboration_markers if marker not in collaboration_schema
        )
        if missing_collaboration:
            raise ProtocolError(
                f"generated schema lacks collaboration markers: {missing_collaboration}"
            )
        return {
            "schema": "decodex/vnext-codex-schema-receipt/1",
            "observed_at_unix": int(time.time()),
            "codex_cli": subprocess.check_output(
                [args.codex, "--version"], text=True
            ).strip(),
            "experimental": True,
            "bundle_sha256": {
                name: sha256_canonical_json(out / name)
                for name in (
                    "ClientRequest.json",
                    "ServerNotification.json",
                    "codex_app_server_protocol.v2.schemas.json",
                )
            },
            "required_request_methods": required_requests,
            "required_notification_methods": required_notifications,
            "validated_collaboration_markers": collaboration_markers,
            "subagent_thread_fields": [
                "parentThreadId",
                "agentNickname",
                "agentRole",
            ],
            "quota_window_identity_field": "windowDurationMins",
            "positional_window_fields_are_identity": False,
            "missing": missing,
        }


def classify_quota_case(case: dict[str, Any]) -> dict[str, Any]:
    inputs = case["input"]
    if "accounts" in inputs:
        ready = {
            account["id"]: max(account["depleted_resets_at"])
            for account in inputs["accounts"]
        }
        return {
            "state": "depleted",
            "decision": "waiting_usage",
            "account_ready_at": ready,
            "earliest_ready_at": min(ready.values()),
        }
    if inputs.get("auth") == "failed":
        return {"state": "auth_failed", "decision": "excluded_auth", "ready_at": None}
    stale_after = inputs.get("stale_after_seconds")
    if stale_after is not None and inputs["now"] - inputs["observed_at"] > stale_after:
        return {"state": "unknown", "decision": "probe_required", "ready_at": None}

    by_duration = {window["duration_minutes"]: window for window in inputs["windows"]}
    if 300 not in by_duration or 10080 not in by_duration:
        return {"state": "unknown", "decision": "probe_required", "ready_at": None}
    classified = {
        "five_hour_source": by_duration[300]["source_field"],
        "seven_day_source": by_duration[10080]["source_field"],
    }
    depleted = [
        window for window in by_duration.values() if window["remaining_percent"] == 0
    ]
    if depleted and any(window["reset_at"] <= inputs["now"] for window in depleted):
        return {
            "state": "unknown",
            "decision": "probe_required",
            "ready_at": None,
            "classified": classified,
        }
    result: dict[str, Any]
    if depleted:
        result = {
            "state": "depleted",
            "decision": "excluded_usage",
            "ready_at": max(window["reset_at"] for window in depleted),
        }
    else:
        result = {"state": "available", "decision": "usable", "ready_at": inputs["now"]}
    result["classified"] = classified
    return result


def validate_checked_receipts(repository: Path) -> dict[str, Any]:
    fixture_dir = repository / "openwiki/evidence/fixtures"
    bundle = json.loads((fixture_dir / "xy-1262-live-receipt.json").read_text())
    collaboration = json.loads(
        (fixture_dir / "xy-1262-native-collaboration.json").read_text()
    )
    quota = json.loads((fixture_dir / "xy-1262-quota-matrix.json").read_text())
    if bundle.get("schema") != "decodex/vnext-codex-evidence-bundle/1":
        raise ProtocolError("checked live evidence bundle has the wrong schema")
    if bundle.get("overall_acceptance") is not False:
        raise ProtocolError("XY-1262 checked evidence must retain the failed gate verdict")

    forbidden_keys = {"access_token", "refresh_token", "id_token", "email", "auth_sha256"}

    def check_redaction(value: Any) -> None:
        if isinstance(value, dict):
            for key, child in value.items():
                if key.lower() in forbidden_keys or "token" in key.lower():
                    raise ProtocolError(f"credential-shaped key in evidence bundle: {key}")
                check_redaction(child)
        elif isinstance(value, list):
            for child in value:
                check_redaction(child)
        elif isinstance(value, str) and "@" in value:
            raise ProtocolError("account-like identity in evidence bundle")

    check_redaction(bundle)
    check_redaction(collaboration)
    if collaboration.get("readbacks", {}).get("thread_list_ancestor", {}).get(
        "parent_thread_id_matches"
    ) is not True:
        raise ProtocolError("native collaboration receipt lacks parent/child readback")
    mismatches = []
    for case in quota["cases"]:
        actual = classify_quota_case(case)
        if any(actual.get(key) != value for key, value in case["expect"].items()):
            mismatches.append({"id": case["id"], "actual": actual, "expect": case["expect"]})
    if mismatches:
        raise ProtocolError(f"quota matrix mismatch: {mismatches}")
    return {
        "schema": "decodex/vnext-codex-evidence-validation/1",
        "live_bundle_redacted": True,
        "quota_cases_validated": len(quota["cases"]),
        "overall_acceptance": False,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("schema", "live", "validate"))
    parser.add_argument("--codex", default="codex")
    parser.add_argument("--cwd", type=Path, default=Path.cwd())
    parser.add_argument("--codex-home", default=str(Path.home() / ".codex"))
    parser.add_argument(
        "--accounts-file", default=str(Path.home() / ".codex/decodex/accounts.jsonl")
    )
    parser.add_argument("--account-a")
    parser.add_argument("--account-b")
    args = parser.parse_args()
    args.cwd = args.cwd.resolve()
    args.auth_sha256_before = sha256_file(
        Path(args.codex_home).expanduser().resolve() / "auth.json"
    )
    if args.mode == "live" and (not args.account_a or not args.account_b):
        parser.error("live mode requires --account-a and --account-b")
    return args


def main() -> int:
    args = parse_args()
    auth_path = Path(args.codex_home).expanduser().resolve() / "auth.json"
    try:
        if args.mode == "schema":
            receipt = schema_receipt(args)
        elif args.mode == "live":
            receipt = live_probe(args)
        else:
            receipt = validate_checked_receipts(args.cwd)
    except (OSError, subprocess.SubprocessError, ProtocolError, KeyError) as error:
        if args.mode == "live" and sha256_file(auth_path) != args.auth_sha256_before:
            error = ProtocolError(
                "normal Codex auth state changed during a failed process-scoped probe"
            )
        print(json.dumps({"schema": PROBE_SCHEMA, "status": "failed", "error": str(error)}))
        return 1
    print(json.dumps(receipt, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
