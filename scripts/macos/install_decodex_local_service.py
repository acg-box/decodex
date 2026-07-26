#!/usr/bin/env python3
"""Provision and install the same-UID Decodex local service on macOS.

This source-install tool owns only the local development installation. It creates
no credential copy. The transitional bridge reads the existing legacy account
pool under its file lock and passes current credentials only to the supervised
daemon process.
"""

from __future__ import annotations

import argparse
import base64
import fcntl
import hashlib
import importlib.util
import json
import os
import plistlib
import pwd
import re
import shutil
import signal
import stat
import subprocess
import sys
import time
import unicodedata
import urllib.request
import uuid
from dataclasses import dataclass
from pathlib import Path
from types import ModuleType
from typing import Any


MAPPING_SCHEMA = "decodex/reset-card-legacy-bridge/1"
LAUNCH_AGENT_LABEL = "space.decodex.local-service"
MAX_ACCOUNT_FILE_BYTES = 4 * 1024 * 1024
MAX_ACCOUNT_LINE_BYTES = 128 * 1024
MAX_CONFIG_FILE_BYTES = 1024 * 1024
MAX_MAPPING_FILE_BYTES = 64 * 1024
MAX_POSTGRES_VERSION_BYTES = 16
LEGACY_LOCK_TIMEOUT_SECONDS = 5
MAX_ACCOUNTS = 64
MAX_ACCESS_TOKEN_BYTES = 64 * 1024
MAX_ACCOUNT_ID_BYTES = 1_024
MAX_EMAIL_BYTES = 320
POSTGRES_PORT = 55_432
POSTGRES_DATABASE = "decodex"
POSTGRES_MIGRATION_ROLE = "decodex_migration"
POSTGRES_RUNTIME_ROLE = "decodex_runtime"
PLAN_TYPES = {
    "free",
    "go",
    "plus",
    "pro",
    "prolite",
    "team",
    "self_serve_business_usage_based",
    "business",
    "enterprise_cbp_usage_based",
    "enterprise",
    "edu",
    "unknown",
}
UUID_PATTERN = re.compile(
    r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"
)
HEX_DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")


class InstallError(RuntimeError):
    """A value-free local-service installation failure."""


@dataclass(frozen=True)
class LegacyAccount:
    provider_account_id: str
    email: str
    plan_type: str

    @property
    def provider_account_id_sha256(self) -> str:
        return hashlib.sha256(self.provider_account_id.encode("utf-8")).hexdigest()


@dataclass(frozen=True)
class Enrollment:
    slot: int
    provider_account_id_sha256: str
    account_id: str
    display_label: str
    initial_state: str
    plan_type: str


@dataclass(frozen=True)
class ExistingEnrollment:
    account_id: str
    display_label: str


@dataclass(frozen=True)
class InstallPaths:
    repository: Path
    root: Path
    config: Path
    mapping: Path
    data_directory: Path
    socket_directory: Path
    log_directory: Path
    postgres_log: Path
    service_log: Path
    legacy_accounts: Path
    launch_agent: Path
    decodexd: Path
    decodex_cli: Path
    codex: Path
    postgres: Path
    initdb: Path
    pg_isready: Path
    psql: Path


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    home = Path.home()
    discovered_codex = shutil.which("codex")
    parser = argparse.ArgumentParser(
        description="Provision the Decodex PostgreSQL 18 local service and LaunchAgent."
    )
    parser.add_argument(
        "--repository",
        type=Path,
        default=Path(__file__).resolve().parents[2],
    )
    parser.add_argument("--root", type=Path, default=home / ".decodex")
    parser.add_argument(
        "--legacy-accounts",
        type=Path,
        default=home / ".codex" / "decodex" / "accounts.jsonl",
    )
    parser.add_argument(
        "--launch-agent",
        type=Path,
        default=home / "Library" / "LaunchAgents" / f"{LAUNCH_AGENT_LABEL}.plist",
    )
    parser.add_argument(
        "--decodexd",
        type=Path,
        default=home / ".local" / "bin" / "decodexd",
    )
    parser.add_argument(
        "--decodex-cli",
        type=Path,
        default=home / ".local" / "bin" / "decodex",
    )
    parser.add_argument(
        "--codex",
        type=Path,
        default=(
            Path(discovered_codex)
            if discovered_codex is not None
            else home / ".codex" / "shims" / "codex"
        ),
        help="Codex executable made discoverable to the supervised daemon.",
    )
    parser.add_argument(
        "--postgres",
        type=Path,
        default=Path("/run/current-system/sw/bin/postgres"),
    )
    parser.add_argument(
        "--initdb",
        type=Path,
        default=Path("/run/current-system/sw/bin/initdb"),
    )
    parser.add_argument(
        "--pg-isready",
        type=Path,
        default=Path("/run/current-system/sw/bin/pg_isready"),
    )
    parser.add_argument(
        "--psql",
        type=Path,
        default=Path("/run/current-system/sw/bin/psql"),
    )
    parser.add_argument(
        "--replace-config",
        action="store_true",
        help="Replace the current vNext config without creating a backup.",
    )
    parser.add_argument(
        "--no-launch",
        action="store_true",
        help="Provision files and PostgreSQL, but do not bootstrap the LaunchAgent.",
    )
    return parser.parse_args(argv)


def install_paths(args: argparse.Namespace) -> InstallPaths:
    root = args.root.expanduser().resolve()
    repository = args.repository.expanduser().resolve()
    return InstallPaths(
        repository=repository,
        root=root,
        config=root / "config.toml",
        mapping=root / "reset-card-legacy-map.json",
        data_directory=root / "postgres" / "data",
        socket_directory=root / "postgres" / "socket",
        log_directory=root / "logs",
        postgres_log=root / "logs" / "postgres.log",
        service_log=root / "logs" / "local-service.log",
        legacy_accounts=args.legacy_accounts.expanduser().resolve(),
        launch_agent=args.launch_agent.expanduser().resolve(),
        decodexd=args.decodexd.expanduser().resolve(),
        decodex_cli=args.decodex_cli.expanduser().resolve(),
        codex=args.codex.expanduser().resolve(),
        postgres=args.postgres.expanduser().resolve(),
        initdb=args.initdb.expanduser().resolve(),
        pg_isready=args.pg_isready.expanduser().resolve(),
        psql=args.psql.expanduser().resolve(),
    )


