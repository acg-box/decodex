#!/usr/bin/env python3
"""Provision and install the same-UID Decodex local service on macOS.

This source-install tool owns the offline one-shot account cutover and local
development installation. It leaves every legacy source unchanged as a cold
backup and starts only the credential-negative vNext service after verification.
"""

from __future__ import annotations

import argparse
import base64
import fcntl
import hashlib
import json
import os
import plistlib
import pwd
import re
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import threading
import time
import unicodedata
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable


MAPPING_SCHEMA = "decodex/reset-card-legacy-bridge/1"
MIGRATION_MANIFEST_SCHEMA = "decodex/account-migration-manifest/1"
LAUNCH_AGENT_LABEL = "space.decodex.local-service"
MAX_ACCOUNT_FILE_BYTES = 4 * 1024 * 1024
MAX_ACCOUNT_LINE_BYTES = 128 * 1024
MAX_CONFIG_FILE_BYTES = 1024 * 1024
MAX_MAPPING_FILE_BYTES = 64 * 1024
MAX_LAUNCH_AGENT_FILE_BYTES = 64 * 1024
MAX_MIGRATION_MANIFEST_BYTES = 1024 * 1024
MAX_POSTGRES_VERSION_BYTES = 16
LEGACY_LOCK_TIMEOUT_SECONDS = 5
LOCAL_SERVICE_SETTLEMENT_TIMEOUT_SECONDS = 300
LOCAL_SERVICE_SETTLEMENT_POLL_SECONDS = 0.25
LOCAL_SERVICE_CONTROL_TIMEOUT_SECONDS = 5
INSTALLER_COMMAND_TIMEOUT_SECONDS = 180
MAX_INSTALLER_CHILD_OUTPUT_BYTES = 1024 * 1024
MAX_TEMPORARY_POSTGRES_OUTPUT_BYTES = 8 * 1024 * 1024
LAUNCHCTL_PRINT_NOT_FOUND_STATUS = 113
MAX_ACCOUNTS = 64
MAX_RUNTIME_ACCOUNTS = 512
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
ACCOUNT_RANDOM_NAMES = (
    "Alex", "Avery", "Bailey", "Blake", "Casey", "Charlie", "Clara", "Dana",
    "Drew", "Eden", "Elliot", "Emery", "Evan", "Finley", "Harper", "Hayden",
    "Iris", "Jamie", "Jordan", "Kai", "Kendall", "Lane", "Liam", "Logan",
    "Mason", "Maya", "Mia", "Morgan", "Noah", "Nora", "Owen", "Paige",
    "Parker", "Quinn", "Reese", "Remy", "Riley", "Rowan", "Sage", "Sasha",
    "Sidney", "Taylor", "Theo", "Val",
)
UUID_PATTERN = re.compile(
    r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"
)
HEX_DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")


class InstallError(RuntimeError):
    """A value-free local-service installation failure."""


@dataclass(frozen=True)
class ProcessIdentity:
    process_id: int
    started_at: str


@dataclass(frozen=True)
class ProcessRecord:
    parent_id: int
    identity: ProcessIdentity


@dataclass(frozen=True)
class ServiceObservation:
    loaded: bool
    active_process_id: int | None
    root: ProcessIdentity | None
    generation: frozenset[ProcessIdentity]


@dataclass(frozen=True)
class LegacyAccount:
    provider_account_id: str
    email: str
    plan_type: str
    disabled: bool
    access_token: str
    refresh_token: str
    id_token: str
    access_token_expires_at_unix_micros: int

    @property
    def provider_account_id_sha256(self) -> str:
        return hashlib.sha256(self.provider_account_id.encode("utf-8")).hexdigest()


@dataclass(frozen=True)
class Enrollment:
    slot: int
    provider_account_id_sha256: str
    account_id: str
    operation_id: str
    display_label: str
    enabled: bool
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
    vnext_config_source: Path
    staging_config: Path
    mapping: Path
    migration_manifest: Path
    credential_directory: Path
    data_directory: Path
    socket_directory: Path
    log_directory: Path
    postgres_log: Path
    service_log: Path
    legacy_accounts: Path
    legacy_config: Path
    launch_agent: Path
    decodexd: Path
    decodex_cli: Path
    codex: Path
    postgres: Path
    initdb: Path
    pg_isready: Path
    psql: Path

    @property
    def server_directory(self) -> Path:
        return self.root / "server"

    @property
    def namespace_lock(self) -> Path:
        return self.server_directory / "decodex.lock"


