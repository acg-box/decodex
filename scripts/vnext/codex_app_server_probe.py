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
import re
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


def sha256_tree(path: Path, excluded_roots: tuple[str, ...] = ()) -> str | None:
    """Hash a tree without emitting paths or file contents."""
    if not path.exists():
        return None
    digest = hashlib.sha256()
    for candidate in sorted(item for item in path.rglob("*") if item.is_file()):
        relative = candidate.relative_to(path)
        if relative.parts and relative.parts[0] in excluded_roots:
            continue
        file_digest = hashlib.sha256()
        with candidate.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                file_digest.update(chunk)
        digest.update(
            json.dumps(
                [relative.as_posix(), file_digest.hexdigest()], separators=(",", ":")
            ).encode()
        )
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


def duration_typed_windows(server: AppServer) -> list[dict[str, Any]]:
    """Return only vNext-recognized windows, identified solely by duration."""
    windows = []
    for window in rate_limit_windows(server):
        duration = window.get("duration_minutes")
        if duration not in (300, 10080):
            continue
        windows.append(
            {
                "duration_minutes": duration,
                "used_percent": window.get("used_percent"),
                "resets_at": window.get("resets_at"),
            }
        )
    return sorted(windows, key=lambda item: item["duration_minutes"])


def quota_state(windows: list[dict[str, Any]], now: int) -> str:
    durations = {window["duration_minutes"] for window in windows}
    if 300 not in durations or 10080 not in durations:
        return "unknown"
    for window in windows:
        used_percent = window.get("used_percent")
        resets_at = window.get("resets_at")
        if (
            isinstance(used_percent, bool)
            or not isinstance(used_percent, (int, float))
            or not 0 <= used_percent <= 100
            or isinstance(resets_at, bool)
            or not isinstance(resets_at, (int, float))
            or resets_at <= 0
        ):
            return "unknown"
    if any(window["resets_at"] <= now for window in windows):
        return "unknown"
    depleted = [window for window in windows if window["used_percent"] >= 100]
    if depleted:
        return "depleted"
    return "available"


def classify_resume_error(_error: ProtocolError) -> str:
    """Do not infer a denial boundary from an untyped protocol/transport failure."""
    return "probe_error"


def plugin_summary(server: AppServer, cwd: Path) -> dict[str, Any]:
    result = server.request(
        "plugin/list", {"cwds": [str(cwd)], "marketplaceKinds": ["local"]}
    )
    marketplaces = result.get("marketplaces", [])
    plugins = [
        plugin
        for marketplace in marketplaces
        for plugin in marketplace.get("plugins", [])
    ]
    return {
        "marketplace_count": len(marketplaces),
        "plugin_count": len(plugins),
        "installed_count": sum(bool(plugin.get("installed")) for plugin in plugins),
        "enabled_count": sum(bool(plugin.get("enabled")) for plugin in plugins),
        "load_error_count": len(result.get("marketplaceLoadErrors", [])),
    }


def skills_summary(server: AppServer, cwd: Path) -> dict[str, Any]:
    result = server.request(
        "skills/list",
        {
            "cwds": [str(cwd)],
            "forceReload": False,
            "perCwdExtraUserRoots": None,
        },
    )
    entries = result.get("data", [])
    skills = [skill for entry in entries for skill in entry.get("skills", [])]
    errors = [error for entry in entries for error in entry.get("errors", [])]
    return {
        "cwd_entry_present": any(entry.get("cwd") == str(cwd) for entry in entries),
        "skill_count": len(skills),
        "enabled_count": sum(bool(skill.get("enabled")) for skill in skills),
        "scan_error_count": len(errors),
    }