def require_regular_executable(path: Path, name: str) -> None:
    try:
        metadata = path.stat()
    except OSError as error:
        raise InstallError(f"{name} executable is unavailable") from error
    if not stat.S_ISREG(metadata.st_mode) or not os.access(path, os.X_OK):
        raise InstallError(f"{name} executable is unavailable")


def require_private_directory(path: Path, uid: int) -> None:
    try:
        metadata = path.lstat()
    except OSError as error:
        raise InstallError("legacy account directory is unavailable") from error
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or stat.S_ISLNK(metadata.st_mode)
        or metadata.st_uid != uid
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        raise InstallError("legacy account directory is not private")


def secure_legacy_file(path: Path, uid: int, *, create: bool = False) -> int:
    flags = os.O_RDWR if create else os.O_RDONLY
    if create:
        flags |= os.O_CREAT
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags, 0o600)
    except OSError as error:
        raise InstallError("legacy account file authority is unsafe") from error
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != uid
            or metadata.st_nlink != 1
        ):
            raise InstallError("legacy account file authority is unsafe")
        os.fchmod(descriptor, 0o600)
    except BaseException:
        os.close(descriptor)
        raise
    return descriptor


def read_bounded_descriptor(descriptor: int, maximum_bytes: int) -> bytes:
    chunks: list[bytes] = []
    remaining = maximum_bytes + 1
    while remaining > 0:
        chunk = os.read(descriptor, min(64 * 1024, remaining))
        if not chunk:
            break
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def file_identity(metadata: os.stat_result) -> tuple[int, int, int, int, int]:
    return (
        metadata.st_dev,
        metadata.st_ino,
        metadata.st_size,
        metadata.st_mtime_ns,
        metadata.st_ctime_ns,
    )


def read_owned_file(path: Path, uid: int, maximum_bytes: int, failure: str) -> bytes:
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise InstallError(failure) from error
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != uid
            or metadata.st_nlink != 1
            or metadata.st_size > maximum_bytes
        ):
            raise InstallError(failure)
        os.fchmod(descriptor, 0o600)
        expected_identity = file_identity(os.fstat(descriptor))
        body = read_bounded_descriptor(descriptor, maximum_bytes)
        if len(body) > maximum_bytes or file_identity(os.fstat(descriptor)) != expected_identity:
            raise InstallError(failure)
        return body
    except OSError as error:
        raise InstallError(failure) from error
    finally:
        os.close(descriptor)


def acquire_legacy_lock(descriptor: int) -> None:
    deadline = time.monotonic() + LEGACY_LOCK_TIMEOUT_SECONDS
    while True:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return
        except BlockingIOError as error:
            if time.monotonic() >= deadline:
                raise InstallError("legacy account lock is unavailable") from error
            time.sleep(0.05)


def bounded_scalar(value: Any, limit: int) -> str | None:
    if not isinstance(value, str):
        return None
    encoded = value.encode("utf-8")
    if (
        not encoded
        or len(encoded) > limit
        or value.strip() != value
        or contains_control(value)
    ):
        return None
    return value


def contains_control(value: str) -> bool:
    return any(unicodedata.category(character).startswith("C") for character in value)


def decode_id_token_claims(token: str) -> dict[str, Any]:
    components = token.split(".")
    if len(components) != 3 or any(not component for component in components):
        raise InstallError("legacy account identity token is malformed")
    payload = components[1]
    payload += "=" * (-len(payload) % 4)
    try:
        decoded = base64.b64decode(
            payload.encode("ascii"),
            altchars=b"-_",
            validate=True,
        )
        claims = json.loads(decoded)
    except (UnicodeEncodeError, ValueError, json.JSONDecodeError) as error:
        raise InstallError("legacy account identity token is malformed") from error
    if not isinstance(claims, dict):
        raise InstallError("legacy account identity token is malformed")
    return claims


def account_from_record(record: Any) -> LegacyAccount:
    if not isinstance(record, dict):
        raise InstallError("legacy account record is malformed")
    has_tokens = record.get("tokens") is not None
    has_auth = record.get("auth") is not None
    if has_tokens == has_auth:
        raise InstallError("legacy account shape is invalid")
    if has_auth:
        if not isinstance(record.get("auth"), dict):
            raise InstallError("legacy account shape is invalid")
        auth = record["auth"]
        outer_email = record.get("email")
    else:
        auth = record
        outer_email = record.get("email")
    tokens = auth.get("tokens")
    if not isinstance(tokens, dict):
        raise InstallError("legacy account credentials are unavailable")
    provider_account_id = bounded_scalar(tokens.get("account_id"), MAX_ACCOUNT_ID_BYTES)
    access_token = bounded_scalar(tokens.get("access_token"), MAX_ACCESS_TOKEN_BYTES)
    id_token = bounded_scalar(tokens.get("id_token"), MAX_ACCESS_TOKEN_BYTES)
    email_value = outer_email if outer_email is not None else auth.get("email")
    email = bounded_scalar(email_value, MAX_EMAIL_BYTES)
    if provider_account_id is None:
        raise InstallError("legacy provider account identity is invalid")
    if access_token is None or id_token is None or email is None or "@" not in email:
        raise InstallError("legacy account credentials are unavailable")
    claims = decode_id_token_claims(id_token)
    authority = claims.get("https://api.openai.com/auth")
    if not isinstance(authority, dict):
        raise InstallError("legacy account identity token lacks account authority")
    claimed_account_id = bounded_scalar(
        authority.get("chatgpt_account_id"), MAX_ACCOUNT_ID_BYTES
    )
    plan_type = bounded_scalar(authority.get("chatgpt_plan_type"), 64)
    if claimed_account_id != provider_account_id or plan_type not in PLAN_TYPES:
        raise InstallError("legacy account identity claims are inconsistent")
    claimed_email_value = claims.get("email")
    if claimed_email_value is not None:
        claimed_email = bounded_scalar(claimed_email_value, MAX_EMAIL_BYTES)
        if claimed_email is None or claimed_email.lower() != email.lower():
            raise InstallError("legacy account email claims are inconsistent")
    return LegacyAccount(
        provider_account_id=provider_account_id,
        email=email,
        plan_type=plan_type,
    )