class InstallerNamespaceLock:
    """Retained installer ownership of the existing local-listener namespace lock."""

    def __init__(
        self,
        paths: InstallPaths,
        uid: int,
        directory_descriptor: int,
        lock_descriptor: int,
        directory_identity: tuple[int, int],
        lock_identity: tuple[int, int, int, int, int],
    ) -> None:
        self.paths = paths
        self.uid = uid
        self.directory_descriptor = directory_descriptor
        self.lock_descriptor = lock_descriptor
        self.directory_identity = directory_identity
        self.lock_identity = lock_identity
        self.closed = False

    @classmethod
    def acquire(cls, paths: InstallPaths, uid: int) -> "InstallerNamespaceLock":
        try:
            directory_descriptor = open_absolute_directory(paths.server_directory)
        except OSError as error:
            raise InstallError("local service namespace directory is unsafe") from error
        lock_descriptor: int | None = None
        try:
            directory_metadata = os.fstat(directory_descriptor)
            require_namespace_directory(directory_metadata, uid)
            lock_flags = os.O_RDWR
            for flag in ("O_NOFOLLOW", "O_CLOEXEC"):
                lock_flags |= getattr(os, flag, 0)
            try:
                lock_descriptor = os.open(
                    "decodex.lock",
                    lock_flags | os.O_CREAT | os.O_EXCL,
                    0o600,
                    dir_fd=directory_descriptor,
                )
                os.fchmod(lock_descriptor, 0o600)
            except FileExistsError:
                lock_descriptor = os.open(
                    "decodex.lock",
                    lock_flags,
                    dir_fd=directory_descriptor,
                )
            lock_metadata = os.fstat(lock_descriptor)
            require_namespace_lock(lock_metadata, uid)
            try:
                fcntl.flock(lock_descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as error:
                raise InstallError("local service namespace is already owned") from error
            guard = cls(
                paths,
                uid,
                directory_descriptor,
                lock_descriptor,
                (directory_metadata.st_dev, directory_metadata.st_ino),
                namespace_lock_identity(lock_metadata),
            )
            guard.verify()
            return guard
        except BaseException as error:
            if lock_descriptor is not None:
                os.close(lock_descriptor)
            os.close(directory_descriptor)
            if isinstance(error, InstallError):
                raise
            raise InstallError("local service namespace lock is unsafe") from error

    def verify(self) -> None:
        if self.closed:
            raise InstallError("local service namespace ownership is unavailable")
        try:
            held_directory = os.fstat(self.directory_descriptor)
            held_lock = os.fstat(self.lock_descriptor)
        except OSError as error:
            raise InstallError("local service namespace ownership changed") from error
        require_namespace_directory(held_directory, self.uid)
        require_namespace_lock(held_lock, self.uid)
        try:
            current_directory_descriptor = open_absolute_directory(
                self.paths.server_directory
            )
        except OSError as error:
            raise InstallError("local service namespace ownership changed") from error
        try:
            current_directory = os.fstat(current_directory_descriptor)
            require_namespace_directory(current_directory, self.uid)
            pinned_lock = os.stat(
                "decodex.lock",
                dir_fd=self.directory_descriptor,
                follow_symlinks=False,
            )
            require_namespace_lock(pinned_lock, self.uid)
            current_lock = os.stat(
                "decodex.lock",
                dir_fd=current_directory_descriptor,
                follow_symlinks=False,
            )
            require_namespace_lock(current_lock, self.uid)
        except OSError as error:
            raise InstallError("local service namespace ownership changed") from error
        finally:
            os.close(current_directory_descriptor)
        if (
            (held_directory.st_dev, held_directory.st_ino) != self.directory_identity
            or (current_directory.st_dev, current_directory.st_ino)
            != self.directory_identity
            or namespace_lock_identity(held_lock) != self.lock_identity
            or namespace_lock_identity(pinned_lock) != self.lock_identity
            or namespace_lock_identity(current_lock) != self.lock_identity
        ):
            raise InstallError("local service namespace ownership changed")

    def borrow(self) -> int:
        self.verify()
        try:
            descriptor = os.dup(self.lock_descriptor)
        except OSError as error:
            raise InstallError("local service namespace ownership is unavailable") from error
        try:
            os.set_inheritable(descriptor, True)
        except BaseException:
            os.close(descriptor)
            raise
        return descriptor

    def close(self) -> None:
        if self.closed:
            return
        self.closed = True
        failure: OSError | None = None
        for descriptor in (self.lock_descriptor, self.directory_descriptor):
            try:
                os.close(descriptor)
            except OSError as error:
                failure = failure or error
        if failure is not None:
            raise InstallError("local service namespace ownership could not close") from failure


def open_absolute_directory(path: Path) -> int:
    if not path.is_absolute() or any(part in {".", ".."} for part in path.parts):
        raise InstallError("local service namespace directory is unsafe")
    flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_CLOEXEC", 0)
    descriptor = os.open("/", flags)
    try:
        for component in path.parts[1:]:
            next_descriptor = os.open(component, flags, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = next_descriptor
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def require_namespace_directory(metadata: os.stat_result, uid: int) -> None:
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or metadata.st_uid != uid
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        raise InstallError("local service namespace directory is unsafe")


def require_namespace_lock(metadata: os.stat_result, uid: int) -> None:
    if (
        not stat.S_ISREG(metadata.st_mode)
        or metadata.st_uid != uid
        or stat.S_IMODE(metadata.st_mode) != 0o600
        or metadata.st_nlink != 1
    ):
        raise InstallError("local service namespace lock is unsafe")


def namespace_lock_identity(metadata: os.stat_result) -> tuple[int, int, int, int, int]:
    return (
        metadata.st_dev,
        metadata.st_ino,
        metadata.st_uid,
        stat.S_IMODE(metadata.st_mode),
        metadata.st_nlink,
    )


@dataclass(frozen=True)
class MigrationStagingOwner:
    staging_config: Path
    credential_directory: Path
    credential_files: tuple[Path, ...]

    @classmethod
    def for_accounts(
        cls,
        paths: InstallPaths,
        account_ids: list[str],
    ) -> "MigrationStagingOwner":
        if len(account_ids) != len(set(account_ids)) or any(
            UUID_PATTERN.fullmatch(account_id) is None for account_id in account_ids
        ):
            raise InstallError("account migration cleanup ownership is invalid")
        return cls(
            staging_config=paths.staging_config,
            credential_directory=paths.credential_directory,
            credential_files=tuple(
                paths.credential_directory / f"{account_id}.json"
                for account_id in account_ids
            ),
        )

    def cleanup(self) -> None:
        failures: list[OSError] = []
        for path in self.credential_files:
            try:
                path.unlink(missing_ok=True)
            except OSError as error:
                failures.append(error)
        try:
            self.credential_directory.rmdir()
        except FileNotFoundError:
            pass
        except OSError as error:
            failures.append(error)
        try:
            self.staging_config.unlink(missing_ok=True)
        except OSError as error:
            failures.append(error)
        if failures:
            raise InstallError(
                "account migration staging could not be retired"
            ) from failures[0]


def finish_account_migration_staging(
    staging_owner: MigrationStagingOwner,
    primary_error: BaseException | None,
) -> None:
    try:
        staging_owner.cleanup()
    except BaseException as cleanup_error:
        if primary_error is not None:
            raise primary_error from cleanup_error
        raise
    if primary_error is not None:
        raise primary_error


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
        vnext_config_source=root / "account-migration-vnext-source.toml",
        staging_config=root / ".account-migration-runtime.toml",
        mapping=root / "reset-card-legacy-map.json",
        migration_manifest=root / "account-migration-manifest.json",
        credential_directory=root / "account-migration-credentials",
        data_directory=root / "postgres" / "data",
        socket_directory=root / "postgres" / "socket",
        log_directory=root / "logs",
        postgres_log=root / "logs" / "postgres.log",
        service_log=root / "logs" / "local-service.log",
        legacy_accounts=args.legacy_accounts.expanduser().absolute(),
        legacy_config=(args.legacy_accounts.expanduser().absolute().parent / "config.toml"),
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


def require_private_legacy_source_chain(path: Path, uid: int) -> None:
    effective_uid = os.geteuid()
    if uid != effective_uid:
        raise InstallError("legacy account source owner is not the effective user")
    try:
        home = Path(pwd.getpwuid(effective_uid).pw_dir)
    except (AttributeError, KeyError, TypeError) as error:
        raise InstallError("login home authority is unavailable") from error
    if not home.is_absolute() or not path.is_absolute() or ".." in path.parts:
        raise InstallError("legacy account source must remain under the user home")
    try:
        path.parent.relative_to(home)
    except ValueError as error:
        raise InstallError("legacy account source must remain under the user home") from error

    direct_parent = path.parent
    try:
        direct_metadata = direct_parent.lstat()
    except OSError as error:
        raise InstallError("legacy account source parent is unsafe") from error
    if (
        not stat.S_ISDIR(direct_metadata.st_mode)
        or stat.S_ISLNK(direct_metadata.st_mode)
        or direct_metadata.st_uid != effective_uid
        or stat.S_IMODE(direct_metadata.st_mode) != 0o700
    ):
        raise InstallError("legacy account source parent is unsafe")

    for current in direct_parent.parents:
        try:
            metadata = current.lstat()
        except OSError as error:
            raise InstallError("legacy account source parent is unsafe") from error
        if (
            not stat.S_ISDIR(metadata.st_mode)
            or stat.S_ISLNK(metadata.st_mode)
            or metadata.st_uid not in (0, effective_uid)
            or stat.S_IMODE(metadata.st_mode) & 0o022 != 0
        ):
            raise InstallError("legacy account source parent is unsafe")


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
            or stat.S_IMODE(metadata.st_mode) != 0o600
        ):
            raise InstallError("legacy account file authority is unsafe")
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


def read_owned_file(
    path: Path,
    uid: int,
    maximum_bytes: int,
    failure: str,
    *,
    required_mode: int | None = None,
) -> bytes:
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
            or (
                required_mode is not None
                and stat.S_IMODE(metadata.st_mode) != required_mode
            )
        ):
            raise InstallError(failure)
        if required_mode is None:
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
        disabled_value = record.get("disabled", False)
    else:
        auth = record
        outer_email = record.get("email")
        disabled_value = record.get("disabled", False)
    if not isinstance(disabled_value, bool):
        raise InstallError("legacy account administrative state is invalid")
    tokens = auth.get("tokens")
    if not isinstance(tokens, dict):
        raise InstallError("legacy account credentials are unavailable")
    provider_account_id = bounded_scalar(tokens.get("account_id"), MAX_ACCOUNT_ID_BYTES)
    access_token = bounded_scalar(tokens.get("access_token"), MAX_ACCESS_TOKEN_BYTES)
    refresh_token = bounded_scalar(tokens.get("refresh_token"), MAX_ACCESS_TOKEN_BYTES)
    id_token = bounded_scalar(tokens.get("id_token"), MAX_ACCESS_TOKEN_BYTES)
    email_value = (
        outer_email
        if outer_email is not None
        else auth.get("email", tokens.get("email"))
    )
    email = bounded_scalar(email_value, MAX_EMAIL_BYTES)
    if provider_account_id is None:
        raise InstallError("legacy provider account identity is invalid")
    if (
        access_token is None
        or refresh_token is None
        or id_token is None
        or email is None
        or "@" not in email
    ):
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
    access_claims = decode_id_token_claims(access_token)
    expires_at = access_claims.get("exp")
    if (
        not isinstance(expires_at, int)
        or isinstance(expires_at, bool)
        or expires_at <= 0
        or expires_at > (2**63 - 1) // 1_000_000
    ):
        raise InstallError("legacy account access-token expiry is invalid")
    return LegacyAccount(
        provider_account_id=provider_account_id,
        email=email,
        plan_type=plan_type,
        disabled=disabled_value,
        access_token=access_token,
        refresh_token=refresh_token,
        id_token=id_token,
        access_token_expires_at_unix_micros=expires_at * 1_000_000,
    )


def lock_and_read_legacy_accounts(
    path: Path,
    uid: int,
) -> tuple[list[LegacyAccount], bytes | None, int | None]:
    if not managed_path_exists(path, "legacy account file authority is unsafe"):
        return [], None, None
    parent = path.parent
    require_private_legacy_source_chain(path, uid)
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
    except BaseException:
        try:
            fcntl.flock(lock_descriptor, fcntl.LOCK_UN)
        finally:
            os.close(lock_descriptor)
        raise
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
    return accounts, body, lock_descriptor


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


