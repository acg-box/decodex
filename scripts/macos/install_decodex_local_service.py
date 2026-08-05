#!/usr/bin/env python3
"""Install the current same-UID Decodex local service on macOS.

This installer provisions only the current architecture. It creates or verifies
the local PostgreSQL cluster, canonical configuration, and user LaunchAgent. It
does not discover, read, transform, retain, or delete account sources.
"""

from __future__ import annotations

import argparse
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
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional, Union


LAUNCH_AGENT_LABEL = "space.decodex.local-service"
MAX_CONFIG_FILE_BYTES = 1024 * 1024
MAX_LAUNCH_AGENT_FILE_BYTES = 64 * 1024
MAX_POSTGRES_VERSION_BYTES = 16
LOCAL_SERVICE_SETTLEMENT_TIMEOUT_SECONDS = 300
LOCAL_SERVICE_SETTLEMENT_POLL_SECONDS = 0.25
LOCAL_SERVICE_CONTROL_TIMEOUT_SECONDS = 5
INSTALLER_COMMAND_TIMEOUT_SECONDS = 180
MAX_INSTALLER_CHILD_OUTPUT_BYTES = 1024 * 1024
MAX_TEMPORARY_POSTGRES_OUTPUT_BYTES = 8 * 1024 * 1024
LAUNCHCTL_PRINT_NOT_FOUND_STATUS = 113
POSTGRES_PORT = 55_432
POSTGRES_DATABASE = "decodex"
POSTGRES_SCHEMA_ROLE = "decodex_schema_owner"
POSTGRES_RUNTIME_ROLE = "decodex_runtime"
UUID_PATTERN = re.compile(
    r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"
)
HEX_DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
DAEMON_WRAPPER_TOOL = Path(__file__).resolve().with_name("decodexd_wrapper.py")
CODESIGN = Path("/usr/bin/codesign")


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
    active_process_id: Optional[int]
    root: Optional[ProcessIdentity]
    generation: frozenset[ProcessIdentity]


@dataclass(frozen=True)
class InstallPaths:
    repository: Path
    root: Path
    config: Path
    data_directory: Path
    socket_directory: Path
    log_directory: Path
    postgres_log: Path
    service_log: Path
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

    @property
    def daemon_wrapper(self) -> Path:
        try:
            contents = self.decodexd.parent.parent
            wrapper = contents.parent
        except IndexError as error:
            raise InstallError("daemon wrapper main path is invalid") from error
        if (
            self.decodexd.name != "decodexd"
            or self.decodexd.parent.name != "MacOS"
            or contents.name != "Contents"
            or wrapper.name != "decodexd.app"
        ):
            raise InstallError("daemon wrapper main path is invalid")
        return wrapper


class InstallerNamespaceLock:
    """Retain exclusive ownership of the local-listener namespace during install."""

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
        lock_descriptor: Optional[int] = None
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

    def close(self) -> None:
        if self.closed:
            return
        self.closed = True
        failure: Optional[OSError] = None
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