def read_legacy_accounts(path: Path, uid: int) -> list[LegacyAccount]:
    parent = path.parent
    require_private_directory(parent, uid)
    lock_path = parent / f".{path.name}.lock"
    lock_descriptor = secure_legacy_file(lock_path, uid, create=True)
    try:
        acquire_legacy_lock(lock_descriptor)
        descriptor = secure_legacy_file(path, uid)
        try:
            body = read_bounded_descriptor(descriptor, MAX_ACCOUNT_FILE_BYTES)
        finally:
            os.close(descriptor)
    finally:
        fcntl.flock(lock_descriptor, fcntl.LOCK_UN)
        os.close(lock_descriptor)
    if len(body) > MAX_ACCOUNT_FILE_BYTES:
        raise InstallError("legacy account file exceeds the supported bound")
    try:
        text = body.decode("utf-8")
    except UnicodeDecodeError as error:
        raise InstallError("legacy account file is not UTF-8") from error
    accounts = []
    for line in text.splitlines():
        if len(line.encode("utf-8")) > MAX_ACCOUNT_LINE_BYTES:
            raise InstallError("legacy account record exceeds the supported bound")
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        try:
            record = json.loads(stripped)
        except json.JSONDecodeError as error:
            raise InstallError("legacy account record is malformed") from error
        accounts.append(account_from_record(record))
    if not accounts or len(accounts) > MAX_ACCOUNTS:
        raise InstallError("legacy account count is outside the supported bound")
    provider_ids = {account.provider_account_id for account in accounts}
    emails = {account.email.casefold() for account in accounts}
    if len(provider_ids) != len(accounts) or len(emails) != len(accounts):
        raise InstallError("legacy account identities are not unique")
    return sorted(accounts, key=lambda account: account.provider_account_id)


def read_usage_presentations() -> dict[str, dict[str, Any]]:
    request = urllib.request.Request(
        "http://127.0.0.1:8192/api/accounts?refresh=1",
        headers={"Accept": "application/json"},
    )
    try:
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with opener.open(request, timeout=20) as response:
            body = response.read(MAX_ACCOUNT_FILE_BYTES + 1)
            if len(body) > MAX_ACCOUNT_FILE_BYTES:
                return {}
            payload = json.loads(body)
    except (OSError, ValueError, json.JSONDecodeError):
        return {}
    rows = payload.get("accounts") if isinstance(payload, dict) else None
    if not isinstance(rows, list) or len(rows) > MAX_ACCOUNTS:
        return {}
    presentations: dict[str, dict[str, Any]] = {}
    for row in rows:
        if not isinstance(row, dict):
            return {}
        email = bounded_scalar(row.get("email"), MAX_EMAIL_BYTES)
        if email is None or email.casefold() in presentations:
            return {}
        presentations[email.casefold()] = row
    return presentations


def managed_path_exists(path: Path, failure: str) -> bool:
    try:
        path.lstat()
    except FileNotFoundError:
        return False
    except OSError as error:
        raise InstallError(failure) from error
    return True


def load_existing_mapping(path: Path, uid: int) -> dict[str, int]:
    if not managed_path_exists(path, "existing reset-card mapping is malformed"):
        return {}
    try:
        payload = json.loads(
            read_owned_file(
                path,
                uid,
                MAX_MAPPING_FILE_BYTES,
                "existing reset-card mapping is malformed",
            ).decode("utf-8")
        )
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise InstallError("existing reset-card mapping is malformed") from error
    if not isinstance(payload, dict) or set(payload) != {"schema", "accounts"}:
        raise InstallError("existing reset-card mapping is malformed")
    if payload["schema"] != MAPPING_SCHEMA or not isinstance(payload["accounts"], list):
        raise InstallError("existing reset-card mapping is malformed")
    mapping: dict[str, int] = {}
    slots: set[int] = set()
    for row in payload["accounts"]:
        if not isinstance(row, dict) or set(row) != {
            "slot",
            "provider_account_id_sha256",
        }:
            raise InstallError("existing reset-card mapping is malformed")
        slot = row["slot"]
        digest = row["provider_account_id_sha256"]
        if (
            not isinstance(slot, int)
            or isinstance(slot, bool)
            or not 1 <= slot <= MAX_ACCOUNTS
            or not isinstance(digest, str)
            or not HEX_DIGEST_PATTERN.fullmatch(digest)
            or slot in slots
            or digest in mapping
        ):
            raise InstallError("existing reset-card mapping is malformed")
        mapping[digest] = slot
        slots.add(slot)
    if sorted(slots) != list(range(1, len(slots) + 1)):
        raise InstallError("existing reset-card mapping slots are not contiguous")
    return mapping