def parse_legacy_account_config(body: bytes | None) -> tuple[str | None, dict[str, int]]:
    if body is None:
        return None, {}
    try:
        lines = body.decode("utf-8").splitlines()
    except UnicodeDecodeError as error:
        raise InstallError("legacy account config is not UTF-8") from error
    section = ""
    fixed_account: str | None = None
    offsets: dict[str, int] = {}
    for raw_line in lines:
        stripped = raw_line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if stripped.startswith("["):
            match = re.fullmatch(r"\[([^\]]+)\]", stripped)
            if match is None:
                raise InstallError("legacy account config section is malformed")
            section = match.group(1)
            continue
        if section == "codex.accounts" and stripped.startswith("fixed_account"):
            match = re.fullmatch(
                r'fixed_account\s*=\s*("(?:[^"\\]|\\.)*")\s*(?:#.*)?',
                stripped,
            )
            if match is None or fixed_account is not None:
                raise InstallError("legacy fixed-account selector is malformed")
            try:
                fixed_account = bounded_scalar(json.loads(match.group(1)), 1024)
            except json.JSONDecodeError as error:
                raise InstallError("legacy fixed-account selector is malformed") from error
            if fixed_account is None:
                raise InstallError("legacy fixed-account selector is malformed")
        elif section == "codex.account_names.offsets":
            match = re.fullmatch(
                r'(?:("(?:[^"\\]|\\.)*")|([A-Za-z0-9_-]+))\s*=\s*(-?[0-9]+)\s*(?:#.*)?',
                stripped,
            )
            if match is None:
                raise InstallError("legacy account-name offset is malformed")
            try:
                key = json.loads(match.group(1)) if match.group(1) else match.group(2)
                offset = int(match.group(3)) % len(ACCOUNT_RANDOM_NAMES)
            except (json.JSONDecodeError, ValueError) as error:
                raise InstallError("legacy account-name offset is malformed") from error
            if bounded_scalar(key, 128) is None or key in offsets:
                raise InstallError("legacy account-name offset is malformed")
            offsets[key] = offset
    return fixed_account, offsets


def redacted_provider_account_id(provider_account_id: str) -> str:
    tail = provider_account_id[-6:]
    return f"...{tail}" if tail else "unknown"


def account_identity_hash(value: str) -> int:
    encoded = (value.strip() or "account").encode("utf-16-le")
    result = 2_166_136_261
    for offset in range(0, len(encoded), 2):
        result ^= int.from_bytes(encoded[offset : offset + 2], "little")
        result = (result * 16_777_619) & 0xFFFFFFFF
    return result


def derive_legacy_labels(
    accounts: list[LegacyAccount],
    offsets: dict[str, int],
) -> dict[str, str]:
    candidates: list[tuple[str, str, str, int]] = []
    for account in accounts:
        seed = redacted_provider_account_id(account.provider_account_id)
        identity_hash = account_identity_hash(seed)
        key = f"{identity_hash:08x}"
        preferred = (identity_hash + offsets.get(key, 0)) % len(ACCOUNT_RANDOM_NAMES)
        candidates.append((key, account.email, account.provider_account_id_sha256, preferred))
    labels: dict[str, str] = {}
    used: set[str] = set()
    for _, _, digest, preferred in sorted(candidates):
        label = ""
        for probe in range(len(ACCOUNT_RANDOM_NAMES)):
            candidate = ACCOUNT_RANDOM_NAMES[(preferred + probe) % len(ACCOUNT_RANDOM_NAMES)]
            if candidate not in used:
                label = candidate
                break
        if not label:
            base = ACCOUNT_RANDOM_NAMES[preferred]
            suffix = 2
            while f"{base} {suffix}" in used:
                suffix += 1
            label = f"{base} {suffix}"
        used.add(label)
        labels[digest] = label
    return labels


def load_existing_manifest_accounts(path: Path, uid: int) -> dict[str, dict[str, Any]]:
    if not managed_path_exists(path, "existing account migration manifest is malformed"):
        return {}
    try:
        payload = json.loads(
            read_owned_file(
                path,
                uid,
                MAX_MIGRATION_MANIFEST_BYTES,
                "existing account migration manifest is malformed",
            )
        )
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise InstallError("existing account migration manifest is malformed") from error
    if (
        not isinstance(payload, dict)
        or payload.get("schema") != MIGRATION_MANIFEST_SCHEMA
        or not isinstance(payload.get("accounts"), list)
    ):
        raise InstallError("existing account migration manifest is malformed")
    result: dict[str, dict[str, Any]] = {}
    for account in payload["accounts"]:
        if not isinstance(account, dict):
            raise InstallError("existing account migration manifest is malformed")
        digest = account.get("provider_account_id_sha256")
        if (
            not isinstance(digest, str)
            or not HEX_DIGEST_PATTERN.fullmatch(digest)
            or not UUID_PATTERN.fullmatch(str(account.get("account_id", "")))
            or not UUID_PATTERN.fullmatch(str(account.get("operation_id", "")))
            or digest in result
        ):
            raise InstallError("existing account migration manifest is malformed")
        result[digest] = account
    return result


def build_enrollments(
    accounts: list[LegacyAccount],
    existing_mapping: dict[str, int],
    existing: dict[int, ExistingEnrollment],
    manifest_accounts: dict[str, dict[str, Any]],
    fixed_selector: str | None,
    offsets: dict[str, int],
) -> tuple[list[Enrollment], str | None, list[str]]:
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
    if manifest_accounts and set(manifest_accounts) != digests:
        raise InstallError("account migration manifest source set changed")
    mapping = existing_mapping or {
        account.provider_account_id_sha256: index
        for index, account in enumerate(accounts, start=1)
    }
    derived_labels = derive_legacy_labels(accounts, offsets)
    enrollments: list[Enrollment] = []
    fixed_account_id: str | None = None
    for ordinal, account in enumerate(accounts):
        slot = mapping[account.provider_account_id_sha256]
        prior_enrollment = existing.get(slot)
        prior_manifest = manifest_accounts.get(account.provider_account_id_sha256)
        if prior_enrollment is not None:
            label = prior_enrollment.display_label
            account_id = prior_enrollment.account_id
        elif prior_manifest is not None:
            label = prior_manifest.get("display_label")
            account_id = prior_manifest.get("account_id")
            if bounded_scalar(label, 128) is None or not UUID_PATTERN.fullmatch(str(account_id)):
                raise InstallError("existing account migration manifest is malformed")
        else:
            label = derived_labels[account.provider_account_id_sha256]
            account_id = str(uuid.uuid4())
        if prior_manifest is not None:
            if prior_manifest.get("account_id") != account_id or prior_manifest.get("display_label") != label:
                raise InstallError("vNext identity decisions do not agree")
            operation_id = prior_manifest["operation_id"]
        else:
            operation_id = str(uuid.uuid4())
        enrollments.append(
            Enrollment(
                slot=slot,
                provider_account_id_sha256=account.provider_account_id_sha256,
                account_id=account_id,
                operation_id=operation_id,
                display_label=label,
                enabled=not account.disabled,
                plan_type=account.plan_type,
            )
        )
        if fixed_selector is not None and fixed_selector in {
            account.email,
            account.provider_account_id,
            redacted_provider_account_id(account.provider_account_id),
        }:
            if fixed_account_id is not None:
                raise InstallError("legacy fixed-account selector is ambiguous")
            fixed_account_id = account_id
    if fixed_selector is not None and fixed_account_id is None:
        raise InstallError("legacy fixed-account selector does not resolve")
    return enrollments, fixed_account_id, [enrollment.account_id for enrollment in enrollments]


def toml_string(value: str | Path) -> str:
    return json.dumps(str(value), ensure_ascii=False)


def render_config(paths: InstallPaths, uid: int) -> str:
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


def source_record(role: str, path: Path, body: bytes | None) -> dict[str, Any]:
    record: dict[str, Any] = {
        "role": role,
        "path": str(path),
        "present": body is not None,
        "byte_count": None,
        "sha256": None,
    }
    if body is not None:
        record["byte_count"] = len(body)
        record["sha256"] = hashlib.sha256(body).hexdigest()
    return record


def read_optional_owned_source(
    path: Path,
    uid: int,
    maximum_bytes: int,
    failure: str,
) -> bytes | None:
    if not managed_path_exists(path, failure):
        return None
    require_private_legacy_source_chain(path, uid)
    return read_owned_file(
        path,
        uid,
        maximum_bytes,
        failure,
        required_mode=0o600,
    )


def prepare_vnext_config_source(paths: InstallPaths, uid: int) -> bytes | None:
    archived = read_optional_owned_source(
        paths.vnext_config_source,
        uid,
        MAX_CONFIG_FILE_BYTES,
        "archived vNext account source is malformed",
    )
    current = read_optional_owned_source(
        paths.config,
        uid,
        MAX_CONFIG_FILE_BYTES,
        "existing Decodex config is malformed",
    )
    if archived is not None:
        if current is not None and b"server_host.reset_card_accounts" in current and current != archived:
            raise InstallError("archived and active vNext account sources do not agree")
        return archived
    if current is None:
        return None
    if b"server_host.reset_card_accounts" not in current:
        raise InstallError("existing Decodex config has no archived account source")
    atomic_write(paths.vnext_config_source, current, 0o600)
    return current


