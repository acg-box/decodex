#!/usr/bin/env python3
"""Install the self-contained Decodex SQLite local service on macOS.

The installer retains the existing signature, namespace, and graceful-drain
boundaries. Fresh installs initialize only the bundled SQLite database. During a
bounded upgrade, it captures the running daemon's credential-negative account
snapshot, stops the old service, and invokes the one-shot retired-vault transfer.
It never deletes retained rollback data.
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
import time
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional


LAUNCH_AGENT_LABEL = "space.decodex.local-service"
MAX_CONFIG_FILE_BYTES = 1024 * 1024
MAX_LAUNCH_AGENT_FILE_BYTES = 64 * 1024
MAX_ACCOUNT_SNAPSHOT_BYTES = 4 * 1024 * 1024
MAX_EXECUTABLE_BYTES = 512 * 1024 * 1024
MAX_INSTALLER_CHILD_OUTPUT_BYTES = 4 * 1024 * 1024
INSTALLER_COMMAND_TIMEOUT_SECONDS = 180
LOCAL_SERVICE_SETTLEMENT_TIMEOUT_SECONDS = 300
LOCAL_SERVICE_SETTLEMENT_POLL_SECONDS = 0.25
LOCAL_SERVICE_CONTROL_TIMEOUT_SECONDS = 5
LAUNCHCTL_PRINT_NOT_FOUND_STATUS = 113
CODESIGN = Path("/usr/bin/codesign")
UUID_PATTERN = re.compile(
    r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"
)


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
    database: Path
    retired_vault: Path
    log_directory: Path
    service_log: Path
    launch_agent: Path
    decodexd: Path
    decodex_cli: Path
    database_transfer: Path
    codex: Path

    @property
    def server_directory(self) -> Path:
        return self.root / "server"

    @property
    def namespace_lock(self) -> Path:
        return self.server_directory / "decodex.lock"


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
            current_directory_descriptor = open_absolute_directory(
                self.paths.server_directory
            )
        except OSError as error:
            raise InstallError("local service namespace ownership changed") from error
        try:
            current_directory = os.fstat(current_directory_descriptor)
            require_namespace_directory(held_directory, self.uid)
            require_namespace_directory(current_directory, self.uid)
            require_namespace_lock(held_lock, self.uid)
            pinned_lock = os.stat(
                "decodex.lock",
                dir_fd=self.directory_descriptor,
                follow_symlinks=False,
            )
            current_lock = os.stat(
                "decodex.lock",
                dir_fd=current_directory_descriptor,
                follow_symlinks=False,
            )
            require_namespace_lock(pinned_lock, self.uid)
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
        description="Install the self-contained Decodex SQLite local service."
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
        default=home / ".local" / "bin" / "decodexd",
    )
    parser.add_argument(
        "--decodex-cli",
        type=Path,
        default=home / ".local" / "bin" / "decodex",
    )
    parser.add_argument(
        "--database-transfer",
        type=Path,
        default=home / ".local" / "bin" / "decodex-database-transfer",
    )
    parser.add_argument(
        "--codex",
        type=Path,
        default=(
            Path(discovered_codex)
            if discovered_codex is not None
            else home / ".codex" / "shims" / "codex"
        ),
        help="Codex executable made discoverable to the daemon.",
    )
    parser.add_argument(
        "--no-launch",
        action="store_true",
        help="Install and validate files, but do not start the LaunchAgent.",
    )
    return parser.parse_args(argv)


def install_paths(args: argparse.Namespace) -> InstallPaths:
    root = args.root.expanduser().resolve()
    return InstallPaths(
        repository=args.repository.expanduser().resolve(),
        root=root,
        config=root / "config.toml",
        database=root / "server" / "decodex.sqlite3",
        retired_vault=root / "server" / "credentials.redb",
        log_directory=root / "logs",
        service_log=root / "logs" / "local-service.log",
        launch_agent=args.launch_agent.expanduser().resolve(),
        decodexd=args.decodexd.expanduser().resolve(),
        decodex_cli=args.decodex_cli.expanduser().resolve(),
        database_transfer=args.database_transfer.expanduser().resolve(),
        codex=args.codex.expanduser().resolve(),
    )


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


def require_owned_private_file(path: Path, uid: int, failure: str) -> os.stat_result:
    try:
        metadata = path.lstat()
    except OSError as error:
        raise InstallError(failure) from error
    if (
        not stat.S_ISREG(metadata.st_mode)
        or stat.S_ISLNK(metadata.st_mode)
        or metadata.st_uid != uid
        or metadata.st_nlink != 1
        or stat.S_IMODE(metadata.st_mode) != 0o600
    ):
        raise InstallError(failure)
    return metadata


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


def render_config(paths: InstallPaths, uid: int) -> bytes:
    del paths
    lines = [
        "version = 1",
        'active_profile = "local"',
        "",
        "[profiles.local]",
        'kind = "local"',
        'policy = "same_uid"',
        f"service_owner_uid = {uid}",
        "",
        "[cache]",
        "max_entries = 2048",
        "max_bytes = 134217728",
        "max_entry_bytes = 4194304",
        "",
    ]
    return "\n".join(lines).encode("utf-8")


def executable_sha256(path: Path, name: str) -> str:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    failure = f"{name} executable authority is unsafe"
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise InstallError(failure) from error
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.geteuid()
            or metadata.st_nlink != 1
            or metadata.st_size <= 0
            or metadata.st_size > MAX_EXECUTABLE_BYTES
            or stat.S_IMODE(metadata.st_mode) & 0o022
            or not stat.S_IMODE(metadata.st_mode) & 0o100
        ):
            raise InstallError(failure)
        digest = hashlib.sha256()
        remaining = metadata.st_size
        while remaining > 0:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                raise InstallError(failure)
            digest.update(chunk)
            remaining -= len(chunk)
        if os.read(descriptor, 1):
            raise InstallError(failure)
        return digest.hexdigest()
    finally:
        os.close(descriptor)


def inspect_signed_executable(path: Path, name: str) -> dict[str, str]:
    digest = executable_sha256(path, name)
    failure = f"{name} signature did not verify"
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
        raise InstallError(failure) from error
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
        or len(teams) != 1
        or not teams[0]
        or teams[0] == "not set"
        or contains_control(teams[0])
        or len(code_directories) != 1
        or "(runtime)" not in code_directories[0]
    ):
        raise InstallError(failure)
    return {
        "identifier": identifiers[0],
        "team_identifier": teams[0],
        "sha256": digest,
    }


def inspect_daemon_executable(paths: InstallPaths) -> dict[str, str]:
    descriptor = inspect_signed_executable(paths.decodexd, "Decodex daemon")
    if descriptor["identifier"] != "box.acg.decodex.daemon":
        raise InstallError("Decodex daemon signature did not verify")
    return descriptor


def verify_signed_peer(
    path: Path,
    name: str,
    expected_team_identifier: str,
    expected_identifier: Optional[str] = None,
) -> dict[str, str]:
    if (
        not expected_team_identifier
        or contains_control(expected_team_identifier)
        or len(expected_team_identifier.encode("utf-8")) > 64
    ):
        raise InstallError("Decodex daemon signing identity is invalid")
    descriptor = inspect_signed_executable(path, name)
    if descriptor["team_identifier"] != expected_team_identifier or (
        expected_identifier is not None
        and descriptor["identifier"] != expected_identifier
    ):
        raise InstallError(f"{name} signature did not verify")
    return descriptor


def verify_daemon_executable(
    paths: InstallPaths,
    expected: dict[str, str],
    *,
    require_launch_agent: bool,
) -> dict[str, str]:
    current = inspect_daemon_executable(paths)
    if current != expected:
        raise InstallError("Decodex daemon identity differs")
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
        if arguments != [str(paths.decodexd), "serve"]:
            raise InstallError("LaunchAgent daemon identity differs")
    return current


def render_launch_agent(paths: InstallPaths) -> bytes:
    payload = {
        "Label": LAUNCH_AGENT_LABEL,
        "ProgramArguments": [str(paths.decodexd), "serve"],
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
    input_bytes: Optional[bytes] = None,
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
        stdin=subprocess.PIPE if input_bytes is not None else subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    if input_bytes is not None:
        if process.stdin is None:
            terminate_bounded_process(process)
            raise InstallError("installer child input pipe is unavailable")
        try:
            process.stdin.write(input_bytes)
            process.stdin.close()
        except (BrokenPipeError, OSError) as error:
            terminate_bounded_process(process)
            raise InstallError("installer child input was refused") from error
    stdout_bytes, stderr_bytes = communicate_bounded(process, command, timeout)
    try:
        stdout = stdout_bytes.decode("utf-8", errors="strict")
        stderr = stderr_bytes.decode("utf-8", errors="strict")
    except UnicodeDecodeError as error:
        raise InstallError("installer child output is malformed") from error
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
                raise subprocess.TimeoutExpired(command, timeout)
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
            raise subprocess.TimeoutExpired(command, timeout)
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
        paths.log_directory,
        paths.root / "blobs",
        paths.root / "blobs" / "sha256",
        paths.root / "cache",
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


def settlement_command_timeout(deadline: float, maximum: float) -> float:
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


def parse_launch_agent_pid(output: str) -> Optional[int]:
    matches = re.findall(r"^\s*pid = ([0-9]+)\s*$", output, re.MULTILINE)
    if not matches:
        return None
    if len(matches) != 1:
        raise InstallError("local service state is malformed")
    process_id = int(matches[0])
    if process_id <= 0:
        raise InstallError("local service state is malformed")
    return process_id


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
    while process_identities:
        current = process_parent_map(deadline)
        process_identities = {
            identity
            for identity in process_identities
            if current.get(identity.process_id) is not None
            and current[identity.process_id].identity == identity
        }
        if not process_identities:
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


def bootout_service(paths: InstallPaths, uid: int) -> bool:
    deadline = time.monotonic() + LOCAL_SERVICE_SETTLEMENT_TIMEOUT_SECONDS
    service = f"gui/{uid}/{LAUNCH_AGENT_LABEL}"
    observed = observe_service(service, deadline)
    was_loaded = observed.loaded
    generation = set(observed.generation)
    if installed_launch_agent_supports_graceful_drain(paths.launch_agent, uid):
        observed = drain_service(service, observed, generation, deadline)
    completed = run_settlement_command(
        ["/bin/launchctl", "bootout", service],
        deadline,
        "existing local service could not be stopped",
        LOCAL_SERVICE_SETTLEMENT_TIMEOUT_SECONDS,
    )
    if completed.returncode != 0:
        loaded = observe_service(service, deadline)
        generation.update(loaded.generation)
        if loaded.loaded:
            raise InstallError("existing local service could not be stopped")
    wait_for_process_generation_exit(generation, deadline)
    return was_loaded


def bootstrap_service(paths: InstallPaths, uid: int) -> None:
    service = f"gui/{uid}/{LAUNCH_AGENT_LABEL}"
    for command in (
        ["/bin/launchctl", "bootstrap", f"gui/{uid}", str(paths.launch_agent)],
        ["/bin/launchctl", "kickstart", service],
    ):
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
        or order != account_ids
        or not isinstance(mode, dict)
        or mode.get("mode") not in {"balanced", "fixed"}
    ):
        return None
    if mode["mode"] == "balanced":
        if set(mode) != {"mode"}:
            return None
    elif set(mode) != {"mode", "account_id"} or mode.get("account_id") not in account_ids:
        return None
    return account_ids


def capture_retired_account_snapshot(paths: InstallPaths, uid: int) -> Optional[bytes]:
    if managed_path_exists(paths.database, "SQLite database authority is unsafe"):
        require_owned_private_file(
            paths.database,
            uid,
            "SQLite database authority is unsafe",
        )
        return None
    if not managed_path_exists(
        paths.retired_vault,
        "retired credential source is unsafe",
    ):
        return None
    require_owned_private_file(
        paths.retired_vault,
        uid,
        "retired credential source is unsafe",
    )
    document = query_accounts(paths)
    account_ids = account_ids_from_result(document) if document is not None else None
    if not account_ids:
        raise InstallError("retired account snapshot is unavailable")
    body = json.dumps(
        document,
        ensure_ascii=True,
        allow_nan=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("ascii")
    if len(body) > MAX_ACCOUNT_SNAPSHOT_BYTES:
        raise InstallError("retired account snapshot is oversized")
    return body


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


def initialize_local_database(paths: InstallPaths) -> None:
    try:
        run(
            [
                str(paths.decodexd),
                "initialize-local-database",
                "--root",
                str(paths.root),
            ],
            cwd=paths.repository,
            capture=True,
        )
    except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        raise InstallError("local SQLite database initialization failed") from error


def validate_local_database(paths: InstallPaths) -> None:
    try:
        run(
            [
                str(paths.decodexd),
                "validate-local-database",
                "--root",
                str(paths.root),
            ],
            cwd=paths.repository,
            capture=True,
        )
    except (OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        raise InstallError("local SQLite database validation failed") from error


def transfer_retired_accounts(
    paths: InstallPaths,
    snapshot: bytes,
    team_identifier: str,
) -> int:
    require_regular_executable(paths.database_transfer, "Decodex database transfer")
    verify_signed_peer(
        paths.database_transfer,
        "Decodex database transfer",
        team_identifier,
        "box.acg.decodex.database-transfer",
    )
    try:
        completed = run(
            [
                str(paths.database_transfer),
                "--root",
                str(paths.root),
            ],
            cwd=paths.repository,
            input_bytes=snapshot,
            capture=True,
        )
        result = json.loads(completed.stdout)
    except (
        OSError,
        subprocess.CalledProcessError,
        subprocess.TimeoutExpired,
        json.JSONDecodeError,
    ) as error:
        raise InstallError("retired account transfer failed") from error
    if (
        not isinstance(result, dict)
        or result.get("schema") != "decodex/local-account-transfer/1"
        or result.get("outcome") not in {"imported", "replayed"}
        or type(result.get("account_count")) is not int
        or result["account_count"] <= 0
        or result.get("source_vault_retained") is not True
    ):
        raise InstallError("retired account transfer readback is malformed")
    return result["account_count"]


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
    if paths.decodexd.name != "decodexd":
        raise InstallError("Decodex daemon executable name is invalid")
    for name, path in (
        ("decodexd", paths.decodexd),
        ("Decodex CLI", paths.decodex_cli),
        ("Codex", paths.codex),
        ("codesign", CODESIGN),
    ):
        require_regular_executable(path, name)
    return uid


def install_under_namespace_lock(
    paths: InstallPaths,
    uid: int,
    namespace_lock: InstallerNamespaceLock,
    daemon_executable: dict[str, str],
    account_snapshot: Optional[bytes],
) -> int:
    namespace_lock.verify()
    ensure_directories(paths, uid)
    verify_daemon_executable(paths, daemon_executable, require_launch_agent=False)
    team_identifier = daemon_executable.get("team_identifier")
    if not isinstance(team_identifier, str):
        raise InstallError("Decodex daemon signing identity is invalid")
    verify_signed_peer(paths.decodex_cli, "Decodex CLI", team_identifier)

    transferred_accounts = 0
    if account_snapshot is None:
        initialize_local_database(paths)
    else:
        transferred_accounts = transfer_retired_accounts(
            paths,
            account_snapshot,
            team_identifier,
        )
    validate_local_database(paths)
    require_owned_private_file(
        paths.database,
        uid,
        "SQLite database authority is unsafe",
    )

    config = render_config(paths, uid)
    launch_agent = render_launch_agent(paths)
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

    namespace_lock.verify()
    verify_daemon_executable(paths, daemon_executable, require_launch_agent=True)
    verify_signed_peer(paths.decodex_cli, "Decodex CLI", team_identifier)
    return transferred_accounts


def main(argv: Optional[list[str]] = None) -> int:
    args = parse_args(argv)
    paths = install_paths(args)
    uid = validate_host(paths)
    daemon_executable = inspect_daemon_executable(paths)
    team_identifier = daemon_executable.get("team_identifier")
    if not isinstance(team_identifier, str):
        raise InstallError("Decodex daemon signing identity is invalid")
    verify_signed_peer(paths.decodex_cli, "Decodex CLI", team_identifier)
    ensure_installer_namespace_layout(paths, uid)

    account_snapshot = capture_retired_account_snapshot(paths, uid)
    bootout_service(paths, uid)
    namespace_lock = InstallerNamespaceLock.acquire(paths, uid)
    try:
        transferred_accounts = install_under_namespace_lock(
            paths,
            uid,
            namespace_lock,
            daemon_executable,
            account_snapshot,
        )
    finally:
        namespace_lock.close()

    launch = not args.no_launch
    account_ids: list[str] = []
    if launch:
        bootstrap_service(paths, uid)
        account_ids = wait_for_service(paths)
        if account_snapshot is not None and len(account_ids) != transferred_accounts:
            raise InstallError("transferred account readback differs")

    print(
        json.dumps(
            {
                "schema": "decodex/local-service-install/1",
                "outcome": "success",
                "database": "sqlite",
                "account_count": len(account_ids) if launch else transferred_accounts,
                "account_transfer": (
                    "completed" if account_snapshot is not None else "not_required"
                ),
                "retired_sources_retained": True,
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