def existing_enrollments(
    config_path: Path,
    uid: int,
) -> dict[int, ExistingEnrollment]:
    if not managed_path_exists(config_path, "existing Decodex config is malformed"):
        return {}
    try:
        lines = read_owned_file(
            config_path,
            uid,
            MAX_CONFIG_FILE_BYTES,
            "existing Decodex config is malformed",
        ).decode("utf-8").splitlines()
    except UnicodeDecodeError as error:
        raise InstallError("existing Decodex config is malformed") from error
    result: dict[int, ExistingEnrollment] = {}
    current_account_id: str | None = None
    current_reference: str | None = None
    current_display_label: str | None = None

    def finish_account() -> None:
        nonlocal current_account_id, current_reference, current_display_label
        if current_account_id is None:
            return
        if current_reference is None or current_display_label is None:
            raise InstallError("existing reset-card enrollment is not bridge-owned")
        match = re.fullmatch(
            r"DECODEX_RESET_CARD_SLOT_([0-9]{2})_ACCESS_TOKEN",
            current_reference,
        )
        if match is None:
            raise InstallError("existing reset-card enrollment is not bridge-owned")
        slot = int(match.group(1))
        if not 1 <= slot <= MAX_ACCOUNTS:
            raise InstallError("existing reset-card enrollment has an invalid slot")
        if slot in result:
            raise InstallError("existing reset-card enrollment has duplicate slots")
        result[slot] = ExistingEnrollment(
            account_id=current_account_id,
            display_label=current_display_label,
        )
        current_account_id = None
        current_reference = None
        current_display_label = None

    for line in lines:
        stripped = line.strip()
        if stripped.startswith("["):
            finish_account()
            match = re.fullmatch(
                r'\[server_host\.reset_card_accounts\."([^"]+)"\]',
                stripped,
            )
            if match is not None:
                account_id = match.group(1)
                if not UUID_PATTERN.fullmatch(account_id):
                    raise InstallError("existing reset-card enrollment is malformed")
                current_account_id = account_id
            elif stripped.startswith("[server_host.reset_card_accounts."):
                raise InstallError("existing reset-card enrollment is malformed")
            continue
        if current_account_id is not None:
            match = re.fullmatch(r'access_token_env_var\s*=\s*"([^"]+)"', stripped)
            if match is not None:
                if current_reference is not None:
                    raise InstallError("existing reset-card enrollment is malformed")
                current_reference = match.group(1)
                continue
            if re.match(r"access_token_env_var\s*=", stripped):
                raise InstallError("existing reset-card enrollment is malformed")
            if re.match(r"display_label\s*=", stripped):
                match = re.fullmatch(
                    r'display_label\s*=\s*("(?:[^"\\]|\\.)*")',
                    stripped,
                )
                if match is None or current_display_label is not None:
                    raise InstallError("existing reset-card enrollment is malformed")
                try:
                    decoded_label = json.loads(match.group(1))
                except json.JSONDecodeError as error:
                    raise InstallError(
                        "existing reset-card enrollment is malformed"
                    ) from error
                current_display_label = bounded_scalar(decoded_label, 128)
                if current_display_label is None:
                    raise InstallError("existing reset-card enrollment is malformed")
    finish_account()
    return result


def build_enrollments(
    accounts: list[LegacyAccount],
    existing_mapping: dict[str, int],
    existing: dict[int, ExistingEnrollment],
    presentations: dict[str, dict[str, Any]],
) -> list[Enrollment]:
    digests = {account.provider_account_id_sha256 for account in accounts}
    if existing_mapping and set(existing_mapping) != digests:
        raise InstallError("legacy account set changed; explicit reconciliation is required")
    if (
        existing_mapping
        and existing
        and set(existing_mapping.values()) != set(existing)
    ):
        raise InstallError("existing mapping and enrollment config do not agree")
    if not existing_mapping and existing:
        raise InstallError("existing enrollment lacks its bridge mapping")
    mapping = existing_mapping or {
        account.provider_account_id_sha256: index
        for index, account in enumerate(accounts, start=1)
    }
    enrollments = []
    for account in accounts:
        slot = mapping[account.provider_account_id_sha256]
        presentation = presentations.get(account.email.casefold(), {})
        observed_plan = presentation.get("plan_type")
        if observed_plan is not None and observed_plan != account.plan_type:
            raise InstallError("legacy account plan observations do not agree")
        prior_enrollment = existing.get(slot)
        if prior_enrollment is None:
            label = bounded_scalar(presentation.get("random_name"), 128)
            if label is None:
                label = f"Account {slot:02d}"
            account_id = str(uuid.uuid4())
        else:
            label = prior_enrollment.display_label
            account_id = prior_enrollment.account_id
        available_count = presentation.get("reset_credits_available_count")
        initial_state = (
            "depleted"
            if isinstance(available_count, int)
            and not isinstance(available_count, bool)
            and available_count == 0
            else "available"
        )
        enrollments.append(
            Enrollment(
                slot=slot,
                provider_account_id_sha256=account.provider_account_id_sha256,
                account_id=account_id,
                display_label=label,
                initial_state=initial_state,
                plan_type=account.plan_type,
            )
        )
    return sorted(enrollments, key=lambda enrollment: enrollment.slot)


def toml_string(value: str | Path) -> str:
    return json.dumps(str(value), ensure_ascii=False)


