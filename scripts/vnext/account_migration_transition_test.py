#!/usr/bin/env python3
"""Run the canonical XY-1422 migration-transition boundary gate."""

from __future__ import annotations

import base64
import fcntl
import hashlib
import importlib.util
import json
import os
import plistlib
import pwd
import re
import secrets
import select
import selectors
import shutil
import signal
import socket
import stat
import subprocess
import sys
import time
import uuid
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Any, Callable

sys.dont_write_bytecode = True

REPO_ROOT = Path(__file__).resolve().parents[2]
INSTALLER_PATH = REPO_ROOT / "scripts/macos/install_decodex_local_service.py"
DAEMON_WRAPPER_PATH = REPO_ROOT / "scripts/macos/decodexd_wrapper.py"
KEYCHAIN_SERVICE = "box.acg.decodex.credentials.v1"
KEYCHAIN_ACCESS_GROUP = "T54QFA7W2S.box.acg.decodex.daemon"
ACCOUNT_MIGRATION_GATE_RUN_SCHEMA = "decodex/account-migration-gate-run/1"
ACCOUNT_MIGRATION_GATE_RUN_FILE = "account-migration-gate-run.json"
GATE_TIMEOUT_SECONDS = 180.0
SUBPROCESS_TIMEOUT_SECONDS = 60.0
MAX_SUBPROCESS_OUTPUT_BYTES = 256 * 1024
MAX_PROVISIONING_PROFILE_CANDIDATES = 128
MACOS_UNIX_PATH_BYTES = 104
PROTECTED_STORE_PHASES = (
    "first_add",
    "first_metadata_readback",
    "duplicate_add",
    "no_overwrite_readback",
    "exact_delete",
    "final_absence",
)
PROTECTED_STORE_SUCCESS_CATEGORIES = (
    "created",
    "exact",
    "already_exists",
    "exact_unchanged",
    "deleted",
    "absent",
)
PROTECTED_STORE_FAILURE_CATEGORIES = frozenset(
    {
        "unavailable",
        "not_found",
        "already_exists",
        "version_conflict",
        "fingerprint_mismatch",
        "provider_mismatch",
        "account_mismatch",
        "writer_mismatch",
        "unsupported_schema",
        "invalid_bundle",
        "corrupt_bundle",
        "not_absent",
        "unexpected_success",
        "mismatch",
        "present",
        "blocked",
        "not_owned",
    }
)
HOST_CREDENTIAL_METADATA_FIELDS = frozenset(
    {
        "schema",
        "present",
        "service",
        "account",
        "store_schema_version",
        "provider",
        "provider_account_id",
        "credential_version",
        "writer_operation_id",
        "fingerprint_sha256",
        "label",
        "access_group",
        "description",
        "accessibility",
        "synchronizing",
        "access_control_present",
        "protected_keychain",
    }
)


class GateFailure(RuntimeError):
    """One bounded gate assertion failed."""


@dataclass(frozen=True)
class AccountCase:
    account_id: str
    operation_id: str
    provider_account_id: str
    email: str
    display_label: str
    enabled: bool
    v26_revision: int | None
    v26_label: str | None
    v26_state: str | None


@dataclass(frozen=True)
class CredentialGateCase:
    account_id: str
    operation_id: str
    provider_account_id: str
    email: str


@dataclass(frozen=True)
class Toolchain:
    postgres: Path
    initdb: Path
    pg_isready: Path
    psql: Path


@dataclass(frozen=True)
class BuildArtifacts:
    decodexd: Path
    migration_fixture: Path


@dataclass(frozen=True)
class LoginIdentity:
    uid: int
    name: str
    home: Path


@dataclass
class StageResult:
    state: str
    kind: str
    dependency: str | None = None
    detail: str | None = None
    evidence: dict[str, Any] | None = None

    def document(self) -> dict[str, Any]:
        document: dict[str, Any] = {"state": self.state, "kind": self.kind}
        if self.dependency is not None:
            document["dependency"] = self.dependency
        if self.detail is not None:
            document["detail"] = self.detail[:512]
        if self.evidence:
            document["evidence"] = self.evidence
        return document


class StageGraph:
    def __init__(self) -> None:
        self.results: dict[str, StageResult] = {}

    def run(
        self,
        name: str,
        dependencies: tuple[str, ...],
        action: Callable[[], dict[str, Any] | None],
    ) -> bool:
        for dependency in dependencies:
            result = self.results.get(dependency)
            if result is None or result.state != "passed":
                self.results[name] = StageResult(
                    state="blocked",
                    kind="dependency",
                    dependency=dependency,
                    detail=(
                        "required stage did not pass"
                        if result is None
                        else f"required stage is {result.state}"
                    ),
                )
                return False
        try:
            evidence = action()
        except BaseException as error:
            self.results[name] = StageResult(
                state="failed",
                kind=type(error).__name__,
                detail=str(error) or type(error).__name__,
            )
            return False
        self.results[name] = StageResult(
            state="passed",
            kind="verified",
            evidence=evidence,
        )
        return True

    def block(self, name: str, dependency: str, detail: str) -> None:
        self.results[name] = StageResult(
            state="blocked",
            kind="dependency",
            dependency=dependency,
            detail=detail,
        )

    def record_capture(
        self,
        name: str,
        captured: StageResult | None,
        dependency: str,
    ) -> None:
        parent = self.results.get(dependency)
        if parent is None or parent.state != "passed":
            self.block(name, dependency, "checkpoint owner did not pass")
        elif captured is None:
            self.block(name, dependency, "checkpoint branch did not run")
        else:
            self.results[name] = captured


CASES: tuple[AccountCase, ...] = ()
CONFLICT_CASE: CredentialGateCase | None = None


def terminate_bounded_subprocess(process: subprocess.Popen[Any]) -> None:
    process_group_id = process.pid
    try:
        os.killpg(process_group_id, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=0.25)
    except subprocess.TimeoutExpired:
        pass
    try:
        os.killpg(process_group_id, signal.SIGKILL)
    except ProcessLookupError:
        pass
    if process.returncode is None:
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired as error:
            raise GateFailure("bounded subprocess could not be reaped") from error


def communicate_bounded_subprocess(
    process: subprocess.Popen[Any],
    command: list[str],
    timeout: float,
) -> tuple[bytes, bytes]:
    if timeout <= 0 or process.stdout is None or process.stderr is None:
        primary_error = GateFailure("bounded subprocess configuration is invalid")
        try:
            terminate_bounded_subprocess(process)
        except BaseException as cleanup_error:
            raise primary_error from cleanup_error
        raise primary_error
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
                raise GateFailure("bounded subprocess timed out")
            for key, _ in selector.select(min(0.25, remaining)):
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
                if output_bytes > MAX_SUBPROCESS_OUTPUT_BYTES:
                    raise GateFailure("bounded subprocess exceeded its output limit")
                chunks[name].append(chunk)
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise GateFailure("bounded subprocess timed out")
        process.wait(timeout=remaining)
    except subprocess.TimeoutExpired as error:
        primary_error = GateFailure("bounded subprocess timed out")
        primary_error.__cause__ = error
        try:
            terminate_bounded_subprocess(process)
        except BaseException as cleanup_error:
            raise primary_error from cleanup_error
        raise primary_error
    except BaseException as primary_error:
        try:
            terminate_bounded_subprocess(process)
        except BaseException as cleanup_error:
            raise primary_error from cleanup_error
        raise
    finally:
        selector.close()
        for _, stream in streams.values():
            if not stream.closed:
                stream.close()
    return b"".join(chunks["stdout"]), b"".join(chunks["stderr"])


def capture_stage(action: Callable[[], dict[str, Any] | None]) -> StageResult:
    try:
        evidence = action()
    except BaseException as error:
        return StageResult(
            state="failed",
            kind=type(error).__name__,
            detail=str(error) or type(error).__name__,
        )
    return StageResult(state="passed", kind="verified", evidence=evidence)


def gate_uuid(run_token: str, slot: str, purpose: str) -> str:
    digest = hashlib.sha256(
        b"decodex-account-migration-gate-v1\0"
        + run_token.encode()
        + b"\0"
        + slot.encode()
        + b"\0"
        + purpose.encode()
    ).digest()
    value = bytearray(digest[:16])
    value[6] = (value[6] & 0x0F) | 0x40
    value[8] = (value[8] & 0x3F) | 0x80
    return str(uuid.UUID(bytes=bytes(value)))


def build_gate_identities(run_token: str) -> tuple[
    tuple[AccountCase, ...],
    CredentialGateCase,
]:
    labels = (
        ("Absent Normal", True, None, None, None),
        ("Existing Normal", True, 7, "V26 Existing Normal", "available"),
        ("Absent Disabled", False, None, None, None),
        ("Existing Disabled", False, 11, "Existing Disabled", "disabled"),
    )
    cases = tuple(
        AccountCase(
            account_id=gate_uuid(run_token, f"account_{ordinal}", "account"),
            operation_id=gate_uuid(run_token, f"account_{ordinal}", "operation"),
            provider_account_id=f"xy1422-{run_token}-{ordinal}",
            email=f"xy1422-{run_token}-{ordinal}@invalid.example",
            display_label=label,
            enabled=enabled,
            v26_revision=revision,
            v26_label=v26_label,
            v26_state=v26_state,
        )
        for ordinal, (label, enabled, revision, v26_label, v26_state) in enumerate(
            labels,
            start=1,
        )
    )
    conflict = CredentialGateCase(
        account_id=gate_uuid(run_token, "conflict", "account"),
        operation_id=gate_uuid(run_token, "conflict", "operation"),
        provider_account_id=f"xy1422-{run_token}-conflict",
        email=f"xy1422-{run_token}-conflict@invalid.example",
    )
    return cases, conflict


def bounded_run(
    command: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    pass_fds: tuple[int, ...] = (),
    timeout: float = SUBPROCESS_TIMEOUT_SECONDS,
    check: bool = False,
) -> subprocess.CompletedProcess[bytes]:
    process = subprocess.Popen(
        command,
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        close_fds=True,
        pass_fds=pass_fds,
        start_new_session=True,
    )
    stdout, stderr = communicate_bounded_subprocess(process, command, timeout)
    completed = subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
    if check and process.returncode != 0:
        raise subprocess.CalledProcessError(
            process.returncode,
            command,
            output=stdout,
            stderr=stderr,
        )
    return completed


def load_installer() -> Any:
    spec = importlib.util.spec_from_file_location(
        "xy1422_install_decodex_local_service",
        INSTALLER_PATH,
    )
    if spec is None or spec.loader is None:
        raise GateFailure("installer module is unavailable")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def load_daemon_wrapper() -> Any:
    spec = importlib.util.spec_from_file_location(
        "xy1422_decodexd_wrapper",
        DAEMON_WRAPPER_PATH,
    )
    if spec is None or spec.loader is None:
        raise GateFailure("daemon wrapper module is unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def jwt(payload: dict[str, Any]) -> str:
    encoded = base64.urlsafe_b64encode(
        json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
    ).decode().rstrip("=")
    return f"header.{encoded}.signature"


def private_write(path: Path, body: bytes, mode: int = 0o600) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(path.parent, 0o700)
    path.write_bytes(body)
    os.chmod(path, mode)


def executable_stub(path: Path, marker: Path) -> None:
    private_write(
        path,
        (
            "#!/bin/sh\n"
            f"printf '%s\\n' invoked >> {json.dumps(str(marker))}\n"
            "exit 97\n"
        ).encode(),
        0o700,
    )


def require_exact_executable(path: Path, name: str) -> None:
    try:
        metadata = path.lstat()
    except OSError as error:
        raise GateFailure(f"required executable is unavailable: {name}") from error
    if (
        stat.S_ISLNK(metadata.st_mode)
        or not stat.S_ISREG(metadata.st_mode)
        or not os.access(path, os.X_OK)
    ):
        raise GateFailure(f"required executable is unsafe: {name}")


def discover_tool(name: str) -> Path:
    location = shutil.which(name)
    if location is None:
        raise GateFailure(f"PostgreSQL 18 tool is unavailable: {name}")
    path = Path(location).resolve(strict=True)
    require_exact_executable(path, name)
    return path


def target_binary(name: str) -> Path:
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO_ROOT / "target"))
    if not target.is_absolute():
        target = REPO_ROOT / target
    try:
        binary = (target / "debug" / name).resolve(strict=True)
    except OSError as error:
        raise GateFailure(f"canonical gate binary is unavailable: {name}") from error
    try:
        require_exact_executable(binary, name)
    except GateFailure as error:
        raise GateFailure(f"canonical gate binary is unavailable: {name}")
    return binary


def preflight_identity(context: dict[str, Any]) -> dict[str, Any]:
    if sys.platform != "darwin":
        raise GateFailure("canonical migration-transition gate is macOS-only")
    uid = os.geteuid()
    if uid == 0:
        raise GateFailure("canonical migration-transition gate refuses root")
    try:
        record = pwd.getpwuid(uid)
        name = record.pw_name
        home = Path(record.pw_dir)
    except (AttributeError, KeyError, TypeError) as error:
        raise GateFailure("real login identity is unavailable") from error
    if (
        not isinstance(name, str)
        or not name
        or any(character.isspace() for character in name)
        or not home.is_absolute()
        or ".." in home.parts
    ):
        raise GateFailure("real login identity is malformed")
    try:
        home_metadata = home.lstat()
    except OSError as error:
        raise GateFailure("real login home is unavailable") from error
    if (
        stat.S_ISLNK(home_metadata.st_mode)
        or not stat.S_ISDIR(home_metadata.st_mode)
        or home_metadata.st_uid not in (0, uid)
        or stat.S_IMODE(home_metadata.st_mode) & 0o022 != 0
    ):
        raise GateFailure("real login home does not satisfy source ancestry")
    for ancestor in home.parents:
        metadata = ancestor.lstat()
        if (
            stat.S_ISLNK(metadata.st_mode)
            or not stat.S_ISDIR(metadata.st_mode)
            or metadata.st_uid not in (0, uid)
            or stat.S_IMODE(metadata.st_mode) & 0o022 != 0
        ):
            raise GateFailure("real login ancestor does not satisfy source ancestry")
    context["identity"] = LoginIdentity(uid=uid, name=name, home=home)
    return {
        "effective_uid": uid,
        "login_name_present": True,
        "real_login_home": str(home),
        "ambient_home_ignored": True,
    }


def preflight_paths(context: dict[str, Any], run_token: str) -> dict[str, Any]:
    identity: LoginIdentity = context["identity"]
    fixture_root = identity.home / f".xy1422-{run_token}"
    runtime_root = fixture_root / ".decodex"
    port = 20_000 + secrets.randbelow(40_000)
    live_default = identity.home / ".codex" / "decodex"
    source_paths = (
        fixture_root / "legacy" / "accounts.jsonl",
        fixture_root / "legacy" / "config.toml",
        runtime_root / "reset-card-legacy-map.json",
        runtime_root / "account-migration-vnext-source.toml",
    )
    if fixture_root.exists() or not fixture_root.is_absolute() or ".." in fixture_root.parts:
        raise GateFailure("prospective fixture root is unavailable")
    try:
        fixture_root.relative_to(identity.home)
    except ValueError as error:
        raise GateFailure("prospective fixture root is outside the real login home") from error
    for source in source_paths:
        try:
            source.relative_to(fixture_root)
        except ValueError as error:
            raise GateFailure("a prospective source escapes the fixture root") from error
        if source == live_default or live_default in source.parents:
            raise GateFailure("a prospective source reaches the live default")
    socket_paths = (
        runtime_root / "postgres" / "socket" / f".s.PGSQL.{port}",
        runtime_root / "server" / "decodex.sock",
        runtime_root / "server" / "decodex.sock.stage",
    )
    for path in socket_paths:
        if len(os.fsencode(path)) + 1 > MACOS_UNIX_PATH_BYTES:
            raise GateFailure("a prospective Unix socket path is overlong")
    context["fixture_root"] = fixture_root
    context["postgres_port"] = port
    context["live_default"] = live_default
    return {
        "fixture_root": str(fixture_root),
        "fixture_root_mode": "0700",
        "postgres_endpoint": str(socket_paths[0]),
        "live_default_excluded": True,
    }


def preflight_system_executables(context: dict[str, Any]) -> dict[str, Any]:
    executables = (Path("/bin/ps"), Path("/bin/sh"), Path("/usr/bin/python3"))
    for executable in executables:
        require_exact_executable(executable, str(executable))
    context["system_executables"] = executables
    return {"executables": [str(path) for path in executables]}


def preflight_postgres_toolchain(context: dict[str, Any]) -> dict[str, Any]:
    tools = Toolchain(
        postgres=discover_tool("postgres"),
        initdb=discover_tool("initdb"),
        pg_isready=discover_tool("pg_isready"),
        psql=discover_tool("psql"),
    )
    tool_paths = (tools.postgres, tools.initdb, tools.pg_isready, tools.psql)
    if len({path.parent for path in tool_paths}) != 1:
        raise GateFailure("PostgreSQL tools do not form one installation")
    versions = {}
    for path in tool_paths:
        completed = bounded_run([str(path), "--version"], timeout=15, check=True)
        output = (completed.stdout + completed.stderr).decode("utf-8", errors="strict").strip()
        if not re.search(r"(?:PostgreSQL\)?\s+)18(?:\.[0-9]+)*\b", output):
            raise GateFailure(f"PostgreSQL 18 version mismatch: {path.name}")
        versions[path.name] = output
    share = tools.initdb.parent.parent / "share" / "postgresql"
    metadata = share.lstat()
    if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(metadata.st_mode):
        raise GateFailure("PostgreSQL 18 share directory is unsafe")
    context["toolchain"] = tools
    return {
        "bin_directory": str(tools.postgres.parent),
        "versions": versions,
        "share_directory": str(share),
    }


def preflight_decodexd_artifact(context: dict[str, Any]) -> dict[str, Any]:
    decodexd = target_binary("decodexd")
    context["decodexd_artifact"] = decodexd
    return {"decodexd": str(decodexd)}


def preflight_migration_fixture_artifact(
    context: dict[str, Any],
) -> dict[str, Any]:
    migration_fixture = target_binary(
        "decodex-account-migration-transition-fixture"
    )
    context["migration_fixture_artifact"] = migration_fixture
    return {"migration_fixture": str(migration_fixture)}


def preflight_build_artifacts(context: dict[str, Any]) -> dict[str, Any]:
    artifacts = BuildArtifacts(
        decodexd=context["decodexd_artifact"],
        migration_fixture=context["migration_fixture_artifact"],
    )
    context["artifacts"] = artifacts
    return {
        "decodexd": str(artifacts.decodexd),
        "migration_fixture": str(artifacts.migration_fixture),
    }


def preflight_installer(context: dict[str, Any]) -> dict[str, Any]:
    installer = load_installer()
    context["installer"] = installer
    return {
        "path": str(INSTALLER_PATH),
        "ambient_home_override": False,
    }