def credential_sources(
    paths: InstallPaths,
    accounts: list[LegacyAccount],
    enrollments: list[Enrollment],
    uid: int,
) -> dict[str, str]:
    try:
        paths.credential_directory.mkdir(mode=0o700, parents=False, exist_ok=True)
    except OSError as error:
        raise InstallError("account migration credential directory is unavailable") from error
    require_private_directory(paths.credential_directory, uid)
    by_digest = {account.provider_account_id_sha256: account for account in accounts}
    expected_names = {f"{enrollment.account_id}.json" for enrollment in enrollments}
    try:
        actual_names = {entry.name for entry in paths.credential_directory.iterdir()}
    except OSError as error:
        raise InstallError("account migration credential directory is unavailable") from error
    if actual_names - expected_names:
        raise InstallError("account migration credential directory contains unexpected files")
    digests: dict[str, str] = {}
    for enrollment in enrollments:
        account = by_digest[enrollment.provider_account_id_sha256]
        payload = {
            "schema": "decodex/account-credential-import/1",
            "provider": "chatgpt",
            "provider_account_id": account.provider_account_id,
            "provider_email": account.email,
            "access_token": account.access_token,
            "refresh_token": account.refresh_token,
            "id_token": account.id_token,
            "plan_type": account.plan_type,
            "token_type": "bearer",
            "access_token_expires_at_unix_micros": account.access_token_expires_at_unix_micros,
        }
        body = (json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n").encode()
        atomic_write(paths.credential_directory / f"{enrollment.account_id}.json", body, 0o600)
        digests[enrollment.account_id] = hashlib.sha256(body).hexdigest()
    return digests


def decision_digest(value: Any) -> str:
    normalized = json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(normalized).hexdigest()


def render_migration_manifest(
    paths: InstallPaths,
    uid: int,
    sources: list[dict[str, Any]],
    enrollments: list[Enrollment],
    credential_digests: dict[str, str],
    fixed_account_id: str | None,
    order: list[str],
) -> bytes:
    routing: dict[str, Any] = {"mode": "balanced", "order": order}
    if fixed_account_id is not None:
        routing = {"mode": "fixed", "account_id": fixed_account_id, "order": order}
    accounts = [
        {
            "source_ordinal": ordinal,
            "account_id": enrollment.account_id,
            "operation_id": enrollment.operation_id,
            "provider": "chatgpt",
            "provider_account_id_sha256": enrollment.provider_account_id_sha256,
            "display_label": enrollment.display_label,
            "enabled": enrollment.enabled,
            "credential_source_sha256": credential_digests[enrollment.account_id],
        }
        for ordinal, enrollment in enumerate(enrollments)
    ]
    decision_fingerprints = {
        "credentials_sha256": decision_digest(
            [
                {
                    "account_id": account["account_id"],
                    "credential_source_sha256": account["credential_source_sha256"],
                }
                for account in accounts
            ]
        ),
        "labels_sha256": decision_digest(
            [
                {
                    "account_id": account["account_id"],
                    "display_label": account["display_label"],
                }
                for account in accounts
            ]
        ),
        "enabled_sha256": decision_digest(
            [
                {"account_id": account["account_id"], "enabled": account["enabled"]}
                for account in accounts
            ]
        ),
        "routing_sha256": decision_digest(routing),
        "provider_sha256": decision_digest(
            [
                {
                    "account_id": account["account_id"],
                    "provider": account["provider"],
                    "provider_account_id_sha256": account[
                        "provider_account_id_sha256"
                    ],
                }
                for account in accounts
            ]
        ),
        "quota_sha256": decision_digest({"policy": "reset_to_unknown"}),
        "usage_profile_sha256": decision_digest({"policy": "start_empty"}),
        "history_sha256": decision_digest({"policy": "do_not_import"}),
    }
    payload = {
        "schema": MIGRATION_MANIFEST_SCHEMA,
        "sources": sources,
        "quota_policy": "reset_to_unknown",
        "usage_profile_policy": "start_empty",
        "history_policy": "do_not_import",
        "decision_fingerprints": decision_fingerprints,
        "accounts": accounts,
        "routing": routing,
    }
    generated = (json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n").encode()
    if managed_path_exists(paths.migration_manifest, "account migration manifest is malformed"):
        existing = read_owned_file(
            paths.migration_manifest,
            uid,
            MAX_MIGRATION_MANIFEST_BYTES,
            "account migration manifest is malformed",
        )
        try:
            existing_payload = json.loads(existing)
            existing_accounts = (
                existing_payload.get("accounts")
                if isinstance(existing_payload, dict)
                else None
            )
            existing_decisions = (
                existing_payload.get("decision_fingerprints")
                if isinstance(existing_payload, dict)
                else None
            )
            if (
                not isinstance(existing_accounts, list)
                or not isinstance(existing_decisions, dict)
                or any(not isinstance(account, dict) for account in existing_accounts)
            ):
                raise InstallError("account migration manifest is malformed")
            for account in existing_accounts:
                account.pop("target", None)
            existing_decisions["credentials_sha256"] = decision_digest(
                [
                    {
                        "account_id": account.get("account_id"),
                        "credential_source_sha256": account.get(
                            "credential_source_sha256"
                        ),
                    }
                    for account in existing_accounts
                ]
            )
            if existing_payload != payload:
                raise InstallError("account migration manifest conflicts with current sources")
        except json.JSONDecodeError as error:
            raise InstallError("account migration manifest is malformed") from error
        return existing
    return generated


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
        # A successful supervised drain leaves the loaded job inactive until the installer
        # removes it. Unexpected failures remain restartable.
        "KeepAlive": {"SuccessfulExit": False},
        "ExitTimeOut": 60,
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
    timeout: float | None = INSTALLER_COMMAND_TIMEOUT_SECONDS,
    pass_fds: tuple[int, ...] = (),
) -> subprocess.CompletedProcess[str]:
    if timeout is None or timeout <= 0:
        raise InstallError("installer child timeout is invalid")
    process = subprocess.Popen(
        command,
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        pass_fds=pass_fds,
        start_new_session=True,
    )
    stdout_bytes, stderr_bytes = communicate_bounded(
        process,
        command,
        timeout,
    )
    stdout = stdout_bytes.decode("utf-8", errors="strict")
    stderr = stderr_bytes.decode("utf-8", errors="strict")
    completed = subprocess.CompletedProcess(
        command,
        process.returncode,
        stdout if capture else None,
        stderr if capture else None,
    )
    if check and process.returncode != 0:
        raise subprocess.CalledProcessError(
            process.returncode,
            command,
            output=stdout if capture else None,
            stderr=stderr if capture else None,
        )
    return completed


def terminate_bounded_process(process: subprocess.Popen[Any]) -> None:
    process_group_id = process.pid
    try:
        os.killpg(process_group_id, signal.SIGTERM)
    except ProcessLookupError:
        pass
    time.sleep(0.25)
    try:
        os.killpg(process_group_id, signal.SIGKILL)
    except ProcessLookupError:
        pass
    if process.returncode is None:
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired as error:
            raise InstallError("installer child could not be reaped") from error


def communicate_bounded(
    process: subprocess.Popen[Any],
    command: list[str],
    timeout: float,
) -> tuple[bytes, bytes]:
    if process.stdout is None or process.stderr is None:
        terminate_bounded_process(process)
        raise InstallError("installer child output pipes are unavailable")
    streams = {
        process.stdout.fileno(): ("stdout", process.stdout),
        process.stderr.fileno(): ("stderr", process.stderr),
    }
    chunks: dict[str, list[bytes]] = {"stdout": [], "stderr": []}
    output_bytes = 0
    deadline = time.monotonic() + timeout
    selector = selectors.DefaultSelector()
    try:
        for descriptor in streams:
            os.set_blocking(descriptor, False)
            selector.register(descriptor, selectors.EVENT_READ)
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise subprocess.TimeoutExpired(
                    command,
                    timeout,
                    output=b"".join(chunks["stdout"]),
                    stderr=b"".join(chunks["stderr"]),
                )
            events = selector.select(min(0.25, remaining))
            for key, _ in events:
                descriptor = key.fd
                name, stream = streams[descriptor]
                try:
                    chunk = os.read(descriptor, 64 * 1024)
                except BlockingIOError:
                    continue
                if not chunk:
                    selector.unregister(descriptor)
                    stream.close()
                    continue
                output_bytes += len(chunk)
                if output_bytes > MAX_INSTALLER_CHILD_OUTPUT_BYTES:
                    raise InstallError("installer child output exceeded its bound")
                chunks[name].append(chunk)
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise subprocess.TimeoutExpired(
                command,
                timeout,
                output=b"".join(chunks["stdout"]),
                stderr=b"".join(chunks["stderr"]),
            )
        process.wait(timeout=remaining)
    except BaseException:
        terminate_bounded_process(process)
        raise
    finally:
        selector.close()
        for _, stream in streams.values():
            if not stream.closed:
                stream.close()
    return b"".join(chunks["stdout"]), b"".join(chunks["stderr"])


def run_installer_child(
    command: list[str],
    namespace_lock: InstallerNamespaceLock,
    *,
    cwd: Path,
    capture: bool,
    transition_gate_fd: int | None = None,
) -> subprocess.CompletedProcess[str]:
    lock_descriptor = namespace_lock.borrow()
    gate_descriptor = (
        os.dup(transition_gate_fd) if transition_gate_fd is not None else None
    )
    inherited = [lock_descriptor]
    child_command = [
        *command,
        "--installer-lock-fd",
        str(lock_descriptor),
    ]
    if gate_descriptor is not None:
        inherited.append(gate_descriptor)
        child_command.extend(["--transition-gate-fd", str(gate_descriptor)])
    try:
        process = subprocess.Popen(
            child_command,
            cwd=cwd,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            pass_fds=tuple(inherited),
            start_new_session=True,
        )
    finally:
        if gate_descriptor is not None:
            os.close(gate_descriptor)
        os.close(lock_descriptor)
    try:
        stdout_bytes, stderr_bytes = communicate_bounded(
            process,
            child_command,
            INSTALLER_COMMAND_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as error:
        raise InstallError("installer child timed out") from error
    stdout = stdout_bytes.decode("utf-8", errors="strict")
    stderr = stderr_bytes.decode("utf-8", errors="strict")
    completed = subprocess.CompletedProcess(
        child_command,
        process.returncode,
        stdout if capture else None,
        stderr if capture else None,
    )
    if process.returncode != 0:
        raise subprocess.CalledProcessError(
            process.returncode,
            child_command,
            output=stdout if capture else None,
            stderr=stderr if capture else None,
        )
    return completed


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
        paths.server_directory,
    ]:
        path.mkdir(parents=True, exist_ok=True, mode=0o700)
        require_owned_directory(path, uid, "local service directory authority is unsafe")
        os.chmod(path, 0o700)
    service_log = open_private_append_file(paths.service_log, uid)
    os.close(service_log)


def ensure_installer_namespace_layout(paths: InstallPaths, uid: int) -> None:
    for path in (paths.root, paths.server_directory):
        path.mkdir(parents=True, exist_ok=True, mode=0o700)
        require_owned_directory(
            path,
            uid,
            "local service namespace directory is unsafe",
        )
        os.chmod(path, 0o700)


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


class _TemporaryPostgresOutput:
    def __init__(
        self,
        process: subprocess.Popen[Any],
        stream: Any,
        log_descriptor: int,
        remaining_bytes: int,
    ) -> None:
        self._process = process
        self._stream = stream
        self._log_descriptor = log_descriptor
        self._remaining_bytes = remaining_bytes
        self._failure: str | None = None
        self._failure_lock = threading.Lock()
        self._settle_requested = threading.Event()
        self._thread = threading.Thread(
            target=self._drain,
            name="decodex-postgres-output",
            daemon=True,
        )

    @property
    def failure(self) -> str | None:
        with self._failure_lock:
            return self._failure

    def start(self) -> None:
        self._thread.start()

    def settle(self) -> None:
        self._settle_requested.set()
        self._thread.join(timeout=LOCAL_SERVICE_CONTROL_TIMEOUT_SECONDS)
        if self._thread.is_alive():
            raise InstallError("temporary PostgreSQL output did not settle")
        failure = self.failure
        if failure is not None:
            raise InstallError(failure)

    def _record_failure(self, message: str) -> None:
        terminate = False
        with self._failure_lock:
            if self._failure is None:
                self._failure = message
                terminate = True
        if terminate:
            try:
                self._process.send_signal(signal.SIGTERM)
            except ProcessLookupError:
                pass

    def _write_log(self, content: bytes) -> None:
        remaining = memoryview(content)
        while remaining:
            written = os.write(self._log_descriptor, remaining)
            if written <= 0:
                raise OSError("temporary PostgreSQL log write failed")
            remaining = remaining[written:]

    def _drain(self) -> None:
        selector = selectors.DefaultSelector()
        try:
            descriptor = self._stream.fileno()
            os.set_blocking(descriptor, False)
            selector.register(descriptor, selectors.EVENT_READ)
            while True:
                events = selector.select(0.25)
                for key, _ in events:
                    while True:
                        try:
                            chunk = os.read(key.fd, 64 * 1024)
                        except BlockingIOError:
                            break
                        if not chunk:
                            return
                        accepted = chunk[: self._remaining_bytes]
                        if accepted:
                            self._write_log(accepted)
                            self._remaining_bytes -= len(accepted)
                        if len(accepted) != len(chunk):
                            self._record_failure(
                                "temporary PostgreSQL output exceeded its bound"
                            )
                if self._settle_requested.is_set():
                    return
        except BaseException:
            self._record_failure("temporary PostgreSQL output could not be recorded")
        finally:
            selector.close()
            try:
                os.fsync(self._log_descriptor)
            except OSError:
                self._record_failure(
                    "temporary PostgreSQL output could not be recorded"
                )
            try:
                self._stream.close()
            except OSError:
                self._record_failure(
                    "temporary PostgreSQL output could not be recorded"
                )
            try:
                os.close(self._log_descriptor)
            except OSError:
                self._record_failure(
                    "temporary PostgreSQL output could not be recorded"
                )


def _temporary_postgres_output(
    process: subprocess.Popen[Any],
) -> _TemporaryPostgresOutput | None:
    output = getattr(process, "_decodex_temporary_postgres_output", None)
    return output if isinstance(output, _TemporaryPostgresOutput) else None


def wait_for_postgres(paths: InstallPaths, process: subprocess.Popen[Any]) -> None:
    output = _temporary_postgres_output(process)
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if output is not None and output.failure is not None:
            raise InstallError(output.failure)
        if process.poll() is not None:
            if output is not None:
                output.settle()
            raise InstallError("PostgreSQL exited during local-service startup")
        completed = run(
            [
                str(paths.pg_isready),
                "-h",
                str(paths.socket_directory),
                "-p",
                str(POSTGRES_PORT),
                "-d",
                "postgres",
            ],
            capture=True,
            check=False,
        )
        if completed.returncode == 0:
            if output is not None and output.failure is not None:
                raise InstallError(output.failure)
            return
        time.sleep(0.25)
    raise InstallError("PostgreSQL did not become ready")


def start_temporary_postgres(paths: InstallPaths) -> subprocess.Popen[Any]:
    log_descriptor = open_private_append_file(paths.postgres_log, os.geteuid())
    try:
        log_size = os.fstat(log_descriptor).st_size
        if log_size > MAX_TEMPORARY_POSTGRES_OUTPUT_BYTES:
            raise InstallError("temporary PostgreSQL output log exceeded its bound")
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
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            close_fds=True,
            start_new_session=True,
        )
    except BaseException:
        os.close(log_descriptor)
        raise
    if process.stdout is None:
        os.close(log_descriptor)
        terminate_bounded_process(process)
        raise InstallError("temporary PostgreSQL output pipe is unavailable")
    output = _TemporaryPostgresOutput(
        process,
        process.stdout,
        log_descriptor,
        MAX_TEMPORARY_POSTGRES_OUTPUT_BYTES - log_size,
    )
    setattr(process, "_decodex_temporary_postgres_output", output)
    try:
        output.start()
    except BaseException as error:
        delattr(process, "_decodex_temporary_postgres_output")
        process.stdout.close()
        os.close(log_descriptor)
        terminate_bounded_process(process)
        raise InstallError("temporary PostgreSQL output drain could not start") from error
    try:
        wait_for_postgres(paths, process)
    except BaseException:
        try:
            stop_temporary_postgres(process)
        except BaseException as error:
            raise InstallError("temporary PostgreSQL startup cleanup failed") from error
        raise
    return process