def inventory_probe(args: argparse.Namespace) -> dict[str, Any]:
    """Authenticate every safe record without starting a turn or consuming quota."""
    codex_home = Path(args.codex_home).expanduser().resolve()
    auth_path = codex_home / "auth.json"
    plugins_path = codex_home / "plugins"
    accounts_path = Path(args.accounts_file).expanduser().resolve()
    before = {
        "auth": sha256_file(auth_path),
        # app-server may refresh its private executable cache while servicing
        # plugin/list. That cache is process machinery, not installed/enabled state.
        "plugins": sha256_tree(plugins_path, (".plugin-appserver",)),
        "accounts": sha256_file(accounts_path),
    }
    accounts = read_accounts(accounts_path)
    proof = json.loads(
        (args.cwd / "openwiki/evidence/fixtures/xy-1262-live-receipt.json").read_text()
    )
    proof_thread = proof["experiments"]["primary_thread"]
    receipt: dict[str, Any] = {
        "schema": "decodex/vnext-codex-account-inventory/1",
        "observed_at_unix": int(time.time()),
        "repository_head": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=args.cwd, text=True
        ).strip(),
        "codex_cli": subprocess.check_output([args.codex, "--version"], text=True).strip(),
        "configured_account_count": len(accounts),
        "accounts": [],
        "turns_started": 0,
        "quota_deliberately_consumed": False,
    }
    title_readback_recorded = False
    for index, record in enumerate(accounts, start=1):
        item: dict[str, Any] = {
            "alias": f"R{index}",
            "disabled": bool(record.get("disabled")),
        }
        tokens = record.get("tokens") or {}
        if item["disabled"]:
            item["authentication"] = "skipped_disabled"
            receipt["accounts"].append(item)
            continue
        if not tokens.get("access_token") or not tokens.get("account_id"):
            item["authentication"] = "skipped_no_process_scoped_tokens"
            receipt["accounts"].append(item)
            continue
        server = AppServer(args.codex, args.cwd)
        try:
            try:
                server.login(
                    {
                        "access_token": tokens["access_token"],
                        "account_id": tokens["account_id"],
                        "plan_type": tokens.get("plan_type"),
                    }
                )
            except ProtocolError:
                item["authentication"] = "rejected"
                receipt["accounts"].append(item)
                continue
            item["authentication"] = "authenticated"
            windows = duration_typed_windows(server)
            item["quota_windows"] = windows
            item["quota_state"] = quota_state(windows, int(time.time()))
            item["plugins"] = plugin_summary(server, args.cwd)
            item["skills"] = skills_summary(server, args.cwd)
            try:
                resumed = server.request(
                    "thread/resume", {"threadId": proof_thread["id"]}
                )["thread"]
                item["same_thread_resume_without_turn"] = (
                    "permitted" if resumed.get("id") == proof_thread["id"] else "incompatible"
                )
            except ProtocolError as error:
                item["same_thread_resume_without_turn"] = classify_resume_error(error)
            if not title_readback_recorded:
                listed = server.request(
                    "thread/list",
                    {"searchTerm": proof_thread["name"], "limit": 20},
                )
                searched = server.request(
                    "thread/search", {"searchTerm": proof_thread["name"], "limit": 20}
                )
                receipt["app_server_title_discovery"] = {
                    "thread_list_found": any(
                        thread.get("id") == proof_thread["id"]
                        for thread in listed.get("data", [])
                    ),
                    "thread_search_found": any(
                        thread.get("id") == proof_thread["id"]
                        for thread in searched.get("data", [])
                    ),
                }
                title_readback_recorded = True
            receipt["accounts"].append(item)
        finally:
            server.close(crash=False)
    after = {
        "auth": sha256_file(auth_path),
        "plugins": sha256_tree(plugins_path, (".plugin-appserver",)),
        "accounts": sha256_file(accounts_path),
    }
    receipt["normal_state_unchanged"] = {
        "auth": before["auth"] == after["auth"],
        "plugin_tree": before["plugins"] == after["plugins"],
        "account_pool": before["accounts"] == after["accounts"],
    }
    receipt["plugin_state_scope"] = (
        "non-transient plugin tree; app-server executable cache excluded"
    )
    receipt["identities_emitted"] = False
    receipt["selectors_emitted"] = False
    receipt["credentials_emitted"] = False
    return receipt


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


def check_redaction(value: Any) -> None:
    forbidden_keys = {"access_token", "refresh_token", "id_token", "email", "account_id"}
    allowed_safety_assertions = {
        "credentials_emitted",
        "identities_emitted",
        "reviewer_identity",
        "selectors_emitted",
    }
    if isinstance(value, dict):
        for key, child in value.items():
            lowered = key.lower()
            if (
                lowered in forbidden_keys
                or "token" in lowered
                or (
                    any(marker in lowered for marker in ("credential", "identity", "selector"))
                    and lowered not in allowed_safety_assertions
                )
            ):
                raise ProtocolError(f"credential/identity-shaped key in evidence: {key}")
            check_redaction(child)
    elif isinstance(value, list):
        for child in value:
            check_redaction(child)
    elif isinstance(value, str) and "@" in value:
        raise ProtocolError("account-like identity in evidence")