def preflight_daemon_wrapper_signing(
    context: dict[str, Any],
) -> dict[str, Any]:
    identity: LoginIdentity = context["identity"]
    wrapper = load_daemon_wrapper()
    try:
        completed = bounded_run(
            [str(wrapper.SECURITY), "find-identity", "-v", "-p", "codesigning"],
            timeout=30,
            check=True,
        )
        output = completed.stdout.decode("utf-8", errors="strict")
    except (
        OSError,
        UnicodeDecodeError,
        subprocess.CalledProcessError,
    ) as error:
        raise GateFailure("code-signing identity inventory is unavailable") from error
    identity_hashes = frozenset(
        match.group(1).lower()
        for line in output.splitlines()
        if (
            match := re.fullmatch(
                r'\s*[0-9]+\)\s+([0-9a-fA-F]{40})\s+".*"',
                line,
            )
        )
    )
    if not identity_hashes:
        raise GateFailure("code-signing identity inventory is empty")

    profile_directories = (
        identity.home
        / "Library/Developer/Xcode/UserData/Provisioning Profiles",
        identity.home / "Library/MobileDevice/Provisioning Profiles",
    )
    candidates: list[Path] = []
    observed_entries = 0
    for directory in profile_directories:
        try:
            metadata = directory.lstat()
        except FileNotFoundError:
            continue
        except OSError as error:
            raise GateFailure("provisioning-profile directory is unavailable") from error
        if (
            stat.S_ISLNK(metadata.st_mode)
            or not stat.S_ISDIR(metadata.st_mode)
            or metadata.st_uid != identity.uid
            or metadata.st_mode & 0o022
        ):
            raise GateFailure("provisioning-profile directory is unsafe")
        try:
            entries = sorted(directory.iterdir(), key=lambda path: path.name)
        except OSError as error:
            raise GateFailure("provisioning-profile inventory is unavailable") from error
        observed_entries += len(entries)
        if observed_entries > MAX_PROVISIONING_PROFILE_CANDIDATES:
            raise GateFailure("provisioning-profile inventory exceeded its bound")
        for entry in entries:
            try:
                entry_metadata = entry.lstat()
            except OSError as error:
                raise GateFailure("provisioning-profile candidate is unavailable") from error
            if (
                stat.S_ISLNK(entry_metadata.st_mode)
                or not stat.S_ISREG(entry_metadata.st_mode)
                or entry_metadata.st_uid != identity.uid
                or entry_metadata.st_mode & 0o022
            ):
                continue
            candidates.append(entry)

    matches: list[tuple[Path, str]] = []
    for profile in candidates:
        try:
            _, document = wrapper._profile_document(profile)
            _, certificates = wrapper.validate_profile(document)
        except wrapper.WrapperError:
            continue
        certificate_hashes = {
            hashlib.sha1(certificate, usedforsecurity=False).hexdigest()
            for certificate in certificates
        }
        for signing_identity in sorted(identity_hashes & certificate_hashes):
            matches.append((profile, signing_identity))
    if len(matches) != 1:
        raise GateFailure(
            "one unique matching development profile and signing identity is required"
        )
    profile, signing_identity = matches[0]
    context["daemon_wrapper_module"] = wrapper
    context["daemon_wrapper_profile"] = profile
    context["daemon_wrapper_signing_identity"] = signing_identity
    return {
        "profile_directories_checked": len(profile_directories),
        "profile_candidates_checked": len(candidates),
        "matching_pairs": 1,
        "team_identifier": wrapper.TEAM_IDENTIFIER,
        "profile_channel": wrapper.PROFILE_CHANNEL,
    }