def render_config(paths: InstallPaths, uid: int, enrollments: list[Enrollment]) -> str:
    lines = [
        "version = 1",
        'active_profile = "local"',
        "",
        "[profiles.local]",
        'kind = "local"',
        'policy = "same_uid"',
        f"service_owner_uid = {uid}",
        "",
        "[server_host.repositories.decodex]",
        f"host_path = {toml_string(paths.repository)}",
        "",
    ]
    for enrollment in enrollments:
        prefix = f"DECODEX_RESET_CARD_SLOT_{enrollment.slot:02d}"
        lines.extend(
            [
                f'[server_host.reset_card_accounts."{enrollment.account_id}"]',
                f"display_label = {toml_string(enrollment.display_label)}",
                f'initial_state = "{enrollment.initial_state}"',
                f'access_token_env_var = "{prefix}_ACCESS_TOKEN"',
                f'provider_account_id_env_var = "{prefix}_ACCOUNT_ID"',
                f'expected_email_env_var = "{prefix}_EMAIL"',
                f'plan_type = "{enrollment.plan_type}"',
                "",
            ]
        )
    lines.extend(
        [
            "[postgres]",
            f"socket_directory = {toml_string(paths.socket_directory)}",
            f"expected_peer_uid = {uid}",
            f"port = {POSTGRES_PORT}",
            f'database = "{POSTGRES_DATABASE}"',
            "",
            "[postgres.migration]",
            f'user = "{POSTGRES_MIGRATION_ROLE}"',
            "",
            "[postgres.runtime]",
            f'user = "{POSTGRES_RUNTIME_ROLE}"',
            "",
            "[cache]",
            "max_entries = 2048",
            "max_bytes = 134217728",
            "max_entry_bytes = 4194304",
            "",
        ]
    )
    return "\n".join(lines)


def render_mapping(enrollments: list[Enrollment]) -> bytes:
    payload = {
        "schema": MAPPING_SCHEMA,
        "accounts": [
            {
                "slot": enrollment.slot,
                "provider_account_id_sha256": enrollment.provider_account_id_sha256,
            }
            for enrollment in enrollments
        ],
    }
    return (json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n").encode()


def render_launch_agent(paths: InstallPaths) -> bytes:
    arguments = [
        str(paths.decodexd),
        "supervise-local",
        "--postgres",
        str(paths.postgres),
        "--pg-isready",
        str(paths.pg_isready),
        "--data-directory",
        str(paths.data_directory),
        "--socket-directory",
        str(paths.socket_directory),
        "--port",
        str(POSTGRES_PORT),
        "--legacy-accounts",
        str(paths.legacy_accounts),
        "--legacy-mapping",
        str(paths.mapping),
        "--working-directory",
        str(paths.repository),
    ]
    payload = {
        "Label": LAUNCH_AGENT_LABEL,
        "ProgramArguments": arguments,
        "EnvironmentVariables": {
            "HOME": str(paths.root.parent),
            "PATH": launch_agent_path(paths),
        },
        "RunAtLoad": True,
        "KeepAlive": True,
        "ExitTimeOut": 360,
        "ThrottleInterval": 5,
        "ProcessType": "Background",
        "WorkingDirectory": str(paths.repository),
        "StandardOutPath": str(paths.service_log),
        "StandardErrorPath": str(paths.service_log),
    }
    return plistlib.dumps(payload, fmt=plistlib.FMT_XML, sort_keys=True)


def launch_agent_path(paths: InstallPaths) -> str:
    directories = [
        str(paths.codex.parent),
        "/usr/local/bin",
        "/opt/homebrew/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ]
    return os.pathsep.join(dict.fromkeys(directories))


def require_owned_directory(path: Path, uid: int, failure: str) -> os.stat_result:
    try:
        metadata = path.lstat()
    except OSError as error:
        raise InstallError(failure) from error
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or stat.S_ISLNK(metadata.st_mode)
        or metadata.st_uid != uid
    ):
        raise InstallError(failure)
    return metadata


def atomic_write(path: Path, content: bytes, mode: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    uid = os.geteuid()
    require_owned_directory(path.parent, uid, "installation destination is unsafe")
    candidate = path.parent / f".{path.name}.install-{uuid.uuid4().hex}"
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    candidate_created = False
    try:
        descriptor = os.open(candidate, flags, mode)
        candidate_created = True
        try:
            with os.fdopen(descriptor, "wb", closefd=False) as output:
                output.write(content)
                output.flush()
                os.fsync(output.fileno())
            os.fchmod(descriptor, mode)
        finally:
            os.close(descriptor)
        os.replace(candidate, path)
        metadata = path.lstat()
        if (
            not stat.S_ISREG(metadata.st_mode)
            or stat.S_ISLNK(metadata.st_mode)
            or metadata.st_uid != uid
            or metadata.st_nlink != 1
            or stat.S_IMODE(metadata.st_mode) != mode
        ):
            raise InstallError("installed file authority is unsafe")
        directory_flags = os.O_RDONLY
        if hasattr(os, "O_DIRECTORY"):
            directory_flags |= os.O_DIRECTORY
        directory = os.open(path.parent, directory_flags)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    except BaseException:
        if candidate_created:
            try:
                candidate.unlink()
            except FileNotFoundError:
                pass
        raise


def run(
    command: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    capture: bool = False,
    check: bool = True,
    timeout: float | None = None,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=cwd,
        env=env,
        check=check,
        text=True,
        capture_output=capture,
        timeout=timeout,
    )


def postgres_major(postgres: Path) -> int:
    completed = run([str(postgres), "--version"], capture=True)
    match = re.search(r"\b([0-9]+)(?:\.[0-9]+)?\b", completed.stdout)
    if match is None:
        raise InstallError("PostgreSQL version is unavailable")
    return int(match.group(1))


def open_private_append_file(path: Path, uid: int) -> int:
    flags = os.O_WRONLY | os.O_CREAT | os.O_APPEND
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags, 0o600)
    except OSError as error:
        raise InstallError("local service log authority is unsafe") from error
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != uid
            or metadata.st_nlink != 1
        ):
            raise InstallError("local service log authority is unsafe")
        os.fchmod(descriptor, 0o600)
    except BaseException:
        os.close(descriptor)
        raise
    return descriptor


def ensure_directories(paths: InstallPaths, uid: int) -> None:
    for path in [
        paths.root,
        paths.data_directory.parent,
        paths.data_directory,
        paths.socket_directory,
        paths.log_directory,
    ]:
        path.mkdir(parents=True, exist_ok=True, mode=0o700)
        require_owned_directory(path, uid, "local service directory authority is unsafe")
        os.chmod(path, 0o700)
    service_log = open_private_append_file(paths.service_log, uid)
    os.close(service_log)