def parse_args(argv: Optional[list[str]] = None) -> argparse.Namespace:
    home = Path.home()
    discovered_codex = shutil.which("codex")
    parser = argparse.ArgumentParser(
        description="Install the current Decodex PostgreSQL 18 local service."
    )
    parser.add_argument(
        "--repository",
        type=Path,
        default=Path(__file__).resolve().parents[2],
    )
    parser.add_argument("--root", type=Path, default=home / ".decodex")
    parser.add_argument(
        "--launch-agent",
        type=Path,
        default=home / "Library" / "LaunchAgents" / f"{LAUNCH_AGENT_LABEL}.plist",
    )
    parser.add_argument(
        "--decodexd",
        type=Path,
        default=(
            home
            / ".local"
            / "bin"
            / "decodexd.app"
            / "Contents"
            / "MacOS"
            / "decodexd"
        ),
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
        data_directory=root / "postgres" / "data",
        socket_directory=root / "postgres" / "socket",
        log_directory=root / "logs",
        postgres_log=root / "logs" / "postgres.log",
        service_log=root / "logs" / "local-service.log",
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
        metadata = path.lstat()
    except OSError as error:
        raise InstallError(f"{name} executable is unavailable") from error
    if (
        not stat.S_ISREG(metadata.st_mode)
        or stat.S_ISLNK(metadata.st_mode)
        or metadata.st_nlink != 1
        or not os.access(path, os.X_OK)
    ):
        raise InstallError(f"{name} executable is unavailable")


def contains_control(value: str) -> bool:
    return any(ord(character) < 0x20 or ord(character) == 0x7F for character in value)


def managed_path_exists(path: Path, failure: str) -> bool:
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return False
    except OSError as error:
        raise InstallError(failure) from error
    if stat.S_ISLNK(metadata.st_mode):
        raise InstallError(failure)
    return True


def read_owned_file(
    path: Path,
    uid: int,
    maximum_bytes: int,
    failure: str,
    *,
    required_mode: Optional[int] = None,
) -> bytes:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
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
            or metadata.st_size < 0
            or metadata.st_size > maximum_bytes
            or (
                required_mode is not None
                and stat.S_IMODE(metadata.st_mode) != required_mode
            )
        ):
            raise InstallError(failure)
        chunks: list[bytes] = []
        remaining = maximum_bytes + 1
        while remaining > 0:
            chunk = os.read(descriptor, min(64 * 1024, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        body = b"".join(chunks)
        if len(body) > maximum_bytes:
            raise InstallError(failure)
        return body
    finally:
        os.close(descriptor)


def toml_string(value: Union[str, Path]) -> str:
    return json.dumps(str(value), ensure_ascii=False)


def render_config(paths: InstallPaths, uid: int) -> bytes:
    lines = [
        "version = 1",
        'active_profile = "local"',
        "",
        "[profiles.local]",
        'kind = "local"',
        'policy = "same_uid"',
        f"service_owner_uid = {uid}",
        "",
        "[postgres]",
        f"socket_directory = {toml_string(paths.socket_directory)}",
        f"expected_peer_uid = {uid}",
        f"port = {POSTGRES_PORT}",
        f'database = "{POSTGRES_DATABASE}"',
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
    return "\n".join(lines).encode("utf-8")


def daemon_wrapper_digest(value: Any) -> str:
    try:
        normalized = json.dumps(
            value,
            ensure_ascii=True,
            allow_nan=False,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("ascii")
    except (TypeError, ValueError, UnicodeEncodeError) as error:
        raise InstallError("daemon wrapper descriptor is malformed") from error
    return hashlib.sha256(normalized).hexdigest()


def inspect_daemon_wrapper(paths: InstallPaths) -> dict[str, Any]:
    try:
        completed = run(
            [
                sys.executable,
                str(DAEMON_WRAPPER_TOOL),
                "inspect",
                "--wrapper",
                str(paths.daemon_wrapper),
            ],
            cwd=paths.repository,
            capture=True,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise InstallError("daemon wrapper identity did not verify") from error
    try:
        result = json.loads(completed.stdout)
    except (TypeError, json.JSONDecodeError) as error:
        raise InstallError("daemon wrapper inspector returned an invalid result") from error
    if (
        not isinstance(result, dict)
        or set(result) != {"schema", "descriptor", "descriptor_sha256"}
        or result.get("schema") != "decodex/daemon-wrapper-result/1"
        or not isinstance(result.get("descriptor"), dict)
        or not isinstance(result.get("descriptor_sha256"), str)
        or not HEX_DIGEST_PATTERN.fullmatch(result["descriptor_sha256"])
        or daemon_wrapper_digest(result["descriptor"]) != result["descriptor_sha256"]
        or result["descriptor"].get("executable_path") != str(paths.decodexd)
    ):
        raise InstallError("daemon wrapper identity did not verify")
    return result["descriptor"]


def verify_daemon_wrapper(
    paths: InstallPaths,
    expected: dict[str, Any],
    *,
    require_launch_agent: bool,
) -> dict[str, Any]:
    current = inspect_daemon_wrapper(paths)
    if daemon_wrapper_digest(current) != daemon_wrapper_digest(expected):
        raise InstallError("daemon wrapper identity differs")
    if require_launch_agent:
        body = read_owned_file(
            paths.launch_agent,
            os.geteuid(),
            MAX_LAUNCH_AGENT_FILE_BYTES,
            "LaunchAgent is malformed",
            required_mode=0o600,
        )
        try:
            launch_agent = plistlib.loads(body)
        except (TypeError, ValueError, plistlib.InvalidFileException) as error:
            raise InstallError("LaunchAgent is malformed") from error
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
            raise InstallError("LaunchAgent daemon wrapper identity differs")
    return current


def verify_signed_cli(path: Path, expected_team_identifier: str) -> dict[str, str]:
    if (
        not expected_team_identifier
        or contains_control(expected_team_identifier)
        or len(expected_team_identifier.encode("utf-8")) > 64
    ):
        raise InstallError("daemon wrapper signing identity is invalid")
    try:
        run(
            [
                str(CODESIGN),
                "--verify",
                "--strict",
                "--all-architectures",
                str(path),
            ],
            capture=True,
        )
        details = run(
            [str(CODESIGN), "-d", "--verbose=4", str(path)],
            capture=True,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise InstallError("Decodex CLI signature did not verify") from error
    output = "\n".join(
        part for part in (details.stdout, details.stderr) if isinstance(part, str)
    )
    identifiers = re.findall(r"^Identifier=(.+)$", output, re.MULTILINE)
    teams = re.findall(r"^TeamIdentifier=(.+)$", output, re.MULTILINE)
    code_directories = re.findall(r"^CodeDirectory .+$", output, re.MULTILINE)
    if (
        len(identifiers) != 1
        or not identifiers[0]
        or contains_control(identifiers[0])
        or teams != [expected_team_identifier]
        or len(code_directories) != 1
        or "(runtime)" not in code_directories[0]
    ):
        raise InstallError("Decodex CLI signature did not verify")
    return {
        "identifier": identifiers[0],
        "team_identifier": teams[0],
    }


def render_launch_agent(
    paths: InstallPaths,
    daemon_wrapper: dict[str, Any],
) -> bytes:
    arguments = [
        daemon_wrapper["executable_path"],
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
    flags |= getattr(os, "O_NOFOLLOW", 0)
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
        directory_flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0)
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
    cwd: Optional[Path] = None,
    env: Optional[dict[str, str]] = None,
    capture: bool = False,
    check: bool = True,
    timeout: Optional[float] = INSTALLER_COMMAND_TIMEOUT_SECONDS,
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
        start_new_session=True,
    )
    stdout_bytes, stderr_bytes = communicate_bounded(process, command, timeout)
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
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=0.25)
    except subprocess.TimeoutExpired:
        pass
    try:
        os.killpg(process.pid, signal.SIGKILL)
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
        primary_error = InstallError("installer child output pipes are unavailable")
        try:
            terminate_bounded_process(process)
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
    except BaseException as primary_error:
        try:
            terminate_bounded_process(process)
        except BaseException as cleanup_error:
            raise primary_error from cleanup_error
        raise
    finally:
        selector.close()
        for _, stream in streams.values():
            if not stream.closed:
                stream.close()
    return b"".join(chunks["stdout"]), b"".join(chunks["stderr"])


def postgres_major(postgres: Path) -> int:
    completed = run([str(postgres), "--version"], capture=True)
    match = re.search(r"\b([0-9]+)(?:\.[0-9]+)?\b", completed.stdout)
    if match is None:
        raise InstallError("PostgreSQL version is unavailable")
    return int(match.group(1))


def open_private_append_file(path: Path, uid: int) -> int:
    flags = os.O_WRONLY | os.O_CREAT | os.O_APPEND
    flags |= getattr(os, "O_NOFOLLOW", 0)
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
    for path in (
        paths.root,
        paths.data_directory.parent,
        paths.data_directory,
        paths.socket_directory,
        paths.log_directory,
        paths.server_directory,
    ):
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


def postgres_version(paths: InstallPaths, uid: int) -> Optional[str]:
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
        self._failure: Optional[str] = None
        self._failure_lock = threading.Lock()
        self._settle_requested = threading.Event()
        self._thread = threading.Thread(
            target=self._drain,
            name="decodex-postgres-output",
            daemon=True,
        )

    @property
    def failure(self) -> Optional[str]:
        with self._failure_lock:
            return self._failure

    def start(self) -> None:
        self._thread.start()

    def settle(self) -> None:
        self._settle_requested.set()
        self._thread.join(timeout=LOCAL_SERVICE_CONTROL_TIMEOUT_SECONDS)
        if self._thread.is_alive():
            raise InstallError("temporary PostgreSQL output did not settle")
        if self.failure is not None:
            raise InstallError(self.failure)

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
) -> Optional[_TemporaryPostgresOutput]:
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
    termination_error: Optional[BaseException] = None
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

    output_error: Optional[BaseException] = None
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


def ensure_roles_and_database(paths: InstallPaths, env: dict[str, str]) -> bool:
    database_exists = psql_scalar(
        paths,
        "postgres",
        "SELECT 1 FROM pg_catalog.pg_database "
        f"WHERE datname='{POSTGRES_DATABASE}'",
        env,
    )
    database_created = database_exists != "1"
    for role in (POSTGRES_SCHEMA_ROLE, POSTGRES_RUNTIME_ROLE):
        exists = psql_scalar(
            paths,
            "postgres",
            f"SELECT 1 FROM pg_catalog.pg_roles WHERE rolname='{role}'",
            env,
        )
        if exists != "1":
            if not database_created:
                raise InstallError("existing PostgreSQL role authority is incomplete")
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
    if database_created:
        psql_scalar(
            paths,
            "postgres",
            f"CREATE DATABASE {POSTGRES_DATABASE} WITH TEMPLATE template0 "
            f"ENCODING 'UTF8' OWNER {POSTGRES_SCHEMA_ROLE}",
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
    if owner != POSTGRES_SCHEMA_ROLE:
        raise InstallError("existing Decodex database has an unexpected owner")
    if database_created:
        psql_scalar(
            paths,
            POSTGRES_DATABASE,
            f"GRANT USAGE, CREATE ON SCHEMA public TO {POSTGRES_SCHEMA_ROLE}",
            env,
        )
        psql_scalar(
            paths,
            "postgres",
            f"REVOKE CREATE ON DATABASE {POSTGRES_DATABASE} FROM PUBLIC; "
            f"GRANT CONNECT, CREATE ON DATABASE {POSTGRES_DATABASE} "
            f"TO {POSTGRES_SCHEMA_ROLE}; "
            f"GRANT CONNECT ON DATABASE {POSTGRES_DATABASE} TO {POSTGRES_RUNTIME_ROLE}",
            env,
        )
    return database_created


def parse_launch_agent_pid(output: str) -> Optional[int]:
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
    root_process_id: int,
    processes: dict[int, ProcessRecord],
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
    process_identities: set[ProcessIdentity],
    deadline: float,
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
            required_mode=0o600,
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


def query_accounts(paths: InstallPaths) -> Optional[dict[str, Any]]:
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


def service_foundation_is_ready(document: Any) -> bool:
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
        "product_store",
        "protocol",
        "protocol_version",
        "server_identity",
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
    return service_foundation_is_ready(document)


def account_ids_from_result(document: dict[str, Any]) -> Optional[list[str]]:
    result = document.get("result")
    if (
        not isinstance(result, dict)
        or set(result) != {"outcome", "data"}
        or result.get("outcome") != "available"
    ):
        return None
    data = result.get("data")
    if not isinstance(data, dict) or set(data) != {"accounts", "routing"}:
        return None
    accounts = data.get("accounts")
    routing = data.get("routing")
    if not isinstance(accounts, list) or not isinstance(routing, dict):
        return None
    account_ids: list[str] = []
    for account in accounts:
        account_id = account.get("account_id") if isinstance(account, dict) else None
        if not isinstance(account_id, str) or not UUID_PATTERN.fullmatch(account_id):
            return None
        account_ids.append(account_id)
    if len(set(account_ids)) != len(account_ids):
        return None
    order = routing.get("order")
    revision = routing.get("revision")
    mode = routing.get("mode")
    if (
        type(revision) is not int
        or revision <= 0
        or not isinstance(order, list)
        or any(
            not isinstance(account_id, str)
            or not UUID_PATTERN.fullmatch(account_id)
            for account_id in order
        )
        or len(order) != len(set(order))
        or set(order) != set(account_ids)
        or not isinstance(mode, dict)
        or mode.get("mode") not in {"balanced", "fixed"}
    ):
        return None
    if mode["mode"] == "balanced":
        if set(mode) != {"mode"}:
            return None
    else:
        if (
            set(mode) != {"mode", "account_id"}
            or mode.get("account_id") not in account_ids
        ):
            return None
    return account_ids


def wait_for_service(paths: InstallPaths) -> list[str]:
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
        else:
            return account_ids
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
    if paths.root != user_home / ".decodex":
        raise InstallError("the local service root must be the platform default")
    if paths.codex.name != "codex":
        raise InstallError("Codex executable name is invalid")
    for name, path in (
        ("decodexd", paths.decodexd),
        ("Decodex CLI", paths.decodex_cli),
        ("Codex", paths.codex),
        ("postgres", paths.postgres),
        ("initdb", paths.initdb),
        ("pg_isready", paths.pg_isready),
        ("psql", paths.psql),
        ("codesign", CODESIGN),
    ):
        require_regular_executable(path, name)
    return uid


def install_under_namespace_lock(
    paths: InstallPaths,
    uid: int,
    namespace_lock: InstallerNamespaceLock,
    daemon_wrapper: dict[str, Any],
) -> None:
    namespace_lock.verify()
    ensure_directories(paths, uid)
    verify_daemon_wrapper(paths, daemon_wrapper, require_launch_agent=False)
    team_identifier = daemon_wrapper.get("team_identifier")
    if not isinstance(team_identifier, str):
        raise InstallError("daemon wrapper signing identity is invalid")
    verify_signed_cli(paths.decodex_cli, team_identifier)

    config = render_config(paths, uid)
    launch_agent = render_launch_agent(paths, daemon_wrapper)
    atomic_write(paths.config, config, 0o600)
    atomic_write(paths.launch_agent, launch_agent, 0o600)
    if read_owned_file(
        paths.config,
        uid,
        MAX_CONFIG_FILE_BYTES,
        "installed Decodex config differs",
        required_mode=0o600,
    ) != config:
        raise InstallError("installed Decodex config differs")
    if read_owned_file(
        paths.launch_agent,
        uid,
        MAX_LAUNCH_AGENT_FILE_BYTES,
        "installed LaunchAgent differs",
        required_mode=0o600,
    ) != launch_agent:
        raise InstallError("installed LaunchAgent differs")

    initialize_cluster(paths, uid)
    postgres = start_temporary_postgres(paths)
    try:
        environment = psql_environment(paths)
        database_created = ensure_roles_and_database(paths, environment)
        if database_created:
            bootstrap_latest_schema(paths)
        else:
            validate_current_authority(paths)
    finally:
        stop_temporary_postgres(postgres)

    namespace_lock.verify()
    verify_daemon_wrapper(paths, daemon_wrapper, require_launch_agent=True)
    verify_signed_cli(paths.decodex_cli, team_identifier)


def bootstrap_latest_schema(paths: InstallPaths) -> None:
    try:
        run(
            [
                str(paths.decodexd),
                "bootstrap-latest-schema",
                "--root",
                str(paths.root),
                "--schema-owner-user",
                POSTGRES_SCHEMA_ROLE,
            ],
            cwd=paths.repository,
            capture=True,
        )
    except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        raise InstallError("latest-schema bootstrap failed") from error


def validate_current_authority(paths: InstallPaths) -> None:
    try:
        run(
            [
                str(paths.decodexd),
                "validate-current-authority",
                "--root",
                str(paths.root),
            ],
            cwd=paths.repository,
            capture=True,
        )
    except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        raise InstallError("current PostgreSQL authority validation failed") from error


def main(argv: Optional[list[str]] = None) -> int:
    args = parse_args(argv)
    paths = install_paths(args)
    uid = validate_host(paths)
    daemon_wrapper = inspect_daemon_wrapper(paths)
    team_identifier = daemon_wrapper.get("team_identifier")
    if not isinstance(team_identifier, str):
        raise InstallError("daemon wrapper signing identity is invalid")
    verify_signed_cli(paths.decodex_cli, team_identifier)
    ensure_installer_namespace_layout(paths, uid)
    if postgres_major(paths.postgres) != 18:
        raise InstallError("PostgreSQL 18 is required")

    bootout_service(paths, uid)
    namespace_lock = InstallerNamespaceLock.acquire(paths, uid)
    try:
        install_under_namespace_lock(
            paths,
            uid,
            namespace_lock,
            daemon_wrapper,
        )
    finally:
        namespace_lock.close()

    launch = not args.no_launch
    account_ids: list[str] = []
    if launch:
        bootstrap_service(paths, uid)
        account_ids = wait_for_service(paths)

    print(
        json.dumps(
            {
                "schema": "decodex/local-service-install/1",
                "outcome": "success",
                "account_count": len(account_ids),
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