def stop_temporary_postgres(process: subprocess.Popen[Any]) -> None:
    termination_error: BaseException | None = None
    try:
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
            try:
                process.wait(timeout=30)
            except subprocess.TimeoutExpired as error:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired as reap_error:
                    termination_error = InstallError(
                        "temporary PostgreSQL could not be reaped"
                    )
                    termination_error.__cause__ = reap_error
                else:
                    termination_error = InstallError(
                        "PostgreSQL did not stop gracefully"
                    )
                    termination_error.__cause__ = error
        else:
            process.wait(timeout=1)
    except BaseException as error:
        termination_error = error

    output_error: BaseException | None = None
    output = _temporary_postgres_output(process)
    if output is not None:
        try:
            output.settle()
        except BaseException as error:
            output_error = error

    if termination_error is not None and output_error is not None:
        raise InstallError(
            "temporary PostgreSQL cleanup and output validation failed"
        ) from termination_error
    if termination_error is not None:
        raise termination_error
    if output_error is not None:
        raise output_error


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


def run_offline_account_migration(
    paths: InstallPaths,
    namespace_lock: InstallerNamespaceLock,
    *,
    transition_gate_fd: int | None = None,
) -> dict[str, Any]:
    completed = run_installer_child(
        [
            str(paths.decodexd),
            "migrate-accounts",
            "--config",
            str(paths.staging_config),
            "--manifest",
            str(paths.migration_manifest),
            "--credential-directory",
            str(paths.credential_directory),
            "--launch-agent",
            str(paths.launch_agent),
        ],
        namespace_lock,
        cwd=paths.repository,
        capture=True,
        transition_gate_fd=transition_gate_fd,
    )
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise InstallError("offline account migration returned an invalid result") from error
    if (
        not isinstance(result, dict)
        or result.get("schema") != "decodex/account-migration-result/1"
        or result.get("outcome") != "destinations_verified"
        or not isinstance(result.get("manifest_sha256"), str)
        or not HEX_DIGEST_PATTERN.fullmatch(result["manifest_sha256"])
        or not isinstance(result.get("intent_recorded"), bool)
        or result.get("receipt_completed") is not False
    ):
        raise InstallError("offline account migration did not verify")
    return result