def postgres_version(paths: InstallPaths, uid: int) -> str | None:
    version_file = paths.data_directory / "PG_VERSION"
    if not managed_path_exists(version_file, "PostgreSQL cluster version is unsafe"):
        return None
    try:
        return read_owned_file(
            version_file,
            uid,
            MAX_POSTGRES_VERSION_BYTES,
            "PostgreSQL cluster version is unsafe",
        ).decode("ascii").strip()
    except UnicodeDecodeError as error:
        raise InstallError("PostgreSQL cluster version is unsafe") from error


def initialize_cluster(paths: InstallPaths, uid: int) -> None:
    version = postgres_version(paths, uid)
    if version is not None:
        if version != "18":
            raise InstallError("existing PostgreSQL cluster is not version 18")
        return
    if any(paths.data_directory.iterdir()):
        raise InstallError("PostgreSQL data directory is nonempty and uninitialized")
    share_directory = paths.initdb.parent.parent / "share" / "postgresql"
    try:
        share_metadata = share_directory.lstat()
    except OSError as error:
        raise InstallError("PostgreSQL 18 share directory is unavailable") from error
    if not stat.S_ISDIR(share_metadata.st_mode) or stat.S_ISLNK(share_metadata.st_mode):
        raise InstallError("PostgreSQL 18 share directory is unavailable")
    run(
        [
            str(paths.initdb),
            "-D",
            str(paths.data_directory),
            "--auth-local=trust",
            "--auth-host=reject",
            "--encoding=UTF8",
            "--locale=C",
            "--data-checksums",
            "-L",
            str(share_directory),
        ]
    )
    os.chmod(paths.data_directory, 0o700)
    if postgres_version(paths, uid) != "18":
        raise InstallError("PostgreSQL 18 initialization did not complete")


def wait_for_postgres(paths: InstallPaths, process: subprocess.Popen[Any]) -> None:
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise InstallError("PostgreSQL exited during local-service startup")
        completed = run(
            [
                str(paths.pg_isready),
                "-h",
                str(paths.socket_directory),
                "-p",
                str(POSTGRES_PORT),
            ],
            capture=True,
            check=False,
        )
        if completed.returncode == 0:
            return
        time.sleep(0.25)
    raise InstallError("PostgreSQL did not become ready")


def start_temporary_postgres(paths: InstallPaths) -> subprocess.Popen[Any]:
    log_descriptor = open_private_append_file(paths.postgres_log, os.geteuid())
    try:
        process = subprocess.Popen(
            [
                str(paths.postgres),
                "-D",
                str(paths.data_directory),
                "-k",
                str(paths.socket_directory),
                "-p",
                str(POSTGRES_PORT),
                "-h",
                "",
            ],
            stdout=log_descriptor,
            stderr=subprocess.STDOUT,
            close_fds=True,
        )
    finally:
        os.close(log_descriptor)
    try:
        wait_for_postgres(paths, process)
    except BaseException:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)
        raise
    return process


def stop_temporary_postgres(process: subprocess.Popen[Any]) -> None:
    if process.poll() is not None:
        return
    process.send_signal(signal.SIGTERM)
    try:
        process.wait(timeout=30)
    except subprocess.TimeoutExpired as error:
        process.kill()
        process.wait(timeout=10)
        raise InstallError("PostgreSQL did not stop gracefully") from error


def psql_environment(paths: InstallPaths) -> dict[str, str]:
    environment = os.environ.copy()
    for name in list(environment):
        if name.startswith("PG"):
            del environment[name]
    try:
        database_superuser = pwd.getpwuid(os.geteuid()).pw_name
    except KeyError as error:
        raise InstallError("PostgreSQL bootstrap user is unavailable") from error
    environment.update(
        {
            "PATH": f"{paths.psql.parent}{os.pathsep}{environment.get('PATH', '')}",
            "PGHOST": str(paths.socket_directory),
            "PGPORT": str(POSTGRES_PORT),
            "PGUSER": database_superuser,
        }
    )
    return environment


def psql_scalar(paths: InstallPaths, database: str, sql: str, env: dict[str, str]) -> str:
    completed = run(
        [
            str(paths.psql),
            "-X",
            "-qAt",
            "-v",
            "ON_ERROR_STOP=1",
            "-d",
            database,
            "-c",
            sql,
        ],
        env=env,
        capture=True,
    )
    return completed.stdout.strip()