def validate_checked_receipts(repository: Path) -> dict[str, Any]:
    fixture_dir = repository / "openwiki/evidence/fixtures"
    bundle = json.loads((fixture_dir / "xy-1262-live-receipt.json").read_text())
    collaboration = json.loads(
        (fixture_dir / "xy-1262-native-collaboration.json").read_text()
    )
    quota = json.loads((fixture_dir / "xy-1262-quota-matrix.json").read_text())
    reconciliation = json.loads(
        (fixture_dir / "xy-1262-gate-reconciliation.json").read_text()
    )
    if bundle.get("schema") != "decodex/vnext-codex-evidence-bundle/1":
        raise ProtocolError("checked live evidence bundle has the wrong schema")
    if bundle.get("overall_acceptance") is not False:
        raise ProtocolError("XY-1262 checked evidence must retain the failed gate verdict")

    check_redaction(bundle)
    check_redaction(collaboration)
    check_redaction(reconciliation)
    if reconciliation.get("overall_acceptance") is not False:
        raise ProtocolError("XY-1262 reconciliation must retain the failed gate verdict")
    expected_top_level = {
        "schema", "overall_acceptance", "observed_at_unix", "repository_head",
        "codex_cli", "sources", "configured_account_count", "accounts",
        "app_server_title_discovery", "codex_desktop_title_discovery",
        "normal_state_unchanged", "plugin_state_scope", "turns_started",
        "quota_deliberately_consumed", "identities_emitted", "selectors_emitted",
        "credentials_emitted", "conclusions",
    }
    if set(reconciliation) != expected_top_level:
        raise ProtocolError("account inventory has an unexpected top-level shape")
    accounts = reconciliation.get("accounts", [])
    if reconciliation.get("configured_account_count") != len(accounts):
        raise ProtocolError("account inventory count does not match redacted records")
    if len(accounts) != 6:
        raise ProtocolError("account inventory does not cover all six configured records")
    if reconciliation.get("turns_started") != 0:
        raise ProtocolError("read-only account inventory unexpectedly started a turn")
    if reconciliation.get("quota_deliberately_consumed") is not False:
        raise ProtocolError("account inventory claims deliberate quota consumption")
    if reconciliation.get("normal_state_unchanged") != {
        "auth": True,
        "plugin_tree": True,
        "account_pool": True,
    }:
        raise ProtocolError("account inventory did not preserve normal auth/plugin state")
    if any(reconciliation.get(key) is not False for key in (
        "identities_emitted", "selectors_emitted", "credentials_emitted"
    )):
        raise ProtocolError("account inventory safety assertions are not false")
    if reconciliation.get("sources") != [
        "python3 scripts/vnext/codex_app_server_probe.py inventory",
        "codex_app.list_threads(exact retained title query)",
    ]:
        raise ProtocolError("account inventory source provenance is incomplete")
    if reconciliation.get("app_server_title_discovery") != {
        "thread_list_found": True,
        "thread_search_found": False,
    } or reconciliation.get("codex_desktop_title_discovery") != {
        "global_title_query_found": False,
        "unavailable_host_count": 0,
    }:
        raise ProtocolError("account inventory title-discovery facts changed")
    expected_account_keys = {
        "alias", "disabled", "authentication", "quota_state", "quota_windows",
        "same_thread_resume_without_turn", "plugins", "skills",
    }
    for index, account in enumerate(accounts, start=1):
        if set(account) != expected_account_keys:
            raise ProtocolError("account inventory has an unexpected account shape")
        if account.get("alias") != f"R{index}" or not re.fullmatch(
            r"R[1-9][0-9]*", account.get("alias", "")
        ):
            raise ProtocolError("account inventory alias is not opaque and sequential")
        if account.get("disabled") is not False or account.get("authentication") != "authenticated":
            raise ProtocolError("account inventory overclaims safe authentication coverage")
        if account.get("quota_state") != "unknown":
            raise ProtocolError("account inventory overclaims a live quota state")
        if account.get("same_thread_resume_without_turn") != "permitted":
            raise ProtocolError("account inventory overclaims a resume boundary")
        plugins = account.get("plugins", {})
        skills = account.get("skills", {})
        if (
            set(plugins) != {
                "marketplace_count", "plugin_count", "installed_count",
                "enabled_count", "load_error_count",
            }
            or plugins.get("load_error_count") != 0
            or set(skills) != {
                "cwd_entry_present", "skill_count", "enabled_count", "scan_error_count",
            }
            or skills.get("cwd_entry_present") is not True
            or skills.get("scan_error_count") != 0
        ):
            raise ProtocolError("account inventory plugin/skill readback is incomplete")
        for window in account.get("quota_windows", []):
            if set(window) != {"duration_minutes", "used_percent", "resets_at"}:
                raise ProtocolError("account inventory retained positional quota fields")
            if window["duration_minutes"] not in (300, 10080):
                raise ProtocolError("account inventory retained an untyped quota duration")
            if (
                isinstance(window["used_percent"], bool)
                or not isinstance(window["used_percent"], (int, float))
                or not 0 <= window["used_percent"] < 100
                or isinstance(window["resets_at"], bool)
                or not isinstance(window["resets_at"], (int, float))
                or window["resets_at"] <= reconciliation["observed_at_unix"]
            ):
                raise ProtocolError("account inventory contradicts the no-depletion claim")
        if any(window["duration_minutes"] == 300 for window in account["quota_windows"]):
            raise ProtocolError("account inventory no longer supports the missing-window conclusion")
    plugin_summaries = [account["plugins"] for account in accounts]
    if any(summary != plugin_summaries[0] for summary in plugin_summaries[1:]):
        raise ProtocolError("account inventory plugin summaries are not consistent")
    if reconciliation.get("conclusions") != {
        "naturally_depleted_account_observed": False,
        "provider_quota_failure_exercised": False,
        "quota_exclusion_failover_exercised": False,
        "same_thread_resume_denied_or_incompatible_observed": False,
        "gate_remains_failed": True,
    }:
        raise ProtocolError("account inventory conclusions do not match the failed gate")
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
        "inventory_accounts_validated": len(reconciliation["accounts"]),
        "overall_acceptance": False,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("schema", "live", "inventory", "validate"))
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
        elif args.mode == "inventory":
            receipt = inventory_probe(args)
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
