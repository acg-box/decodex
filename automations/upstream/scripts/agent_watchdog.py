#!/usr/bin/env python3
"""Supervise one ephemeral Codex child and remove its authentication capsule."""

from __future__ import annotations

import argparse
import fcntl
import json
import os
from pathlib import Path
import secrets
import signal
import stat
import subprocess
import sys
import time


POLL_SECONDS = 0.05
MAX_TIMEOUT_SECONDS = 24 * 60 * 60
MAX_AUTH_BYTES = 64 * 1024
MAX_PROCESS_TABLE_BYTES = 16 * 1024 * 1024
MAX_PROCESS_ARGUMENT_BYTES = 4 * 1024 * 1024
MAX_MARKER_SCAN_BYTES = 64 * 1024 * 1024
SUPERVISION_ENV = "DECODEX_AGENT_SUPERVISION"
_termination_signal: int | None = None


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--parent-pid", type=int, required=True)
    parser.add_argument("--timeout-seconds", type=int, required=True)
    parser.add_argument("--auth-path", type=Path, required=True)
    parser.add_argument("--auth-stdin", action="store_true")
    parser.add_argument("--lock-fd", type=int, required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.command[:1] == ["--"]:
        args.command = args.command[1:]
    if (
        args.parent_pid < 2
        or not 1 <= args.timeout_seconds <= MAX_TIMEOUT_SECONDS
        or not args.auth_path.is_absolute()
        or not args.auth_stdin
        or args.lock_fd < 0
        or not args.command
        or any(
            not isinstance(value, str)
            or not value
            or "\0" in value
            for value in args.command
        )
    ):
        raise SystemExit(64)
    return args


def _record_signal(signum: int, _frame: object) -> None:
    global _termination_signal
    _termination_signal = signum


def _validate_lock(descriptor: int) -> None:
    metadata = os.fstat(descriptor)
    if (
        not stat.S_ISREG(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o600
        or metadata.st_nlink != 1
    ):
        raise OSError("invalid lifecycle lock")
    fcntl.flock(
        descriptor,
        fcntl.LOCK_EX | fcntl.LOCK_NB,
    )


def _create_auth_capsule(path: Path) -> None:
    payload = sys.stdin.buffer.read(MAX_AUTH_BYTES + 1)
    if not 1 <= len(payload) <= MAX_AUTH_BYTES:
        raise OSError("invalid authentication capsule payload")
    try:
        value = json.loads(payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise OSError("invalid authentication capsule payload") from error
    if (
        not isinstance(value, dict)
        or value.get("auth_mode") != "chatgpt"
        or not isinstance(value.get("tokens"), dict)
    ):
        raise OSError("invalid authentication capsule payload")
    directory_descriptor = os.open(
        path.parent,
        os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
    )
    descriptor: int | None = None
    try:
        directory_metadata = os.fstat(directory_descriptor)
        if (
            path.name != "auth.json"
            or not stat.S_ISDIR(directory_metadata.st_mode)
            or directory_metadata.st_uid != os.getuid()
            or stat.S_IMODE(directory_metadata.st_mode) != 0o700
        ):
            raise OSError("invalid authentication capsule directory")
        descriptor = os.open(
            path.name,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
            0o600,
            dir_fd=directory_descriptor,
        )
        offset = 0
        while offset < len(payload):
            written = os.write(descriptor, payload[offset:])
            if written <= 0:
                raise OSError("authentication capsule write failed")
            offset += written
        os.fsync(descriptor)
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_nlink != 1
        ):
            raise OSError("invalid authentication capsule")
        os.fsync(directory_descriptor)
    finally:
        if descriptor is not None:
            os.close(descriptor)
        os.close(directory_descriptor)


def _remove_auth_capsule(path: Path) -> None:
    descriptor: int | None = None
    directory_descriptor: int | None = None
    try:
        directory = path.parent
        directory_descriptor = os.open(
            directory,
            os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
        )
        directory_metadata = os.fstat(directory_descriptor)
        if (
            path.name != "auth.json"
            or not stat.S_ISDIR(directory_metadata.st_mode)
            or directory_metadata.st_uid != os.getuid()
            or stat.S_IMODE(directory_metadata.st_mode) != 0o700
        ):
            raise OSError("invalid authentication capsule directory")
        try:
            descriptor = os.open(
                path.name,
                os.O_RDONLY | os.O_NOFOLLOW,
                dir_fd=directory_descriptor,
            )
        except FileNotFoundError:
            return
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_nlink != 1
        ):
            raise OSError("invalid authentication capsule")
        current = os.stat(
            path.name,
            dir_fd=directory_descriptor,
            follow_symlinks=False,
        )
        if (current.st_dev, current.st_ino) != (
            metadata.st_dev,
            metadata.st_ino,
        ):
            raise OSError("authentication capsule changed")
        os.close(descriptor)
        descriptor = None
        os.unlink(path.name, dir_fd=directory_descriptor)
        os.fsync(directory_descriptor)
    finally:
        if descriptor is not None:
            os.close(descriptor)
        if directory_descriptor is not None:
            os.close(directory_descriptor)


def _normalize_process_start(value: str) -> str:
    fields = value.split()
    if len(fields) != 5:
        raise OSError("process start time invalid")
    return " ".join(fields)


def _process_table() -> dict[int, tuple[int, int, str]]:
    try:
        completed = subprocess.run(
            ["/bin/ps", "-axo", "pid=,ppid=,uid=,lstart="],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            close_fds=True,
            check=False,
            timeout=5,
            env={"LC_ALL": "C", "PATH": "/usr/bin:/bin"},
        )
    except subprocess.TimeoutExpired as error:
        raise OSError("process table timed out") from error
    if (
        completed.returncode != 0
        or not 1 <= len(completed.stdout) <= MAX_PROCESS_TABLE_BYTES
    ):
        raise OSError("process table unavailable")
    table: dict[int, tuple[int, int, str]] = {}
    try:
        lines = completed.stdout.decode(
            "ascii",
            errors="strict",
        ).splitlines()
    except UnicodeDecodeError as error:
        raise OSError("process table invalid") from error
    for raw_line in lines:
        fields = raw_line.strip().split(maxsplit=3)
        if len(fields) != 4:
            raise OSError("process table invalid")
        try:
            pid, parent_pid, uid = (
                int(value) for value in fields[:3]
            )
        except ValueError as error:
            raise OSError("process table invalid") from error
        if pid < 1 or parent_pid < 0 or not fields[3]:
            raise OSError("process table invalid")
        table[pid] = (
            parent_pid,
            uid,
            _normalize_process_start(fields[3]),
        )
    return table


def _refresh_descendants(
    root_pid: int,
    tracked: dict[int, str],
    discovered: dict[int, str] | None = None,
) -> dict[int, str]:
    table = _process_table()
    own_uid = os.getuid()
    root = table.get(root_pid)
    if root is not None:
        if root[1] != own_uid:
            raise OSError("child process ownership changed")
        tracked.setdefault(root_pid, root[2])
    live_tracked = {
        pid: started
        for pid, started in tracked.items()
        if pid in table
        and table[pid][1] == own_uid
        and table[pid][2] == started
    }
    for pid, started in (discovered or {}).items():
        if (
            pid in table
            and table[pid][1] == own_uid
            and table[pid][2] == started
            and pid not in live_tracked
        ):
            live_tracked[pid] = started
    while True:
        added = False
        parents = set(live_tracked)
        for pid, (parent_pid, uid, started) in table.items():
            if (
                uid == own_uid
                and parent_pid in parents
                and pid not in live_tracked
            ):
                live_tracked[pid] = started
                added = True
        if not added:
            break
    tracked.update(live_tracked)
    return live_tracked


def _kill_child_tree(
    process: subprocess.Popen[bytes],
    tracked: dict[int, str],
    supervision_marker: bytes,
) -> None:
    cleanup_error: OSError | None = None
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    try:
        deadline = time.monotonic() + 5
        while True:
            process.poll()
            discovered = _marked_processes(supervision_marker)
            live = _refresh_descendants(
                process.pid,
                tracked,
                discovered,
            )
            live.pop(process.pid, None)
            for pid in sorted(live, reverse=True):
                if pid == os.getpid():
                    raise OSError("watchdog entered child process set")
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            if not live:
                break
            if time.monotonic() >= deadline:
                raise OSError("child process tree survived")
            time.sleep(POLL_SECONDS)
    except OSError as error:
        cleanup_error = error
    finally:
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    if cleanup_error is not None:
        raise cleanup_error


def _marked_processes(marker: bytes) -> dict[int, str]:
    if (
        not marker.startswith(f"{SUPERVISION_ENV}=".encode("ascii"))
        or not 33 <= len(marker) <= 256
        or any(value < 32 or value > 126 for value in marker)
    ):
        raise OSError("invalid supervision marker")
    process = subprocess.Popen(
        [
            "/bin/ps",
            "eww",
            "-U",
            str(os.getuid()),
            "-o",
            "pid=,lstart=,command=",
        ],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        close_fds=True,
        env={"LC_ALL": "C", "PATH": "/usr/bin:/bin"},
    )
    discovered: dict[int, str] = {}
    total_bytes = 0
    try:
        if process.stdout is None:
            raise OSError("process marker scan unavailable")
        while True:
            raw_line = process.stdout.readline(MAX_PROCESS_ARGUMENT_BYTES + 1)
            if not raw_line:
                break
            total_bytes += len(raw_line)
            if (
                len(raw_line) > MAX_PROCESS_ARGUMENT_BYTES
                or total_bytes > MAX_MARKER_SCAN_BYTES
            ):
                raise OSError("process marker scan exceeded bounds")
            fields = raw_line.strip().split(maxsplit=6)
            if len(fields) != 7:
                raise OSError("process marker scan invalid")
            try:
                pid = int(fields[0])
            except ValueError as error:
                raise OSError("process marker scan invalid") from error
            if pid < 1:
                raise OSError("process marker scan invalid")
            if pid != os.getpid() and marker in fields[6]:
                try:
                    discovered[pid] = _normalize_process_start(
                        b" ".join(fields[1:6]).decode(
                            "ascii",
                            errors="strict",
                        )
                    )
                except UnicodeDecodeError as error:
                    raise OSError("process marker scan invalid") from error
        try:
            return_code = process.wait(timeout=5)
        except subprocess.TimeoutExpired as error:
            raise OSError("process marker scan timed out") from error
        if return_code != 0:
            raise OSError("process marker scan failed")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        if process.stdout is not None:
            process.stdout.close()
    return discovered


def main() -> int:
    args = _arguments()
    if os.getppid() != args.parent_pid:
        return 75
    for handled in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
        signal.signal(handled, _record_signal)

    process: subprocess.Popen[bytes] | None = None
    tracked_descendants: dict[int, str] = {}
    supervision_token = os.environ.pop(
        SUPERVISION_ENV,
        secrets.token_urlsafe(32),
    )
    if (
        not isinstance(supervision_token, str)
        or not 32 <= len(supervision_token) <= 128
        or any(
            character
            not in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
            for character in supervision_token
        )
    ):
        return 70
    supervision_marker = (
        f"{SUPERVISION_ENV}={supervision_token}".encode("ascii")
    )
    return_code = 70
    try:
        _validate_lock(args.lock_fd)
        _create_auth_capsule(args.auth_path)
        child_environment = dict(os.environ)
        child_environment[SUPERVISION_ENV] = supervision_token
        process = subprocess.Popen(
            args.command,
            stdin=subprocess.DEVNULL,
            stdout=sys.stdout.buffer,
            stderr=sys.stderr.buffer,
            start_new_session=True,
            close_fds=True,
            env=child_environment,
        )
        deadline = time.monotonic() + args.timeout_seconds
        while True:
            _refresh_descendants(
                process.pid,
                tracked_descendants,
            )
            observed = process.poll()
            if observed is not None:
                return_code = observed
                break
            if _termination_signal is not None:
                return_code = 128 + _termination_signal
                break
            if os.getppid() != args.parent_pid:
                return_code = 75
                break
            if time.monotonic() >= deadline:
                return_code = 124
                break
            time.sleep(POLL_SECONDS)
    except (OSError, ValueError):
        return_code = 70
    finally:
        if process is not None:
            try:
                _kill_child_tree(
                    process,
                    tracked_descendants,
                    supervision_marker,
                )
            except OSError:
                return_code = 74
        try:
            _remove_auth_capsule(args.auth_path)
        except OSError:
            return_code = 74
        supervision_token = ""
        supervision_marker = b""
    return return_code


if __name__ == "__main__":
    raise SystemExit(main())