def ensure_roles_and_database(paths: InstallPaths, env: dict[str, str]) -> None:
    for role in [POSTGRES_MIGRATION_ROLE, POSTGRES_RUNTIME_ROLE]:
        exists = psql_scalar(
            paths,
            "postgres",
            f"SELECT 1 FROM pg_catalog.pg_roles WHERE rolname='{role}'",
            env,
        )
        if exists != "1":
            psql_scalar(
                paths,
                "postgres",
                f"CREATE ROLE {role} LOGIN NOINHERIT NOSUPERUSER NOCREATEDB "
                "NOCREATEROLE NOREPLICATION NOBYPASSRLS CONNECTION LIMIT -1 "
                "VALID UNTIL 'infinity'",
                env,
            )
        safe = psql_scalar(
            paths,
            "postgres",
            "SELECT CASE WHEN role.rolcanlogin AND NOT role.rolinherit "
            "AND NOT role.rolsuper AND NOT role.rolcreatedb "
            "AND NOT role.rolcreaterole AND NOT role.rolreplication "
            "AND NOT role.rolbypassrls AND role.rolconnlimit = -1 "
            "AND role.rolvaliduntil = 'infinity'::timestamptz "
            "AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_db_role_setting AS setting "
            "WHERE setting.setrole = role.oid) "
            "AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_auth_members AS membership "
            "WHERE membership.roleid = role.oid OR membership.member = role.oid) "
            f"THEN 1 ELSE 0 END FROM pg_catalog.pg_roles AS role WHERE role.rolname='{role}'",
            env,
        )
        if safe != "1":
            raise InstallError("existing PostgreSQL role authority is unsafe")
    database_exists = psql_scalar(
        paths,
        "postgres",
        "SELECT 1 FROM pg_catalog.pg_database "
        f"WHERE datname='{POSTGRES_DATABASE}'",
        env,
    )
    if database_exists != "1":
        psql_scalar(
            paths,
            "postgres",
            f"CREATE DATABASE {POSTGRES_DATABASE} WITH TEMPLATE template0 "
            f"ENCODING 'UTF8' OWNER {POSTGRES_MIGRATION_ROLE}",
            env,
        )
    owner = psql_scalar(
        paths,
        "postgres",
        "SELECT role.rolname FROM pg_catalog.pg_database AS database "
        "JOIN pg_catalog.pg_roles AS role ON role.oid=database.datdba "
        f"WHERE database.datname='{POSTGRES_DATABASE}'",
        env,
    )
    if owner != POSTGRES_MIGRATION_ROLE:
        raise InstallError("existing Decodex database has an unexpected owner")
    psql_scalar(
        paths,
        POSTGRES_DATABASE,
        f"GRANT USAGE, CREATE ON SCHEMA public TO {POSTGRES_MIGRATION_ROLE}",
        env,
    )
    psql_scalar(
        paths,
        "postgres",
        f"REVOKE CREATE ON DATABASE {POSTGRES_DATABASE} FROM PUBLIC; "
        f"GRANT CONNECT, CREATE ON DATABASE {POSTGRES_DATABASE} "
        f"TO {POSTGRES_MIGRATION_ROLE}; "
        f"GRANT CONNECT ON DATABASE {POSTGRES_DATABASE} TO {POSTGRES_RUNTIME_ROLE}",
        env,
    )


def database_url(paths: InstallPaths, role: str) -> str:
    return (
        f"postgresql://{role}@/{POSTGRES_DATABASE}"
        f"?host={paths.socket_directory.as_posix()}&port={POSTGRES_PORT}"
    )


def load_postgres_harness(paths: InstallPaths) -> ModuleType:
    harness_path = paths.repository / "scripts" / "vnext" / "postgres_store_test.py"
    spec = importlib.util.spec_from_file_location("decodex_postgres_store_test", harness_path)
    if spec is None or spec.loader is None:
        raise InstallError("PostgreSQL provisioning helper is unavailable")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def migrate_and_provision(paths: InstallPaths, env: dict[str, str]) -> None:
    migration_env = env.copy()
    migration_env["DECODEX_TEST_MIGRATION_DATABASE_URL"] = database_url(
        paths, POSTGRES_MIGRATION_ROLE
    )
    migration_env["DECODEX_TEST_RUNTIME_DATABASE_URL"] = database_url(
        paths, POSTGRES_RUNTIME_ROLE
    )
    run(
        [
            "cargo",
            "nextest",
            "run",
            "-p",
            "decodex-postgres",
            "--test",
            "postgres_store",
            "--run-ignored",
            "all",
            "--",
            "postgres_migration_contract",
            "--exact",
        ],
        cwd=paths.repository,
        env=migration_env,
    )
    harness = load_postgres_harness(paths)
    harness.provision_runtime(POSTGRES_DATABASE, POSTGRES_RUNTIME_ROLE, env)


def bootout_service(uid: int) -> None:
    service = f"gui/{uid}/{LAUNCH_AGENT_LABEL}"
    completed = run(
        ["launchctl", "bootout", service],
        capture=True,
        check=False,
    )
    if completed.returncode != 0:
        loaded = run(
            ["launchctl", "print", service],
            capture=True,
            check=False,
        )
        if loaded.returncode == 0:
            raise InstallError("existing local service could not be stopped")


def bootstrap_service(paths: InstallPaths, uid: int) -> None:
    run(["launchctl", "bootstrap", f"gui/{uid}", str(paths.launch_agent)])
    run(["launchctl", "kickstart", "-k", f"gui/{uid}/{LAUNCH_AGENT_LABEL}"])