def create_fixture_root(context: dict[str, Any]) -> dict[str, Any]:
    fixture_root: Path = context["fixture_root"]
    os.mkdir(fixture_root, 0o700)
    metadata = fixture_root.lstat()
    identity: LoginIdentity = context["identity"]
    if (
        stat.S_ISLNK(metadata.st_mode)
        or not stat.S_ISDIR(metadata.st_mode)
        or metadata.st_uid != identity.uid
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        raise GateFailure("fixture root did not retain its exact authority")
    return {
        "created_exclusively": True,
        "mode": "0700",
        "endpoint_namespace_reserved": True,
    }


def make_paths(
    installer: Any,
    fixture_root: Path,
    marker: Path,
    toolchain: Toolchain,
    artifacts: BuildArtifacts,
) -> Any:
    root = fixture_root / ".decodex"
    binary_directory = fixture_root / "bin"
    decodex_cli = binary_directory / "decodex"
    codex = binary_directory / "codex"
    executable_stub(decodex_cli, marker)
    executable_stub(codex, marker)
    return installer.InstallPaths(
        repository=REPO_ROOT,
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
        legacy_accounts=fixture_root / "legacy" / "accounts.jsonl",
        legacy_config=fixture_root / "legacy" / "config.toml",
        launch_agent=fixture_root
        / "LaunchAgents"
        / "space.decodex.local-service.plist",
        decodexd=artifacts.decodexd,
        decodex_cli=decodex_cli,
        codex=codex,
        postgres=toolchain.postgres,
        initdb=toolchain.initdb,
        pg_isready=toolchain.pg_isready,
        psql=toolchain.psql,
    )


def setup_fixture(context: dict[str, Any], installer: Any) -> dict[str, Any]:
    fixture_root: Path = context["fixture_root"]
    metadata = fixture_root.lstat()
    identity: LoginIdentity = context["identity"]
    if (
        stat.S_ISLNK(metadata.st_mode)
        or not stat.S_ISDIR(metadata.st_mode)
        or metadata.st_uid != identity.uid
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        raise GateFailure("fixture root authority changed before full setup")
    marker = fixture_root / "unexpected-runtime-spawn"
    toolchain = context.get("toolchain")
    if toolchain is None:
        unavailable = fixture_root / "unavailable-postgresql-toolchain"
        toolchain = Toolchain(
            postgres=unavailable / "postgres",
            initdb=unavailable / "initdb",
            pg_isready=unavailable / "pg_isready",
            psql=unavailable / "psql",
        )
    artifacts = context.get("artifacts")
    if artifacts is None:
        decodexd = context.get(
            "decodexd_artifact",
            fixture_root / "unavailable-decodexd",
        )
        artifacts = BuildArtifacts(
            decodexd=decodexd,
            migration_fixture=fixture_root / "unavailable-migration-fixture",
        )
    wrapper = context.get("daemon_wrapper_module")
    profile = context.get("daemon_wrapper_profile")
    signing_identity = context.get("daemon_wrapper_signing_identity")
    if (
        wrapper is None
        or not isinstance(profile, Path)
        or not isinstance(signing_identity, str)
    ):
        raise GateFailure("daemon wrapper signing authority is unavailable")
    binary_directory = fixture_root / "bin"
    binary_directory.mkdir(mode=0o700)
    descriptor = wrapper.compose_wrapper(
        artifacts.decodexd,
        profile,
        signing_identity,
        binary_directory / wrapper.WRAPPER_NAME,
    )
    artifacts = replace(
        artifacts,
        decodexd=Path(descriptor["executable_path"]),
    )
    context["artifacts"] = artifacts
    context["daemon_wrapper"] = descriptor
    paths = make_paths(
        installer,
        fixture_root,
        marker,
        toolchain,
        artifacts,
    )
    context["paths"] = paths
    context["marker"] = marker
    installer.POSTGRES_PORT = context["postgres_port"]
    installer.ensure_directories(paths, context["identity"].uid)
    create_legacy_sources(installer, paths)
    if CONFLICT_CASE is None:
        raise GateFailure("run-unique conflict identity is unavailable")
    conflict_sources = create_conflict_credential_sources(
        fixture_root,
        CONFLICT_CASE,
    )
    context["conflict_sources"] = conflict_sources
    private_write(
        fixture_root / ACCOUNT_MIGRATION_GATE_RUN_FILE,
        (
            json.dumps(
                {
                    "schema": ACCOUNT_MIGRATION_GATE_RUN_SCHEMA,
                    "run_id": context["run_token"],
                },
                sort_keys=True,
                separators=(",", ":"),
            )
            + "\n"
        ).encode(),
    )
    return {
        "fixture_root_created": True,
        "source_count": 4,
        "credential_conflict_sources": 2,
        "credential_gate_descriptor": "strict_run_token_only",
        "daemon_wrapper_sha256": installer.daemon_wrapper_digest(descriptor),
        "daemon_wrapper_main": descriptor["executable_path"],
    }


def expect_install_refusal(action: Callable[[], Any], failure: str) -> None:
    try:
        action()
    except BaseException as error:
        if error.__class__.__name__ == "InstallError":
            return
        raise
    raise GateFailure(failure)


def verify_source_path_predicate(
    installer: Any,
    identity: LoginIdentity,
    fixture_root: Path,
) -> dict[str, Any]:
    accepted_ancestor = fixture_root / "accepted-ancestor"
    accepted_ancestor.mkdir(mode=0o755)
    os.chmod(accepted_ancestor, 0o755)
    accepted_parent = accepted_ancestor / "accepted-source"
    accepted_parent.mkdir(mode=0o700)
    accepted = accepted_parent / "accounts.jsonl"
    private_write(accepted, b"{}\n")
    installer.require_private_legacy_source_chain(accepted, identity.uid)
    descriptor = installer.secure_legacy_file(accepted, identity.uid)
    os.close(descriptor)

    test_parent = fixture_root / "source-predicate"
    test_parent.mkdir(mode=0o700)
    os.chmod(test_parent, 0o700)
    loose = test_parent / "loose.json"
    private_write(loose, b"{}\n", 0o640)
    expect_install_refusal(
        lambda: installer.secure_legacy_file(loose, identity.uid),
        "a non-0600 source was accepted",
    )
    if stat.S_IMODE(loose.stat().st_mode) != 0o640:
        raise GateFailure("a refused source mode was repaired")

    linked = test_parent / "linked.json"
    linked_peer = test_parent / "linked-peer.json"
    private_write(linked, b"{}\n")
    os.link(linked, linked_peer)
    expect_install_refusal(
        lambda: installer.secure_legacy_file(linked, identity.uid),
        "a multiply-linked source was accepted",
    )

    symlink = test_parent / "symlink.json"
    symlink.symlink_to(linked.name)
    expect_install_refusal(
        lambda: installer.secure_legacy_file(symlink, identity.uid),
        "a symbolic-link source was accepted",
    )

    os.chmod(test_parent, 0o750)
    try:
        expect_install_refusal(
            lambda: installer.require_private_legacy_source_chain(
                loose,
                identity.uid,
            ),
            "a non-private direct source parent was accepted",
        )
    finally:
        os.chmod(test_parent, 0o700)

    os.chmod(fixture_root, 0o720)
    try:
        expect_install_refusal(
            lambda: installer.require_private_legacy_source_chain(
                accepted,
                identity.uid,
            ),
            "a writable source ancestor was accepted",
        )
    finally:
        os.chmod(fixture_root, 0o700)

    symlink_parent = fixture_root / "source-parent-link"
    symlink_parent.symlink_to(test_parent.name)
    expect_install_refusal(
        lambda: installer.require_private_legacy_source_chain(
            symlink_parent / "loose.json",
            identity.uid,
        ),
        "a symbolic-link source parent was accepted",
    )
    installer.require_private_legacy_source_chain(accepted, identity.uid)
    return {
        "real_login_home_authority": str(identity.home),
        "harmless_ancestor_read_execute_bits": "accepted",
        "direct_parent_mode": "0700",
        "source_mode": "0600",
        "no_follow_and_one_link": "verified",
        "acl_claim": "not_made",
    }


def fixture_tree_snapshot(root: Path) -> str:
    records = []
    for path in sorted((root, *root.rglob("*")), key=lambda value: str(value)):
        metadata = path.lstat()
        relative = "." if path == root else str(path.relative_to(root))
        record: dict[str, Any] = {
            "path": relative,
            "mode": stat.S_IMODE(metadata.st_mode),
            "uid": metadata.st_uid,
        }
        if stat.S_ISDIR(metadata.st_mode):
            record["type"] = "directory"
        elif stat.S_ISREG(metadata.st_mode):
            record["type"] = "file"
            record["links"] = metadata.st_nlink
            record["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
        elif stat.S_ISLNK(metadata.st_mode):
            record["type"] = "symlink"
            record["target"] = os.readlink(path)
        else:
            record["type"] = "other"
        records.append(record)
    return hashlib.sha256(
        json.dumps(records, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()


def create_legacy_sources(installer: Any, paths: Any) -> None:
    records = []
    mapping = []
    config_lines = ["version = 1", ""]
    for slot, case in enumerate(CASES, start=1):
        id_token = jwt(
            {
                "email": case.email,
                "https://api.openai.com/auth": {
                    "chatgpt_account_id": case.provider_account_id,
                    "chatgpt_plan_type": "pro",
                },
            }
        )
        records.append(
            {
                "email": case.email,
                "disabled": not case.enabled,
                "tokens": {
                    "access_token": jwt({"exp": 4_102_444_800, "slot": slot}),
                    "account_id": case.provider_account_id,
                    "id_token": id_token,
                    "refresh_token": f"xy1422-private-refresh-{slot}",
                },
            }
        )
        digest = hashlib.sha256(case.provider_account_id.encode()).hexdigest()
        mapping.append({"slot": slot, "provider_account_id_sha256": digest})
        config_lines.extend(
            [
                f'[server_host.reset_card_accounts."{case.account_id}"]',
                f"display_label = {json.dumps(case.display_label)}",
                f'access_token_env_var = "DECODEX_RESET_CARD_SLOT_{slot:02d}_ACCESS_TOKEN"',
                "",
            ]
        )
    private_write(
        paths.legacy_accounts,
        b"".join(
            (
                json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n"
            ).encode()
            for record in records
        ),
    )
    private_write(
        paths.mapping,
        (
            json.dumps(
                {"schema": installer.MAPPING_SCHEMA, "accounts": mapping},
                sort_keys=True,
                separators=(",", ":"),
            )
            + "\n"
        ).encode(),
    )
    private_write(
        paths.vnext_config_source,
        ("\n".join(config_lines) + "\n").encode(),
    )


def create_conflict_credential_sources(
    fixture_root: Path,
    case: CredentialGateCase,
) -> tuple[Path, Path]:
    directory = fixture_root / "protected-store-conflict"
    directory.mkdir(mode=0o700, parents=False, exist_ok=False)
    os.chmod(directory, 0o700)
    sources = []
    for variant in (1, 2):
        payload = {
            "schema": "decodex/account-credential-import/1",
            "provider": "chatgpt",
            "provider_account_id": case.provider_account_id,
            "provider_email": case.email,
            "access_token": jwt({"exp": 4_102_444_800, "variant": variant}),
            "refresh_token": f"xy1422-gate-conflict-refresh-{variant}",
            "id_token": jwt({"variant": variant}),
            "plan_type": "pro",
            "token_type": "bearer",
            "access_token_expires_at_unix_micros": 4_102_444_800_000_000,
        }
        source = directory / f"credential-{variant}.json"
        private_write(
            source,
            (
                json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n"
            ).encode(),
        )
        sources.append(source)
    return sources[0], sources[1]


def sql(installer: Any, paths: Any, statement: str) -> str:
    return installer.psql_scalar(
        paths,
        installer.POSTGRES_DATABASE,
        statement,
        installer.psql_environment(paths),
    )


def apply_v26_fixture(
    installer: Any,
    paths: Any,
    artifacts: BuildArtifacts,
) -> None:
    environment = os.environ.copy()
    for name in list(environment):
        if name.startswith("PG"):
            del environment[name]
    completed = bounded_run(
        [
            str(artifacts.migration_fixture),
            "--socket-directory",
            str(paths.socket_directory),
            "--port",
            str(installer.POSTGRES_PORT),
            "--database",
            installer.POSTGRES_DATABASE,
            "--user",
            installer.POSTGRES_MIGRATION_ROLE,
        ],
        cwd=REPO_ROOT,
        env=environment,
        timeout=GATE_TIMEOUT_SECONDS,
        check=False,
    )
    if completed.returncode != 0:
        raise GateFailure("the real V1 through V26 migration fixture failed")
    values = []
    for case in CASES:
        if case.v26_revision is None:
            continue
        values.append(
            "("
            f"'{case.account_id}',"
            f"'{case.v26_label}',"
            f"'{case.v26_state}',"
            f"{case.v26_revision}"
            ")"
        )
    sql(
        installer,
        paths,
        "INSERT INTO decodex.accounts"
        "(account_id,display_label,state,revision) VALUES "
        + ",".join(values),
    )


def populated_v26_without_handoff_refusal(
    context: dict[str, Any],
) -> dict[str, Any]:
    installer = context["installer"]
    paths = context["paths"]
    artifacts: BuildArtifacts = context["artifacts"]
    postgres = start_owned_postgres(installer, paths)
    context["postgres"] = postgres
    try:
        snapshot_statement = """
SELECT pg_catalog.json_build_object(
  'history',(
    SELECT pg_catalog.json_agg(pg_catalog.row_to_json(history) ORDER BY version)
    FROM public.refinery_schema_history AS history
  ),
  'accounts',(
    SELECT pg_catalog.json_agg(pg_catalog.row_to_json(account) ORDER BY account_id)
    FROM decodex.accounts AS account
  ),
  'v27_relations',(
    SELECT pg_catalog.count(*) FROM pg_catalog.pg_class AS relation
    JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=relation.relnamespace
    WHERE namespace.nspname='decodex'
      AND relation.relname IN ('account_migration_receipts','account_operations')
  ),
  'v27_types',(
    SELECT pg_catalog.count(*) FROM pg_catalog.pg_type AS type
    JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=type.typnamespace
    WHERE namespace.nspname='decodex'
      AND type.typname IN (
        'account_provider_kind','account_operation_kind',
        'account_operation_phase','account_selection_mode','account_store_observation'
      )
  )
)::text
"""
        before = sql(installer, paths, snapshot_statement)
        before_document = json.loads(before)
        if (
            len(before_document.get("history") or []) != 26
            or before_document["history"][-1].get("version") != 26
            or before_document.get("v27_relations") != 0
            or before_document.get("v27_types") != 0
        ):
            raise GateFailure("populated V26 no-handoff precondition differed")
        environment = os.environ.copy()
        for name in list(environment):
            if name.startswith("PG"):
                del environment[name]
        completed = bounded_run(
            [
                str(artifacts.migration_fixture),
                "--socket-directory",
                str(paths.socket_directory),
                "--port",
                str(installer.POSTGRES_PORT),
                "--database",
                installer.POSTGRES_DATABASE,
                "--user",
                installer.POSTGRES_MIGRATION_ROLE,
                "--attempt-v27-without-handoff",
            ],
            cwd=REPO_ROOT,
            env=environment,
            timeout=GATE_TIMEOUT_SECONDS,
            check=False,
        )
        if completed.returncode == 0:
            raise GateFailure("populated V26 migrated without an exact manifest handoff")
        after = sql(installer, paths, snapshot_statement)
        if after != before:
            raise GateFailure("no-handoff V27 refusal changed the populated V26 destination")
        return {
            "populated_v26": True,
            "handoff": "absent",
            "v27_result": "refused_before_destination_mutation",
            "history_terminal_version": 26,
            "v27_objects_committed": 0,
        }
    finally:
        stop_owned_postgres(installer, postgres)
        context["postgres"] = None


def migration_command(paths: Any, manifest: Path | None = None) -> list[str]:
    return [
        str(paths.decodexd),
        "migrate-accounts",
        "--config",
        str(paths.staging_config),
        "--manifest",
        str(manifest or paths.migration_manifest),
        "--credential-directory",
        str(paths.credential_directory),
        "--launch-agent",
        str(paths.launch_agent),
    ]


def installed_asset_arguments(installer: Any, paths: Any) -> list[str]:
    arguments = []
    for asset in installer.migration_installed_assets(paths):
        arguments.extend(["--installed-asset", str(asset)])
    return arguments


def prepared_verifier_command(paths: Any) -> list[str]:
    return [
        str(paths.decodexd),
        "verify-prepared-account-migration",
        "--config",
        str(paths.config),
        "--manifest",
        str(paths.migration_manifest),
        "--launch-agent",
        str(paths.launch_agent),
    ]


def finalizer_command(installer: Any, paths: Any) -> list[str]:
    return [
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
        *installed_asset_arguments(installer, paths),
    ]


def completed_verifier_command(installer: Any, paths: Any) -> list[str]:
    return [
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
        *installed_asset_arguments(installer, paths),
    ]


def assert_contended(lock_path: Path) -> None:
    descriptor = os.open(
        lock_path,
        os.O_RDWR | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_CLOEXEC", 0),
    )
    acquired = False
    try:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            acquired = True
        except BlockingIOError:
            return
    finally:
        if acquired:
            fcntl.flock(descriptor, fcntl.LOCK_UN)
        os.close(descriptor)
    raise GateFailure("external namespace-lock contender crossed a retained barrier")


def assert_reacquirable(lock_path: Path) -> None:
    descriptor = os.open(
        lock_path,
        os.O_RDWR | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_CLOEXEC", 0),
    )
    try:
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        fcntl.flock(descriptor, fcntl.LOCK_UN)
    except BlockingIOError as error:
        raise GateFailure("final namespace-lock holder did not release") from error
    finally:
        os.close(descriptor)


def wait_reacquirable(lock_path: Path) -> None:
    deadline = time.monotonic() + 10
    while True:
        try:
            assert_reacquirable(lock_path)
            return
        except GateFailure:
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.05)


class GateSocket:
    def __init__(self, connection: socket.socket) -> None:
        self.connection = connection
        self.buffer = bytearray()

    def next_event(self, alive: Callable[[], bool]) -> str | None:
        deadline = time.monotonic() + GATE_TIMEOUT_SECONDS
        while True:
            newline = self.buffer.find(b"\n")
            if newline >= 0:
                line = bytes(self.buffer[:newline])
                del self.buffer[: newline + 1]
                try:
                    return line.decode("ascii")
                except UnicodeDecodeError as error:
                    raise GateFailure("transition checkpoint was not ASCII") from error
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise GateFailure("transition checkpoint timed out")
            readable, _, _ = select.select(
                [self.connection.fileno()],
                [],
                [],
                min(0.25, remaining),
            )
            if not readable:
                if not alive():
                    return None
                continue
            chunk = self.connection.recv(4096)
            if not chunk:
                return None
            self.buffer.extend(chunk)
            if len(self.buffer) > MAX_SUBPROCESS_OUTPUT_BYTES:
                raise GateFailure("transition checkpoint output exceeded its bound")

    def continue_child(self) -> None:
        self.connection.sendall(b"c")


def capture_process_identity(installer: Any, process_id: int) -> Any:
    deadline = time.monotonic() + 5
    while True:
        processes = installer.process_parent_map(deadline)
        record = processes.get(process_id)
        if record is not None:
            return record.identity
        if time.monotonic() >= deadline:
            raise GateFailure("gate-owned process identity was not observable")
        time.sleep(0.02)


def exact_process_is_live(installer: Any, identity: Any) -> bool:
    processes = installer.process_parent_map(time.monotonic() + 5)
    record = processes.get(identity.process_id)
    return record is not None and record.identity == identity


def signal_exact_process(installer: Any, identity: Any, requested_signal: int) -> bool:
    if not exact_process_is_live(installer, identity):
        return False
    os.kill(identity.process_id, requested_signal)
    return True


def terminate_owned_process(
    installer: Any,
    process: subprocess.Popen[Any],
    identity: Any,
) -> None:
    if process.poll() is not None:
        process.wait(timeout=1)
        return
    if not exact_process_is_live(installer, identity):
        raise GateFailure("gate-owned process identity changed before cleanup")
    terminate_bounded_subprocess(process)


def terminate_direct_child_without_identity(process: subprocess.Popen[Any]) -> None:
    if process.poll() is not None:
        process.wait(timeout=1)
        return
    terminate_bounded_subprocess(process)


@dataclass
class OwnedPostgres:
    process: subprocess.Popen[Any]
    identity: Any


def start_owned_postgres(installer: Any, paths: Any) -> OwnedPostgres:
    process = installer.start_temporary_postgres(paths)
    try:
        identity = capture_process_identity(installer, process.pid)
    except BaseException:
        installer.stop_temporary_postgres(process)
        raise
    return OwnedPostgres(process=process, identity=identity)


def stop_owned_postgres(installer: Any, postgres: OwnedPostgres) -> None:
    if (
        postgres.process.poll() is None
        and not exact_process_is_live(installer, postgres.identity)
    ):
        raise GateFailure("PostgreSQL process identity changed before cleanup")
    installer.stop_temporary_postgres(postgres.process)


def live_daemon_exclusion(
    installer: Any,
    paths: Any,
    uid: int,
) -> dict[str, Any]:
    parent: socket.socket | None = None
    child: socket.socket | None = None
    barrier_descriptor: int | None = None
    process: subprocess.Popen[Any] | None = None
    identity: Any = None
    result: dict[str, Any] | None = None
    primary_error: BaseException | None = None
    try:
        parent, child = socket.socketpair()
        gate = GateSocket(parent)
        barrier_descriptor = os.dup(child.fileno())
        process = subprocess.Popen(
            [
                str(paths.decodexd),
                "account-migration-live-daemon-gate",
                "--root",
                str(paths.root),
                "--barrier-fd",
                str(barrier_descriptor),
            ],
            cwd=REPO_ROOT,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            close_fds=True,
            pass_fds=(barrier_descriptor,),
            start_new_session=True,
        )
        os.close(barrier_descriptor)
        barrier_descriptor = None
        child.close()
        child = None
        identity = capture_process_identity(installer, process.pid)
        event = gate.next_event(lambda: process is not None and process.poll() is None)
        if event != "live_daemon_ready":
            raise GateFailure("real local transport owner did not reach its barrier")
        competing = None
        try:
            competing = installer.InstallerNamespaceLock.acquire(paths, uid)
        except installer.InstallError:
            pass
        else:
            competing.close()
            raise GateFailure("installer crossed a live daemon namespace owner")
        gate.continue_child()
        stdout, stderr = communicate_bounded_subprocess(
            process,
            [
                str(paths.decodexd),
                "account-migration-live-daemon-gate",
            ],
            30,
        )
        if process.returncode != 0:
            raise GateFailure("live-daemon gate did not exit cleanly")
        try:
            report = json.loads(stdout)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise GateFailure("live-daemon gate report was malformed") from error
        if report != {
            "schema": "decodex/account-migration-live-daemon-gate/1",
            "namespace_retained": True,
        }:
            raise GateFailure("live-daemon gate report differed")
        assert_reacquirable(paths.namespace_lock)
        if (
            (paths.server_directory / "decodex.sock").exists()
            or (paths.server_directory / "decodex.sock.stage").exists()
        ):
            raise GateFailure("live-daemon gate left a socket publication")
        result = {
            "owner": "LocalTransportAuthority",
            "installer_refused": True,
            "publication_cleaned": True,
        }
    except BaseException as error:
        primary_error = error

    cleanup_error: BaseException | None = None
    if barrier_descriptor is not None:
        try:
            os.close(barrier_descriptor)
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if child is not None:
        try:
            child.close()
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if process is not None and process.poll() is None:
        try:
            if identity is None:
                terminate_direct_child_without_identity(process)
            else:
                terminate_owned_process(installer, process, identity)
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if process is not None:
        for stream in (process.stdout, process.stderr):
            if stream is None or stream.closed:
                continue
            try:
                stream.close()
            except BaseException as error:
                cleanup_error = cleanup_error or error
    if parent is not None:
        try:
            parent.close()
        except BaseException as error:
            cleanup_error = cleanup_error or error
    try:
        assert_reacquirable(paths.namespace_lock)
    except BaseException as error:
        cleanup_error = cleanup_error or error
    if primary_error is not None:
        if cleanup_error is not None:
            raise primary_error from cleanup_error
        raise primary_error
    if cleanup_error is not None:
        raise cleanup_error
    if result is None:
        raise GateFailure("live-daemon exclusion produced no result")
    return result


def account_migration_admission_gate(
    paths: Any,
    boundary: str,
    account_id: str,
    expected_revision: int,
) -> dict[str, Any]:
    completed = bounded_run(
        [
            str(paths.decodexd),
            "account-migration-admission-gate",
            boundary,
            "--config",
            str(paths.config if boundary == "completed" else paths.staging_config),
            "--account-id",
            account_id,
            "--expected-revision",
            str(expected_revision),
        ],
        cwd=REPO_ROOT,
        timeout=GATE_TIMEOUT_SECONDS,
        check=False,
    )
    if completed.returncode != 0:
        raise GateFailure(f"{boundary} runtime admission gate failed")
    try:
        document = json.loads(completed.stdout)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise GateFailure(f"{boundary} runtime admission report was malformed") from error
    if (
        not isinstance(document, dict)
        or document.get("schema")
        != "decodex/account-migration-admission-gate/1"
        or document.get("boundary") != boundary
        or document.get("process_generation_owner_composed") is not True
        or any(
            document.get(field) not in {"refused", "admitted", "unexpected"}
            for field in (
                "initial_selection",
                "process_spawn_admission",
                "reset_card_admission",
            )
        )
    ):
        raise GateFailure(f"{boundary} runtime admission evidence differed")
    return document


def account_migration_recovery_gate(
    paths: Any,
    phase: str,
) -> dict[str, Any]:
    phase_argument = {
        "prepared": "prepared",
        "recovery_required": "recovery-required",
    }.get(phase)
    if phase_argument is None:
        raise GateFailure("migration recovery gate phase is invalid")
    completed = bounded_run(
        [
            str(paths.decodexd),
            "account-migration-recovery-gate",
            phase_argument,
            "--run-descriptor",
            str(paths.root.parent / ACCOUNT_MIGRATION_GATE_RUN_FILE),
        ],
        cwd=REPO_ROOT,
        timeout=GATE_TIMEOUT_SECONDS,
        check=False,
    )
    if completed.returncode != 0:
        raise GateFailure(f"{phase} migration recovery gate failed")
    try:
        document = json.loads(completed.stdout)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise GateFailure("migration recovery gate report was malformed") from error
    if document != {
        "schema": "decodex/account-migration-recovery-gate/1",
        "phase": phase,
        "direct_cancel_refused": True,
        "logical_command_cancel_refused": True,
        "logical_command_receipt_replayed": True,
        "operation_unchanged": True,
        "account_unchanged": True,
    }:
        raise GateFailure(f"{phase} migration recovery evidence differed")
    return document


def validate_admission_branch(
    report: dict[str, Any],
    field: str,
    expected: str,
) -> dict[str, Any]:
    if report.get(field) != expected:
        raise GateFailure(f"{field} returned {report.get(field)!r}")
    return {
        "owner_result": report[field],
        "process_generation_owner_composed": report[
            "process_generation_owner_composed"
        ],
    }


def admission_durable_footprint(
    installer: Any,
    paths: Any,
    *,
    expected_reset_rows: int,
) -> dict[str, Any]:
    process_rows = sql(
        installer,
        paths,
        "SELECT pg_catalog.count(*) FROM decodex.process_generations",
    )
    reset_rows = sql(
        installer,
        paths,
        "SELECT pg_catalog.count(*) FROM decodex.outbox "
        "WHERE aggregate_kind='reset_card_operation'",
    )
    if process_rows != "0" or reset_rows != str(expected_reset_rows):
        raise GateFailure("runtime admission durable footprint differs")
    return {
        "process_generation_rows": 0,
        "reset_card_outbox_rows": expected_reset_rows,
    }


def host_credential_gate(
    paths: Any,
    action: str,
    account_id: str | None = None,
) -> dict[str, Any]:
    action_argument = {
        "inspect": "readback",
        "prove_conflict": "prove-create-conflict",
        "cleanup_run": "cleanup-run",
    }.get(action)
    if action_argument is None:
        raise GateFailure("protected-store gate action is invalid")
    command = [
        str(paths.decodexd),
        "account-migration-credential-gate",
        action_argument,
        "--run-descriptor",
        str(paths.root.parent / ACCOUNT_MIGRATION_GATE_RUN_FILE),
    ]
    if action == "inspect":
        slot = None
        for ordinal, case in enumerate(CASES, start=1):
            if case.account_id == account_id:
                slot = f"account-{ordinal}"
                break
        if CONFLICT_CASE is not None and CONFLICT_CASE.account_id == account_id:
            slot = "conflict"
        if slot is None:
            raise GateFailure("credential identity is outside the finite gate slots")
        command.extend(["--slot", slot])
    elif account_id is not None:
        raise GateFailure("bounded credential gate action does not accept an identity")
    completed = bounded_run(command, cwd=REPO_ROOT, timeout=30)
    if completed.returncode != 0:
        raise GateFailure(f"protected-store {action} failed")
    try:
        document = json.loads(completed.stdout)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise GateFailure("protected-store metadata was malformed") from error
    if not isinstance(document, dict):
        raise GateFailure("protected-store metadata was not an object")
    return document


def validate_host_credential_metadata(
    document: dict[str, Any],
    account_id: str,
    *,
    expected: dict[str, Any] | None,
) -> None:
    if (
        set(document) != HOST_CREDENTIAL_METADATA_FIELDS
        or document.get("schema")
        != "decodex/account-migration-credential-gate-readback/1"
        or document.get("service") != KEYCHAIN_SERVICE
        or document.get("account") != account_id
        or document.get("protected_keychain") is not True
    ):
        raise GateFailure("protected-store identity metadata differs")
    if expected is None:
        optional = {
            "store_schema_version",
            "provider",
            "provider_account_id",
            "credential_version",
            "writer_operation_id",
            "fingerprint_sha256",
            "label",
            "access_group",
            "description",
            "accessibility",
            "synchronizing",
            "access_control_present",
        }
        if document.get("present") is not False or any(
            document.get(field) is not None for field in optional
        ):
            raise GateFailure("protected-store identity was not absent")
        return
    if (
        document.get("present") is not True
        or document.get("store_schema_version")
        != expected.get("store_schema_version", 1)
        or document.get("provider") != "chatgpt"
        or document.get("provider_account_id") != expected["provider_account_id"]
        or document.get("credential_version")
        != expected.get("credential_version", 1)
        or document.get("writer_operation_id") != expected["writer_operation_id"]
        or document.get("fingerprint_sha256")
        != expected.get("fingerprint_sha256", document.get("fingerprint_sha256"))
        or not isinstance(document.get("fingerprint_sha256"), str)
        or not re.fullmatch(r"[0-9a-f]{64}", document["fingerprint_sha256"])
        or document.get("label") != "box.acg.decodex"
        or document.get("access_group") != KEYCHAIN_ACCESS_GROUP
        or document.get("description")
        != "Decodex daemon account credential bundle"
        or document.get("accessibility") != "cku"
        or document.get("synchronizing") is not False
        or document.get("access_control_present") is not True
    ):
        raise GateFailure("protected-store exact metadata differs")


def validate_protected_store_conflict_report(
    report: dict[str, Any],
) -> dict[str, Any]:
    if (
        set(report)
        != {
            "schema",
            "complete",
            "phases",
            "readback",
            "primary_failure",
            "cleanup_failure",
        }
        or report.get("schema")
        != "decodex/account-migration-credential-gate-conflict/1"
        or type(report.get("complete")) is not bool
        or not isinstance(report.get("phases"), list)
        or len(report["phases"]) != len(PROTECTED_STORE_PHASES)
    ):
        raise GateFailure("protected-store report malformed")

    observed: list[tuple[str, str]] = []
    for index, phase in enumerate(report["phases"]):
        if (
            not isinstance(phase, dict)
            or set(phase) != {"phase", "category"}
            or phase.get("phase") != PROTECTED_STORE_PHASES[index]
            or not isinstance(phase.get("category"), str)
            or phase["category"]
            not in (
                PROTECTED_STORE_FAILURE_CATEGORIES
                | {PROTECTED_STORE_SUCCESS_CATEGORIES[index]}
            )
        ):
            raise GateFailure("protected-store report malformed")
        observed.append((phase["phase"], phase["category"]))

    failures: list[tuple[str, str] | None] = []
    for field in ("primary_failure", "cleanup_failure"):
        failure = report[field]
        if failure is None:
            failures.append(None)
            continue
        if (
            not isinstance(failure, dict)
            or set(failure) != {"phase", "category"}
            or not isinstance(failure.get("phase"), str)
            or not isinstance(failure.get("category"), str)
            or (failure["phase"], failure["category"]) not in observed
            or failure["category"] not in PROTECTED_STORE_FAILURE_CATEGORIES
        ):
            raise GateFailure("protected-store report malformed")
        failures.append((failure["phase"], failure["category"]))

    success = observed == list(
        zip(PROTECTED_STORE_PHASES, PROTECTED_STORE_SUCCESS_CATEGORIES, strict=True)
    )
    if (
        report["complete"] is not success
        or success != (failures == [None, None])
        or (success and not isinstance(report.get("readback"), dict))
        or (not success and report.get("readback") is not None
            and not isinstance(report["readback"], dict))
    ):
        raise GateFailure("protected-store report malformed")
    if not success:
        diagnostic = failures[0] or failures[1]
        if diagnostic is None:
            diagnostic = next(
                (
                    observed_phase
                    for observed_phase, expected_category in zip(
                        observed,
                        PROTECTED_STORE_SUCCESS_CATEGORIES,
                        strict=True,
                    )
                    if observed_phase[1] != expected_category
                ),
                None,
            )
        if diagnostic is None or diagnostic[1] not in PROTECTED_STORE_FAILURE_CATEGORIES:
            raise GateFailure("protected-store report malformed")
        raise GateFailure(f"protected-store {diagnostic[0]} {diagnostic[1]}")
    return report["readback"]


class CredentialOwnership:
    def __init__(self, selected: tuple[CredentialGateCase, ...]) -> None:
        self.selected = {case.account_id: case for case in selected}
        self.absence_proved = False
        self.created: dict[str, dict[str, Any]] = {}

    def bind_migration_operations(self, cases: tuple[AccountCase, ...]) -> None:
        if not self.absence_proved:
            raise GateFailure("credential identities were not proved absent")
        for case in cases:
            selected = self.selected.get(case.account_id)
            if selected is None:
                raise GateFailure("manifest introduced an unselected credential identity")
            if (
                selected.provider_account_id != case.provider_account_id
                or selected.email != case.email
            ):
                raise GateFailure("manifest changed a selected credential identity")
            self.selected[case.account_id] = CredentialGateCase(
                account_id=case.account_id,
                operation_id=case.operation_id,
                provider_account_id=case.provider_account_id,
                email=case.email,
            )

    def prove_absent(self, paths: Any) -> dict[str, Any]:
        for account_id in self.selected:
            document = host_credential_gate(paths, "inspect", account_id)
            assert document is not None
            validate_host_credential_metadata(document, account_id, expected=None)
        self.absence_proved = True
        return {"identities": len(self.selected), "all_absent": True}

    def inspect_and_record(
        self,
        paths: Any,
        account_id: str,
        expected: dict[str, Any],
    ) -> dict[str, Any]:
        if not self.absence_proved or account_id not in self.selected:
            raise GateFailure("credential ownership was not established")
        document = host_credential_gate(paths, "inspect", account_id)
        assert document is not None
        validate_host_credential_metadata(document, account_id, expected=expected)
        previous = self.created.get(account_id)
        if previous is not None and previous != document:
            raise GateFailure("a gate-created credential item changed metadata")
        self.created[account_id] = document
        return document

    def discover_gate_created(
        self,
        paths: Any,
        expectations: dict[str, dict[str, Any]],
    ) -> None:
        if not self.absence_proved:
            return
        for account_id, selected in self.selected.items():
            document = host_credential_gate(paths, "inspect", account_id)
            assert document is not None
            if document.get("present") is not True:
                validate_host_credential_metadata(document, account_id, expected=None)
                continue
            expected = expectations.get(
                account_id,
                {
                    "provider_account_id": selected.provider_account_id,
                    "writer_operation_id": selected.operation_id,
                },
            )
            validate_host_credential_metadata(document, account_id, expected=expected)
            previous = self.created.get(account_id)
            if previous is not None and previous != document:
                raise GateFailure("a gate-created credential item changed before cleanup")
            self.created[account_id] = document

    def cleanup(self, paths: Any) -> dict[str, Any]:
        allowed = {case.account_id for case in CASES}
        if CONFLICT_CASE is not None:
            allowed.add(CONFLICT_CASE.account_id)
        if any(account_id not in allowed for account_id in self.created):
            raise GateFailure("credential cleanup set is outside the finite run slots")
        manifest_count = len(CASES) if paths.migration_manifest.exists() else 0
        report = host_credential_gate(paths, "cleanup_run")
        if (
            set(report)
            != {
                "schema",
                "finite_slot_count",
                "manifest_account_count",
                "conflict_slot_checked",
                "deleted",
                "absence_verified",
            }
            or report.get("schema")
            != "decodex/account-migration-credential-gate-cleanup/1"
            or report.get("finite_slot_count") != len(CASES) + 1
            or report.get("manifest_account_count") != manifest_count
            or report.get("conflict_slot_checked") is not True
            or type(report.get("deleted")) is not int
            or report["deleted"] < 0
            or report["deleted"] > len(CASES) + 1
            or report.get("absence_verified") is not True
        ):
            raise GateFailure("run-bound credential cleanup report differed")
        for account_id in allowed:
            absent = host_credential_gate(paths, "inspect", account_id)
            validate_host_credential_metadata(absent, account_id, expected=None)
        return {
            "recorded": len(self.created),
            "deleted": report["deleted"],
            "absence_verified": True,
        }


def manifest_credential_expectations(paths: Any) -> dict[str, dict[str, Any]]:
    try:
        manifest = json.loads(paths.migration_manifest.read_bytes())
    except (OSError, json.JSONDecodeError) as error:
        raise GateFailure("frozen migration manifest is unavailable") from error
    accounts = manifest.get("accounts") if isinstance(manifest, dict) else None
    if not isinstance(accounts, list):
        raise GateFailure("frozen migration manifest has no accounts")
    expectations = {}
    for account in accounts:
        target = account.get("target") if isinstance(account, dict) else None
        account_id = account.get("account_id") if isinstance(account, dict) else None
        if not isinstance(account_id, str) or not isinstance(target, dict):
            raise GateFailure("frozen migration target is unavailable")
        expectations[account_id] = {
            "store_schema_version": target.get("store_schema_version"),
            "provider_account_id": target.get("provider_account_id"),
            "credential_version": target.get("credential_version"),
            "writer_operation_id": target.get("writer_operation_id"),
            "fingerprint_sha256": target.get("fingerprint_sha256"),
        }
    return expectations


def adopt_manifest_operation_ids(
    paths: Any,
    ownership: CredentialOwnership,
) -> dict[str, Any]:
    global CASES

    try:
        manifest = json.loads(paths.migration_manifest.read_bytes())
    except (OSError, json.JSONDecodeError) as error:
        raise GateFailure("frozen migration manifest is unavailable") from error
    accounts = manifest.get("accounts") if isinstance(manifest, dict) else None
    if not isinstance(accounts, list) or len(accounts) != len(CASES):
        raise GateFailure("frozen migration account universe differs")
    by_id = {case.account_id: case for case in CASES}
    rebound = []
    for ordinal, account in enumerate(accounts):
        if not isinstance(account, dict):
            raise GateFailure("frozen migration account entry is malformed")
        account_id = account.get("account_id")
        operation_id = account.get("operation_id")
        case = by_id.get(account_id)
        if (
            case is None
            or case != CASES[ordinal]
            or account.get("source_ordinal") != ordinal
            or account.get("display_label") != case.display_label
            or account.get("enabled") is not case.enabled
            or not isinstance(operation_id, str)
            or not re.fullmatch(
                r"[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}",
                operation_id,
            )
        ):
            raise GateFailure("frozen migration account identity differs")
        rebound.append(replace(case, operation_id=operation_id))
    CASES = tuple(rebound)
    ownership.bind_migration_operations(CASES)
    return {
        "account_ids": [case.account_id for case in CASES],
        "operation_ids_bound": len(CASES),
    }


def verify_daemon_wrapper_manifest_binding(
    context: dict[str, Any],
) -> dict[str, Any]:
    installer = context["installer"]
    paths = context["paths"]
    expected = context["daemon_wrapper"]
    wrapper = context["daemon_wrapper_module"]
    try:
        manifest = json.loads(paths.migration_manifest.read_bytes())
    except (OSError, json.JSONDecodeError) as error:
        raise GateFailure("frozen daemon wrapper manifest is unavailable") from error
    fingerprints = (
        manifest.get("decision_fingerprints")
        if isinstance(manifest, dict)
        else None
    )
    if (
        not isinstance(fingerprints, dict)
        or manifest.get("daemon_wrapper") != expected
        or any(
            not isinstance(key, str) or "wrapper" in key
            for key in fingerprints
        )
    ):
        raise GateFailure("frozen daemon wrapper manifest binding differs")
    current = installer.inspect_daemon_wrapper(paths)
    if current != expected:
        raise GateFailure("current daemon wrapper differs from the frozen manifest")
    try:
        launch_agent = plistlib.loads(paths.launch_agent.read_bytes())
    except (
        OSError,
        ValueError,
        plistlib.InvalidFileException,
    ) as error:
        raise GateFailure("daemon wrapper LaunchAgent is malformed") from error
    arguments = (
        launch_agent.get("ProgramArguments")
        if isinstance(launch_agent, dict)
        else None
    )
    if (
        not isinstance(arguments, list)
        or not arguments
        or arguments[0] != expected.get("executable_path")
    ):
        raise GateFailure("daemon wrapper LaunchAgent binding differs")
    installed_assets = installer.migration_installed_assets(paths)
    executable = Path(expected["executable_path"])
    if installed_assets.count(executable) != 1:
        raise GateFailure("daemon wrapper installed-asset binding differs")
    descriptor_only_paths = {
        Path(expected["info_plist_path"]),
        Path(expected["embedded_profile_path"]),
    }
    if any(path in installed_assets for path in descriptor_only_paths):
        raise GateFailure("descriptor-only wrapper assets became installed assets")
    executable_body = wrapper.read_regular(
        executable,
        wrapper.MAX_EXECUTABLE_BYTES,
        "daemon wrapper executable is unavailable",
        executable=True,
    )
    if (
        len(executable_body) != expected.get("executable_byte_count")
        or hashlib.sha256(executable_body).hexdigest()
        != expected.get("executable_sha256")
    ):
        raise GateFailure("daemon wrapper installed-asset identity differs")
    return {
        "manifest_descriptor_sha256": installer.daemon_wrapper_digest(expected),
        "launch_agent_main_bound": True,
        "wrapper_main_installed_asset_count": 1,
        "descriptor_only_assets_excluded": True,
    }


def protected_store_snapshot(paths: Any) -> str:
    expectations = manifest_credential_expectations(paths)
    documents = []
    for case in CASES:
        document = host_credential_gate(paths, "inspect", case.account_id)
        assert document is not None
        validate_host_credential_metadata(
            document,
            case.account_id,
            expected=expectations[case.account_id],
        )
        documents.append(document)
    encoded = json.dumps(documents, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def verify_protected_store_contract_and_conflict(
    paths: Any,
    ownership: CredentialOwnership,
    case: CredentialGateCase,
    sources: tuple[Path, Path],
) -> dict[str, Any]:
    if not ownership.absence_proved:
        raise GateFailure("protected-store absence was not proved")
    if sources != (
        paths.root.parent / "protected-store-conflict" / "credential-1.json",
        paths.root.parent / "protected-store-conflict" / "credential-2.json",
    ):
        raise GateFailure("protected-store conflict sources escaped their finite slots")
    report = host_credential_gate(paths, "prove_conflict")
    created = validate_protected_store_conflict_report(report)
    expected = {
        "provider_account_id": case.provider_account_id,
        "writer_operation_id": case.operation_id,
    }
    validate_host_credential_metadata(created, case.account_id, expected=expected)
    absent = host_credential_gate(paths, "inspect", case.account_id)
    validate_host_credential_metadata(absent, case.account_id, expected=None)
    return {
        "create_contract": "production",
        "readback": "exact_non_secret_metadata",
        "conflict": "already_exists_without_overwrite",
        "cleanup": "same_invocation_created_set_only",
    }


def destination_snapshot(installer: Any, paths: Any) -> str:
    return sql(
        installer,
        paths,
        """
SELECT pg_catalog.json_build_object(
  'accounts',(
    SELECT pg_catalog.json_agg(pg_catalog.json_build_object(
      'account_id',account_id,
      'display_label',display_label,
      'enabled',enabled,
      'revision',revision,
      'provider',provider_kind,
      'provider_account_id',provider_account_id,
      'schema',credential_store_schema_version,
      'version',credential_version,
      'fingerprint',credential_fingerprint,
      'writer',credential_writer_operation_id
    ) ORDER BY account_id) FROM decodex.accounts
  ),
  'operations',(
    SELECT pg_catalog.json_agg(pg_catalog.json_build_object(
      'operation_id',operation_id,
      'account_id',account_id,
      'kind',kind,
      'phase',phase,
      'expected_revision',expected_account_revision,
      'requested_display_label',requested_display_label,
      'requested_enabled',requested_enabled,
      'target_schema',target_store_schema_version,
      'target_version',target_credential_version,
      'target_fingerprint',target_credential_fingerprint,
      'target_writer',target_credential_writer_operation_id,
      'provider_account_id',provider_account_id
    ) ORDER BY operation_id) FROM decodex.account_operations
  ),
  'routing',(
    SELECT pg_catalog.json_build_object(
      'mode',control.mode,
      'fixed',control.fixed_account_id,
      'revision',control.revision,
      'order',pg_catalog.array_agg(ordering.account_id ORDER BY ordering.position)
    )
    FROM decodex.account_routing_control AS control
    LEFT JOIN decodex.account_routing_order AS ordering ON true
    WHERE control.singleton
    GROUP BY control.mode,control.fixed_account_id,control.revision
  ),
  'receipt',(
    SELECT pg_catalog.json_build_object(
      'phase',phase,
      'manifest_sha256',manifest_sha256,
      'account_count',account_count
    ) FROM decodex.account_migration_receipts WHERE singleton
  ),
  'process_generations',(SELECT pg_catalog.count(*) FROM decodex.process_generations),
  'reset_card_outbox',(
    SELECT pg_catalog.count(*) FROM decodex.outbox
    WHERE aggregate_kind='reset_card_operation'
  )
)::text
""",
    )


def expect_entrypoint_failure(
    installer: Any,
    paths: Any,
    namespace_lock: Any,
    command: list[str],
) -> None:
    try:
        installer.run_installer_child(
            command,
            namespace_lock,
            cwd=REPO_ROOT,
            capture=True,
        )
    except subprocess.CalledProcessError:
        return
    raise GateFailure("a conflicting real entrypoint invocation unexpectedly succeeded")


def assert_expected_destination(installer: Any, paths: Any) -> None:
    payload = json.loads(destination_snapshot(installer, paths))
    manifest = json.loads(paths.migration_manifest.read_bytes())
    targets = {
        account["account_id"]: account["target"]
        for account in manifest["accounts"]
    }
    accounts = {row["account_id"]: row for row in payload["accounts"]}
    mismatches = []
    expected_revisions = {
        CASES[0].account_id: 2,
        CASES[1].account_id: 9,
        CASES[2].account_id: 2,
        CASES[3].account_id: 12,
    }
    for case in CASES:
        row = accounts.get(case.account_id)
        if row is None:
            mismatches.append(f"{case.account_id}: absent")
            continue
        target = targets[case.account_id]
        if (
            row["display_label"] != case.display_label
            or row["enabled"] != case.enabled
            or row["revision"] != expected_revisions[case.account_id]
            or row["provider"] != "chatgpt"
            or row["provider_account_id"] != target["provider_account_id"]
            or row["schema"] != target["store_schema_version"]
            or row["version"] != target["credential_version"]
            or row["fingerprint"] != target["fingerprint_sha256"]
            or row["writer"] != target["writer_operation_id"]
        ):
            mismatches.append(f"{case.account_id}: projection or revision")
    operations = payload["operations"]
    if (
        len(operations) != len(CASES)
        or any(operation["phase"] != "committed" for operation in operations)
    ):
        mismatches.append("operation journal")
    else:
        operations_by_account = {
            operation["account_id"]: operation for operation in operations
        }
        for case in CASES:
            operation = operations_by_account.get(case.account_id)
            target = targets[case.account_id]
            expected_revision = case.v26_revision
            expected_label = (
                case.display_label
                if case.v26_revision is None
                else case.v26_label
            )
            expected_enabled = case.enabled if case.v26_revision is None else False
            if (
                operation is None
                or operation["operation_id"] != case.operation_id
                or operation["kind"] != "import"
                or operation["expected_revision"] != expected_revision
                or operation["requested_display_label"] != expected_label
                or operation["requested_enabled"] != expected_enabled
                or operation["target_schema"] != target["store_schema_version"]
                or operation["target_version"] != target["credential_version"]
                or operation["target_fingerprint"] != target["fingerprint_sha256"]
                or operation["target_writer"] != target["writer_operation_id"]
                or operation["provider_account_id"] != target["provider_account_id"]
            ):
                mismatches.append(f"{case.account_id}: immutable operation descriptor")
    if (
        payload["routing"]["mode"] != "balanced"
        or payload["routing"]["revision"] != 2
        or payload["routing"]["order"] != [case.account_id for case in CASES]
    ):
        mismatches.append("routing")
    if payload["receipt"]["phase"] != "prepared":
        mismatches.append("prepared receipt")
    if payload["process_generations"] != 0:
        mismatches.append("process generation")
    if payload["reset_card_outbox"] != 0:
        mismatches.append("Reset Card outbox")
    if mismatches:
        raise GateFailure("destination mismatches: " + ", ".join(mismatches))


def mutate_manifest_digest(installer: Any, paths: Any) -> Path:
    payload = json.loads(paths.migration_manifest.read_bytes())
    payload["accounts"][0]["display_label"] = "Different Digest"
    payload["decision_fingerprints"]["labels_sha256"] = installer.decision_digest(
        [
            {
                "account_id": account["account_id"],
                "display_label": account["display_label"],
            }
            for account in payload["accounts"]
        ]
    )
    path = paths.root / "different-account-migration-manifest.json"
    private_write(
        path,
        (
            json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n"
        ).encode(),
    )
    return path


def mutate_manifest_store_binding(
    installer: Any,
    paths: Any,
    field: str,
    value: str,
) -> Path:
    payload = json.loads(paths.migration_manifest.read_bytes())
    payload["accounts"][0]["target"][field] = value
    payload["decision_fingerprints"]["credentials_sha256"] = (
        installer.decision_digest(
            [
                {
                    "account_id": account["account_id"],
                    "credential_source_sha256": account[
                        "credential_source_sha256"
                    ],
                    "target": account["target"],
                }
                for account in payload["accounts"]
            ]
        )
    )
    path = paths.root / f"different-{field}-account-migration-manifest.json"
    private_write(
        path,
        (
            json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n"
        ).encode(),
    )
    return path


def remove_operation_for_positive_binding(
    installer: Any,
    paths: Any,
    operation_id: str,
) -> str:
    document = sql(
        installer,
        paths,
        "SELECT pg_catalog.row_to_json(operation)::text "
        "FROM decodex.account_operations AS operation "
        f"WHERE operation_id='{operation_id}'",
    )
    if not document:
        raise GateFailure("matching-positive fixture operation is absent")
    sql(
        installer,
        paths,
        "DELETE FROM decodex.account_operations "
        f"WHERE operation_id='{operation_id}'",
    )
    return document


def restore_operation(
    installer: Any,
    paths: Any,
    document: str,
) -> None:
    literal = document.replace("'", "''")
    sql(
        installer,
        paths,
        "INSERT INTO decodex.account_operations "
        "SELECT restored.* FROM pg_catalog.json_populate_record("
        "NULL::decodex.account_operations,"
        f"'{literal}'::json) AS restored",
    )


def verify_replay_and_drift(
    installer: Any,
    paths: Any,
    namespace_lock: Any,
) -> None:
    before = destination_snapshot(installer, paths)
    keychain_before = protected_store_snapshot(paths)
    replay = installer.run_offline_account_migration(paths, namespace_lock)
    if replay["intent_recorded"] or replay["receipt_completed"]:
        raise GateFailure("same-digest replay reported a new durable transition")
    if before != destination_snapshot(installer, paths):
        raise GateFailure("same-digest replay changed PostgreSQL state")
    if keychain_before != protected_store_snapshot(paths):
        raise GateFailure("same-digest replay rewrote a Keychain item")

    positive_case = CASES[2]
    operation_document = remove_operation_for_positive_binding(
        installer,
        paths,
        positive_case.operation_id,
    )
    try:
        positive_before = destination_snapshot(installer, paths)
        positive_keychain = protected_store_snapshot(paths)
        positive_replay = installer.run_offline_account_migration(
            paths,
            namespace_lock,
        )
        if positive_replay["intent_recorded"] or positive_replay["receipt_completed"]:
            raise GateFailure("matching positive binding reported a new transition")
        if positive_before != destination_snapshot(installer, paths):
            raise GateFailure("matching positive binding created an operation or revision")
        if positive_keychain != protected_store_snapshot(paths):
            raise GateFailure("matching positive binding rewrote the Keychain item")
    finally:
        restore_operation(installer, paths, operation_document)
    if before != destination_snapshot(installer, paths):
        raise GateFailure("positive-binding fixture did not restore exact journal state")

    different = mutate_manifest_digest(installer, paths)
    expect_entrypoint_failure(
        installer,
        paths,
        namespace_lock,
        migration_command(paths, different),
    )
    if before != destination_snapshot(installer, paths):
        raise GateFailure("different-digest refusal changed destination state")
    for field, value in (
        ("host_store", "drifted_host_store"),
        ("postgres_projection", "drifted_postgres_projection"),
    ):
        drifted_binding = mutate_manifest_store_binding(
            installer,
            paths,
            field,
            value,
        )
        expect_entrypoint_failure(
            installer,
            paths,
            namespace_lock,
            migration_command(paths, drifted_binding),
        )
        if before != destination_snapshot(installer, paths):
            raise GateFailure(f"{field} refusal changed destination state")

    credential = paths.credential_directory / f"{CASES[0].account_id}.json"
    original_credential = credential.read_bytes()
    credential.write_bytes(original_credential + b" ")
    try:
        expect_entrypoint_failure(
            installer,
            paths,
            namespace_lock,
            migration_command(paths),
        )
    finally:
        credential.write_bytes(original_credential)
        os.chmod(credential, 0o600)

    manifest = json.loads(paths.migration_manifest.read_bytes())
    by_account = {account["account_id"]: account for account in manifest["accounts"]}
    existing = CASES[1]
    target = by_account[existing.account_id]["target"]
    mutations = (
        (
            f"expected_account_revision={existing.v26_revision + 1}",
            f"expected_account_revision={existing.v26_revision}",
        ),
        (
            "requested_display_label='Existing Normal'",
            "requested_display_label='V26 Existing Normal'",
        ),
        ("requested_enabled=true", "requested_enabled=false"),
        (
            "target_credential_version=2",
            f"target_credential_version={target['credential_version']}",
        ),
        (
            "target_credential_fingerprint='" + ("f" * 64) + "'",
            f"target_credential_fingerprint='{target['fingerprint_sha256']}'",
        ),
        (
            "target_credential_writer_operation_id="
            "'14220000-0000-4000-9000-000000000098'",
            "target_credential_writer_operation_id="
            f"'{target['writer_operation_id']}'",
        ),
        (
            "provider_account_id='xy1422-provider-drift'",
            f"provider_account_id='{target['provider_account_id']}'",
        ),
        ("phase='cancelled'", "phase='committed'"),
    )
    drift_failures = []
    for mutation, restore in mutations:
        try:
            sql(
                installer,
                paths,
                "UPDATE decodex.account_operations SET "
                + mutation
                + f" WHERE operation_id='{existing.operation_id}'",
            )
        except BaseException as error:
            drift_failures.append(f"{mutation}: mutation failed: {error}")
            continue
        try:
            try:
                expect_entrypoint_failure(
                    installer,
                    paths,
                    namespace_lock,
                    migration_command(paths),
                )
            except BaseException as error:
                drift_failures.append(f"{mutation}: {error}")
        finally:
            sql(
                installer,
                paths,
                "UPDATE decodex.account_operations SET "
                + restore
                + f" WHERE operation_id='{existing.operation_id}'",
            )
    if drift_failures:
        raise GateFailure("operation drift mismatches: " + "; ".join(drift_failures))
    if before != destination_snapshot(installer, paths):
        raise GateFailure("descriptor or target drift checks changed durable state")


def verify_prepared_current_tuple_drift(
    context: dict[str, Any],
) -> dict[str, Any]:
    installer = context["installer"]
    paths = context["paths"]
    uid = context["identity"].uid
    case = CASES[0]
    namespace_lock = installer.InstallerNamespaceLock.acquire(paths, uid)
    context["namespace_lock"] = namespace_lock
    postgres = start_owned_postgres(installer, paths)
    context["postgres"] = postgres
    try:
        before = sql(
            installer,
            paths,
            "SELECT pg_catalog.row_to_json(account)::text "
            "FROM decodex.accounts AS account "
            f"WHERE account_id='{case.account_id}'",
        )
        mutations = (
            (
                "label",
                "display_label='Prepared Tuple Drift'",
                f"display_label='{case.display_label}'",
            ),
            ("enabled", "enabled=false", "enabled=true"),
            ("revision", "revision=2", "revision=1"),
        )
        failures = []
        for name, mutation, restore in mutations:
            try:
                sql(
                    installer,
                    paths,
                    "UPDATE decodex.accounts SET "
                    + mutation
                    + f" WHERE account_id='{case.account_id}'",
                )
                expect_entrypoint_failure(
                    installer,
                    paths,
                    namespace_lock,
                    migration_command(paths),
                )
                metadata = host_credential_gate(
                    paths,
                    "inspect",
                    case.account_id,
                )
                assert metadata is not None
                validate_host_credential_metadata(
                    metadata,
                    case.account_id,
                    expected=None,
                )
            except BaseException as error:
                failures.append(f"{name}: {type(error).__name__}: {error}")
            finally:
                sql(
                    installer,
                    paths,
                    "UPDATE decodex.accounts SET "
                    + restore
                    + f" WHERE account_id='{case.account_id}'",
                )
        after = sql(
            installer,
            paths,
            "SELECT pg_catalog.row_to_json(account)::text "
            "FROM decodex.accounts AS account "
            f"WHERE account_id='{case.account_id}'",
        )
        if before != after:
            failures.append("prepared account tuple did not restore exactly")
        account_checkpoint_state(
            installer,
            paths,
            case,
            "operation_prepared",
        )
        if failures:
            raise GateFailure(
                "prepared current-tuple drift mismatches: "
                + "; ".join(failures)
            )
        return {
            "current_tuple_conflicts": [name for name, _, _ in mutations],
            "keychain_effects": 0,
            "operation_identity_preserved": True,
        }
    finally:
        stop_owned_postgres(installer, postgres)
        context["postgres"] = None
        namespace_lock.close()
        context["namespace_lock"] = None


def migration_cancel_receipts(
    installer: Any,
    paths: Any,
    run_token: str,
) -> list[dict[str, Any]]:
    keys = [
        f"xy1422-{run_token}-prepared-cancel",
        f"xy1422-{run_token}-recovery_required-cancel",
    ]
    document = sql(
        installer,
        paths,
        "SELECT COALESCE(pg_catalog.json_agg(pg_catalog.json_build_object("
        "'idempotency_key',idempotency_key,"
        "'request_hash',request_hash,"
        "'protocol_version',protocol_version,"
        "'operation',operation,"
        "'entity_id',entity_id,"
        "'expected_revision',expected_revision,"
        "'receipt_state',receipt_state,"
        "'response',response,"
        "'response_bytes_match',"
        "pg_catalog.convert_from(response_bytes,'UTF8')::jsonb=response"
        ") ORDER BY idempotency_key),'[]'::json)::text "
        "FROM decodex.command_receipts WHERE idempotency_key IN ("
        + ",".join(f"'{key}'" for key in keys)
        + ")",
    )
    try:
        receipts = json.loads(document)
    except json.JSONDecodeError as error:
        raise GateFailure("migration cancellation receipts were malformed") from error
    if not isinstance(receipts, list):
        raise GateFailure("migration cancellation receipts were not an array")
    return receipts


def validate_migration_cancel_receipts(
    receipts: list[dict[str, Any]],
    run_token: str,
    operation_id: str,
    expected_revision: int,
    phases: tuple[str, ...] = ("prepared", "recovery_required"),
) -> None:
    expected_keys = {
        f"xy1422-{run_token}-{phase}-cancel"
        for phase in phases
    }
    if {receipt.get("idempotency_key") for receipt in receipts} != expected_keys:
        raise GateFailure("migration cancellation receipt identities differed")
    for receipt in receipts:
        response = receipt.get("response")
        if (
            not isinstance(receipt.get("request_hash"), str)
            or not re.fullmatch(r"[0-9a-f]{64}", receipt["request_hash"])
            or receipt.get("protocol_version") != "decodex/account-command/1"
            or receipt.get("operation") != "recover_account_operation"
            or receipt.get("entity_id") != operation_id
            or receipt.get("expected_revision") != expected_revision
            or receipt.get("receipt_state") != "completed"
            or receipt.get("response_bytes_match") is not True
            or not isinstance(response, dict)
            or response.get("outcome") != "rejected"
            or response.get("data")
            != {
                "schema": "decodex/account-command-result/1",
                "error": {
                    "reason": "account_command_rejected",
                    "rejection": "operation_unsettled",
                },
            }
        ):
            raise GateFailure("migration cancellation receipt semantics differed")


def migration_cancel_boundary_snapshot(installer: Any, paths: Any, case: AccountCase) -> str:
    return sql(
        installer,
        paths,
        "SELECT pg_catalog.json_build_object("
        "'operation',(SELECT pg_catalog.row_to_json(operation) "
        "FROM decodex.account_operations AS operation "
        f"WHERE operation_id='{case.operation_id}'),"
        "'account',(SELECT pg_catalog.row_to_json(account) "
        "FROM decodex.accounts AS account "
        f"WHERE account_id='{case.account_id}'),"
        "'migration_receipt',(SELECT pg_catalog.row_to_json(receipt) "
        "FROM decodex.account_migration_receipts AS receipt WHERE singleton)"
        ")::text",
    )


def verify_prepared_exclusion(
    installer: Any,
    paths: Any,
    run_token: str,
) -> dict[str, Any]:
    case = CASES[0]
    phase = sql(
        installer,
        paths,
        "SELECT phase FROM decodex.account_operations "
        f"WHERE operation_id='{case.operation_id}'",
    )
    revision = sql(
        installer,
        paths,
        "SELECT revision FROM decodex.accounts "
        f"WHERE account_id='{case.account_id}'",
    )
    unsettled = sql(
        installer,
        paths,
        "SELECT pg_catalog.count(*) FROM "
        "decodex.read_unsettled_account_operations_exact(512)",
    )
    if phase != "prepared" or revision != "1" or unsettled != "0":
        raise GateFailure(
            "manifest-bound prepared Import was reclassified or exposed to generic cancellation"
        )
    metadata = host_credential_gate(paths, "inspect", case.account_id)
    assert metadata is not None
    try:
        validate_host_credential_metadata(metadata, case.account_id, expected=None)
    except GateFailure as error:
        raise GateFailure("prepared-before-Keychain barrier already has a credential item")
    before = migration_cancel_boundary_snapshot(installer, paths, case)
    try:
        before_document = json.loads(before)
    except json.JSONDecodeError as error:
        raise GateFailure("prepared cancellation boundary was malformed") from error
    operation_document = before_document.get("operation")
    account_document = before_document.get("account")
    if (
        not isinstance(operation_document, dict)
        or not isinstance(account_document, dict)
        or operation_document.get("phase") != "prepared"
        or account_document.get("revision") != 1
    ):
        raise GateFailure("prepared cancellation boundary identity differed")
    prepared_report = account_migration_recovery_gate(paths, "prepared")
    if migration_cancel_boundary_snapshot(installer, paths, case) != before:
        raise GateFailure("prepared cancellation changed operation, account, or intent")
    receipts = migration_cancel_receipts(installer, paths, run_token)
    validate_migration_cancel_receipts(
        receipts,
        run_token,
        case.operation_id,
        account_document["revision"],
        ("prepared",),
    )
    metadata = host_credential_gate(paths, "inspect", case.account_id)
    validate_host_credential_metadata(metadata, case.account_id, expected=None)
    return {
        "generic_startup_exclusion": True,
        "prepared": prepared_report,
        "typed_refusal_receipts": len(receipts),
        "operation_revision_and_intent_preserved": True,
        "keychain_present": False,
    }


def verify_recovery_required_exclusion(
    context: dict[str, Any],
) -> dict[str, Any]:
    installer = context["installer"]
    paths = context["paths"]
    case = CASES[0]
    namespace_lock = installer.InstallerNamespaceLock.acquire(
        paths,
        context["identity"].uid,
    )
    context["namespace_lock"] = namespace_lock
    postgres = None
    staging_created = False
    try:
        if paths.staging_config.exists():
            raise GateFailure(
                "failed migration retained its staging configuration"
            )
        private_write(
            paths.staging_config,
            installer.render_config(
                paths,
                context["identity"].uid,
            ).encode("utf-8"),
        )
        staging_created = True
        postgres = start_owned_postgres(installer, paths)
        context["postgres"] = postgres
        unsettled = sql(
            installer,
            paths,
            "SELECT pg_catalog.count(*) FROM "
            "decodex.read_unsettled_account_operations_exact(512)",
        )
        before = migration_cancel_boundary_snapshot(installer, paths, case)
        try:
            boundary = json.loads(before)
        except json.JSONDecodeError as error:
            raise GateFailure(
                "recovery-required cancellation boundary was malformed"
            ) from error
        operation = boundary.get("operation")
        account = boundary.get("account")
        receipt = boundary.get("migration_receipt")
        if (
            unsettled != "0"
            or not isinstance(operation, dict)
            or operation.get("phase") != "recovery_required"
            or not isinstance(account, dict)
            or account.get("revision") != 1
            or not isinstance(receipt, dict)
            or receipt.get("phase") != "prepared"
        ):
            raise GateFailure(
                "manifest-bound recovery-required Import was reclassified"
            )
        metadata = host_credential_gate(paths, "inspect", case.account_id)
        validate_host_credential_metadata(
            metadata,
            case.account_id,
            expected=None,
        )
        report = account_migration_recovery_gate(paths, "recovery_required")
        if migration_cancel_boundary_snapshot(installer, paths, case) != before:
            raise GateFailure(
                "recovery-required cancellation changed operation, account, or intent"
            )
        receipts = migration_cancel_receipts(
            installer,
            paths,
            context["run_token"],
        )
        validate_migration_cancel_receipts(
            receipts,
            context["run_token"],
            case.operation_id,
            account["revision"],
        )
        return {
            "generic_startup_exclusion": True,
            "recovery_required": report,
            "typed_refusal_receipts": len(receipts),
            "operation_revision_and_intent_preserved": True,
            "keychain_present": False,
        }
    finally:
        if postgres is not None:
            stop_owned_postgres(installer, postgres)
            context["postgres"] = None
        if staging_created:
            paths.staging_config.unlink(missing_ok=True)
        namespace_lock.close()
        context["namespace_lock"] = None


def require_refusal_without_fixture_change(
    paths: Any,
    action: Callable[[], subprocess.CompletedProcess[bytes]],
    failure: str,
) -> subprocess.CompletedProcess[bytes]:
    before = fixture_tree_snapshot(paths.root.parent)
    completed = action()
    if completed.returncode == 0:
        raise GateFailure(failure)
    if before != fixture_tree_snapshot(paths.root.parent):
        raise GateFailure("refused child invocation changed fixture state")
    return completed


def direct_invocation_refusal_checks(installer: Any, paths: Any) -> dict[str, Any]:
    direct_commands = [
        migration_command(paths),
        prepared_verifier_command(paths),
        finalizer_command(installer, paths),
        completed_verifier_command(installer, paths),
    ]
    for command in direct_commands:
        require_refusal_without_fixture_change(
            paths,
            lambda command=command: bounded_run(
                command,
                cwd=REPO_ROOT,
                check=False,
                timeout=30,
            ),
            "a direct installer-only invocation succeeded",
        )
    return {"entrypoints_refused": len(direct_commands)}


def malformed_descriptor_refusal(paths: Any) -> dict[str, Any]:
    require_refusal_without_fixture_change(
        paths,
        lambda: bounded_run(
            [*migration_command(paths), "--installer-lock-fd", "99"],
            cwd=REPO_ROOT,
            check=False,
            timeout=30,
        ),
        "a malformed installer descriptor succeeded",
    )
    return {"malformed_descriptor_refused": True}


def wrong_identity_descriptor_refusal(paths: Any) -> dict[str, Any]:
    wrong_file = paths.root / "wrong-installer-lock"
    private_write(wrong_file, b"wrong\n")
    descriptor = os.open(wrong_file, os.O_RDWR)
    try:
        require_refusal_without_fixture_change(
            paths,
            lambda: bounded_run(
                [
                    *migration_command(paths),
                    "--installer-lock-fd",
                    str(descriptor),
                ],
                cwd=REPO_ROOT,
                pass_fds=(descriptor,),
                check=False,
                timeout=30,
            ),
            "a wrong-identity installer descriptor succeeded",
        )
    finally:
        os.close(descriptor)
    return {"wrong_identity_refused": True}


def aliased_descriptor_refusal(
    installer: Any,
    paths: Any,
    uid: int,
) -> dict[str, Any]:
    namespace_lock: Any = None
    descriptor: int | None = None
    result: dict[str, Any] | None = None
    primary_error: BaseException | None = None
    try:
        namespace_lock = installer.InstallerNamespaceLock.acquire(paths, uid)
        before = fixture_tree_snapshot(paths.root.parent)
        descriptor = namespace_lock.borrow()
        completed = bounded_run(
            [
                *migration_command(paths),
                "--installer-lock-fd",
                str(descriptor),
                "--transition-gate-fd",
                str(descriptor),
            ],
            cwd=REPO_ROOT,
            pass_fds=(descriptor,),
            check=False,
            timeout=30,
        )
        if completed.returncode == 0:
            raise GateFailure("aliased installer and transition descriptors succeeded")
        if before != fixture_tree_snapshot(paths.root.parent):
            raise GateFailure("aliased descriptor refusal changed fixture state")
        result = {
            "same_valid_descriptor_refused": True,
            "fixture_unchanged": True,
            "fresh_lock_acquired": True,
        }
    except BaseException as error:
        primary_error = error

    cleanup_error: BaseException | None = None
    if descriptor is not None:
        try:
            os.close(descriptor)
        except BaseException as error:
            cleanup_error = error
    if namespace_lock is not None:
        try:
            namespace_lock.close()
        except BaseException as error:
            cleanup_error = cleanup_error or error
        try:
            assert_reacquirable(paths.namespace_lock)
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if primary_error is not None:
        if cleanup_error is not None:
            raise primary_error from cleanup_error
        raise primary_error
    if cleanup_error is not None:
        raise cleanup_error
    if result is None:
        raise GateFailure("aliased descriptor refusal produced no result")
    return result


def pre_spawn_cleanup_fault(
    installer: Any,
    paths: Any,
    uid: int,
    fault_point: str,
) -> dict[str, Any]:
    allowed = {
        "lock_acquisition",
        "lock_duplicate",
        "socket_creation",
        "transition_gate_duplicate",
    }
    if fault_point not in allowed:
        raise GateFailure("pre-spawn cleanup fault point is invalid")
    namespace_lock: Any = None
    parent: socket.socket | None = None
    child: socket.socket | None = None
    lock_descriptor: int | None = None
    gate_descriptor: int | None = None
    injected_error = GateFailure(f"injected cleanup fault after {fault_point}")
    primary_error: BaseException | None = None
    injected = False
    try:
        namespace_lock = installer.InstallerNamespaceLock.acquire(paths, uid)
        if fault_point == "lock_acquisition":
            injected = True
            raise injected_error
        lock_descriptor = namespace_lock.borrow()
        if fault_point == "lock_duplicate":
            injected = True
            raise injected_error
        parent, child = socket.socketpair()
        if fault_point == "socket_creation":
            injected = True
            raise injected_error
        gate_descriptor = os.dup(child.fileno())
        injected = True
        raise injected_error
    except BaseException as error:
        primary_error = error

    cleanup_error: BaseException | None = None
    for descriptor in (gate_descriptor, lock_descriptor):
        if descriptor is None:
            continue
        try:
            os.close(descriptor)
        except BaseException as error:
            cleanup_error = cleanup_error or error
    for connection in (child, parent):
        if connection is None:
            continue
        try:
            connection.close()
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if namespace_lock is not None:
        try:
            namespace_lock.close()
        except BaseException as error:
            cleanup_error = cleanup_error or error
        try:
            assert_reacquirable(paths.namespace_lock)
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if not injected or primary_error is not injected_error:
        if primary_error is not None and cleanup_error is not None:
            raise primary_error from cleanup_error
        if primary_error is not None:
            raise primary_error
        raise GateFailure("pre-spawn cleanup fault did not execute")
    if cleanup_error is not None:
        raise injected_error from cleanup_error
    return {
        "fault_point": fault_point,
        "primary_failure_preserved": True,
        "resources_closed": True,
        "fresh_lock_acquired": True,
    }


def post_spawn_cleanup_fault(
    installer: Any,
    paths: Any,
    uid: int,
    fault_point: str,
) -> dict[str, Any]:
    if fault_point not in {"spawn", "child_identity_capture"}:
        raise GateFailure("post-spawn cleanup fault point is invalid")
    namespace_lock: Any = None
    parent: socket.socket | None = None
    child: socket.socket | None = None
    lock_descriptor: int | None = None
    gate_descriptor: int | None = None
    process: subprocess.Popen[Any] | None = None
    identity: Any = None
    injected_error = GateFailure(f"injected cleanup fault after {fault_point}")
    primary_error: BaseException | None = None
    injected = False
    try:
        namespace_lock = installer.InstallerNamespaceLock.acquire(paths, uid)
        parent, child = socket.socketpair()
        lock_descriptor = namespace_lock.borrow()
        gate_descriptor = os.dup(child.fileno())
        command = [
            *migration_command(paths),
            "--installer-lock-fd",
            str(lock_descriptor),
            "--transition-gate-fd",
            str(gate_descriptor),
        ]
        process = subprocess.Popen(
            command,
            cwd=REPO_ROOT,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            close_fds=True,
            pass_fds=(lock_descriptor, gate_descriptor),
            start_new_session=True,
        )
        os.close(lock_descriptor)
        lock_descriptor = None
        os.close(gate_descriptor)
        gate_descriptor = None
        child.close()
        child = None
        if fault_point == "spawn":
            injected = True
            raise injected_error
        identity = capture_process_identity(installer, process.pid)
        injected = True
        raise injected_error
    except BaseException as error:
        primary_error = error

    cleanup_error: BaseException | None = None
    for descriptor in (gate_descriptor, lock_descriptor):
        if descriptor is None:
            continue
        try:
            os.close(descriptor)
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if child is not None:
        try:
            child.close()
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if process is not None and process.poll() is None:
        try:
            if identity is None:
                terminate_direct_child_without_identity(process)
            else:
                terminate_owned_process(installer, process, identity)
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if process is not None:
        for stream in (process.stdout, process.stderr):
            if stream is None or stream.closed:
                continue
            try:
                stream.close()
            except BaseException as error:
                cleanup_error = cleanup_error or error
    if parent is not None:
        try:
            parent.close()
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if namespace_lock is not None:
        try:
            namespace_lock.close()
        except BaseException as error:
            cleanup_error = cleanup_error or error
        try:
            assert_reacquirable(paths.namespace_lock)
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if not injected or primary_error is not injected_error:
        if primary_error is not None and cleanup_error is not None:
            raise primary_error from cleanup_error
        if primary_error is not None:
            raise primary_error
        raise GateFailure("post-spawn cleanup fault did not execute")
    if cleanup_error is not None:
        raise injected_error from cleanup_error
    return {
        "fault_point": fault_point,
        "primary_failure_preserved": True,
        "owned_child_reaped": True,
        "resources_closed": True,
        "fresh_lock_acquired": True,
    }


def valid_descriptor_cloexec_refusal(
    installer: Any,
    paths: Any,
    uid: int,
) -> dict[str, Any]:
    namespace_lock: Any = None
    parent: socket.socket | None = None
    child: socket.socket | None = None
    lock_descriptor: int | None = None
    gate_descriptor: int | None = None
    process: subprocess.Popen[Any] | None = None
    identity: Any = None
    result: dict[str, Any] | None = None
    primary_error: BaseException | None = None
    command: list[str] = []
    try:
        namespace_lock = installer.InstallerNamespaceLock.acquire(paths, uid)
        before = fixture_tree_snapshot(paths.root.parent)
        parent, child = socket.socketpair()
        gate = GateSocket(parent)
        lock_descriptor = namespace_lock.borrow()
        gate_descriptor = os.dup(child.fileno())
        command = [
            *migration_command(paths),
            "--installer-lock-fd",
            str(lock_descriptor),
            "--transition-gate-fd",
            str(gate_descriptor),
        ]
        process = subprocess.Popen(
            command,
            cwd=REPO_ROOT,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            close_fds=True,
            pass_fds=(lock_descriptor, gate_descriptor),
            start_new_session=True,
        )
        os.close(lock_descriptor)
        lock_descriptor = None
        os.close(gate_descriptor)
        gate_descriptor = None
        child.close()
        child = None
        identity = capture_process_identity(installer, process.pid)
        event = gate.next_event(lambda: process is not None and process.poll() is None)
        if event is None or event.partition("|")[0] != "installer_lock_cloexec_verified":
            raise GateFailure("valid borrowed descriptor did not reach the CLOEXEC proof")
        assert_contended(paths.namespace_lock)
        gate.continue_child()
        stdout, stderr = communicate_bounded_subprocess(process, command, 30)
        if process.returncode == 0:
            raise GateFailure("CLOEXEC refusal child unexpectedly completed migration")
        if stdout:
            raise GateFailure("CLOEXEC refusal child emitted an unexpected report")
        if before != fixture_tree_snapshot(paths.root.parent):
            raise GateFailure("CLOEXEC refusal child changed fixture state")
        result = {
            "borrowed_descriptor_validated": True,
            "descendant_descriptor_excluded": True,
            "post_proof_missing_config_refused": True,
            "stderr_bounded": len(stderr),
        }
    except BaseException as error:
        primary_error = error

    cleanup_error: BaseException | None = None
    for descriptor in (gate_descriptor, lock_descriptor):
        if descriptor is None:
            continue
        try:
            os.close(descriptor)
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if child is not None:
        try:
            child.close()
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if process is not None and process.poll() is None:
        try:
            if identity is None:
                terminate_direct_child_without_identity(process)
            else:
                terminate_owned_process(installer, process, identity)
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if process is not None:
        for stream in (process.stdout, process.stderr):
            if stream is None or stream.closed:
                continue
            try:
                stream.close()
            except BaseException as error:
                cleanup_error = cleanup_error or error
    if parent is not None:
        try:
            parent.close()
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if namespace_lock is not None:
        try:
            namespace_lock.close()
        except BaseException as error:
            cleanup_error = cleanup_error or error
        try:
            assert_reacquirable(paths.namespace_lock)
        except BaseException as error:
            cleanup_error = cleanup_error or error
    if primary_error is not None:
        if cleanup_error is not None:
            raise primary_error from cleanup_error
        raise primary_error
    if cleanup_error is not None:
        raise cleanup_error
    if result is None:
        raise GateFailure("CLOEXEC refusal produced no result")
    return result


def path_drift_descriptor_refusal(
    paths: Any,
    namespace_lock: Any,
) -> dict[str, Any]:
    before = fixture_tree_snapshot(paths.root.parent)
    borrowed = namespace_lock.borrow()
    original_path = paths.namespace_lock.with_name("decodex.lock.xy1422-original")
    replacement_descriptor = None
    original_moved = False
    try:
        os.rename(paths.namespace_lock, original_path)
        original_moved = True
        replacement_descriptor = os.open(
            paths.namespace_lock,
            os.O_RDWR
            | os.O_CREAT
            | os.O_EXCL
            | getattr(os, "O_NOFOLLOW", 0)
            | getattr(os, "O_CLOEXEC", 0),
            0o600,
        )
        os.close(replacement_descriptor)
        replacement_descriptor = None
        path_drift = bounded_run(
            [
                *migration_command(paths),
                "--installer-lock-fd",
                str(borrowed),
            ],
            cwd=REPO_ROOT,
            pass_fds=(borrowed,),
            check=False,
            timeout=30,
        )
    finally:
        if replacement_descriptor is not None:
            os.close(replacement_descriptor)
        if original_moved:
            try:
                paths.namespace_lock.unlink(missing_ok=True)
            finally:
                os.rename(original_path, paths.namespace_lock)
        os.close(borrowed)
    namespace_lock.verify()
    if path_drift.returncode == 0:
        raise GateFailure("a path-drifted installer descriptor succeeded")
    if before != fixture_tree_snapshot(paths.root.parent):
        raise GateFailure("path-drift refusal changed fixture state")
    return {"path_identity_drift_refused": True}


def spawn_installer_worker(
    installer: Any,
    paths: Any,
    uid: int,
) -> tuple[int, Any, GateSocket]:
    parent: socket.socket | None = None
    child: socket.socket | None = None
    try:
        parent, child = socket.socketpair()
        pid = os.fork()
    except BaseException as primary_error:
        cleanup_error: BaseException | None = None
        for connection in (child, parent):
            if connection is None:
                continue
            try:
                connection.close()
            except BaseException as error:
                cleanup_error = cleanup_error or error
        try:
            assert_reacquirable(paths.namespace_lock)
        except BaseException as error:
            cleanup_error = cleanup_error or error
        if cleanup_error is not None:
            raise primary_error from cleanup_error
        raise
    if pid == 0:
        assert parent is not None
        assert child is not None
        parent.close()
        namespace_lock = None
        try:
            namespace_lock = installer.InstallerNamespaceLock.acquire(paths, uid)

            def checkpoint(name: str) -> None:
                child.sendall(f"installer_{name}|-\n".encode())
                if child.recv(1) != b"c":
                    os._exit(91)

            installer.install_under_namespace_lock(
                paths,
                uid,
                namespace_lock,
                launch_requested=False,
                transition_checkpoint=checkpoint,
                transition_gate_fd=child.fileno(),
            )
            os._exit(0)
        except BaseException as error:
            try:
                child.sendall(
                    f"worker_error_{type(error).__name__}|-\n".encode("ascii")
                )
                if child.recv(1) != b"c":
                    os._exit(93)
            except OSError:
                pass
            os._exit(92)
        finally:
            if namespace_lock is not None:
                namespace_lock.close()
    assert parent is not None
    assert child is not None
    child.close()
    try:
        identity = capture_process_identity(installer, pid)
    except BaseException as primary_error:
        cleanup_error: BaseException | None = None
        try:
            parent.close()
        except BaseException as error:
            cleanup_error = error
        try:
            wait_owned_worker(pid, time.monotonic() + GATE_TIMEOUT_SECONDS)
        except BaseException as error:
            cleanup_error = cleanup_error or error
        try:
            assert_reacquirable(paths.namespace_lock)
        except BaseException as error:
            cleanup_error = cleanup_error or error
        if cleanup_error is not None:
            raise primary_error from cleanup_error
        raise
    return pid, identity, GateSocket(parent)


def wait_worker_checkpoint(
    installer: Any,
    pid: int,
    identity: Any,
    gate: GateSocket,
    paths: Any,
    target: str,
    target_account_id: str | None = None,
) -> list[str]:
    seen = []
    while True:
        event = gate.next_event(lambda: exact_process_is_live(installer, identity))
        if event is None:
            raise GateFailure(f"installer exited before checkpoint {target}")
        if event.startswith("worker_error_"):
            failure = event.partition("|")[0]
            raise GateFailure(
                "installer worker failed before its target checkpoint: "
                f"{failure}; seen={','.join(seen)}"
            )
        assert_contended(paths.namespace_lock)
        event_name, separator, event_account_id = event.partition("|")
        seen.append(event_name)
        if (
            event_name == target
            and (
                target_account_id is None
                or (
                    separator == "|"
                    and event_account_id == target_account_id
                )
            )
        ):
            if "installer_lock_cloexec_verified" not in seen:
                raise GateFailure("installer child effect preceded the CLOEXEC proof")
            if target in {
                "installer_staging_retired",
                "installer_active_legacy_retired",
            } and "prepared_destination_verified" not in seen:
                raise GateFailure(
                    "prepared destination was not reverified before retirement"
                )
            return seen
        gate.continue_child()


def wait_worker_success(
    installer: Any,
    pid: int,
    identity: Any,
    gate: GateSocket,
    paths: Any,
) -> list[str]:
    seen = []
    while True:
        event = gate.next_event(lambda: exact_process_is_live(installer, identity))
        if event is None:
            break
        if event.startswith("worker_error_"):
            raise GateFailure("completed installer worker reported an error")
        assert_contended(paths.namespace_lock)
        seen.append(event.partition("|")[0])
        gate.continue_child()
    _, status = os.waitpid(pid, 0)
    if not os.WIFEXITED(status) or os.WEXITSTATUS(status) != 0:
        raise GateFailure("completed installer worker did not exit successfully")
    return seen


def find_decodexd_child(installer: Any, parent_identity: Any) -> Any:
    processes = installer.process_parent_map(time.monotonic() + 5)
    generation = installer.process_generation(parent_identity.process_id, processes)
    completed = bounded_run(
        ["/bin/ps", "-axo", "pid=,ppid=,comm="],
        check=True,
        timeout=10,
    )
    command_by_pid = {}
    for line in completed.stdout.decode("utf-8", errors="strict").splitlines():
        fields = line.split(None, 2)
        if len(fields) == 3 and fields[0].isascii() and fields[0].isdecimal():
            command_by_pid[int(fields[0])] = Path(fields[2]).name
    for identity in generation:
        if (
            identity.process_id != parent_identity.process_id
            and command_by_pid.get(identity.process_id) == "decodexd"
        ):
            return identity
    raise GateFailure("blocked installer migration child was not discoverable")


def wait_owned_worker(pid: int, deadline: float) -> int:
    while True:
        try:
            completed_pid, status = os.waitpid(pid, os.WNOHANG)
        except ChildProcessError:
            return 0
        if completed_pid == pid:
            return status
        if time.monotonic() >= deadline:
            raise GateFailure("installer worker did not exit within its bound")
        time.sleep(0.02)


def captured_process_generation(installer: Any, root_identity: Any) -> set[Any]:
    processes = installer.process_parent_map(time.monotonic() + 5)
    root = processes.get(root_identity.process_id)
    if root is None or root.identity != root_identity:
        return set()
    return set(installer.process_generation(root_identity.process_id, processes))


def cleanup_captured_generation(
    installer: Any,
    identities: set[Any],
    *,
    exclude: set[Any] | None = None,
) -> None:
    remaining = set(identities) - (exclude or set())
    for identity in sorted(
        remaining,
        key=lambda value: value.process_id,
        reverse=True,
    ):
        signal_exact_process(installer, identity, signal.SIGTERM)
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        live = {identity for identity in remaining if exact_process_is_live(installer, identity)}
        if not live:
            return
        time.sleep(0.05)
    for identity in remaining:
        signal_exact_process(installer, identity, signal.SIGKILL)
    installer.wait_for_process_generation_exit(
        remaining,
        time.monotonic() + 10,
    )


def kill_worker(
    installer: Any,
    pid: int,
    identity: Any,
    gate: GateSocket,
) -> None:
    generation = captured_process_generation(installer, identity)
    try:
        signal_exact_process(installer, identity, signal.SIGKILL)
        wait_owned_worker(pid, time.monotonic() + 10)
        cleanup_captured_generation(installer, generation, exclude={identity})
    finally:
        gate.connection.close()


def crash_installer_at(
    installer: Any,
    paths: Any,
    uid: int,
    checkpoint: str,
    *,
    target_account_id: str | None = None,
    at_checkpoint: Callable[[], dict[str, Any] | None] | None = None,
) -> dict[str, Any]:
    pid, identity, gate = spawn_installer_worker(installer, paths, uid)
    cleaned = False
    evidence = None
    try:
        seen = wait_worker_checkpoint(
            installer,
            pid,
            identity,
            gate,
            paths,
            checkpoint,
            target_account_id,
        )
        if at_checkpoint is not None:
            evidence = at_checkpoint()
        kill_worker(installer, pid, identity, gate)
        cleaned = True
        wait_reacquirable(paths.namespace_lock)
        return {
            "checkpoint": checkpoint,
            "observed": seen,
            "checkpoint_evidence": evidence,
        }
    finally:
        if not cleaned:
            kill_worker(installer, pid, identity, gate)
            wait_reacquirable(paths.namespace_lock)


def create_recovery_required_from_prewrite_unavailable(
    installer: Any,
    paths: Any,
    uid: int,
) -> dict[str, Any]:
    case = CASES[0]
    credential_lock = (
        paths.server_directory / "account-credential-store.lock"
    )
    pid, identity, gate = spawn_installer_worker(installer, paths, uid)
    cleaned = False
    credential_lock_descriptor: int | None = None
    original_mode: int | None = None
    try:
        seen = wait_worker_checkpoint(
            installer,
            pid,
            identity,
            gate,
            paths,
            "operation_prepared",
            case.account_id,
        )
        account_checkpoint_state(
            installer,
            paths,
            case,
            "operation_prepared",
        )
        metadata = host_credential_gate(
            paths,
            "inspect",
            case.account_id,
        )
        validate_host_credential_metadata(
            metadata,
            case.account_id,
            expected=None,
        )
        credential_lock_descriptor = os.open(
            credential_lock,
            os.O_RDWR
            | getattr(os, "O_NOFOLLOW", 0)
            | getattr(os, "O_CLOEXEC", 0),
        )
        lock_metadata = os.fstat(credential_lock_descriptor)
        original_mode = stat.S_IMODE(lock_metadata.st_mode)
        if (
            not stat.S_ISREG(lock_metadata.st_mode)
            or lock_metadata.st_uid != uid
            or lock_metadata.st_nlink != 1
            or original_mode != 0o600
        ):
            raise GateFailure(
                "run-owned credential lock precondition differed"
            )
        os.fchmod(credential_lock_descriptor, 0o640)
        gate.continue_child()
        event = gate.next_event(
            lambda: exact_process_is_live(installer, identity)
        )
        if event is None or not event.startswith("worker_error_"):
            raise GateFailure(
                "pre-write credential unavailability did not fail migration"
            )
        assert_contended(paths.namespace_lock)
        os.fchmod(credential_lock_descriptor, original_mode)
        original_mode = None
        os.close(credential_lock_descriptor)
        credential_lock_descriptor = None
        gate.continue_child()
        status = wait_owned_worker(
            pid,
            time.monotonic() + GATE_TIMEOUT_SECONDS,
        )
        if (
            not os.WIFEXITED(status)
            or os.WEXITSTATUS(status) != 92
        ):
            raise GateFailure(
                "pre-write credential failure worker exit differed"
            )
        gate.connection.close()
        cleaned = True
        wait_reacquirable(paths.namespace_lock)
        return {
            "checkpoint": "operation_prepared",
            "observed": seen,
            "production_failure": event.partition("|")[0],
            "credential_write": "not_attempted",
            "expected_phase": "recovery_required",
        }
    finally:
        if original_mode is not None and credential_lock_descriptor is not None:
            os.fchmod(credential_lock_descriptor, original_mode)
        if credential_lock_descriptor is not None:
            os.close(credential_lock_descriptor)
        if not cleaned:
            kill_worker(installer, pid, identity, gate)
            wait_reacquirable(paths.namespace_lock)


def account_checkpoint_state(
    installer: Any,
    paths: Any,
    case: AccountCase,
    checkpoint: str,
) -> dict[str, Any]:
    document = sql(
        installer,
        paths,
        "SELECT pg_catalog.json_build_object("
        "'phase',operation.phase,"
        "'revision',account.revision,"
        "'enabled',account.enabled,"
        "'label',account.display_label"
        ")::text FROM decodex.account_operations AS operation "
        "JOIN decodex.accounts AS account USING (account_id) "
        f"WHERE operation.operation_id='{case.operation_id}'",
    )
    try:
        state = json.loads(document)
    except json.JSONDecodeError as error:
        raise GateFailure("account checkpoint state was malformed") from error
    start_revision = case.v26_revision
    if checkpoint == "operation_prepared":
        expected_phase = "prepared"
        expected_revision = start_revision or 1
    elif checkpoint == "keychain_applied":
        expected_phase = (
            "recovery_required"
            if case == CASES[0]
            else "prepared"
        )
        expected_revision = start_revision or 1
    elif checkpoint == "store_applied":
        expected_phase = "store_applied"
        expected_revision = start_revision or 1
    elif checkpoint == "credential_committed":
        expected_phase = "committed"
        expected_revision = (start_revision + 1) if start_revision is not None else 2
    elif checkpoint == "administration_applied":
        expected_phase = "committed"
        expected_revision = (
            2
            if start_revision is None
            else start_revision
            + 1
            + int(case.v26_label != case.display_label or case.enabled)
        )
    else:
        raise GateFailure("unsupported account checkpoint")
    if (
        state.get("phase") != expected_phase
        or state.get("revision") != expected_revision
    ):
        raise GateFailure(f"{checkpoint} account revision or phase differs")
    if checkpoint == "administration_applied" and (
        state.get("enabled") is not case.enabled
        or state.get("label") != case.display_label
    ):
        raise GateFailure("final account administration differs")
    return {
        "account_id": case.account_id,
        "phase": state["phase"],
        "revision": state["revision"],
    }


def crash_installer_with_surviving_child(
    installer: Any,
    paths: Any,
    uid: int,
    checkpoint: str,
) -> dict[str, Any]:
    pid, identity, gate = spawn_installer_worker(installer, paths, uid)
    generation: set[Any] = set()
    child_identity = None
    worker_reaped = False
    try:
        seen = wait_worker_checkpoint(
            installer,
            pid,
            identity,
            gate,
            paths,
            checkpoint,
        )
        generation = captured_process_generation(installer, identity)
        child_identity = find_decodexd_child(installer, identity)
        if not signal_exact_process(installer, identity, signal.SIGKILL):
            raise GateFailure("installer process identity changed before lineage crash")
        wait_owned_worker(pid, time.monotonic() + 10)
        worker_reaped = True
        assert_contended(paths.namespace_lock)
        if not exact_process_is_live(installer, child_identity):
            raise GateFailure(
                "surviving child did not retain the installer lock descriptor"
            )
        cleanup_captured_generation(
            installer,
            generation,
            exclude={identity},
        )
        wait_reacquirable(paths.namespace_lock)
        return {
            "checkpoint": checkpoint,
            "observed": seen,
            "surviving_child_start_identity": child_identity.started_at,
            "final_holder_released": True,
        }
    finally:
        cleanup_captured_generation(
            installer,
            generation,
            exclude={identity},
        )
        gate.connection.close()
        if not worker_reaped:
            if exact_process_is_live(installer, identity):
                signal_exact_process(installer, identity, signal.SIGKILL)
            wait_owned_worker(pid, time.monotonic() + 10)
        wait_reacquirable(paths.namespace_lock)


def crash_migration_child_with_surviving_installer(
    installer: Any,
    paths: Any,
    uid: int,
) -> dict[str, Any]:
    pid, identity, gate = spawn_installer_worker(installer, paths, uid)
    generation: set[Any] = set()
    child_identity = None
    worker_reaped = False
    try:
        seen = wait_worker_checkpoint(
            installer,
            pid,
            identity,
            gate,
            paths,
            "manifest_frozen",
        )
        generation = captured_process_generation(installer, identity)
        child_identity = find_decodexd_child(installer, identity)
        if not signal_exact_process(installer, child_identity, signal.SIGKILL):
            raise GateFailure("migration child identity changed before lineage crash")
        deadline = time.monotonic() + 10
        while exact_process_is_live(installer, child_identity):
            if time.monotonic() >= deadline:
                raise GateFailure("migration child remained live after bounded termination")
            time.sleep(0.02)
        event = gate.next_event(lambda: exact_process_is_live(installer, identity))
        if event is None or not event.startswith("worker_error_"):
            raise GateFailure("installer did not observe the migration child death")
        if not exact_process_is_live(installer, identity):
            raise GateFailure("installer exited before child-death lock verification")
        assert_contended(paths.namespace_lock)
        gate.continue_child()
        status = wait_owned_worker(pid, time.monotonic() + 10)
        worker_reaped = True
        if not os.WIFEXITED(status) or os.WEXITSTATUS(status) != 92:
            raise GateFailure("installer worker did not retain the expected failure path")
        wait_reacquirable(paths.namespace_lock)
        return {
            "checkpoint": "manifest_frozen",
            "observed": seen,
            "child_death_observed": True,
            "installer_guard_remained_locked": True,
            "final_holder_released": True,
        }
    finally:
        if not worker_reaped:
            if exact_process_is_live(installer, identity):
                signal_exact_process(installer, identity, signal.SIGKILL)
            wait_owned_worker(pid, time.monotonic() + 10)
        cleanup_captured_generation(
            installer,
            generation,
            exclude={identity},
        )
        gate.connection.close()
        wait_reacquirable(paths.namespace_lock)


def complete_installer_resume(
    installer: Any,
    paths: Any,
    uid: int,
) -> dict[str, Any]:
    os.chmod(paths.legacy_accounts, 0)
    os.chmod(paths.migration_manifest, 0)
    completed_pid = None
    completed_identity = None
    completed_gate = None
    completed = False
    seen: list[str] = []
    try:
        completed_pid, completed_identity, completed_gate = spawn_installer_worker(
            installer,
            paths,
            uid,
        )
        seen = wait_worker_success(
            installer,
            completed_pid,
            completed_identity,
            completed_gate,
            paths,
        )
        completed_gate.connection.close()
        completed = True
    finally:
        if (
            not completed
            and completed_pid is not None
            and completed_identity is not None
            and completed_gate is not None
        ):
            kill_worker(
                installer,
                completed_pid,
                completed_identity,
                completed_gate,
            )
        os.chmod(paths.legacy_accounts, 0o600)
        os.chmod(paths.migration_manifest, 0o600)
    if not {
        "installer_lock_cloexec_verified",
        "completed_verified",
        "installer_completed_receipt_verified",
        "installer_launch_decided",
    }.issubset(seen):
        raise GateFailure("completed credential-negative installer verification was incomplete")
    assert_reacquirable(paths.namespace_lock)
    return {
        "completed_checkpoints": seen,
        "launch_decision": "no_launch",
    }


def verify_completed_daemon_wrapper_binding(
    installer: Any,
    paths: Any,
    namespace_lock: Any,
    manifest: dict[str, Any],
) -> dict[str, Any]:
    expected = manifest.get("daemon_wrapper")
    if not isinstance(expected, dict):
        raise GateFailure("completed manifest has no daemon wrapper binding")
    try:
        retirement = json.loads(
            sql(
                installer,
                paths,
                "SELECT retirement_receipt::text "
                "FROM decodex.account_migration_receipts WHERE singleton",
            )
        )
    except json.JSONDecodeError as error:
        raise GateFailure("completed retirement receipt is malformed") from error
    digest = installer.daemon_wrapper_digest(expected)
    assets = retirement.get("installed_assets") if isinstance(retirement, dict) else None
    matches = (
        [
            asset
            for asset in assets
            if isinstance(asset, dict)
            and asset.get("path") == expected.get("executable_path")
        ]
        if isinstance(assets, list)
        else []
    )
    wrapper_asset_index = (
        next(
            index
            for index, asset in enumerate(assets)
            if isinstance(asset, dict)
            and asset.get("path") == expected.get("executable_path")
        )
        if matches
        else -1
    )
    descriptor_only_paths = {
        expected.get("info_plist_path"),
        expected.get("embedded_profile_path"),
    }
    if (
        not isinstance(retirement, dict)
        or retirement.get("daemon_wrapper_verified") is not True
        or retirement.get("daemon_wrapper_identity_sha256") != digest
        or len(matches) != 1
        or matches[0].get("sha256") != expected.get("executable_sha256")
        or matches[0].get("byte_count") != expected.get("executable_byte_count")
        or any(
            isinstance(asset, dict)
            and asset.get("path") in descriptor_only_paths
            for asset in (assets if isinstance(assets, list) else [])
        )
    ):
        raise GateFailure("completed daemon wrapper receipt binding differs")
    installer.verify_daemon_wrapper(
        paths,
        expected,
        require_launch_agent=True,
    )

    original_launch_agent = paths.launch_agent.read_bytes()
    try:
        launch_agent = plistlib.loads(original_launch_agent)
        arguments = launch_agent.get("ProgramArguments")
        if not isinstance(arguments, list) or not arguments:
            raise GateFailure("completed LaunchAgent is malformed")
        arguments[0] = str(paths.decodex_cli)
        paths.launch_agent.write_bytes(
            plistlib.dumps(launch_agent, sort_keys=True)
        )
        expect_install_refusal(
            lambda: installer.run_completed_account_migration_verifier(
                paths,
                namespace_lock,
            ),
            "completed verification accepted LaunchAgent wrapper drift",
        )
    finally:
        paths.launch_agent.write_bytes(original_launch_agent)

    executable = Path(expected["executable_path"])
    original_executable = executable.read_bytes()
    if not original_executable:
        raise GateFailure("completed daemon wrapper executable is empty")
    tampered = bytearray(original_executable)
    tampered[-1] ^= 1
    try:
        executable.write_bytes(tampered)
        expect_install_refusal(
            lambda: installer.run_completed_account_migration_verifier(
                paths,
                namespace_lock,
            ),
            "completed verification accepted current wrapper drift",
        )
    finally:
        executable.write_bytes(original_executable)
    installer.verify_daemon_wrapper(
        paths,
        expected,
        require_launch_agent=True,
    )
    return {
        "receipt_descriptor_digest_bound": True,
        "wrapper_main_installed_asset_count": 1,
        "wrapper_main_installed_asset_index": wrapper_asset_index,
        "descriptor_only_assets_excluded": True,
        "launch_agent_drift_refused": True,
        "current_wrapper_drift_refused": True,
    }


def completed_verifier_drift(
    installer: Any,
    paths: Any,
    uid: int,
) -> dict[str, Any]:
    namespace_lock = installer.InstallerNamespaceLock.acquire(paths, uid)
    postgres = start_owned_postgres(installer, paths)
    try:
        account = CASES[1]
        sql(
            installer,
            paths,
            "INSERT INTO decodex.account_quota_facts("
            "account_id,duration_minutes,used_percent,resets_at_micros,"
            "error_code,observed_at_micros) VALUES ("
            f"'{account.account_id}',300,25,4102444800000000,NULL,2000000000000000)",
        )
        completed = installer.run_completed_account_migration_verifier(
            paths,
            namespace_lock,
        )
        if completed.get("outcome") != "verified":
            raise GateFailure("completed verification required quota Unknown")
        manifest = json.loads(
            sql(
                installer,
                paths,
                "SELECT manifest::text FROM decodex.account_migration_receipts "
                "WHERE singleton",
            )
        )
        wrapper_binding = verify_completed_daemon_wrapper_binding(
            installer,
            paths,
            namespace_lock,
            manifest,
        )
        expected_wrapper = manifest["daemon_wrapper"]
        wrapper_identity_sha256 = installer.daemon_wrapper_digest(
            expected_wrapper
        )
        wrapper_asset_index = wrapper_binding[
            "wrapper_main_installed_asset_index"
        ]
        drift_digest = "e" * 64
        target = {
            row["account_id"]: row["target"] for row in manifest["accounts"]
        }[account.account_id]
        mutations = (
            (
                "label",
                "UPDATE decodex.accounts SET display_label='Completed Drift' "
                f"WHERE account_id='{account.account_id}'",
                f"UPDATE decodex.accounts SET display_label='{account.display_label}' "
                f"WHERE account_id='{account.account_id}'",
            ),
            (
                "enabled",
                "UPDATE decodex.accounts SET enabled=false "
                f"WHERE account_id='{account.account_id}'",
                "UPDATE decodex.accounts SET enabled=true "
                f"WHERE account_id='{account.account_id}'",
            ),
            (
                "account_revision",
                "UPDATE decodex.accounts SET revision=revision+1 "
                f"WHERE account_id='{account.account_id}'",
                "UPDATE decodex.accounts SET revision=revision-1 "
                f"WHERE account_id='{account.account_id}'",
            ),
            (
                "routing_revision",
                "UPDATE decodex.account_routing_control SET revision=revision+1 "
                "WHERE singleton",
                "UPDATE decodex.account_routing_control SET revision=revision-1 "
                "WHERE singleton",
            ),
            (
                "routing_mode",
                "UPDATE decodex.account_routing_control SET "
                f"mode='fixed',fixed_account_id='{account.account_id}' WHERE singleton",
                "UPDATE decodex.account_routing_control SET "
                "mode='balanced',fixed_account_id=NULL WHERE singleton",
            ),
            (
                "routing_order",
                "DELETE FROM decodex.account_routing_order "
                f"WHERE account_id='{CASES[-1].account_id}'",
                "INSERT INTO decodex.account_routing_order(account_id,position) "
                f"VALUES ('{CASES[-1].account_id}',3)",
            ),
            (
                "provider",
                "UPDATE decodex.accounts SET provider_account_id='completed-drift' "
                f"WHERE account_id='{account.account_id}'",
                "UPDATE decodex.accounts SET provider_account_id="
                f"'{target['provider_account_id']}' "
                f"WHERE account_id='{account.account_id}'",
            ),
            (
                "credential_version",
                "UPDATE decodex.accounts SET credential_version=2 "
                f"WHERE account_id='{account.account_id}'",
                "UPDATE decodex.accounts SET credential_version=1 "
                f"WHERE account_id='{account.account_id}'",
            ),
            (
                "writer_operation",
                "UPDATE decodex.accounts SET credential_writer_operation_id="
                "'14220000-0000-4000-9000-000000000099' "
                f"WHERE account_id='{account.account_id}'",
                "UPDATE decodex.accounts SET credential_writer_operation_id="
                f"'{account.operation_id}' WHERE account_id='{account.account_id}'",
            ),
            (
                "fingerprint",
                "UPDATE decodex.accounts SET credential_fingerprint='" + ("e" * 64) + "' "
                f"WHERE account_id='{account.account_id}'",
                "UPDATE decodex.accounts SET credential_fingerprint="
                f"'{target['fingerprint_sha256']}' "
                f"WHERE account_id='{account.account_id}'",
            ),
            (
                "host_store_receipt",
                "UPDATE decodex.account_migration_receipts SET destination_receipt="
                "pg_catalog.jsonb_set(destination_receipt,'{accounts,1,host_store}',"
                "pg_catalog.to_jsonb('drifted_host_store'::text)) WHERE singleton",
                "UPDATE decodex.account_migration_receipts SET destination_receipt="
                "pg_catalog.jsonb_set(destination_receipt,'{accounts,1,host_store}',"
                "pg_catalog.to_jsonb('macos_keychain_generic_password_v1'::text)) "
                "WHERE singleton",
            ),
            (
                "postgres_projection_receipt",
                "UPDATE decodex.account_migration_receipts SET destination_receipt="
                "pg_catalog.jsonb_set(destination_receipt,"
                "'{accounts,1,postgres_projection}',"
                "pg_catalog.to_jsonb('drifted_projection'::text)) WHERE singleton",
                "UPDATE decodex.account_migration_receipts SET destination_receipt="
                "pg_catalog.jsonb_set(destination_receipt,"
                "'{accounts,1,postgres_projection}',"
                "pg_catalog.to_jsonb("
                "'decodex.accounts_credential_binding_v27'::text)) WHERE singleton",
            ),
            (
                "store_schema_receipt",
                "UPDATE decodex.account_migration_receipts SET destination_receipt="
                "pg_catalog.jsonb_set(destination_receipt,"
                "'{accounts,1,store_schema_version}','2'::jsonb) WHERE singleton",
                "UPDATE decodex.account_migration_receipts SET destination_receipt="
                "pg_catalog.jsonb_set(destination_receipt,"
                "'{accounts,1,store_schema_version}','1'::jsonb) WHERE singleton",
            ),
            (
                "daemon_wrapper_verified",
                "UPDATE decodex.account_migration_receipts SET retirement_receipt="
                "pg_catalog.jsonb_set(retirement_receipt,"
                "'{daemon_wrapper_verified}','false'::jsonb) WHERE singleton",
                "UPDATE decodex.account_migration_receipts SET retirement_receipt="
                "pg_catalog.jsonb_set(retirement_receipt,"
                "'{daemon_wrapper_verified}','true'::jsonb) WHERE singleton",
            ),
            (
                "daemon_wrapper_identity",
                "UPDATE decodex.account_migration_receipts SET retirement_receipt="
                "pg_catalog.jsonb_set(retirement_receipt,"
                "'{daemon_wrapper_identity_sha256}',"
                f"pg_catalog.to_jsonb('{drift_digest}'::text)) WHERE singleton",
                "UPDATE decodex.account_migration_receipts SET retirement_receipt="
                "pg_catalog.jsonb_set(retirement_receipt,"
                "'{daemon_wrapper_identity_sha256}',"
                f"pg_catalog.to_jsonb('{wrapper_identity_sha256}'::text)) "
                "WHERE singleton",
            ),
            (
                "daemon_wrapper_installed_asset",
                "UPDATE decodex.account_migration_receipts SET retirement_receipt="
                "pg_catalog.jsonb_set(retirement_receipt,"
                f"'{{installed_assets,{wrapper_asset_index},sha256}}',"
                f"pg_catalog.to_jsonb('{drift_digest}'::text)) WHERE singleton",
                "UPDATE decodex.account_migration_receipts SET retirement_receipt="
                "pg_catalog.jsonb_set(retirement_receipt,"
                f"'{{installed_assets,{wrapper_asset_index},sha256}}',"
                "pg_catalog.to_jsonb("
                f"'{expected_wrapper['executable_sha256']}'::text)) "
                "WHERE singleton",
            ),
        )
        drift_failures = []
        for name, mutation, restore in mutations:
            try:
                sql(installer, paths, mutation)
            except BaseException as error:
                drift_failures.append(f"{name}: mutation failed: {error}")
                continue
            try:
                try:
                    expect_entrypoint_failure(
                        installer,
                        paths,
                        namespace_lock,
                        completed_verifier_command(installer, paths),
                    )
                except BaseException as error:
                    drift_failures.append(f"{name}: {error}")
            finally:
                sql(installer, paths, restore)
        if drift_failures:
            raise GateFailure(
                "completed destination drift mismatches: "
                + "; ".join(drift_failures)
            )
        return {
            "drift_cases": len(mutations),
            "quota_unknown_not_required": True,
            "daemon_wrapper": wrapper_binding,
        }
    finally:
        stop_owned_postgres(installer, postgres)
        namespace_lock.close()


def verify_cancel_refusals_after_same_digest_completion(
    context: dict[str, Any],
) -> dict[str, Any]:
    installer = context["installer"]
    paths = context["paths"]
    namespace_lock = installer.InstallerNamespaceLock.acquire(
        paths,
        context["identity"].uid,
    )
    context["namespace_lock"] = namespace_lock
    postgres = start_owned_postgres(installer, paths)
    context["postgres"] = postgres
    try:
        case = CASES[0]
        phase = sql(
            installer,
            paths,
            "SELECT phase FROM decodex.account_operations "
            f"WHERE operation_id='{case.operation_id}'",
        )
        revision = sql(
            installer,
            paths,
            "SELECT revision FROM decodex.accounts "
            f"WHERE account_id='{case.account_id}'",
        )
        receipt_phase = sql(
            installer,
            paths,
            "SELECT phase FROM decodex.account_migration_receipts WHERE singleton",
        )
        receipts = migration_cancel_receipts(
            installer,
            paths,
            context["run_token"],
        )
        validate_migration_cancel_receipts(
            receipts,
            context["run_token"],
            case.operation_id,
            1,
        )
        if phase != "committed" or revision != "2" or receipt_phase != "completed":
            raise GateFailure("same-digest completion did not settle the refused operation")
        return {
            "same_manifest_operation_phase": "committed",
            "account_revision": 2,
            "migration_receipt_phase": "completed",
            "typed_cancel_refusal_receipts": len(receipts),
            "receipt_semantics_preserved": True,
        }
    finally:
        stop_owned_postgres(installer, postgres)
        context["postgres"] = None
        namespace_lock.close()
        context["namespace_lock"] = None


def establish_v26_fixture(context: dict[str, Any]) -> dict[str, Any]:
    installer = context["installer"]
    paths = context["paths"]
    installer.initialize_cluster(paths, context["identity"].uid)
    postgres = start_owned_postgres(installer, paths)
    context["postgres"] = postgres
    try:
        environment = installer.psql_environment(paths)
        installer.ensure_roles_and_database(paths, environment)
        apply_v26_fixture(installer, paths, context["artifacts"])
        rows = sql(
            installer,
            paths,
            "SELECT pg_catalog.json_agg(pg_catalog.json_build_object("
            "'account_id',account_id,'state',state,'revision',revision"
            ") ORDER BY account_id)::text FROM decodex.accounts",
        )
        parsed = json.loads(rows)
        expected = {
            case.account_id: {
                "state": case.v26_state,
                "revision": case.v26_revision,
            }
            for case in CASES
            if case.v26_revision is not None
        }
        actual = {
            row["account_id"]: {
                "state": row["state"],
                "revision": row["revision"],
            }
            for row in parsed
        }
        if actual != expected:
            raise GateFailure("populated V26 fixture differs")
        return {
            "postgres_major": 18,
            "toolchain_bin": str(context["toolchain"].postgres.parent),
            "populated_accounts": len(expected),
            "endpoint_namespace": str(paths.socket_directory),
        }
    finally:
        stop_owned_postgres(installer, postgres)
        context["postgres"] = None


def verify_prepared_replay_stage(
    context: dict[str, Any],
    ownership: CredentialOwnership,
) -> dict[str, Any]:
    installer = context["installer"]
    paths = context["paths"]
    uid = context["identity"].uid
    namespace_lock = installer.InstallerNamespaceLock.acquire(paths, uid)
    context["namespace_lock"] = namespace_lock
    postgres = start_owned_postgres(installer, paths)
    context["postgres"] = postgres
    try:
        assert_expected_destination(installer, paths)
        before = protected_store_snapshot(paths)
        verify_replay_and_drift(installer, paths, namespace_lock)
        after = protected_store_snapshot(paths)
        if before != after:
            raise GateFailure("prepared replay changed protected-store metadata")
        expectations = manifest_credential_expectations(paths)
        for case in CASES:
            ownership.inspect_and_record(
                paths,
                case.account_id,
                expectations[case.account_id],
            )
        return {
            "same_digest": "no_new_effect",
            "positive_binding": "no_new_operation",
            "different_digest": "refused",
            "operation_descriptor_and_terminal_drift_cases": 8,
            "manifest_store_binding_drift_cases": 2,
            "source_digest_drift": "refused",
        }
    finally:
        stop_owned_postgres(installer, postgres)
        context["postgres"] = None
        namespace_lock.close()
        context["namespace_lock"] = None


def verify_completed_admission_stage(context: dict[str, Any]) -> dict[str, Any]:
    installer = context["installer"]
    paths = context["paths"]
    uid = context["identity"].uid
    namespace_lock = installer.InstallerNamespaceLock.acquire(paths, uid)
    context["namespace_lock"] = namespace_lock
    postgres = start_owned_postgres(installer, paths)
    context["postgres"] = postgres
    try:
        report = account_migration_admission_gate(
            paths,
            "completed",
            CASES[1].account_id,
            9,
        )
        context["completed_admission_report"] = report
        context["completed_admission_footprint"] = capture_stage(
            lambda: admission_durable_footprint(
                installer,
                paths,
                expected_reset_rows=1,
            )
        )
        return {
            "report_captured": True,
        }
    finally:
        stop_owned_postgres(installer, postgres)
        context["postgres"] = None
        namespace_lock.close()
        context["namespace_lock"] = None


def case_projection_evidence(context: dict[str, Any], case: AccountCase) -> dict[str, Any]:
    snapshot = context.get("prepared_destination")
    if not isinstance(snapshot, dict):
        raise GateFailure("prepared destination snapshot is unavailable")
    account = next(
        (
            row
            for row in snapshot.get("accounts") or []
            if row.get("account_id") == case.account_id
        ),
        None,
    )
    operation = next(
        (
            row
            for row in snapshot.get("operations") or []
            if row.get("operation_id") == case.operation_id
        ),
        None,
    )
    expected_revision = {
        CASES[0].account_id: 2,
        CASES[1].account_id: 9,
        CASES[2].account_id: 2,
        CASES[3].account_id: 12,
    }[case.account_id]
    if (
        account is None
        or operation is None
        or account.get("revision") != expected_revision
        or account.get("enabled") is not case.enabled
        or account.get("display_label") != case.display_label
        or operation.get("phase") != "committed"
    ):
        raise GateFailure("account case projection differs")
    return {
        "account_id": case.account_id,
        "revision": expected_revision,
        "operation_id": case.operation_id,
        "phase": "committed",
    }


def cleanup_gate(
    context: dict[str, Any],
    ownership: CredentialOwnership,
) -> dict[str, Any]:
    errors = []
    owner_cleanup_failed = False
    installer = context.get("installer")
    postgres = context.get("postgres")
    if postgres is not None and installer is not None:
        try:
            stop_owned_postgres(installer, postgres)
            context["postgres"] = None
        except BaseException as error:
            owner_cleanup_failed = True
            errors.append(f"PostgreSQL: {type(error).__name__}: {error}")
    namespace_lock = context.get("namespace_lock")
    if namespace_lock is not None:
        try:
            namespace_lock.close()
            context["namespace_lock"] = None
        except BaseException as error:
            owner_cleanup_failed = True
            errors.append(f"namespace lock: {type(error).__name__}: {error}")

    paths = context.get("paths")
    credential_result = {
        "recorded": 0,
        "deleted": 0,
        "absence_verified": not ownership.absence_proved,
    }
    if paths is not None and ownership.absence_proved:
        expectations: dict[str, dict[str, Any]] = {}
        if paths.migration_manifest.exists():
            try:
                expectations = manifest_credential_expectations(paths)
            except BaseException as error:
                errors.append(
                    f"credential expectation discovery: {type(error).__name__}: {error}"
                )
        try:
            ownership.discover_gate_created(paths, expectations)
        except BaseException as error:
            errors.append(f"credential discovery: {type(error).__name__}: {error}")
        try:
            credential_result = ownership.cleanup(paths)
        except BaseException as error:
            errors.append(f"credential cleanup: {type(error).__name__}: {error}")

    fixture_root = context.get("fixture_root")
    fixture_removed = fixture_root is None or not fixture_root.exists()
    if (
        not owner_cleanup_failed
        and fixture_root is not None
        and fixture_root.exists()
    ):
        try:
            for path in sorted(
                fixture_root.rglob("*"),
                key=lambda value: len(value.parts),
                reverse=True,
            ):
                metadata = path.lstat()
                if stat.S_ISDIR(metadata.st_mode):
                    os.chmod(path, 0o700, follow_symlinks=False)
                elif stat.S_ISREG(metadata.st_mode):
                    os.chmod(path, 0o600, follow_symlinks=False)
            os.chmod(fixture_root, 0o700, follow_symlinks=False)
            shutil.rmtree(fixture_root)
            fixture_removed = not fixture_root.exists()
            if not fixture_removed:
                raise GateFailure("fixture root remained after cleanup")
        except BaseException as error:
            errors.append(f"fixture cleanup: {type(error).__name__}: {error}")
    if errors:
        raise GateFailure("; ".join(errors))
    return {
        "credential_items": credential_result,
        "fixture_removed": fixture_removed,
        "process_cleanup": "exact_start_identity_and_owned_handles",
        "lock_cleanup": "closed",
    }


def main() -> int:
    global CASES, CONFLICT_CASE

    run_token = secrets.token_hex(8)
    CASES, CONFLICT_CASE = build_gate_identities(run_token)
    selected = tuple(
        CredentialGateCase(
            account_id=case.account_id,
            operation_id=case.operation_id,
            provider_account_id=case.provider_account_id,
            email=case.email,
        )
        for case in CASES
    ) + (CONFLICT_CASE,)
    ownership = CredentialOwnership(selected)
    context: dict[str, Any] = {
        "run_token": run_token,
        "postgres": None,
        "namespace_lock": None,
    }
    graph = StageGraph()

    graph.run("preflight_identity", (), lambda: preflight_identity(context))
    graph.run(
        "preflight_paths",
        ("preflight_identity",),
        lambda: preflight_paths(context, run_token),
    )
    graph.run(
        "preflight_system_executables",
        (),
        lambda: preflight_system_executables(context),
    )
    graph.run(
        "preflight_postgresql_18",
        (),
        lambda: preflight_postgres_toolchain(context),
    )
    graph.run(
        "preflight_decodexd_artifact",
        (),
        lambda: preflight_decodexd_artifact(context),
    )
    graph.run(
        "preflight_migration_fixture_artifact",
        (),
        lambda: preflight_migration_fixture_artifact(context),
    )
    graph.run(
        "preflight_build_artifacts",
        (
            "preflight_decodexd_artifact",
            "preflight_migration_fixture_artifact",
        ),
        lambda: preflight_build_artifacts(context),
    )
    graph.run("preflight_installer", (), lambda: preflight_installer(context))
    graph.run(
        "preflight_daemon_wrapper_signing",
        ("preflight_identity",),
        lambda: preflight_daemon_wrapper_signing(context),
    )
    preflight_dependencies = (
        "preflight_identity",
        "preflight_paths",
        "preflight_system_executables",
        "preflight_postgresql_18",
        "preflight_decodexd_artifact",
        "preflight_migration_fixture_artifact",
        "preflight_build_artifacts",
        "preflight_installer",
        "preflight_daemon_wrapper_signing",
    )
    graph.run(
        "preflight_complete",
        (),
        lambda: {
            "side_effect_free": True,
            "live_default_access": "excluded",
            "preflight_states": {
                name: graph.results[name].state
                for name in preflight_dependencies
            },
        },
    )

    graph.run(
        "fixture_root",
        ("preflight_identity", "preflight_paths", "preflight_installer"),
        lambda: create_fixture_root(context),
    )
    graph.run(
        "source_path_predicate",
        ("fixture_root", "preflight_installer"),
        lambda: verify_source_path_predicate(
            context["installer"],
            context["identity"],
            context["fixture_root"],
        ),
    )
    graph.run(
        "fixture_setup",
        (
            "fixture_root",
            "preflight_complete",
            "preflight_installer",
            "preflight_build_artifacts",
            "preflight_daemon_wrapper_signing",
        ),
        lambda: setup_fixture(context, context["installer"]),
    )

    graph.run(
        "live_daemon_exclusion",
        (
            "fixture_setup",
            "preflight_system_executables",
            "preflight_decodexd_artifact",
        ),
        lambda: live_daemon_exclusion(
            context["installer"],
            context["paths"],
            context["identity"].uid,
        ),
    )
    graph.run(
        "direct_invocation_refusal",
        ("fixture_setup", "preflight_decodexd_artifact"),
        lambda: direct_invocation_refusal_checks(
            context["installer"],
            context["paths"],
        ),
    )
    graph.run(
        "malformed_descriptor_refusal",
        ("fixture_setup", "preflight_decodexd_artifact"),
        lambda: malformed_descriptor_refusal(context["paths"]),
    )
    graph.run(
        "wrong_identity_descriptor_refusal",
        ("fixture_setup", "preflight_decodexd_artifact"),
        lambda: wrong_identity_descriptor_refusal(context["paths"]),
    )
    graph.run(
        "aliased_descriptor_refusal",
        ("fixture_setup", "preflight_decodexd_artifact"),
        lambda: aliased_descriptor_refusal(
            context["installer"],
            context["paths"],
            context["identity"].uid,
        ),
    )
    graph.run(
        "valid_descriptor_cloexec_refusal",
        (
            "fixture_setup",
            "preflight_system_executables",
            "preflight_decodexd_artifact",
        ),
        lambda: valid_descriptor_cloexec_refusal(
            context["installer"],
            context["paths"],
            context["identity"].uid,
        ),
    )
    for fault_point in (
        "lock_acquisition",
        "lock_duplicate",
        "socket_creation",
        "transition_gate_duplicate",
    ):
        graph.run(
            f"cleanup_fault_{fault_point}",
            ("fixture_setup", "preflight_installer"),
            lambda fault_point=fault_point: pre_spawn_cleanup_fault(
                context["installer"],
                context["paths"],
                context["identity"].uid,
                fault_point,
            ),
        )
    for fault_point in ("spawn", "child_identity_capture"):
        graph.run(
            f"cleanup_fault_{fault_point}",
            (
                "fixture_setup",
                "preflight_system_executables",
                "preflight_decodexd_artifact",
            ),
            lambda fault_point=fault_point: post_spawn_cleanup_fault(
                context["installer"],
                context["paths"],
                context["identity"].uid,
                fault_point,
            ),
        )

    def external_contention() -> dict[str, Any]:
        lock = context["installer"].InstallerNamespaceLock.acquire(
            context["paths"],
            context["identity"].uid,
        )
        context["namespace_lock"] = lock
        try:
            assert_contended(context["paths"].namespace_lock)
            return {"independent_open_file_description_refused": True}
        finally:
            lock.close()
            context["namespace_lock"] = None
            assert_reacquirable(context["paths"].namespace_lock)

    def path_drift_refusal() -> dict[str, Any]:
        lock = context["installer"].InstallerNamespaceLock.acquire(
            context["paths"],
            context["identity"].uid,
        )
        context["namespace_lock"] = lock
        try:
            return path_drift_descriptor_refusal(context["paths"], lock)
        finally:
            lock.close()
            context["namespace_lock"] = None
            assert_reacquirable(context["paths"].namespace_lock)

    graph.run(
        "external_lock_contention",
        ("fixture_setup",),
        external_contention,
    )
    graph.run(
        "path_identity_drift_refusal",
        ("fixture_setup", "preflight_decodexd_artifact"),
        path_drift_refusal,
    )

    graph.run(
        "keychain_absence_precondition",
        ("fixture_setup", "preflight_decodexd_artifact"),
        lambda: ownership.prove_absent(context["paths"]),
    )
    graph.run(
        "protected_store_contract",
        ("keychain_absence_precondition",),
        lambda: verify_protected_store_contract_and_conflict(
            context["paths"],
            ownership,
            CONFLICT_CASE,
            context["conflict_sources"],
        ),
    )
    graph.run(
        "v26_fixture",
        (
            "fixture_setup",
            "source_path_predicate",
            "protected_store_contract",
            "preflight_postgresql_18",
            "preflight_build_artifacts",
        ),
        lambda: establish_v26_fixture(context),
    )
    graph.run(
        "populated_v26_without_handoff_refusal",
        ("v26_fixture",),
        lambda: populated_v26_without_handoff_refusal(context),
    )

    def bind_frozen_manifest() -> dict[str, Any]:
        evidence = adopt_manifest_operation_ids(context["paths"], ownership)
        evidence["daemon_wrapper"] = verify_daemon_wrapper_manifest_binding(
            context
        )
        context["manifest_identity"] = evidence
        return evidence

    graph.run(
        "orchestration_manifest_frozen",
        ("populated_v26_without_handoff_refusal",),
        lambda: crash_installer_at(
            context["installer"],
            context["paths"],
            context["identity"].uid,
            "manifest_frozen",
            at_checkpoint=bind_frozen_manifest,
        ),
    )
    graph.run(
        "manifest_identity_binding",
        ("orchestration_manifest_frozen",),
        lambda: context["manifest_identity"],
    )

    def cloexec_evidence() -> dict[str, Any]:
        observed = graph.results[
            "orchestration_manifest_frozen"
        ].evidence["observed"]
        if "installer_lock_cloexec_verified" not in observed:
            raise GateFailure("migration child did not prove close-on-exec")
        return {"descendant_descriptor_excluded": True}

    graph.run(
        "installer_lock_cloexec",
        ("orchestration_manifest_frozen",),
        cloexec_evidence,
    )
    graph.run(
        "migration_child_death_lock_lineage",
        ("installer_lock_cloexec",),
        lambda: crash_migration_child_with_surviving_installer(
            context["installer"],
            context["paths"],
            context["identity"].uid,
        ),
    )
    graph.run(
        "orchestration_intent_prepared",
        ("manifest_identity_binding", "migration_child_death_lock_lineage"),
        lambda: crash_installer_at(
            context["installer"],
            context["paths"],
            context["identity"].uid,
            "intent_prepared",
        ),
    )

    previous_stage = "orchestration_intent_prepared"
    checkpoint_stages = []
    for case_index in (0, 1):
        for checkpoint in (
            "operation_prepared",
            "keychain_applied",
            "store_applied",
            "credential_committed",
            "administration_applied",
        ):
            case = CASES[case_index]
            transition = "absent_initialize" if case.v26_revision is None else "existing_hydrate"
            stage_name = f"{transition}_{checkpoint}"
            state_name = f"{stage_name}_state"
            captures: dict[str, StageResult] = {}

            def capture_checkpoint(
                case: AccountCase = case,
                checkpoint: str = checkpoint,
                captures: dict[str, StageResult] = captures,
            ) -> dict[str, Any]:
                captures["state"] = capture_stage(
                    lambda: account_checkpoint_state(
                        context["installer"],
                        context["paths"],
                        case,
                        checkpoint,
                    )
                )
                if case == CASES[0] and checkpoint == "operation_prepared":
                    captures["prepared_exclusion"] = capture_stage(
                        lambda: verify_prepared_exclusion(
                            context["installer"],
                            context["paths"],
                            context["run_token"],
                        )
                    )

                    def capture_unsettled_admission() -> dict[str, Any]:
                        report = account_migration_admission_gate(
                            context["paths"],
                            "unsettled",
                            case.account_id,
                            1,
                        )
                        context["unsettled_admission_report"] = report
                        return {"report_captured": True}

                    captures["admission"] = capture_stage(
                        capture_unsettled_admission
                    )
                    captures["admission_footprint"] = capture_stage(
                        lambda: admission_durable_footprint(
                            context["installer"],
                            context["paths"],
                            expected_reset_rows=0,
                        )
                    )
                if checkpoint == "keychain_applied":
                    captures["keychain"] = capture_stage(
                        lambda: ownership.inspect_and_record(
                            context["paths"],
                            case.account_id,
                            manifest_credential_expectations(context["paths"])[
                                case.account_id
                            ],
                        )
                    )
                return {"captured_branches": sorted(captures)}

            graph.run(
                stage_name,
                (previous_stage,),
                lambda stage_name=stage_name, checkpoint=checkpoint, case=case: (
                    crash_installer_at(
                        context["installer"],
                        context["paths"],
                        context["identity"].uid,
                        checkpoint,
                        target_account_id=case.account_id,
                        at_checkpoint=capture_checkpoint,
                    )
                ),
            )
            next_stage = stage_name
            graph.record_capture(state_name, captures.get("state"), stage_name)
            checkpoint_stages.append(state_name)
            if case_index == 0 and checkpoint == "operation_prepared":
                graph.record_capture(
                    "prepared_reclassification_exclusion",
                    captures.get("prepared_exclusion"),
                    stage_name,
                )
                graph.record_capture(
                    "unsettled_admission_probe",
                    captures.get("admission"),
                    stage_name,
                )
                graph.record_capture(
                    "unsettled_admission_footprint",
                    captures.get("admission_footprint"),
                    stage_name,
                )
                for branch in (
                    "initial_selection",
                    "process_spawn_admission",
                    "reset_card_admission",
                ):
                    graph.run(
                        f"unsettled_{branch}",
                        ("unsettled_admission_probe",),
                        lambda branch=branch: validate_admission_branch(
                            context["unsettled_admission_report"],
                            branch,
                            "refused",
                        ),
                    )
                graph.run(
                    "prepared_current_tuple_drift_refusal",
                    (
                        state_name,
                        "prepared_reclassification_exclusion",
                        "unsettled_admission_footprint",
                    ),
                    lambda: verify_prepared_current_tuple_drift(context),
                )
                graph.run(
                    "prewrite_credential_unavailable_recovery_required",
                    ("prepared_current_tuple_drift_refusal",),
                    lambda: create_recovery_required_from_prewrite_unavailable(
                        context["installer"],
                        context["paths"],
                        context["identity"].uid,
                    ),
                )
                graph.run(
                    "recovery_required_reclassification_exclusion",
                    (
                        "prewrite_credential_unavailable_recovery_required",
                    ),
                    lambda: verify_recovery_required_exclusion(context),
                )
                next_stage = (
                    "recovery_required_reclassification_exclusion"
                )
            if checkpoint == "keychain_applied":
                graph.record_capture(
                    f"{transition}_keychain_binding",
                    captures.get("keychain"),
                    stage_name,
                )
            previous_stage = next_stage

    routing_captures: dict[str, StageResult] = {}

    def capture_routing_destination() -> dict[str, Any]:
        def capture_destination() -> dict[str, Any]:
            assert_expected_destination(context["installer"], context["paths"])
            context["prepared_destination"] = json.loads(
                destination_snapshot(context["installer"], context["paths"])
            )
            return {
                "account_count": len(CASES),
                "routing_revision": 2,
                "receipt_phase": "prepared",
            }

        def capture_credentials() -> dict[str, Any]:
            expectations = manifest_credential_expectations(context["paths"])
            for case in CASES:
                ownership.inspect_and_record(
                    context["paths"],
                    case.account_id,
                    expectations[case.account_id],
                )
            return {
                "items": len(CASES),
                "metadata_digest": protected_store_snapshot(context["paths"]),
            }

        routing_captures["destination"] = capture_stage(capture_destination)
        routing_captures["credentials"] = capture_stage(capture_credentials)
        return {"captured_branches": sorted(routing_captures)}

    graph.run(
        "orchestration_routing_applied",
        (previous_stage,),
        lambda: crash_installer_at(
            context["installer"],
            context["paths"],
            context["identity"].uid,
            "routing_applied",
            at_checkpoint=capture_routing_destination,
        ),
    )
    graph.record_capture(
        "prepared_destination_exact",
        routing_captures.get("destination"),
        "orchestration_routing_applied",
    )
    graph.record_capture(
        "all_keychain_bindings_exact",
        routing_captures.get("credentials"),
        "orchestration_routing_applied",
    )

    case_stage_names = []
    for case, name in zip(
        CASES,
        (
            "absent_normal",
            "populated_normal",
            "absent_disabled",
            "populated_disabled",
        ),
    ):
        stage_name = f"case_{name}"
        graph.run(
            stage_name,
            ("prepared_destination_exact",),
            lambda case=case: case_projection_evidence(context, case),
        )
        case_stage_names.append(stage_name)
    graph.run(
        "operation_phase_resume",
        tuple(checkpoint_stages) + tuple(case_stage_names),
        lambda: {
            "checkpoint_states": len(checkpoint_stages),
            "transition_kinds": ["AbsentInitialize", "ExistingHydrate"],
        },
    )
    graph.run(
        "prepared_replay_and_drift",
        (
            "prepared_destination_exact",
            "all_keychain_bindings_exact",
            "operation_phase_resume",
        ),
        lambda: verify_prepared_replay_stage(context, ownership),
    )

    graph.run(
        "installer_config_swap_crash",
        ("prepared_replay_and_drift",),
        lambda: crash_installer_at(
            context["installer"],
            context["paths"],
            context["identity"].uid,
            "installer_config_swapped",
        ),
    )
    graph.run(
        "prepared_verifier_and_staging_retirement",
        ("installer_config_swap_crash",),
        lambda: crash_installer_at(
            context["installer"],
            context["paths"],
            context["identity"].uid,
            "installer_staging_retired",
        ),
    )
    graph.run(
        "active_legacy_retirement",
        ("prepared_verifier_and_staging_retirement",),
        lambda: crash_installer_at(
            context["installer"],
            context["paths"],
            context["identity"].uid,
            "installer_active_legacy_retired",
        ),
    )
    graph.run(
        "finalizer_child_lock_lineage",
        ("active_legacy_retirement",),
        lambda: crash_installer_with_surviving_child(
            context["installer"],
            context["paths"],
            context["identity"].uid,
            "final_destination_verified",
        ),
    )
    graph.run(
        "final_receipt_child_lock_lineage",
        ("finalizer_child_lock_lineage",),
        lambda: crash_installer_with_surviving_child(
            context["installer"],
            context["paths"],
            context["identity"].uid,
            "receipt_completed",
        ),
    )
    graph.run(
        "completed_verifier_and_launch_decision",
        ("final_receipt_child_lock_lineage",),
        lambda: complete_installer_resume(
            context["installer"],
            context["paths"],
            context["identity"].uid,
        ),
    )
    graph.run(
        "completed_credential_negative_drift",
        ("completed_verifier_and_launch_decision",),
        lambda: completed_verifier_drift(
            context["installer"],
            context["paths"],
            context["identity"].uid,
        ),
    )
    graph.run(
        "manifest_cancel_refusal_same_digest_completion",
        ("completed_credential_negative_drift",),
        lambda: verify_cancel_refusals_after_same_digest_completion(context),
    )
    graph.run(
        "completed_admission_probe",
        ("manifest_cancel_refusal_same_digest_completion",),
        lambda: verify_completed_admission_stage(context),
    )
    graph.record_capture(
        "completed_admission_footprint",
        context.get("completed_admission_footprint"),
        "completed_admission_probe",
    )
    for branch in (
        "initial_selection",
        "process_spawn_admission",
        "reset_card_admission",
    ):
        graph.run(
            f"completed_{branch}",
            ("completed_admission_probe",),
            lambda branch=branch: validate_admission_branch(
                context["completed_admission_report"],
                branch,
                "admitted",
            ),
        )

    graph.run("cleanup", (), lambda: cleanup_gate(context, ownership))

    documents = {
        name: result.document()
        for name, result in graph.results.items()
    }
    failed = [
        name for name, result in graph.results.items() if result.state == "failed"
    ]
    blocked = [
        name for name, result in graph.results.items() if result.state == "blocked"
    ]
    outcome = "passed" if not failed and not blocked else "failed"
    report = {
        "schema": "decodex/account-migration-transition-gate/1",
        "outcome": outcome,
        "run_id": run_token,
        "stages": documents,
        "failed": failed,
        "blocked": blocked,
        "acl_boundary": "posix_mode_evidence_only",
        "live_default_source_access": False,
    }
    encoded = json.dumps(report, sort_keys=True, separators=(",", ":"))
    if len(encoded.encode("utf-8")) > MAX_SUBPROCESS_OUTPUT_BYTES:
        fallback = {
            "schema": "decodex/account-migration-transition-gate/1",
            "outcome": "failed",
            "failed": ["report_output_bound"],
            "blocked": blocked,
        }
        print(json.dumps(fallback, sort_keys=True, separators=(",", ":")))
        return 1
    print(encoded)
    return 0 if outcome == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