def run_account_migration_finalizer(
    paths: InstallPaths,
    namespace_lock: InstallerNamespaceLock,
    *,
    transition_gate_fd: int | None = None,
) -> dict[str, Any]:
    command = [
        str(paths.decodexd),
        "finalize-account-migration",
        "--config",
        str(paths.config),
        "--manifest",
        str(paths.migration_manifest),
        "--launch-agent",
        str(paths.launch_agent),
        "--retired-staging-config",
        str(paths.staging_config),
        "--retired-credential-directory",
        str(paths.credential_directory),
        "--retired-active-source",
        str(paths.mapping),
        "--retired-active-source",
        str(paths.vnext_config_source),
    ]
    for asset in migration_installed_assets(paths):
        command.extend(["--installed-asset", str(asset)])
    completed = run_installer_child(
        command,
        namespace_lock,
        cwd=paths.repository,
        capture=True,
        transition_gate_fd=transition_gate_fd,
    )
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise InstallError("account migration finalizer returned an invalid result") from error
    account_ids = result.get("account_ids") if isinstance(result, dict) else None
    if (
        not isinstance(result, dict)
        or result.get("schema") != "decodex/account-migration-result/1"
        or result.get("outcome") != "verified"
        or not isinstance(result.get("manifest_sha256"), str)
        or not HEX_DIGEST_PATTERN.fullmatch(result["manifest_sha256"])
        or not isinstance(result.get("account_count"), int)
        or not isinstance(account_ids, list)
        or len(account_ids) > MAX_RUNTIME_ACCOUNTS
        or result["account_count"] != len(account_ids)
        or any(
            not isinstance(account_id, str)
            or not UUID_PATTERN.fullmatch(account_id)
            for account_id in account_ids
        )
        or len(set(account_ids)) != len(account_ids)
        or not isinstance(result.get("intent_recorded"), bool)
        or result.get("receipt_completed") is not True
    ):
        raise InstallError("account migration retirement did not verify")
    return result


def run_prepared_account_migration_verifier(
    paths: InstallPaths,
    namespace_lock: InstallerNamespaceLock,
    *,
    transition_gate_fd: int | None = None,
) -> dict[str, Any]:
    completed = run_installer_child(
        [
            str(paths.decodexd),
            "verify-prepared-account-migration",
            "--config",
            str(paths.config),
            "--manifest",
            str(paths.migration_manifest),
            "--launch-agent",
            str(paths.launch_agent),
        ],
        namespace_lock,
        cwd=paths.repository,
        capture=True,
        transition_gate_fd=transition_gate_fd,
    )
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise InstallError(
            "prepared account migration verifier returned an invalid result"
        ) from error
    account_ids = result.get("account_ids") if isinstance(result, dict) else None
    if (
        not isinstance(result, dict)
        or result.get("schema") != "decodex/account-migration-result/1"
        or result.get("outcome") != "destinations_verified"
        or not isinstance(result.get("manifest_sha256"), str)
        or not HEX_DIGEST_PATTERN.fullmatch(result["manifest_sha256"])
        or not isinstance(result.get("account_count"), int)
        or not isinstance(account_ids, list)
        or len(account_ids) > MAX_RUNTIME_ACCOUNTS
        or result["account_count"] != len(account_ids)
        or any(
            not isinstance(account_id, str)
            or not UUID_PATTERN.fullmatch(account_id)
            for account_id in account_ids
        )
        or len(set(account_ids)) != len(account_ids)
        or result.get("intent_recorded") is not False
        or result.get("receipt_completed") is not False
    ):
        raise InstallError("prepared account migration destination did not verify")
    return result


def migration_installed_assets(paths: InstallPaths) -> list[Path]:
    return [
        paths.decodexd,
        paths.decodex_cli,
        paths.codex,
        paths.postgres,
        paths.pg_isready,
    ]


def migration_receipt_phase(
    paths: InstallPaths,
    environment: dict[str, str],
) -> str | None:
    relation = psql_scalar(
        paths,
        POSTGRES_DATABASE,
        "SELECT pg_catalog.to_regclass('decodex.account_migration_receipts')::text",
        environment,
    )
    if relation == "":
        return None
    if relation != "decodex.account_migration_receipts":
        raise InstallError("account migration receipt authority is malformed")
    phase = psql_scalar(
        paths,
        POSTGRES_DATABASE,
        "SELECT phase FROM decodex.account_migration_receipts WHERE singleton",
        environment,
    )
    if phase == "":
        return None
    if phase not in {"prepared", "completed"}:
        raise InstallError("account migration receipt authority is malformed")
    return phase


def run_completed_account_migration_verifier(
    paths: InstallPaths,
    namespace_lock: InstallerNamespaceLock,
    *,
    transition_gate_fd: int | None = None,
) -> dict[str, Any]:
    command = [
        str(paths.decodexd),
        "verify-account-migration",
        "--config",
        str(paths.config),
        "--launch-agent",
        str(paths.launch_agent),
        "--retired-staging-config",
        str(paths.staging_config),
        "--retired-credential-directory",
        str(paths.credential_directory),
        "--retired-active-source",
        str(paths.mapping),
        "--retired-active-source",
        str(paths.vnext_config_source),
    ]
    for asset in migration_installed_assets(paths):
        command.extend(["--installed-asset", str(asset)])
    completed = run_installer_child(
        command,
        namespace_lock,
        cwd=paths.repository,
        capture=True,
        transition_gate_fd=transition_gate_fd,
    )
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise InstallError("account migration verifier returned an invalid result") from error
    account_ids = result.get("account_ids") if isinstance(result, dict) else None
    if (
        not isinstance(result, dict)
        or result.get("schema") != "decodex/account-migration-result/1"
        or result.get("outcome") != "verified"
        or not isinstance(result.get("manifest_sha256"), str)
        or not HEX_DIGEST_PATTERN.fullmatch(result["manifest_sha256"])
        or not isinstance(result.get("account_count"), int)
        or not isinstance(account_ids, list)
        or len(account_ids) > MAX_RUNTIME_ACCOUNTS
        or result["account_count"] != len(account_ids)
        or any(
            not isinstance(account_id, str)
            or not UUID_PATTERN.fullmatch(account_id)
            for account_id in account_ids
        )
        or len(set(account_ids)) != len(account_ids)
        or result.get("intent_recorded") is not False
        or result.get("receipt_completed") is not True
    ):
        raise InstallError("completed account migration did not verify")
    return result


def retire_active_migration_sources(paths: InstallPaths) -> None:
    for path in [paths.mapping, paths.vnext_config_source]:
        try:
            path.unlink(missing_ok=True)
        except OSError as error:
            raise InstallError("active legacy account bridge could not be retired") from error


def parse_launch_agent_pid(output: str) -> int | None:
    match = re.search(r"^\s*pid = ([1-9][0-9]*)\s*$", output, re.MULTILINE)
    return int(match.group(1)) if match is not None else None


def settlement_command_timeout(
    deadline: float,
    maximum: float = LOCAL_SERVICE_CONTROL_TIMEOUT_SECONDS,
) -> float:
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise InstallError("existing local service did not settle")
    return min(maximum, remaining)