def query_reset_card(
    paths: InstallPaths,
    arguments: list[str],
) -> dict[str, Any] | None:
    try:
        completed = run(
            [
                str(paths.decodex_cli),
                "--root",
                str(paths.root),
                "--output",
                "json",
                "reset-card",
                *arguments,
            ],
            cwd=paths.repository,
            capture=True,
            check=False,
            timeout=45,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if completed.returncode != 0:
        return None
    try:
        document = json.loads(completed.stdout)
    except json.JSONDecodeError:
        return None
    if (
        not isinstance(document, dict)
        or document.get("schema") != "decodex/reset-card-cli/1"
        or document.get("outcome") != "available"
    ):
        return None
    return document


def critical_doctor_is_ready(document: Any) -> bool:
    if (
        not isinstance(document, dict)
        or document.get("schema") != "decodex/cli-diagnostics/1"
        or document.get("command") != "doctor"
        or document.get("outcome") != "report"
    ):
        return False
    report = document.get("report")
    checks = report.get("checks") if isinstance(report, dict) else None
    if not isinstance(checks, list):
        return False
    required = {
        "configuration",
        "database",
        "protocol",
        "protocol_version",
        "server_identity",
        "server_repositories",
        "credential_vault",
    }
    observed: set[str] = set()
    for check in checks:
        if not isinstance(check, dict):
            return False
        component = check.get("component")
        status = check.get("status")
        kind = component.get("kind") if isinstance(component, dict) else None
        if kind in required:
            if kind in observed or status != {"state": "ready"}:
                return False
            observed.add(kind)
    return observed == required


def query_doctor(paths: InstallPaths) -> bool:
    try:
        completed = run(
            [
                str(paths.decodex_cli),
                "--root",
                str(paths.root),
                "--output",
                "json",
                "doctor",
            ],
            cwd=paths.repository,
            capture=True,
            check=False,
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired):
        return False
    if completed.returncode not in {0, 1}:
        return False
    try:
        document = json.loads(completed.stdout)
    except json.JSONDecodeError:
        return False
    return critical_doctor_is_ready(document)


def account_ids_from_result(document: dict[str, Any]) -> list[str] | None:
    if document.get("command") != "accounts":
        return None
    result = document.get("result")
    if not isinstance(result, dict) or result.get("outcome") != "available":
        return None
    data = result.get("data")
    accounts = data.get("accounts") if isinstance(data, dict) else None
    if not isinstance(accounts, list):
        return None
    account_ids: list[str] = []
    for account in accounts:
        account_id = account.get("account_id") if isinstance(account, dict) else None
        if not isinstance(account_id, str) or not UUID_PATTERN.fullmatch(account_id):
            return None
        account_ids.append(account_id)
    if len(set(account_ids)) != len(account_ids):
        return None
    return account_ids


def inventory_is_available(document: dict[str, Any], account_id: str) -> bool:
    if document.get("command") != "list":
        return False
    result = document.get("result")
    if not isinstance(result, dict) or result.get("outcome") != "available":
        return False
    data = result.get("data")
    return isinstance(data, dict) and data.get("account_id") == account_id


def wait_for_service(paths: InstallPaths, expected_account_ids: set[str]) -> None:
    deadline = time.monotonic() + 180
    last_issue = "reset-card service did not answer"
    while time.monotonic() < deadline:
        if not query_doctor(paths):
            last_issue = "local-service authority is unavailable"
            time.sleep(1)
            continue
        accounts_document = query_reset_card(paths, ["accounts"])
        account_ids = (
            account_ids_from_result(accounts_document)
            if accounts_document is not None
            else None
        )
        if account_ids is None:
            last_issue = "reset-card account discovery is unavailable"
        elif set(account_ids) != expected_account_ids:
            last_issue = "reset-card account discovery set does not agree"
        else:
            inventories_ready = True
            for account_id in account_ids:
                inventory = query_reset_card(
                    paths,
                    ["list", "--account", account_id],
                )
                if inventory is None or not inventory_is_available(inventory, account_id):
                    inventories_ready = False
                    last_issue = "reset-card inventory is unavailable"
                    break
            if inventories_ready:
                return
        time.sleep(1)
    raise InstallError(last_issue)


def validate_host(paths: InstallPaths) -> int:
    if sys.platform != "darwin":
        raise InstallError("the local-service installer is macOS-only")
    if sys.version_info < (3, 9):
        raise InstallError("Python 3.9 or newer is required")
    uid = os.geteuid()
    if uid == 0:
        raise InstallError("the local service must not be installed as root")
    try:
        user_home = Path(pwd.getpwuid(uid).pw_dir).resolve()
    except KeyError as error:
        raise InstallError("the current user home is unavailable") from error
    if not (paths.repository / "Cargo.toml").is_file():
        raise InstallError("repository root is invalid")
    expected_root = user_home / ".decodex"
    if paths.root != expected_root:
        raise InstallError("the local service root must be the platform default")
    if paths.codex.name != "codex":
        raise InstallError("Codex executable name is invalid")
    for name, path in [
        ("decodexd", paths.decodexd),
        ("Decodex CLI", paths.decodex_cli),
        ("Codex", paths.codex),
        ("postgres", paths.postgres),
        ("initdb", paths.initdb),
        ("pg_isready", paths.pg_isready),
        ("psql", paths.psql),
    ]:
        require_regular_executable(path, name)
    if postgres_major(paths.postgres) != 18:
        raise InstallError("PostgreSQL 18 is required")
    return uid


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    paths = install_paths(args)
    uid = validate_host(paths)
    ensure_directories(paths, uid)
    accounts = read_legacy_accounts(paths.legacy_accounts, uid)
    existing_mapping = load_existing_mapping(paths.mapping, uid)
    config_exists = managed_path_exists(
        paths.config,
        "existing Decodex config is malformed",
    )
    existing = existing_enrollments(paths.config, uid) if config_exists else {}
    if config_exists and (not existing_mapping or not existing):
        if not args.replace_config:
            raise InstallError("existing config requires explicit --replace-config")
        existing = {}
    enrollments = build_enrollments(
        accounts,
        existing_mapping,
        existing,
        read_usage_presentations(),
    )
    config = render_config(paths, uid, enrollments).encode("utf-8")
    mapping = render_mapping(enrollments)
    launch_agent = render_launch_agent(paths)

    bootout_service(uid)
    initialize_cluster(paths, uid)
    postgres = start_temporary_postgres(paths)
    try:
        environment = psql_environment(paths)
        ensure_roles_and_database(paths, environment)
        migrate_and_provision(paths, environment)
    finally:
        stop_temporary_postgres(postgres)

    atomic_write(paths.mapping, mapping, 0o600)
    atomic_write(paths.config, config, 0o600)
    atomic_write(paths.launch_agent, launch_agent, 0o600)

    if not args.no_launch:
        bootstrap_service(paths, uid)
        wait_for_service(
            paths,
            {enrollment.account_id for enrollment in enrollments},
        )

    print(
        json.dumps(
            {
                "schema": "decodex/local-service-install/1",
                "outcome": "success",
                "account_count": len(enrollments),
                "postgres_major": 18,
                "launch_agent": LAUNCH_AGENT_LABEL,
                "launched": not args.no_launch,
            },
            sort_keys=True,
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except InstallError as error:
        print(f"decodex local-service install failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