def run_settlement_command(
    command: list[str],
    deadline: float,
    failure: str,
    maximum_timeout: float = LOCAL_SERVICE_CONTROL_TIMEOUT_SECONDS,
) -> subprocess.CompletedProcess[str]:
    try:
        return run(
            command,
            capture=True,
            check=False,
            timeout=settlement_command_timeout(deadline, maximum_timeout),
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise InstallError(failure) from error


def process_parent_map(deadline: float) -> dict[int, ProcessRecord]:
    completed = run_settlement_command(
        ["/bin/ps", "-axo", "pid=,ppid=,lstart="],
        deadline,
        "local service process inventory is unavailable",
    )
    if completed.returncode != 0:
        raise InstallError("local service process inventory is unavailable")
    processes: dict[int, ProcessRecord] = {}
    for line in completed.stdout.splitlines():
        fields = line.split()
        if not fields:
            continue
        if (
            len(fields) < 3
            or not all(field.isascii() and field.isdecimal() for field in fields[:2])
        ):
            raise InstallError("local service process inventory is malformed")
        process_id, parent_id = (int(field) for field in fields[:2])
        started_at = " ".join(fields[2:])
        if (
            process_id <= 0
            or parent_id < 0
            or process_id in processes
            or len(started_at) > 128
            or contains_control(started_at)
        ):
            raise InstallError("local service process inventory is malformed")
        processes[process_id] = ProcessRecord(
            parent_id=parent_id,
            identity=ProcessIdentity(process_id=process_id, started_at=started_at),
        )
    return processes


def process_generation(
    root_process_id: int, processes: dict[int, ProcessRecord]
) -> frozenset[ProcessIdentity]:
    if root_process_id not in processes:
        return frozenset()
    generation_process_ids = {root_process_id}
    changed = True
    while changed:
        changed = False
        for process_id, record in processes.items():
            if (
                record.parent_id in generation_process_ids
                and process_id not in generation_process_ids
            ):
                generation_process_ids.add(process_id)
                changed = True
    return frozenset(
        processes[process_id].identity for process_id in generation_process_ids
    )


def wait_for_process_generation_exit(
    process_identities: set[ProcessIdentity], deadline: float
) -> None:
    if not process_identities:
        return
    while True:
        current = process_parent_map(deadline)
        live = {
            identity
            for identity in process_identities
            if current.get(identity.process_id) is not None
            and current[identity.process_id].identity == identity
        }
        if not live:
            return
        if time.monotonic() >= deadline:
            raise InstallError("existing local service did not settle")
        time.sleep(
            min(
                LOCAL_SERVICE_SETTLEMENT_POLL_SECONDS,
                max(0, deadline - time.monotonic()),
            )
        )


def installed_launch_agent_supports_graceful_drain(path: Path, uid: int) -> bool:
    try:
        body = read_owned_file(
            path,
            uid,
            MAX_LAUNCH_AGENT_FILE_BYTES,
            "installed LaunchAgent is unavailable",
        )
        document = plistlib.loads(body)
    except (InstallError, ValueError, plistlib.InvalidFileException):
        return False
    if not isinstance(document, dict) or document.get("Label") != LAUNCH_AGENT_LABEL:
        return False
    keep_alive = document.get("KeepAlive")
    return (
        isinstance(keep_alive, dict)
        and set(keep_alive) == {"SuccessfulExit"}
        and keep_alive["SuccessfulExit"] is False
        and type(document.get("ExitTimeOut")) is int
        and document["ExitTimeOut"] == 60
    )


def observe_service(service: str, deadline: float) -> ServiceObservation:
    completed = run_settlement_command(
        ["/bin/launchctl", "print", service],
        deadline,
        "local service state is unavailable",
    )
    if completed.returncode == LAUNCHCTL_PRINT_NOT_FOUND_STATUS:
        return ServiceObservation(False, None, None, frozenset())
    if completed.returncode != 0:
        raise InstallError("local service state is unavailable")
    root_process_id = parse_launch_agent_pid(completed.stdout)
    if root_process_id is None:
        return ServiceObservation(True, None, None, frozenset())
    generation = process_generation(root_process_id, process_parent_map(deadline))
    root = next(
        (
            identity
            for identity in generation
            if identity.process_id == root_process_id
        ),
        None,
    )
    return ServiceObservation(True, root_process_id, root, generation)


def drain_service(
    service: str,
    observation: ServiceObservation,
    captured: set[ProcessIdentity],
    deadline: float,
) -> ServiceObservation:
    signaled: set[ProcessIdentity] = set()
    current = observation
    while current.loaded and current.active_process_id is not None:
        captured.update(current.generation)
        if current.root is not None and current.root not in signaled:
            completed = run_settlement_command(
                ["/bin/launchctl", "kill", "SIGTERM", service],
                deadline,
                "existing local service could not be signaled",
            )
            if completed.returncode == 0:
                signaled.add(current.root)
            else:
                after_failure = observe_service(service, deadline)
                captured.update(after_failure.generation)
                if after_failure.root == current.root:
                    raise InstallError("existing local service could not be signaled")
                current = after_failure
                continue
        time.sleep(
            min(
                LOCAL_SERVICE_SETTLEMENT_POLL_SECONDS,
                max(0, deadline - time.monotonic()),
            )
        )
        current = observe_service(service, deadline)
    captured.update(current.generation)
    return current


def bootout_service(paths: InstallPaths, uid: int) -> None:
    deadline = time.monotonic() + LOCAL_SERVICE_SETTLEMENT_TIMEOUT_SECONDS
    service = f"gui/{uid}/{LAUNCH_AGENT_LABEL}"
    observed = observe_service(service, deadline)
    generation = set(observed.generation)
    if installed_launch_agent_supports_graceful_drain(paths.launch_agent, uid):
        observed = drain_service(service, observed, generation, deadline)

    try:
        completed = run_settlement_command(
            ["/bin/launchctl", "bootout", service],
            deadline,
            "existing local service could not be stopped",
            LOCAL_SERVICE_SETTLEMENT_TIMEOUT_SECONDS,
        )
    except InstallError:
        wait_for_process_generation_exit(generation, deadline)
        raise
    if completed.returncode != 0:
        loaded = observe_service(service, deadline)
        generation.update(loaded.generation)
        if loaded.loaded:
            raise InstallError("existing local service could not be stopped")
    wait_for_process_generation_exit(generation, deadline)


def bootstrap_service(paths: InstallPaths, uid: int) -> None:
    service = f"gui/{uid}/{LAUNCH_AGENT_LABEL}"
    commands = (
        ["/bin/launchctl", "bootstrap", f"gui/{uid}", str(paths.launch_agent)],
        ["/bin/launchctl", "kickstart", service],
    )
    for command in commands:
        try:
            run(command, timeout=LOCAL_SERVICE_CONTROL_TIMEOUT_SECONDS)
        except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
            raise InstallError("local service could not be started") from error


def query_accounts(paths: InstallPaths) -> dict[str, Any] | None:
    try:
        completed = run(
            [
                str(paths.decodex_cli),
                "--root",
                str(paths.root),
                "--output",
                "json",
                "account",
                "list",
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
        or document.get("schema") != "decodex/cli-account/1"
        or document.get("command") != "list"
        or document.get("outcome") != "success"
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
    result = document.get("result")
    accounts = result.get("accounts") if isinstance(result, dict) else None
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


def wait_for_service(paths: InstallPaths, expected_account_ids: set[str]) -> None:
    deadline = time.monotonic() + 180
    last_issue = "account service did not answer"
    while time.monotonic() < deadline:
        if not query_doctor(paths):
            last_issue = "local-service authority is unavailable"
            time.sleep(1)
            continue
        accounts_document = query_accounts(paths)
        account_ids = (
            account_ids_from_result(accounts_document)
            if accounts_document is not None
            else None
        )
        if account_ids is None:
            last_issue = "account registry is unavailable"
        elif set(account_ids) != expected_account_ids:
            last_issue = "account registry set does not agree"
        else:
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
    return uid


def resume_prepared_account_migration(
    paths: InstallPaths,
    uid: int,
    namespace_lock: InstallerNamespaceLock,
    *,
    checkpoint: Callable[[str], None],
    transition_gate_fd: int | None,
) -> dict[str, Any]:
    if not managed_path_exists(
        paths.migration_manifest,
        "prepared account migration manifest is unavailable",
    ):
        raise InstallError("prepared account migration manifest is unavailable")
    manifest_accounts = load_existing_manifest_accounts(
        paths.migration_manifest,
        uid,
    )
    staging_owner = MigrationStagingOwner.for_accounts(
        paths,
        [str(account["account_id"]) for account in manifest_accounts.values()],
    )
    primary_error: BaseException | None = None
    completed_result: dict[str, Any] | None = None
    staging_retired = False
    try:
        run_prepared_account_migration_verifier(
            paths,
            namespace_lock,
            transition_gate_fd=transition_gate_fd,
        )
        checkpoint("destination_reverified")
        staging_owner.cleanup()
        staging_retired = True
        checkpoint("staging_retired")
        retire_active_migration_sources(paths)
        checkpoint("active_legacy_retired")
        checkpoint("before_finalizer")
        completed_result = run_account_migration_finalizer(
            paths,
            namespace_lock,
            transition_gate_fd=transition_gate_fd,
        )
        checkpoint("final_receipt_recorded")
    except BaseException as error:
        primary_error = error

    if staging_retired:
        if primary_error is not None:
            raise primary_error
    else:
        finish_account_migration_staging(staging_owner, primary_error)
    if completed_result is None:
        raise InstallError("prepared account migration finalization is unavailable")
    return completed_result


def install_under_namespace_lock(
    paths: InstallPaths,
    uid: int,
    namespace_lock: InstallerNamespaceLock,
    *,
    launch_requested: bool,
    transition_checkpoint: Callable[[str], None] | None = None,
    transition_gate_fd: int | None = None,
) -> tuple[dict[str, Any], list[str], bool]:
    def checkpoint(name: str) -> None:
        if transition_checkpoint is not None:
            transition_checkpoint(name)

    current_config = read_optional_owned_source(
        paths.config,
        uid,
        MAX_CONFIG_FILE_BYTES,
        "existing Decodex config is malformed",
    )
    completed_result: dict[str, Any] | None = None
    if (
        current_config is not None
        and b"server_host.reset_card_accounts" not in current_config
    ):
        initialize_cluster(paths, uid)
        postgres = start_temporary_postgres(paths)
        try:
            environment = psql_environment(paths)
            ensure_roles_and_database(paths, environment)
            receipt_phase = migration_receipt_phase(paths, environment)
            if receipt_phase == "completed":
                completed_result = run_completed_account_migration_verifier(
                    paths,
                    namespace_lock,
                    transition_gate_fd=transition_gate_fd,
                )
                checkpoint("completed_receipt_verified")
            elif receipt_phase == "prepared":
                completed_result = resume_prepared_account_migration(
                    paths,
                    uid,
                    namespace_lock,
                    checkpoint=checkpoint,
                    transition_gate_fd=transition_gate_fd,
                )
            else:
                raise InstallError(
                    "active vNext config has no account migration receipt"
                )
        finally:
            stop_temporary_postgres(postgres)

    if completed_result is not None:
        migration_result = completed_result
        enrollments = completed_result["account_ids"]
    else:
        accounts, account_pool_body, legacy_lock = lock_and_read_legacy_accounts(
            paths.legacy_accounts,
            uid,
        )
        staging_owner: MigrationStagingOwner | None = None
        staging_cleanup_attempted = False
        staging_retired = False
        primary_error: BaseException | None = None
        try:
            vnext_config_body = prepare_vnext_config_source(paths, uid)
            if managed_path_exists(paths.legacy_config, "legacy account config is malformed"):
                require_private_legacy_source_chain(paths.legacy_config, uid)
            legacy_config_body = read_optional_owned_source(
                paths.legacy_config,
                uid,
                MAX_CONFIG_FILE_BYTES,
                "legacy account config is malformed",
            )
            mapping_body = read_optional_owned_source(
                paths.mapping,
                uid,
                MAX_MAPPING_FILE_BYTES,
                "existing reset-card mapping is malformed",
            )
            existing_mapping = load_existing_mapping(paths.mapping, uid)
            existing = (
                existing_enrollments(paths.vnext_config_source, uid)
                if vnext_config_body is not None
                else {}
            )
            if bool(existing_mapping) != bool(existing):
                raise InstallError("vNext UUID bridge and account source do not agree")
            fixed_selector, offsets = parse_legacy_account_config(legacy_config_body)
            prior_manifest = load_existing_manifest_accounts(paths.migration_manifest, uid)
            planned, fixed_account_id, order = build_enrollments(
                accounts,
                existing_mapping,
                existing,
                prior_manifest,
                fixed_selector,
                offsets,
            )
            staging_owner = MigrationStagingOwner.for_accounts(
                paths,
                [enrollment.account_id for enrollment in planned],
            )
            config = render_config(paths, uid).encode("utf-8")
            launch_agent = render_launch_agent(paths)
            credential_digests = credential_sources(paths, accounts, planned, uid)
            sources = [
                source_record("legacy_account_pool", paths.legacy_accounts, account_pool_body),
                source_record("legacy_account_config", paths.legacy_config, legacy_config_body),
                source_record("vnext_uuid_bridge", paths.mapping, mapping_body),
                source_record(
                    "vnext_account_config",
                    paths.vnext_config_source,
                    vnext_config_body,
                ),
            ]
            manifest = render_migration_manifest(
                paths,
                uid,
                sources,
                planned,
                credential_digests,
                fixed_account_id,
                order,
            )
            atomic_write(paths.staging_config, config, 0o600)
            atomic_write(paths.migration_manifest, manifest, 0o600)
            atomic_write(paths.launch_agent, launch_agent, 0o600)

            initialize_cluster(paths, uid)
            postgres = start_temporary_postgres(paths)
            try:
                postgres_scope_error: BaseException | None = None
                try:
                    environment = psql_environment(paths)
                    ensure_roles_and_database(paths, environment)
                    prepared = run_offline_account_migration(
                        paths,
                        namespace_lock,
                        transition_gate_fd=transition_gate_fd,
                    )
                    checkpoint("destination_verified")
                    canonical_manifest = read_owned_file(
                        paths.migration_manifest,
                        uid,
                        MAX_MIGRATION_MANIFEST_BYTES,
                        "account migration manifest is malformed",
                    )
                    expected_digest = decision_digest(json.loads(canonical_manifest))
                    if prepared["manifest_sha256"] != expected_digest:
                        raise InstallError(
                            "offline account migration intent identity differs"
                        )
                    checkpoint("before_config_swap")
                    atomic_write(paths.config, config, 0o600)
                    checkpoint("config_swapped")
                    staging_owner.cleanup()
                    staging_retired = True
                    checkpoint("staging_retired")
                    retire_active_migration_sources(paths)
                    checkpoint("active_legacy_retired")
                    checkpoint("before_finalizer")
                    migration_result = run_account_migration_finalizer(
                        paths,
                        namespace_lock,
                        transition_gate_fd=transition_gate_fd,
                    )
                    checkpoint("final_receipt_recorded")
                except BaseException as error:
                    postgres_scope_error = error
                staging_cleanup_attempted = True
                if staging_retired:
                    if postgres_scope_error is not None:
                        raise postgres_scope_error
                else:
                    finish_account_migration_staging(
                        staging_owner,
                        postgres_scope_error,
                    )
            finally:
                stop_temporary_postgres(postgres)
            if migration_result["manifest_sha256"] != expected_digest:
                raise InstallError("account migration receipt identity differs")
            enrollments = [enrollment.account_id for enrollment in planned]
        except BaseException as error:
            primary_error = error

        cleanup_error: BaseException | None = None
        if staging_owner is not None and not staging_cleanup_attempted:
            try:
                staging_owner.cleanup()
            except BaseException as error:
                cleanup_error = error
        if legacy_lock is not None:
            try:
                fcntl.flock(legacy_lock, fcntl.LOCK_UN)
            finally:
                os.close(legacy_lock)
        if primary_error is not None:
            if cleanup_error is not None:
                raise primary_error from cleanup_error
            raise primary_error
        if cleanup_error is not None:
            raise cleanup_error

    launch = launch_requested
    checkpoint("launch_decided")
    return migration_result, enrollments, launch


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    paths = install_paths(args)
    uid = validate_host(paths)
    ensure_installer_namespace_layout(paths, uid)
    if postgres_major(paths.postgres) != 18:
        raise InstallError("PostgreSQL 18 is required")
    bootout_service(paths, uid)
    namespace_lock = InstallerNamespaceLock.acquire(paths, uid)
    try:
        ensure_directories(paths, uid)
        migration_result, enrollments, launch = install_under_namespace_lock(
            paths,
            uid,
            namespace_lock,
            launch_requested=not args.no_launch,
        )
    finally:
        namespace_lock.close()

    if launch:
        bootstrap_service(paths, uid)
        wait_for_service(
            paths,
            set(enrollments),
        )

    print(
        json.dumps(
            {
                "schema": "decodex/local-service-install/1",
                "outcome": "success",
                "account_count": len(enrollments),
                "migration_manifest_sha256": migration_result["manifest_sha256"],
                "postgres_major": 18,
                "launch_agent": LAUNCH_AGENT_LABEL,
                "launched": launch,
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
