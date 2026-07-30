"""Run and validate exact repository-bound automation verification profiles."""

from __future__ import annotations

from contextlib import contextmanager
from dataclasses import dataclass
import fcntl
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import stat
import sys
import tempfile
import tomllib
from typing import Any, Iterator

from .core import (
    FULL_GATE_EXACT_PATHS,
    FULL_GATE_PREFIXES,
    FULL_VALIDATION_PROFILE,
    MAX_STATE_BYTES,
    REQUIRED_VALIDATION_PROFILES,
    REASON_PATTERN,
    SHA_PATTERN,
    STATE_SCHEMA,
    TERMINAL_STATUSES,
    TRUSTED_SYSTEM_EXECUTABLE_ROOTS,
    TRUSTED_SYSTEM_TOOL_DIRECTORIES,
    VALIDATION_AUTHORITY_PATHS,
    VALIDATION_AUTHORITY_PREFIXES,
    VALIDATION_PROFILE_COMMANDS,
    AutopilotError,
    CommandFailure,
    canonical_json,
    command_succeeds,
    ensure_cache_root,
    hash_file_bounded,
    has_exact_keys,
    is_sha256,
    real_home_directory,
    run_command,
    sha256_value,
    target_origin_urls,
    trusted_executable,
    utc_now,
)


PROFILE_RESULT_KEYS = {
    "name",
    "effective_task",
    "command_sha256",
    "environment_sha256",
    "exit_code",
    "output_sha256",
    "toolchain_evidence",
}
VALIDATION_RECEIPT_KEYS = {
    "role",
    "base_head",
    "repository_head",
    "repository_tree",
    "changed_path_count",
    "changed_paths_sha256",
    "candidate_path_classification",
    "candidate_path_policy_sha256",
    "requires_full_gate",
    "sandbox_task_graph_sha256",
    "live_postgres_gate",
    "validation_authority",
    "profiles",
    "completed_at",
}
VALIDATION_AUTHORITY_KEYS = {
    "repository_head",
    "repository_tree",
    "closure_sha256",
}
VALIDATION_TOOL_NAMES = (
    "cargo",
    "cargo-make",
    "cargo-nextest",
    "cp",
    "git",
    "node",
    "npm",
    "python3",
    "rustup",
    "sandbox-exec",
    "taplo",
)
TRUSTED_TOOL_DISCOVERY_PATHS = tuple(
    str(path) for path in TRUSTED_SYSTEM_TOOL_DIRECTORIES
)
FULL_XCODE_EVIDENCE_KEYS = {
    "clang_sha256",
    "clangxx_sha256",
    "developer_dir_sha256",
    "metal_sha256",
    "metallib_sha256",
    "sdk_root_sha256",
    "xcode_select_sha256",
    "xcode_version_sha256",
    "xcodebuild_sha256",
    "xcrun_sha256",
    "xcrun_proxy_sha256",
}
FULL_XCODE_DISCOVERY_EVIDENCE_KEYS = FULL_XCODE_EVIDENCE_KEYS - {
    "xcrun_proxy_sha256"
}
TOOLCHAIN_EVIDENCE_KEYS = {
    "full_xcode",
    "sandbox",
    "validation_tools",
}
SANDBOX_EVIDENCE_KEYS = {
    "dependency_preparation_sha256",
    "nightly_cargo_fmt_sha256",
    "nightly_rustfmt_sha256",
    "profile_sha256",
    "sandbox_executable_sha256",
    "stable_cargo_sha256",
}
FORMATTER_TOOLCHAIN = "nightly-2026-07-16"
VALIDATION_TEMP_PREFIX = "dxv-"
DARWIN_VALIDATION_TEMP_PARENT = Path("/private/tmp")
CANDIDATE_OUTPUT_ENV = "DECODEX_VALIDATION_REPO_OUTPUT"
CANDIDATE_OUTPUT_NAME_PATTERN = re.compile(
    r"decodex-validation-[0-9a-f]{32}"
)
VALIDATION_DIAGNOSTIC_SCHEMA = (
    "decodex/codex-upstream-validation-diagnostic/2"
)
MAX_VALIDATION_DIAGNOSTICS = 512
MAX_VALIDATION_DIAGNOSTIC_BYTES = 64 * 1024
MAX_VALIDATION_DIAGNOSTIC_TOTAL_BYTES = 8 * 1024 * 1024
MAX_DIAGNOSTIC_TEST_IDS = 32
MAX_DIAGNOSTIC_FAILURE_CLASSES = 16
MAX_DIAGNOSTIC_REASON_CODES = 16
_UNITTEST_FAILURE_PATTERN = re.compile(
    r"^(?:FAIL|ERROR):\s+[A-Za-z0-9_]{1,128}\s+"
    r"\(([A-Za-z0-9_.]{1,512})\)$",
    re.MULTILINE,
)
_FAILURE_CLASS_PATTERN = re.compile(
    r"^([A-Za-z][A-Za-z0-9_.]{0,127}(?:Error|Exception))(?::|$)",
    re.MULTILINE,
)
_TEST_COUNT_PATTERN = re.compile(r"^Ran ([0-9]{1,9}) tests?\b", re.MULTILINE)
_FAILURE_COUNT_PATTERNS = {
    "errors": re.compile(r"\berrors=([0-9]{1,9})\b"),
    "failures": re.compile(r"\bfailures=([0-9]{1,9})\b"),
    "skipped": re.compile(r"\bskipped=([0-9]{1,9})\b"),
}
_DIAGNOSTIC_REASON_MARKERS = {
    "command_not_found": ("command not found",),
    "module_not_found": ("modulenotfounderror", "no module named"),
    "no_such_file": ("no such file or directory",),
    "operation_not_permitted": ("operation not permitted",),
    "permission_denied": ("permission denied",),
    "process_killed": ("killed",),
    "timed_out": ("timed out", "timeout expired"),
}


@dataclass(frozen=True)
class FullXcodeConfiguration:
    """Bind a full-gate environment to exact Xcode tools and read roots."""

    environment: dict[str, str]
    evidence: dict[str, str]
    developer_dir: Path
    metal_toolchain_root: Path
    sdk_root: Path
    xcrun_tools: tuple[tuple[str, Path], ...]


def validation_failure_facts(output_tail: bytes) -> dict[str, Any]:
    """Extract bounded diagnostic facts without retaining command output."""

    text = output_tail.decode("utf-8", errors="replace")
    lowered = text.lower()
    test_ids = sorted(set(_UNITTEST_FAILURE_PATTERN.findall(text)))[
        :MAX_DIAGNOSTIC_TEST_IDS
    ]
    failure_classes = sorted(set(_FAILURE_CLASS_PATTERN.findall(text)))[
        :MAX_DIAGNOSTIC_FAILURE_CLASSES
    ]
    reason_codes = sorted(
        reason
        for reason, markers in _DIAGNOSTIC_REASON_MARKERS.items()
        if any(marker in lowered for marker in markers)
    )[:MAX_DIAGNOSTIC_REASON_CODES]
    counts: dict[str, int] = {}
    test_count = _TEST_COUNT_PATTERN.search(text)
    if test_count is not None:
        counts["tests"] = int(test_count.group(1))
    for name, pattern in _FAILURE_COUNT_PATTERNS.items():
        matches = pattern.findall(text)
        if matches:
            counts[name] = int(matches[-1])
    return {
        "test_ids": test_ids,
        "failure_classes": failure_classes,
        "reason_codes": reason_codes,
        "counts": counts,
    }


def _validate_private_descriptor(
    descriptor: int,
    *,
    directory: bool,
    exact_mode: int,
    maximum_size: int | None = None,
) -> os.stat_result:
    metadata = os.fstat(descriptor)
    expected_type = stat.S_ISDIR if directory else stat.S_ISREG
    if (
        not expected_type(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != exact_mode
        or (not directory and metadata.st_nlink != 1)
        or (
            maximum_size is not None
            and not 1 <= metadata.st_size <= maximum_size
        )
    ):
        raise AutopilotError("validation_diagnostic_path_invalid")
    return metadata


@contextmanager
def _locked_validation_diagnostic_directory(
    repo_root: Path,
) -> Iterator[tuple[int, int]]:
    cache_root = ensure_cache_root(
        repo_root / ".agent/automations/upstream/cache"
    )
    directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    cache_descriptor: int | None = None
    diagnostic_descriptor: int | None = None
    lock_descriptor: int | None = None
    try:
        cache_descriptor = os.open(cache_root, directory_flags)
        _validate_private_descriptor(
            cache_descriptor,
            directory=True,
            exact_mode=0o700,
        )
        try:
            os.mkdir("diagnostics", mode=0o700, dir_fd=cache_descriptor)
        except FileExistsError:
            pass
        diagnostic_descriptor = os.open(
            "diagnostics",
            directory_flags,
            dir_fd=cache_descriptor,
        )
        _validate_private_descriptor(
            diagnostic_descriptor,
            directory=True,
            exact_mode=0o700,
        )
        lock_descriptor = os.open(
            "diagnostics.lock",
            os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW,
            0o600,
            dir_fd=cache_descriptor,
        )
        _validate_private_descriptor(
            lock_descriptor,
            directory=False,
            exact_mode=0o600,
        )
        fcntl.flock(lock_descriptor, fcntl.LOCK_EX)
        yield cache_descriptor, diagnostic_descriptor
    except AutopilotError:
        raise
    except OSError as error:
        raise AutopilotError("validation_diagnostic_root_unavailable") from error
    finally:
        if lock_descriptor is not None:
            try:
                fcntl.flock(lock_descriptor, fcntl.LOCK_UN)
            finally:
                os.close(lock_descriptor)
        if diagnostic_descriptor is not None:
            os.close(diagnostic_descriptor)
        if cache_descriptor is not None:
            os.close(cache_descriptor)


def _read_bounded_json_at(
    directory_descriptor: int,
    name: str,
    *,
    maximum_size: int,
) -> tuple[dict[str, Any], os.stat_result]:
    descriptor: int | None = None
    try:
        descriptor = os.open(
            name,
            os.O_RDONLY | os.O_NOFOLLOW,
            dir_fd=directory_descriptor,
        )
        metadata = _validate_private_descriptor(
            descriptor,
            directory=False,
            exact_mode=0o600,
            maximum_size=maximum_size,
        )
        payload = bytearray()
        while len(payload) <= maximum_size:
            chunk = os.read(
                descriptor,
                min(4096, maximum_size + 1 - len(payload)),
            )
            if not chunk:
                break
            payload.extend(chunk)
        if len(payload) != metadata.st_size:
            raise AutopilotError("validation_diagnostic_path_invalid")
        value = json.loads(bytes(payload).decode("utf-8"))
        if not isinstance(value, dict):
            raise AutopilotError("validation_diagnostic_content_invalid")
        return value, metadata
    except AutopilotError:
        raise
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise AutopilotError("validation_diagnostic_read_failed") from error
    finally:
        if descriptor is not None:
            os.close(descriptor)


def _nonterminal_error_digest_references(
    cache_descriptor: int,
) -> set[str] | None:
    state_names = ("state.json", "state.recovery.json")
    present: list[str] = []
    for name in state_names:
        try:
            os.stat(name, dir_fd=cache_descriptor, follow_symlinks=False)
            present.append(name)
        except FileNotFoundError:
            continue
        except OSError:
            return None
    if not present:
        return set()
    lock_descriptor: int | None = None
    try:
        lock_descriptor = os.open(
            "state.lock",
            os.O_RDONLY | os.O_NOFOLLOW,
            dir_fd=cache_descriptor,
        )
        _validate_private_descriptor(
            lock_descriptor,
            directory=False,
            exact_mode=0o600,
        )
        fcntl.flock(lock_descriptor, fcntl.LOCK_SH)
        valid: list[dict[str, Any]] = []
        for name in present:
            try:
                state, _metadata = _read_bounded_json_at(
                    cache_descriptor,
                    name,
                    maximum_size=MAX_STATE_BYTES,
                )
            except AutopilotError:
                continue
            if (
                state.get("schema") != STATE_SCHEMA
                or not isinstance(state.get("persistence_generation"), int)
                or state["persistence_generation"] < 0
                or not isinstance(state.get("candidates"), list)
                or len(state["candidates"]) > 512
            ):
                continue
            valid.append(state)
        if not valid:
            return None
        valid.sort(
            key=lambda value: value["persistence_generation"],
            reverse=True,
        )
        if (
            len(valid) > 1
            and valid[0]["persistence_generation"]
            == valid[1]["persistence_generation"]
            and canonical_json(valid[0]) != canonical_json(valid[1])
        ):
            return None
        references: set[str] = set()
        for candidate in valid[0]["candidates"]:
            if not isinstance(candidate, dict):
                return None
            if candidate.get("status") in TERMINAL_STATUSES:
                continue
            result = candidate.get("result")
            if not isinstance(result, dict):
                continue
            digest = result.get("error_digest")
            if isinstance(digest, str) and is_sha256(digest):
                references.add(digest)
        return references
    except (AutopilotError, OSError):
        return None
    finally:
        if lock_descriptor is not None:
            try:
                fcntl.flock(lock_descriptor, fcntl.LOCK_UN)
            finally:
                os.close(lock_descriptor)


def _diagnostic_cause_payload(
    *,
    profile: str,
    repository_tree: str,
    failure: CommandFailure,
    facts: dict[str, Any],
) -> dict[str, Any]:
    return {
        "profile": profile,
        "failure_code": failure.code,
        "failure_kind": failure.failure_kind,
        "return_code": failure.return_code,
        "repository_tree": repository_tree,
        "facts": facts,
    }


def _diagnostic_artifact_payload(
    *,
    cause_digest: str,
    cause_payload: dict[str, Any],
    repository_head: str,
    output_sha256: str,
) -> dict[str, Any]:
    record = {
        **cause_payload,
        "repository_head": repository_head,
        "output_sha256": output_sha256,
    }
    return {
        "schema": VALIDATION_DIAGNOSTIC_SCHEMA,
        "cause_digest": cause_digest,
        "artifact_sha256": sha256_value(record),
        **record,
    }


def _validate_diagnostic_payload(
    payload: dict[str, Any],
    *,
    expected_cause_digest: str,
) -> None:
    expected_keys = {
        "schema",
        "cause_digest",
        "artifact_sha256",
        "profile",
        "failure_code",
        "failure_kind",
        "return_code",
        "repository_head",
        "repository_tree",
        "output_sha256",
        "facts",
    }
    record = {
        key: value
        for key, value in payload.items()
        if key not in {"schema", "cause_digest", "artifact_sha256"}
    }
    cause_payload = {
        key: record[key]
        for key in (
            "profile",
            "failure_code",
            "failure_kind",
            "return_code",
            "repository_tree",
            "facts",
        )
        if key in record
    }
    facts = payload.get("facts")
    return_code = payload.get("return_code")
    valid_facts = (
        isinstance(facts, dict)
        and set(facts)
        == {"test_ids", "failure_classes", "reason_codes", "counts"}
        and isinstance(facts["test_ids"], list)
        and len(facts["test_ids"]) <= MAX_DIAGNOSTIC_TEST_IDS
        and all(
            isinstance(value, str)
            and re.fullmatch(r"[A-Za-z0-9_.]{1,512}", value)
            for value in facts["test_ids"]
        )
        and isinstance(facts["failure_classes"], list)
        and len(facts["failure_classes"])
        <= MAX_DIAGNOSTIC_FAILURE_CLASSES
        and all(
            isinstance(value, str)
            and re.fullmatch(
                r"[A-Za-z][A-Za-z0-9_.]{0,127}(?:Error|Exception)",
                value,
            )
            for value in facts["failure_classes"]
        )
        and isinstance(facts["reason_codes"], list)
        and len(facts["reason_codes"]) <= MAX_DIAGNOSTIC_REASON_CODES
        and all(
            isinstance(value, str)
            and REASON_PATTERN.fullmatch(value) is not None
            for value in facts["reason_codes"]
        )
        and isinstance(facts["counts"], dict)
        and set(facts["counts"])
        <= {"tests", "errors", "failures", "skipped"}
        and all(
            isinstance(value, int) and 0 <= value <= 999_999_999
            for value in facts["counts"].values()
        )
    )
    if (
        set(payload) != expected_keys
        or payload.get("schema") != VALIDATION_DIAGNOSTIC_SCHEMA
        or payload.get("cause_digest") != expected_cause_digest
        or payload.get("profile") not in VALIDATION_PROFILE_COMMANDS
        or REASON_PATTERN.fullmatch(str(payload.get("failure_code"))) is None
        or REASON_PATTERN.fullmatch(str(payload.get("failure_kind"))) is None
        or (
            return_code is not None
            and (
                not isinstance(return_code, int)
                or not -255 <= return_code <= 255
            )
        )
        or SHA_PATTERN.fullmatch(str(payload.get("repository_head"))) is None
        or SHA_PATTERN.fullmatch(str(payload.get("repository_tree"))) is None
        or not valid_facts
        or sha256_value(cause_payload) != expected_cause_digest
        or sha256_value(record) != payload.get("artifact_sha256")
        or not is_sha256(payload.get("output_sha256"))
    ):
        raise AutopilotError("validation_diagnostic_content_invalid")


def _diagnostic_entries(
    diagnostic_descriptor: int,
) -> list[tuple[int, str, int]]:
    entries: list[tuple[int, str, int]] = []
    try:
        names = os.listdir(diagnostic_descriptor)
    except OSError as error:
        raise AutopilotError("validation_diagnostic_read_failed") from error
    for name in names:
        match = re.fullmatch(r"([0-9a-f]{64})\.json", name)
        if match is None:
            if re.fullmatch(
                r"\.diagnostic-[1-9][0-9]*-[0-9a-f]{16}\.tmp",
                name,
            ):
                _unlink_diagnostic_at(
                    diagnostic_descriptor,
                    name,
                    allow_empty=True,
                )
                continue
            raise AutopilotError("validation_diagnostic_path_invalid")
        payload, metadata = _read_bounded_json_at(
            diagnostic_descriptor,
            name,
            maximum_size=MAX_VALIDATION_DIAGNOSTIC_BYTES,
        )
        _validate_diagnostic_payload(
            payload,
            expected_cause_digest=match.group(1),
        )
        entries.append((metadata.st_mtime_ns, name, metadata.st_size))
    return entries


def _unlink_diagnostic_at(
    diagnostic_descriptor: int,
    name: str,
    *,
    allow_empty: bool = False,
) -> None:
    descriptor: int | None = None
    try:
        descriptor = os.open(
            name,
            os.O_RDONLY | os.O_NOFOLLOW,
            dir_fd=diagnostic_descriptor,
        )
        metadata = _validate_private_descriptor(
            descriptor,
            directory=False,
            exact_mode=0o600,
            maximum_size=None if allow_empty else MAX_VALIDATION_DIAGNOSTIC_BYTES,
        )
        if allow_empty and metadata.st_size > MAX_VALIDATION_DIAGNOSTIC_BYTES:
            raise AutopilotError("validation_diagnostic_prune_failed")
        current = os.stat(
            name,
            dir_fd=diagnostic_descriptor,
            follow_symlinks=False,
        )
        if (metadata.st_dev, metadata.st_ino) != (
            current.st_dev,
            current.st_ino,
        ):
            raise AutopilotError("validation_diagnostic_prune_failed")
        os.unlink(name, dir_fd=diagnostic_descriptor)
    except AutopilotError:
        raise
    except OSError as error:
        raise AutopilotError("validation_diagnostic_prune_failed") from error
    finally:
        if descriptor is not None:
            os.close(descriptor)


def _write_diagnostic_at(
    diagnostic_descriptor: int,
    name: str,
    payload: bytes,
) -> None:
    temporary_name = f".diagnostic-{os.getpid()}-{secrets.token_hex(8)}.tmp"
    descriptor: int | None = None
    try:
        descriptor = os.open(
            temporary_name,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
            0o600,
            dir_fd=diagnostic_descriptor,
        )
        offset = 0
        while offset < len(payload):
            offset += os.write(descriptor, payload[offset:])
        os.fsync(descriptor)
        _validate_private_descriptor(
            descriptor,
            directory=False,
            exact_mode=0o600,
            maximum_size=MAX_VALIDATION_DIAGNOSTIC_BYTES,
        )
        os.close(descriptor)
        descriptor = None
        os.link(
            temporary_name,
            name,
            src_dir_fd=diagnostic_descriptor,
            dst_dir_fd=diagnostic_descriptor,
            follow_symlinks=False,
        )
        os.unlink(temporary_name, dir_fd=diagnostic_descriptor)
        os.fsync(diagnostic_descriptor)
    except FileExistsError:
        raise AutopilotError("validation_diagnostic_path_conflict")
    except AutopilotError:
        raise
    except OSError as error:
        raise AutopilotError("validation_diagnostic_write_failed") from error
    finally:
        if descriptor is not None:
            os.close(descriptor)
        try:
            os.unlink(
                temporary_name,
                dir_fd=diagnostic_descriptor,
            )
        except FileNotFoundError:
            pass
        except OSError:
            pass


def write_validation_failure_diagnostic(
    repo_root: Path,
    *,
    profile: str,
    repository_head: str,
    repository_tree: str,
    failure: CommandFailure,
) -> str:
    """Persist one bounded local artifact keyed by a stable cause digest."""

    if (
        profile not in VALIDATION_PROFILE_COMMANDS
        or SHA_PATTERN.fullmatch(repository_head) is None
        or SHA_PATTERN.fullmatch(repository_tree) is None
        or (
            failure.return_code is not None
            and (
                not isinstance(failure.return_code, int)
                or not -255 <= failure.return_code <= 255
            )
        )
        or REASON_PATTERN.fullmatch(failure.failure_kind) is None
        or not is_sha256(failure.output_sha256)
    ):
        raise AutopilotError("validation_diagnostic_input_invalid")
    facts = validation_failure_facts(failure.output_tail)
    cause_payload = _diagnostic_cause_payload(
        profile=profile,
        repository_tree=repository_tree,
        failure=failure,
        facts=facts,
    )
    cause_digest = sha256_value(cause_payload)
    artifact = _diagnostic_artifact_payload(
        cause_digest=cause_digest,
        cause_payload=cause_payload,
        repository_head=repository_head,
        output_sha256=failure.output_sha256,
    )
    encoded = (
        json.dumps(artifact, indent=2, sort_keys=True).encode("utf-8") + b"\n"
    )
    if len(encoded) > MAX_VALIDATION_DIAGNOSTIC_BYTES:
        raise AutopilotError("validation_diagnostic_budget_exceeded")
    name = f"{cause_digest}.json"
    with _locked_validation_diagnostic_directory(repo_root) as (
        cache_descriptor,
        diagnostic_descriptor,
    ):
        entries = _diagnostic_entries(diagnostic_descriptor)
        existing_names = {entry_name for _mtime, entry_name, _size in entries}
        if name in existing_names:
            existing, _metadata = _read_bounded_json_at(
                diagnostic_descriptor,
                name,
                maximum_size=MAX_VALIDATION_DIAGNOSTIC_BYTES,
            )
            _validate_diagnostic_payload(
                existing,
                expected_cause_digest=cause_digest,
            )
            return cause_digest
        references = _nonterminal_error_digest_references(cache_descriptor)
        total_bytes = sum(size for _mtime, _name, size in entries)
        required_count = len(entries) + 1
        required_bytes = total_bytes + len(encoded)
        protected = references if references is not None else {
            entry_name.removesuffix(".json")
            for _mtime, entry_name, _size in entries
        }
        for _mtime, entry_name, size in sorted(entries):
            if (
                required_count <= MAX_VALIDATION_DIAGNOSTICS
                and required_bytes
                <= MAX_VALIDATION_DIAGNOSTIC_TOTAL_BYTES
            ):
                break
            if entry_name.removesuffix(".json") in protected:
                continue
            _unlink_diagnostic_at(diagnostic_descriptor, entry_name)
            required_count -= 1
            required_bytes -= size
        if (
            required_count > MAX_VALIDATION_DIAGNOSTICS
            or required_bytes > MAX_VALIDATION_DIAGNOSTIC_TOTAL_BYTES
        ):
            raise AutopilotError("validation_diagnostic_capacity_exhausted")
        _write_diagnostic_at(diagnostic_descriptor, name, encoded)
        stored, _metadata = _read_bounded_json_at(
            diagnostic_descriptor,
            name,
            maximum_size=MAX_VALIDATION_DIAGNOSTIC_BYTES,
        )
        _validate_diagnostic_payload(
            stored,
            expected_cause_digest=cause_digest,
        )
    return cause_digest


def read_validation_failure_diagnostic(
    repo_root: Path,
    *,
    cause_digest: str,
) -> dict[str, Any]:
    """Read and validate one cause-addressed diagnostic artifact."""

    if not is_sha256(cause_digest):
        raise AutopilotError("validation_diagnostic_digest_invalid")
    with _locked_validation_diagnostic_directory(repo_root) as (
        _cache_descriptor,
        diagnostic_descriptor,
    ):
        payload, _metadata = _read_bounded_json_at(
            diagnostic_descriptor,
            f"{cause_digest}.json",
            maximum_size=MAX_VALIDATION_DIAGNOSTIC_BYTES,
        )
        _validate_diagnostic_payload(
            payload,
            expected_cause_digest=cause_digest,
        )
        return payload


TRUSTED_TOOLCHAIN_EXECUTABLES = {"cargo", "cargo-fmt", "rustfmt"}
SANDBOX_PROFILE_TASKS = {
    "focused_tests": "test-automations",
    "cargo_make_check_upstream_automation": (
        "check-upstream-automation-sandboxed"
    ),
    FULL_VALIDATION_PROFILE: "check-sandboxed",
}
SANDBOX_TEST_AGGREGATES = {
    "test-sandboxed": "test",
    "test-headless-sandboxed": "test-headless",
}
SANDBOX_OMITTED_TASK = "test-vnext-postgres-store"
LIVE_POSTGRES_GATE_STATUS = "omitted_sandbox_incompatible"
MAX_CARGO_LOCK_BYTES = 16 * 1024 * 1024
MAX_CARGO_PACKAGES = 4096
CRATES_IO_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
GIT_SOURCE_PATTERN = re.compile(
    r"^git\+(https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?:\.git)?)"
    r"\?rev=([0-9a-f]{40})#([0-9a-f]{40})$"
)
MINIMUM_VALIDATION_PYTHON = (3, 11)


def repository_identity(worktree: Path) -> tuple[str, str]:
    head = run_command(
        ["git", "rev-parse", "HEAD"],
        cwd=worktree,
        failure_code="validation_head_unavailable",
    )
    tree = run_command(
        ["git", "rev-parse", "HEAD^{tree}"],
        cwd=worktree,
        failure_code="validation_tree_unavailable",
    )
    status = run_command(
        ["git", "status", "--porcelain=v1"],
        cwd=worktree,
        failure_code="validation_status_unavailable",
    )
    if (
        SHA_PATTERN.fullmatch(head) is None
        or SHA_PATTERN.fullmatch(tree) is None
        or status
    ):
        raise AutopilotError("validation_worktree_not_clean")
    return head, tree


def repository_git_common_directory(
    repo_root: Path,
) -> Path:
    checkout = repo_root.resolve()
    top_level = run_command(
        ["git", "rev-parse", "--show-toplevel"],
        cwd=checkout,
        failure_code="validation_git_authority_unavailable",
    )
    common_text = run_command(
        [
            "git",
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ],
        cwd=checkout,
        failure_code="validation_git_authority_unavailable",
    )
    common_source = Path(common_text)
    try:
        common = common_source.resolve(strict=True)
        metadata = common.stat()
        top_level_path = Path(top_level).resolve(strict=True)
    except OSError as error:
        raise AutopilotError(
            "validation_git_authority_unavailable"
        ) from error
    if (
        top_level_path != checkout
        or common.name != ".git"
        or common_source.is_symlink()
        or not common.is_dir()
        or metadata.st_uid != os.getuid()
        or metadata.st_mode & 0o022
    ):
        raise AutopilotError("validation_git_authority_unavailable")
    return common


def validation_authority_identity(repo_root: Path) -> dict[str, str]:
    head, tree = repository_identity(repo_root)
    closure = run_command(
        [
            "git",
            "ls-tree",
            "-r",
            "--full-tree",
            "HEAD",
            "--",
            *sorted(VALIDATION_AUTHORITY_PATHS),
            *VALIDATION_AUTHORITY_PREFIXES,
        ],
        cwd=repo_root,
        failure_code="validation_authority_unavailable",
    )
    if not closure:
        raise AutopilotError("validation_authority_unavailable")
    return {
        "repository_head": head,
        "repository_tree": tree,
        "closure_sha256": sha256_value(
            {
                "paths": [
                    *sorted(VALIDATION_AUTHORITY_PATHS),
                    *VALIDATION_AUTHORITY_PREFIXES,
                ],
                "git_tree_entries": closure,
            }
        ),
    }


def changed_paths_between(
    worktree: Path,
    *,
    base_head: str,
    head: str,
    policy: dict[str, Any],
) -> tuple[str, ...]:
    if (
        SHA_PATTERN.fullmatch(base_head) is None
        or SHA_PATTERN.fullmatch(head) is None
        or not command_succeeds(
            ["git", "merge-base", "--is-ancestor", base_head, head],
            cwd=worktree,
            failure_code="validation_base_unavailable",
        )
    ):
        raise AutopilotError("validation_base_invalid")
    arguments = [
        "git",
        "diff",
        "--find-renames",
        "--find-copies",
        "--diff-filter=ACDMRT",
        "-z",
        f"{base_head}..{head}",
        "--",
    ]
    output = run_command(
        [
            *arguments[:2],
            "--name-status",
            *arguments[2:],
        ],
        cwd=worktree,
        failure_code="validation_diff_unavailable",
        max_output_bytes=8 * 1024 * 1024,
    )
    paths = parse_name_status_paths(output)
    raw_output = run_command(
        [
            *arguments[:2],
            "--raw",
            "--no-abbrev",
            *arguments[2:],
        ],
        cwd=worktree,
        failure_code="validation_diff_unavailable",
        max_output_bytes=8 * 1024 * 1024,
    )
    raw_paths, modes = parse_raw_diff_paths(raw_output)
    if (
        len(paths) > 4096
        or raw_paths != paths
        or len(set(paths)) != len(paths)
        or any(
            Path(path).is_absolute()
            or ".." in Path(path).parts
            or path in {"", "."}
            or len(path) > 512
            or "\\" in path
            or any(ord(character) < 32 for character in path)
            for path in paths
        )
        or any("160000" in (old_mode, new_mode) for old_mode, new_mode in modes)
        or any(
            (
                path_matches_sandbox_policy(path, policy)
                or path in VALIDATION_AUTHORITY_PATHS
                or any(
                    path.startswith(prefix)
                    for prefix in VALIDATION_AUTHORITY_PREFIXES
                )
            )
            and "120000" in (old_mode, new_mode)
            for path, (old_mode, new_mode) in zip(paths, modes, strict=True)
        )
    ):
        raise AutopilotError("validation_diff_invalid")
    return paths


def parse_name_status_paths(output: str) -> tuple[str, ...]:
    if not output:
        return ()
    tokens = output.split("\0")
    if tokens[-1] != "":
        raise AutopilotError("validation_diff_invalid")
    tokens.pop()
    paths: list[str] = []
    index = 0
    while index < len(tokens):
        status = tokens[index]
        index += 1
        if re.fullmatch(r"[ACDMT]", status):
            path_count = 1
        elif re.fullmatch(r"[RC](?:100|[0-9]{1,2})", status):
            path_count = 2
        else:
            raise AutopilotError("validation_diff_invalid")
        if index + path_count > len(tokens):
            raise AutopilotError("validation_diff_invalid")
        paths.extend(tokens[index:index + path_count])
        index += path_count
    return tuple(paths)


def parse_raw_diff_paths(
    output: str,
) -> tuple[tuple[str, ...], tuple[tuple[str, str], ...]]:
    if not output:
        return (), ()
    tokens = output.split("\0")
    if tokens[-1] != "":
        raise AutopilotError("validation_diff_invalid")
    tokens.pop()
    paths: list[str] = []
    modes: list[tuple[str, str]] = []
    index = 0
    header_pattern = re.compile(
        r"^:([0-7]{6}) ([0-7]{6}) "
        r"([0-9a-f]{40}) ([0-9a-f]{40}) "
        r"([ACDMT]|[RC](?:100|[0-9]{1,2}))$"
    )
    while index < len(tokens):
        match = header_pattern.fullmatch(tokens[index])
        index += 1
        if match is None:
            raise AutopilotError("validation_diff_invalid")
        path_count = 2 if match.group(5).startswith(("R", "C")) else 1
        if index + path_count > len(tokens):
            raise AutopilotError("validation_diff_invalid")
        for path in tokens[index:index + path_count]:
            paths.append(path)
            modes.append((match.group(1), match.group(2)))
        index += path_count
    return tuple(paths), tuple(modes)


def path_matches_sandbox_policy(path: str, policy: dict[str, Any]) -> bool:
    return (
        path in policy["sandbox_incompatible_exact_paths"]
        or any(
            path.startswith(prefix)
            for prefix in policy["sandbox_incompatible_path_prefixes"]
        )
    )


def candidate_path_policy_sha256(policy: dict[str, Any]) -> str:
    return sha256_value(
        {
            "schema": policy["schema"],
            "exact_paths": policy["sandbox_incompatible_exact_paths"],
            "path_prefixes": policy["sandbox_incompatible_path_prefixes"],
            "live_postgres_gate": LIVE_POSTGRES_GATE_STATUS,
        }
    )


def path_requires_full_gate(path: str) -> bool:
    value = Path(path)
    return (
        path in FULL_GATE_EXACT_PATHS
        or any(path.startswith(prefix) for prefix in FULL_GATE_PREFIXES)
        or value.name in {"Cargo.toml", "build.rs"}
        or value.suffix in {".entitlements", ".swift", ".xcconfig", ".xcodeproj"}
        or ".xcodeproj" in value.parts
    )


def classify_validation_scope(
    changed_paths: tuple[str, ...],
    *,
    candidate_kind: str,
    policy: dict[str, Any],
) -> dict[str, Any]:
    if any(path_matches_sandbox_policy(path, policy) for path in changed_paths):
        raise AutopilotError("candidate_path_sandbox_incompatible")
    authority_paths = tuple(
        path
        for path in changed_paths
        if path in VALIDATION_AUTHORITY_PATHS
        or any(path.startswith(prefix) for prefix in VALIDATION_AUTHORITY_PREFIXES)
    )
    if authority_paths and candidate_kind != "automation_repair":
        raise AutopilotError("validation_authority_change_not_repair")
    requires_full_gate = bool(authority_paths) or any(
        path_requires_full_gate(path) for path in changed_paths
    )
    return {
        "changed_path_count": len(changed_paths),
        "changed_paths_sha256": sha256_value(list(changed_paths)),
        "candidate_path_classification": "sandbox_eligible",
        "candidate_path_policy_sha256": candidate_path_policy_sha256(policy),
        "requires_full_gate": requires_full_gate,
    }


def required_profile_names(requires_full_gate: bool) -> tuple[str, ...]:
    if requires_full_gate:
        return (*REQUIRED_VALIDATION_PROFILES, FULL_VALIDATION_PROFILE)
    return REQUIRED_VALIDATION_PROFILES


def trusted_profile_command(
    repo_root: Path,
    name: str,
    *,
    cargo_executable: Path | None = None,
) -> list[str]:
    command = VALIDATION_PROFILE_COMMANDS.get(name)
    if command is None or command[:2] != ["cargo", "make"] or len(command) != 3:
        raise AutopilotError("validation_profile_command_invalid")
    sandbox_task = SANDBOX_PROFILE_TASKS.get(name)
    if sandbox_task is None:
        raise AutopilotError("validation_profile_command_invalid")
    return [
        str(cargo_executable or "cargo"),
        "make",
        "--makefile",
        str((repo_root / "Makefile.toml").resolve()),
        sandbox_task,
    ]


def profile_command_sha256(name: str) -> str:
    command = VALIDATION_PROFILE_COMMANDS.get(name)
    effective_task = SANDBOX_PROFILE_TASKS.get(name)
    if command is None or effective_task is None:
        raise AutopilotError("validation_profile_command_invalid")
    return sha256_value(
        {
            "declared_command": command,
            "effective_task": effective_task,
            "makefile_authority": "primary",
        }
    )


def sandbox_task_graph_sha256(repo_root: Path) -> str:
    source = repo_root / "Makefile.toml"
    try:
        resolved = source.resolve(strict=True)
        if (
            source.is_symlink()
            or resolved != source.absolute()
            or not source.is_file()
            or source.stat().st_size > 1024 * 1024
        ):
            raise AutopilotError("validation_task_graph_invalid")
        with source.open("rb") as handle:
            document = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise AutopilotError("validation_task_graph_invalid") from error
    tasks = document.get("tasks")
    if not isinstance(tasks, dict) or not 1 <= len(tasks) <= 512:
        raise AutopilotError("validation_task_graph_invalid")
    for sandboxed, ordinary in SANDBOX_TEST_AGGREGATES.items():
        sandboxed_task = tasks.get(sandboxed)
        ordinary_task = tasks.get(ordinary)
        if not isinstance(sandboxed_task, dict) or not isinstance(
            ordinary_task,
            dict,
        ):
            raise AutopilotError("validation_task_graph_invalid")
        sandboxed_dependencies = sandboxed_task.get("dependencies")
        ordinary_dependencies = ordinary_task.get("dependencies")
        if (
            not isinstance(sandboxed_dependencies, list)
            or not isinstance(ordinary_dependencies, list)
            or ordinary_dependencies.count(SANDBOX_OMITTED_TASK) != 1
            or sandboxed_dependencies
            != [
                dependency
                for dependency in ordinary_dependencies
                if dependency != SANDBOX_OMITTED_TASK
            ]
        ):
            raise AutopilotError("validation_task_graph_invalid")

    roots = tuple(SANDBOX_PROFILE_TASKS.values())
    closure: dict[str, Any] = {}
    visiting: set[str] = set()

    def visit(name: str) -> None:
        if name in closure:
            return
        if name in visiting:
            raise AutopilotError("validation_task_graph_invalid")
        task = tasks.get(name)
        if not isinstance(task, dict):
            raise AutopilotError("validation_task_graph_invalid")
        dependencies = task.get("dependencies", [])
        if (
            not isinstance(dependencies, list)
            or any(
                not isinstance(dependency, str) or not dependency
                for dependency in dependencies
            )
        ):
            raise AutopilotError("validation_task_graph_invalid")
        visiting.add(name)
        for dependency in dependencies:
            visit(dependency)
        visiting.remove(name)
        closure[name] = task

    for root in roots:
        visit(root)
    if SANDBOX_OMITTED_TASK in closure:
        raise AutopilotError("validation_task_graph_invalid")
    return sha256_value(
        {
            "roots": roots,
            "tasks": {name: closure[name] for name in sorted(closure)},
            "live_postgres_gate": LIVE_POSTGRES_GATE_STATUS,
        }
    )


def _xctoolchain_root(path: Path) -> Path:
    for parent in (path, *path.parents):
        if parent.name.endswith(".xctoolchain"):
            return parent
    raise AutopilotError("full_xcode_unavailable")


def full_xcode_environment() -> FullXcodeConfiguration:
    xcode_select = Path("/usr/bin/xcode-select")
    xcrun = Path("/usr/bin/xcrun")
    if (
        xcode_select.is_symlink()
        or not xcode_select.is_file()
        or xcrun.is_symlink()
        or not xcrun.is_file()
    ):
        raise AutopilotError("full_xcode_unavailable")
    candidates: list[Path] = []
    configured = os.environ.get("DEVELOPER_DIR", "").strip()
    if configured:
        candidates.append(Path(configured))
    selected = run_command(
        [str(xcode_select), "-p"],
        failure_code="xcode_select_unavailable",
        allow_failure=True,
    )
    if selected:
        candidates.append(Path(selected))
    try:
        candidates.extend(
            sorted(
                (
                    app / "Contents/Developer"
                    for app in Path("/Applications").glob("Xcode*.app")
                ),
                key=str,
            )[:16]
        )
    except OSError:
        pass

    seen: set[Path] = set()
    for candidate in candidates:
        try:
            resolved = candidate.expanduser().resolve(strict=True)
        except OSError:
            continue
        if resolved in seen or not resolved.is_dir():
            continue
        seen.add(resolved)
        xcodebuild = resolved / "usr/bin/xcodebuild"
        if xcodebuild.is_symlink() or not xcodebuild.is_file():
            continue
        environment = {"DEVELOPER_DIR": str(resolved)}
        discovered_tools = {
            name: run_command(
                [str(xcrun), "--find", name],
                environment=environment,
                failure_code="full_xcode_unavailable",
                allow_failure=True,
            )
            for name in ("clang", "clang++", "metal", "metallib")
        }
        sdk_root = run_command(
            [str(xcrun), "--sdk", "macosx", "--show-sdk-path"],
            environment=environment,
            failure_code="full_xcode_unavailable",
            allow_failure=True,
        )
        version = run_command(
            [str(xcodebuild), "-version"],
            environment=environment,
            failure_code="full_xcode_unavailable",
            allow_failure=True,
        )
        if not all(discovered_tools.values()) or not sdk_root or not version:
            continue
        try:
            execution_paths = {
                name: Path(value).expanduser().absolute().parent.resolve(
                    strict=True
                )
                / Path(value).name
                for name, value in discovered_tools.items()
            }
            tool_paths = {
                name: path.resolve(strict=True)
                for name, path in execution_paths.items()
            }
            sdk_path = Path(sdk_root).resolve(strict=True)
            metal_root = _xctoolchain_root(tool_paths["metal"])
            metal_execution_root = _xctoolchain_root(
                execution_paths["metal"]
            ).resolve(strict=True)
            if (
                _xctoolchain_root(tool_paths["metallib"]) != metal_root
                or _xctoolchain_root(
                    execution_paths["metallib"]
                ).resolve(strict=True)
                != metal_execution_root
                or metal_execution_root != metal_root
                or not tool_paths["clang"].is_relative_to(resolved)
                or not tool_paths["clang++"].is_relative_to(resolved)
                or not execution_paths["clang"].is_relative_to(resolved)
                or not execution_paths["clang++"].is_relative_to(resolved)
                or not sdk_path.is_relative_to(resolved)
            ):
                continue
            full_environment = {
                "CC": str(execution_paths["clang"]),
                "CXX": str(execution_paths["clang++"]),
                "DEVELOPER_DIR": str(resolved),
                "SDKROOT": str(sdk_path),
                "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER": str(
                    execution_paths["clang"]
                ),
                "CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER": str(
                    execution_paths["clang"]
                ),
            }
            evidence = {
                "clang_sha256": hash_file_bounded(tool_paths["clang"]),
                "clangxx_sha256": hash_file_bounded(
                    tool_paths["clang++"]
                ),
                "developer_dir_sha256": sha256_value(str(resolved)),
                "metallib_sha256": hash_file_bounded(
                    tool_paths["metallib"]
                ),
                "sdk_root_sha256": sha256_value(str(sdk_path)),
                "xcode_select_sha256": hash_file_bounded(xcode_select),
                "xcode_version_sha256": sha256_value(version),
                "xcodebuild_sha256": hash_file_bounded(xcodebuild),
                "xcrun_sha256": hash_file_bounded(xcrun),
                "metal_sha256": hash_file_bounded(tool_paths["metal"]),
            }
        except (OSError, AutopilotError):
            continue
        if set(evidence) != FULL_XCODE_DISCOVERY_EVIDENCE_KEYS:
            raise AutopilotError("full_xcode_unavailable")
        return FullXcodeConfiguration(
            environment=full_environment,
            evidence=evidence,
            developer_dir=resolved,
            metal_toolchain_root=metal_root,
            sdk_root=sdk_path,
            xcrun_tools=tuple(
                (name, execution_paths[name])
                for name in ("metal", "metallib")
            ),
        )
    raise AutopilotError("full_xcode_unavailable")


def initialize_xcrun_proxy(
    temporary_home: Path,
    configuration: FullXcodeConfiguration,
    python_executable: Path,
) -> tuple[Path, str]:
    """Create a fail-closed xcrun proxy that cannot touch the host cache."""

    temporary_home = temporary_home.resolve(strict=True)
    tools = {name: str(path) for name, path in configuration.xcrun_tools}
    payload = (
        f"#!{python_executable}\n"
        "import os\n"
        "import sys\n"
        f"SDK_ROOT = {str(configuration.sdk_root)!r}\n"
        f"MODULE_CACHE = {str(temporary_home / 'metal-module-cache')!r}\n"
        f"TOOLS = {tools!r}\n"
        "args = sys.argv[1:]\n"
        "sdk_queries = ([\"--show-sdk-path\"], "
        "[\"--sdk\", \"macosx\", \"--show-sdk-path\"], "
        "[\"-sdk\", \"macosx\", \"--show-sdk-path\"], "
        "[\"--show-sdk-path\", \"--sdk\", \"macosx\"], "
        "[\"--show-sdk-path\", \"-sdk\", \"macosx\"])\n"
        "if args in sdk_queries:\n"
        "    print(SDK_ROOT)\n"
        "    raise SystemExit(0)\n"
        "if len(args) == 2 and args[0] in {\"--find\", \"-f\"}:\n"
        "    tool = TOOLS.get(args[1])\n"
        "    if tool is not None:\n"
        "        print(tool)\n"
        "        raise SystemExit(0)\n"
        "if (len(args) >= 3 and args[0] in {\"--sdk\", \"-sdk\"} "
        "and args[1] == \"macosx\"):\n"
        "    tool = TOOLS.get(args[2])\n"
        "    if tool is not None:\n"
        "        os.environ[\"SDKROOT\"] = SDK_ROOT\n"
        "        tool_args = args[3:]\n"
        "        if args[2] == \"metal\":\n"
        "            tool_args = [*tool_args, "
        "f\"-fmodules-cache-path={MODULE_CACHE}\"]\n"
        "        os.execv(tool, [tool, *tool_args])\n"
        "print(\"sandbox xcrun invocation denied\", file=sys.stderr)\n"
        "raise SystemExit(64)\n"
    ).encode("utf-8")
    proxy_directory = temporary_home / "trusted-xcrun"
    proxy = proxy_directory / "xcrun"
    descriptor: int | None = None
    try:
        proxy_directory.mkdir(mode=0o700)
        descriptor = os.open(
            proxy,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
            0o500,
        )
        written = 0
        while written < len(payload):
            count = os.write(descriptor, payload[written:])
            if count <= 0:
                raise OSError("xcrun proxy write made no progress")
            written += count
        os.fsync(descriptor)
        metadata = os.fstat(descriptor)
        directory_metadata = proxy_directory.stat()
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or metadata.st_nlink != 1
            or stat.S_IMODE(metadata.st_mode) != 0o500
            or metadata.st_size != len(payload)
            or not stat.S_ISDIR(directory_metadata.st_mode)
            or directory_metadata.st_uid != os.getuid()
            or stat.S_IMODE(directory_metadata.st_mode) != 0o700
        ):
            raise AutopilotError("full_xcode_proxy_invalid")
    except AutopilotError:
        raise
    except OSError as error:
        raise AutopilotError("full_xcode_proxy_invalid") from error
    finally:
        if descriptor is not None:
            os.close(descriptor)
    return proxy, hash_file_bounded(proxy)


def validation_tools(
    repo_root: Path,
) -> tuple[dict[str, Path], dict[str, str]]:
    paths: dict[str, Path] = {}
    discovery_path = os.pathsep.join(TRUSTED_TOOL_DISCOVERY_PATHS)
    for name in VALIDATION_TOOL_NAMES:
        if name == "python3":
            paths[name] = trusted_validation_python(repo_root)
            continue
        located = shutil.which(name, path=discovery_path)
        if located is None:
            raise AutopilotError("validation_tool_unavailable")
        path = Path(located).absolute()
        if path.is_symlink():
            target = Path(os.readlink(path))
            if not target.is_absolute():
                target = path.parent / target
            if (
                len(target.parts) >= 3
                and target.parts[:3] == ("/", "nix", "store")
                and target.name == name
            ):
                path = target
        try:
            resolved = path.resolve(strict=True)
        except OSError as error:
            raise AutopilotError("validation_tool_unavailable") from error
        if path.name != name:
            raise AutopilotError("validation_tool_name_mismatch")
        if (
            _path_is_within(path, repo_root)
            or _path_is_within(resolved, repo_root)
            or ".worktrees" in path.parts
            or ".worktrees" in resolved.parts
        ):
            raise AutopilotError("validation_tool_inside_candidate")
        paths[name] = path
    paths["cargo"] = trusted_toolchain_cargo(
        repo_root,
        paths["rustup"],
        repository_toolchain_channel(repo_root),
    )
    environment_path = os.pathsep.join(
        _validation_path_entries(paths)
    )
    for name, expected in paths.items():
        selected = shutil.which(name, path=environment_path)
        if selected is None or Path(selected).absolute() != expected:
            raise AutopilotError("validation_tool_path_conflict")
    return paths, validation_tool_evidence(paths)


def trusted_validation_python(repo_root: Path) -> Path:
    if not MINIMUM_VALIDATION_PYTHON <= sys.version_info[:2] < (4, 0):
        raise AutopilotError("validation_python_runtime_unsupported")
    runtime = Path(sys.executable).absolute()
    try:
        runtime_resolved = runtime.resolve(strict=True)
        canonical_candidate = runtime_resolved.parent / "python3"
        try:
            canonical_resolved = canonical_candidate.resolve(strict=True)
        except OSError:
            canonical_resolved = None
        candidate = canonical_candidate if (
            canonical_resolved == runtime_resolved
        ) else runtime.parent / "python3"
        candidate_metadata = candidate.lstat()
        parent_metadata = candidate.parent.stat()
        resolved = candidate.resolve(strict=True)
        resolved_metadata = resolved.stat()
    except OSError as error:
        raise AutopilotError("validation_python_runtime_unavailable") from error
    if (
        candidate.name != "python3"
        or resolved != runtime_resolved
        or not resolved.is_file()
        or candidate_metadata.st_uid != 0
        or parent_metadata.st_uid != 0
        or resolved_metadata.st_uid != 0
        or candidate_metadata.st_mode & 0o022
        or parent_metadata.st_mode & 0o022
        or resolved_metadata.st_mode & 0o022
        or not any(
            _path_is_within(candidate, root)
            and _path_is_within(resolved, root)
            for root in TRUSTED_SYSTEM_EXECUTABLE_ROOTS
        )
        or _path_is_within(candidate, repo_root)
        or _path_is_within(resolved, repo_root)
        or ".worktrees" in candidate.parts
        or ".worktrees" in resolved.parts
    ):
        raise AutopilotError("validation_python_runtime_untrusted")
    hash_file_bounded(resolved)
    return candidate


def _path_is_within(path: Path, root: Path) -> bool:
    try:
        path.absolute().relative_to(root.resolve())
    except ValueError:
        return False
    return True


def _validation_path_entries(tools: dict[str, Path]) -> list[str]:
    paths = [
        *(path.parent for path in tools.values()),
        Path("/usr/bin"),
        Path("/bin"),
        Path("/usr/sbin"),
        Path("/sbin"),
    ]
    unique: list[str] = []
    for path in paths:
        value = str(path)
        if value not in unique:
            unique.append(value)
    return unique


def trusted_rustup_home() -> Path:
    home = real_home_directory().resolve()
    configured = home / ".rustup"
    try:
        resolved = configured.resolve(strict=True)
        stat = resolved.stat()
    except OSError as error:
        raise AutopilotError("rustup_toolchain_unavailable") from error
    if (
        not _path_is_within(resolved, home)
        or not resolved.is_dir()
        or stat.st_uid != os.getuid()
        or stat.st_mode & 0o022
        or not (resolved / "toolchains").is_dir()
    ):
        raise AutopilotError("rustup_toolchain_unavailable")
    return resolved


def repository_toolchain_channel(repo_root: Path) -> str:
    source = repo_root / "rust-toolchain.toml"
    path = source.resolve()
    if (
        not _path_is_within(path, repo_root)
        or source.is_symlink()
        or not path.is_file()
        or path.stat().st_size > 16 * 1024
    ):
        raise AutopilotError("rust_toolchain_policy_invalid")
    try:
        with path.open("rb") as handle:
            value = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise AutopilotError("rust_toolchain_policy_invalid") from error
    toolchain = value.get("toolchain")
    channel = toolchain.get("channel") if isinstance(toolchain, dict) else None
    if (
        not isinstance(channel, str)
        or re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", channel) is None
    ):
        raise AutopilotError("rust_toolchain_policy_invalid")
    return channel


def trusted_toolchain_cargo(
    repo_root: Path,
    rustup: Path,
    toolchain: str,
) -> Path:
    return trusted_toolchain_executable(
        repo_root,
        rustup,
        toolchain,
        "cargo",
    )


def trusted_toolchain_executable(
    repo_root: Path,
    rustup: Path,
    toolchain: str,
    executable: str,
) -> Path:
    if (
        _path_is_within(rustup, repo_root)
        or executable not in TRUSTED_TOOLCHAIN_EXECUTABLES
        or re.fullmatch(
            r"(?:[0-9]+\.[0-9]+\.[0-9]+|nightly-[0-9]{4}-[0-9]{2}-[0-9]{2})",
            toolchain,
        )
        is None
    ):
        raise AutopilotError("rust_toolchain_policy_invalid")
    output = run_command(
        [
            str(rustup),
            "which",
            executable,
            "--toolchain",
            toolchain,
        ],
        failure_code="rust_toolchain_unavailable",
    )
    try:
        path = Path(output).resolve(strict=True)
        metadata = path.stat()
        path.relative_to(trusted_rustup_home() / "toolchains")
    except (OSError, ValueError) as error:
        raise AutopilotError("rust_toolchain_unavailable") from error
    if (
        path.name != executable
        or not path.is_file()
        or metadata.st_uid != os.getuid()
        or metadata.st_mode & 0o022
    ):
        raise AutopilotError("rust_toolchain_unavailable")
    hash_file_bounded(path)
    return path


def trusted_formatter_tools(
    repo_root: Path,
    rustup: Path,
) -> dict[str, Path]:
    return {
        executable: trusted_toolchain_executable(
            repo_root,
            rustup,
            FORMATTER_TOOLCHAIN,
            executable,
        )
        for executable in ("cargo-fmt", "rustfmt")
    }


def validation_tool_evidence(tools: dict[str, Path]) -> dict[str, str]:
    if set(tools) != set(VALIDATION_TOOL_NAMES):
        raise AutopilotError("validation_tool_set_invalid")
    evidence: dict[str, str] = {}
    for name, path in tools.items():
        try:
            resolved = path.resolve(strict=True)
            link_target = os.readlink(path) if path.is_symlink() else None
        except OSError as error:
            raise AutopilotError("validation_tool_unavailable") from error
        evidence[name] = sha256_value(
            {
                "execution_path": str(path),
                "link_target": link_target,
                "resolved_path": str(resolved),
                "resolved_sha256": hash_file_bounded(resolved),
            }
        )
    return evidence


def sanitized_validation_environment(
    temporary_home: Path,
    tools: dict[str, Path],
    *,
    cargo_home: Path | None = None,
    offline: bool = False,
    overrides: dict[str, str] | None = None,
) -> dict[str, str]:
    rustup_home = trusted_rustup_home()
    npm_global_config = temporary_home / "npm-globalrc"
    npm_project_config = temporary_home / "npm-projectrc"
    npm_user_config = temporary_home / "npm-userrc"
    environment = {
        "ASTRO_TELEMETRY_DISABLED": "1",
        "CARGO_HOME": str(cargo_home or temporary_home / "cargo-home"),
        "CARGO_BUILD_RUSTC_WRAPPER": "",
        "CARGO_NET_GIT_FETCH_WITH_CLI": "false",
        "CARGO_TARGET_DIR": str(temporary_home / "cargo-target"),
        "CARGO_TERM_COLOR": "never",
        "CI": "1",
        "DECODEX_CANDIDATE_SANDBOX": "1",
        "GCM_INTERACTIVE": "never",
        "GH_PROMPT_DISABLED": "1",
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_TERMINAL_PROMPT": "0",
        "HOME": str(temporary_home),
        "LANG": os.environ.get("LANG", "C.UTF-8"),
        "NPM_CONFIG_CACHE": str(temporary_home / "npm-cache"),
        "NPM_CONFIG_GLOBALCONFIG": str(npm_global_config),
        "NPM_CONFIG_IGNORE_SCRIPTS": "true",
        "NPM_CONFIG_PROJECTCONFIG": str(npm_project_config),
        "NPM_CONFIG_REGISTRY": "https://registry.npmjs.org/",
        "NPM_CONFIG_USERCONFIG": str(npm_user_config),
        "PATH": os.pathsep.join(_validation_path_entries(tools)),
        "PYTHONDONTWRITEBYTECODE": "1",
        "RUSTUP_HOME": str(rustup_home),
        "RUSTC_WRAPPER": "",
        "TMPDIR": str(temporary_home),
        "XDG_CACHE_HOME": str(temporary_home / "xdg-cache"),
        "XDG_CONFIG_HOME": str(temporary_home / "xdg-config"),
    }
    if offline:
        environment["CARGO_NET_OFFLINE"] = "true"
        environment["NPM_CONFIG_OFFLINE"] = "true"
    for name in ("NIX_SSL_CERT_FILE", "SSL_CERT_FILE", "SSL_CERT_DIR"):
        value = os.environ.get(name)
        if value:
            environment[name] = value
    locale = os.environ.get("LC_ALL")
    if locale:
        environment["LC_ALL"] = locale
    environment.update(overrides or {})
    return environment


def initialize_validation_home(temporary_home: Path) -> None:
    for name in ("npm-globalrc", "npm-projectrc", "npm-userrc"):
        path = temporary_home / name
        try:
            with path.open("x", encoding="utf-8") as handle:
                handle.write("")
                handle.flush()
                os.fsync(handle.fileno())
            os.chmod(path, 0o400)
        except OSError as error:
            raise AutopilotError("validation_home_initialization_failed") from error


def validation_temporary_directory():
    parent = None
    if sys.platform == "darwin":
        parent = DARWIN_VALIDATION_TEMP_PARENT
        if os.environ.get("DECODEX_CANDIDATE_SANDBOX") == "1":
            try:
                home = Path(os.environ["HOME"]).resolve(strict=True)
                temporary = Path(os.environ["TMPDIR"]).resolve(strict=True)
            except (KeyError, OSError) as error:
                raise AutopilotError(
                    "validation_sandbox_path_invalid"
                ) from error
            if home != temporary or not temporary.is_dir():
                raise AutopilotError("validation_sandbox_path_invalid")
            parent = temporary
    try:
        return tempfile.TemporaryDirectory(
            prefix=VALIDATION_TEMP_PREFIX,
            dir=parent,
        )
    except OSError as error:
        raise AutopilotError("validation_sandbox_path_invalid") from error


def validate_validation_receipt(
    receipt: Any,
    *,
    role: str | None = None,
    expected_base_head: str | None = None,
    expected_head: str | None = None,
    expected_tree: str | None = None,
) -> None:
    if not has_exact_keys(receipt, VALIDATION_RECEIPT_KEYS):
        raise AutopilotError("validation_receipt_invalid")
    if receipt.get("role") not in {"maintainer", "reviewer"}:
        raise AutopilotError("validation_receipt_invalid")
    if role is not None and receipt["role"] != role:
        raise AutopilotError("validation_receipt_role_mismatch")
    if (
        SHA_PATTERN.fullmatch(str(receipt.get("base_head", ""))) is None
        or SHA_PATTERN.fullmatch(str(receipt.get("repository_head", "")))
        is None
        or SHA_PATTERN.fullmatch(str(receipt.get("repository_tree", ""))) is None
        or not isinstance(receipt.get("changed_path_count"), int)
        or not 0 <= receipt["changed_path_count"] <= 4096
        or not is_sha256(receipt.get("changed_paths_sha256"))
        or receipt.get("candidate_path_classification") != "sandbox_eligible"
        or not is_sha256(receipt.get("candidate_path_policy_sha256"))
        or not isinstance(receipt.get("requires_full_gate"), bool)
        or not is_sha256(receipt.get("sandbox_task_graph_sha256"))
        or receipt.get("live_postgres_gate") != LIVE_POSTGRES_GATE_STATUS
        or not isinstance(receipt.get("completed_at"), int)
    ):
        raise AutopilotError("validation_receipt_invalid")
    if expected_base_head is not None and receipt["base_head"] != expected_base_head:
        raise AutopilotError("validation_receipt_base_mismatch")
    if expected_head is not None and receipt["repository_head"] != expected_head:
        raise AutopilotError("validation_receipt_head_mismatch")
    if expected_tree is not None and receipt["repository_tree"] != expected_tree:
        raise AutopilotError("validation_receipt_tree_mismatch")
    authority = receipt.get("validation_authority")
    if (
        not has_exact_keys(authority, VALIDATION_AUTHORITY_KEYS)
        or SHA_PATTERN.fullmatch(str(authority.get("repository_head", ""))) is None
        or SHA_PATTERN.fullmatch(str(authority.get("repository_tree", ""))) is None
        or not is_sha256(authority.get("closure_sha256"))
        or (
            (receipt["base_head"] == receipt["repository_head"])
            != (receipt["changed_path_count"] == 0)
        )
        or (
            receipt["changed_path_count"] == 0
            and receipt["changed_paths_sha256"] != sha256_value([])
        )
    ):
        raise AutopilotError("validation_receipt_invalid")
    expected_names = required_profile_names(receipt["requires_full_gate"])
    profiles = receipt.get("profiles")
    if not isinstance(profiles, list) or len(profiles) != len(expected_names):
        raise AutopilotError("validation_receipt_invalid")
    names: list[str] = []
    for profile in profiles:
        if (
            not has_exact_keys(profile, PROFILE_RESULT_KEYS)
            or REASON_PATTERN.fullmatch(str(profile.get("name", ""))) is None
            or not is_sha256(profile.get("command_sha256"))
            or not is_sha256(profile.get("environment_sha256"))
            or profile.get("exit_code") != 0
            or not is_sha256(profile.get("output_sha256"))
        ):
            raise AutopilotError("validation_receipt_invalid")
        names.append(profile["name"])
        expected_command = VALIDATION_PROFILE_COMMANDS.get(profile["name"])
        expected_task = SANDBOX_PROFILE_TASKS.get(profile["name"])
        if (
            expected_command is None
            or expected_task is None
            or profile["effective_task"] != expected_task
            or profile["command_sha256"]
            != profile_command_sha256(profile["name"])
        ):
            raise AutopilotError("validation_receipt_command_mismatch")
        toolchain = profile.get("toolchain_evidence")
        if (
            not has_exact_keys(toolchain, TOOLCHAIN_EVIDENCE_KEYS)
            or not has_exact_keys(
                toolchain.get("validation_tools"),
                set(VALIDATION_TOOL_NAMES),
            )
            or any(
                not is_sha256(value)
                for value in toolchain["validation_tools"].values()
            )
        ):
            raise AutopilotError("validation_receipt_toolchain_invalid")
        full_xcode = toolchain.get("full_xcode")
        if profile["name"] == FULL_VALIDATION_PROFILE:
            if (
                not has_exact_keys(
                    full_xcode,
                    FULL_XCODE_EVIDENCE_KEYS,
                )
                or any(not is_sha256(value) for value in full_xcode.values())
            ):
                raise AutopilotError(
                    "validation_receipt_toolchain_invalid"
                )
        elif full_xcode is not None:
            raise AutopilotError("validation_receipt_toolchain_invalid")
        sandbox = toolchain.get("sandbox")
        if (
            not has_exact_keys(sandbox, SANDBOX_EVIDENCE_KEYS)
            or any(not is_sha256(value) for value in sandbox.values())
        ):
            raise AutopilotError("validation_receipt_toolchain_invalid")
    if names != list(expected_names):
        raise AutopilotError("validation_receipt_invalid")


def validate_receipt_against_policy(
    receipt: dict[str, Any],
    policy: dict[str, Any],
    *,
    role: str,
    expected_base_head: str,
    expected_head: str,
    expected_tree: str,
) -> None:
    validate_validation_receipt(
        receipt,
        role=role,
        expected_base_head=expected_base_head,
        expected_head=expected_head,
        expected_tree=expected_tree,
    )
    if (
        receipt["candidate_path_policy_sha256"]
        != candidate_path_policy_sha256(policy)
    ):
        raise AutopilotError("validation_receipt_path_policy_mismatch")
    for profile in receipt["profiles"]:
        command = policy["validation_profiles"][profile["name"]]
        if (
            command != VALIDATION_PROFILE_COMMANDS[profile["name"]]
            or profile["command_sha256"]
            != profile_command_sha256(profile["name"])
        ):
            raise AutopilotError("validation_receipt_command_mismatch")


def validation_receipt_is_current(
    receipt: dict[str, Any],
    *,
    current_main_head: str,
    current_authority: dict[str, str],
) -> bool:
    validate_validation_receipt(receipt)
    if SHA_PATTERN.fullmatch(current_main_head) is None:
        raise AutopilotError("current_main_head_invalid")
    if (
        not has_exact_keys(current_authority, VALIDATION_AUTHORITY_KEYS)
        or any(
            SHA_PATTERN.fullmatch(str(current_authority.get(key, "")))
            is None
            for key in ("repository_head", "repository_tree")
        )
        or not is_sha256(current_authority.get("closure_sha256"))
    ):
        raise AutopilotError("validation_authority_invalid")
    return (
        receipt["base_head"] == current_main_head
        and receipt["validation_authority"] == current_authority
    )


def _load_cargo_lock(path: Path) -> dict[str, Any]:
    if (
        path.is_symlink()
        or not path.is_file()
        or path.stat().st_size > MAX_CARGO_LOCK_BYTES
    ):
        raise AutopilotError("cargo_lock_unavailable")
    try:
        with path.open("rb") as handle:
            value = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise AutopilotError("cargo_lock_unavailable") from error
    packages = value.get("package")
    if (
        value.get("version") != 4
        or not isinstance(packages, list)
        or not 1 <= len(packages) <= MAX_CARGO_PACKAGES
    ):
        raise AutopilotError("cargo_lock_invalid")
    return value


def cargo_lock_provenance(repo_root: Path, worktree: Path) -> dict[str, Any]:
    trusted = _load_cargo_lock(repo_root / "Cargo.lock")
    candidate = _load_cargo_lock(worktree / "Cargo.lock")

    def inspect(value: dict[str, Any]) -> tuple[set[str], list[dict[str, Any]]]:
        git_repositories: set[str] = set()
        identities: list[dict[str, Any]] = []
        for package in value["package"]:
            if (
                not isinstance(package, dict)
                or not isinstance(package.get("name"), str)
                or not 1 <= len(package["name"]) <= 256
                or not isinstance(package.get("version"), str)
                or not 1 <= len(package["version"]) <= 128
            ):
                raise AutopilotError("cargo_lock_invalid")
            source = package.get("source")
            checksum = package.get("checksum")
            if source is None:
                if checksum is not None:
                    raise AutopilotError("cargo_lock_provenance_invalid")
                continue
            if source == CRATES_IO_SOURCE:
                if (
                    not isinstance(checksum, str)
                    or re.fullmatch(r"[0-9a-f]{64}", checksum) is None
                ):
                    raise AutopilotError("cargo_lock_provenance_invalid")
            else:
                match = GIT_SOURCE_PATTERN.fullmatch(str(source))
                if (
                    match is None
                    or match.group(2) != match.group(3)
                    or checksum is not None
                ):
                    raise AutopilotError("cargo_lock_provenance_invalid")
                git_repositories.add(match.group(1))
            identities.append(
                {
                    "name": package["name"],
                    "version": package["version"],
                    "source": source,
                    "checksum": checksum,
                }
            )
        identities.sort(
            key=lambda item: (
                item["name"],
                item["version"],
                item["source"],
            )
        )
        return git_repositories, identities

    trusted_git, _trusted_identities = inspect(trusted)
    candidate_git, candidate_identities = inspect(candidate)
    if not candidate_git.issubset(trusted_git):
        raise AutopilotError("cargo_lock_git_source_not_approved")
    return {
        "package_count": len(candidate_identities),
        "git_repositories": sorted(candidate_git),
        "graph_sha256": sha256_value(candidate_identities),
    }


def prepare_dependency_cache(
    repo_root: Path,
    worktree: Path,
    temporary_home: Path,
    tools: dict[str, Path],
    trusted_audit: Path,
) -> tuple[dict[str, str], dict[str, Any]]:
    cargo_home = temporary_home / "cargo-home"
    cargo_home.mkdir(mode=0o700)
    source_cargo_home = real_home_directory() / ".cargo"
    for name in ("registry", "git"):
        source = source_cargo_home / name
        destination = cargo_home / name
        if source.is_symlink() or not source.is_dir():
            raise AutopilotError("cargo_cache_source_invalid")
        run_command(
            [
                str(tools["cp"]),
                "-ac",
                str(source),
                str(destination),
            ],
            environment=sanitized_validation_environment(
                temporary_home,
                tools,
                cargo_home=cargo_home,
            ),
            inherit_environment=False,
            failure_code="cargo_cache_clone_failed",
            timeout_seconds=900,
        )
    environment = sanitized_validation_environment(
        temporary_home,
        tools,
        cargo_home=cargo_home,
    )
    cargo_provenance = cargo_lock_provenance(repo_root, worktree)
    cargo_fetch = run_command(
        [
            str(tools["cargo"]),
            "--config",
            "net.git-fetch-with-cli=false",
            "--config",
            'registry.global-credential-providers=["cargo:token"]',
            "fetch",
            "--locked",
            "--manifest-path",
            str((worktree / "Cargo.toml").resolve()),
        ],
        cwd=worktree,
        environment=environment,
        inherit_environment=False,
        failure_code="cargo_dependency_fetch_failed",
        timeout_seconds=1800,
    )
    npm_install = run_command(
        [
            str(tools["npm"]),
            "ci",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
        ],
        cwd=worktree / "site",
        environment=environment,
        inherit_environment=False,
        failure_code="node_dependency_install_failed",
        timeout_seconds=900,
    )
    npm_advisories = run_command(
        [str(tools["npm"]), "audit", "--audit-level=high"],
        cwd=worktree / "site",
        environment=environment,
        inherit_environment=False,
        failure_code="node_dependency_advisory_failed",
        timeout_seconds=900,
    )
    npm_signatures = run_command(
        [str(tools["npm"]), "audit", "signatures"],
        cwd=worktree / "site",
        environment=environment,
        inherit_environment=False,
        failure_code="node_dependency_signature_failed",
        timeout_seconds=900,
    )
    node_provenance = run_command(
        [
            str(tools["python3"]),
            str(trusted_audit),
            "--site",
            str((worktree / "site").resolve()),
        ],
        cwd=worktree,
        environment=environment,
        inherit_environment=False,
        failure_code="validation_node_provenance_failed",
    )
    evidence = {
        "cargo_lock_provenance": cargo_provenance,
        "cargo_fetch_sha256": sha256_value(cargo_fetch),
        "npm_install_sha256": sha256_value(npm_install),
        "npm_advisories_sha256": sha256_value(npm_advisories),
        "npm_signatures_sha256": sha256_value(npm_signatures),
        "node_provenance_sha256": sha256_value(node_provenance),
    }
    return (
        sanitized_validation_environment(
            temporary_home,
            tools,
            cargo_home=cargo_home,
            offline=True,
        ),
        evidence,
    )


@dataclass(frozen=True)
class PinnedCandidateOutput:
    """Descriptor-pinned candidate-local validation output."""

    path: Path
    name: str
    candidate_descriptor: int
    target_descriptor: int
    output_descriptor: int


def _candidate_output_identity(
    parent_descriptor: int,
    name: str,
    *,
    failure_code: str,
) -> tuple[int, int]:
    try:
        metadata = os.stat(
            name,
            dir_fd=parent_descriptor,
            follow_symlinks=False,
        )
    except OSError as error:
        raise AutopilotError(failure_code) from error
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        raise AutopilotError(failure_code)
    return metadata.st_dev, metadata.st_ino


def _verify_pinned_directory(
    parent_descriptor: int,
    name: str,
    descriptor: int,
    *,
    exact_mode: int | None,
    failure_code: str,
) -> None:
    try:
        pinned = os.fstat(descriptor)
        current = os.stat(
            name,
            dir_fd=parent_descriptor,
            follow_symlinks=False,
        )
    except OSError as error:
        raise AutopilotError(failure_code) from error
    pinned_mode = stat.S_IMODE(pinned.st_mode)
    current_mode = stat.S_IMODE(current.st_mode)
    if (
        not stat.S_ISDIR(pinned.st_mode)
        or not stat.S_ISDIR(current.st_mode)
        or pinned.st_uid != os.getuid()
        or current.st_uid != os.getuid()
        or pinned_mode & 0o022
        or current_mode & 0o022
        or (exact_mode is not None and pinned_mode != exact_mode)
        or (exact_mode is not None and current_mode != exact_mode)
        or (pinned.st_dev, pinned.st_ino)
        != (current.st_dev, current.st_ino)
    ):
        raise AutopilotError(failure_code)


def _verify_candidate_output_directory(
    output: PinnedCandidateOutput,
    *,
    changed: bool,
) -> None:
    failure_code = (
        "validation_candidate_output_changed"
        if changed
        else "validation_candidate_output_invalid"
    )
    _verify_pinned_directory(
        output.candidate_descriptor,
        "target",
        output.target_descriptor,
        exact_mode=None,
        failure_code=failure_code,
    )
    _verify_pinned_directory(
        output.target_descriptor,
        output.name,
        output.output_descriptor,
        exact_mode=0o700,
        failure_code=failure_code,
    )


def _verify_candidate_output_empty(
    output: PinnedCandidateOutput,
    *,
    changed: bool,
) -> None:
    _verify_candidate_output_directory(output, changed=changed)
    failure_code = (
        "validation_candidate_output_changed"
        if changed
        else "validation_candidate_output_invalid"
    )
    try:
        entries = os.listdir(output.output_descriptor)
    except OSError as error:
        raise AutopilotError(failure_code) from error
    if entries:
        raise AutopilotError(failure_code)


@contextmanager
def pinned_candidate_output_directory(
    worktree: Path,
) -> Iterator[PinnedCandidateOutput]:
    """Pin the only candidate-local directory writable during validation."""

    candidate = worktree.resolve()
    candidate_descriptor: int | None = None
    target_descriptor: int | None = None
    output_descriptor: int | None = None
    output: PinnedCandidateOutput | None = None
    output_created = False
    output_identity: tuple[int, int] | None = None
    output_name = f"decodex-validation-{secrets.token_hex(16)}"
    try:
        candidate_descriptor = os.open(
            candidate,
            os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
        )
        try:
            os.mkdir(
                "target",
                mode=0o700,
                dir_fd=candidate_descriptor,
            )
        except FileExistsError:
            pass
        target_descriptor = os.open(
            "target",
            os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
            dir_fd=candidate_descriptor,
        )
        _verify_pinned_directory(
            candidate_descriptor,
            "target",
            target_descriptor,
            exact_mode=None,
            failure_code="validation_candidate_output_invalid",
        )
        os.mkdir(
            output_name,
            mode=0o700,
            dir_fd=target_descriptor,
        )
        output_created = True
        output_identity = _candidate_output_identity(
            target_descriptor,
            output_name,
            failure_code="validation_candidate_output_invalid",
        )
        output_descriptor = os.open(
            output_name,
            os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
            dir_fd=target_descriptor,
        )
        output = PinnedCandidateOutput(
            path=candidate / "target" / output_name,
            name=output_name,
            candidate_descriptor=candidate_descriptor,
            target_descriptor=target_descriptor,
            output_descriptor=output_descriptor,
        )
        _verify_candidate_output_empty(
            output,
            changed=False,
        )
        yield output
    except AutopilotError:
        raise
    except OSError as error:
        raise AutopilotError(
            "validation_candidate_output_invalid"
        ) from error
    finally:
        active_error = sys.exception()
        cleanup_error: BaseException | None = None
        if output_created and target_descriptor is not None:
            try:
                if output is not None:
                    _verify_pinned_directory(
                        output.target_descriptor,
                        output.name,
                        output.output_descriptor,
                        exact_mode=0o700,
                        failure_code=(
                            "validation_candidate_output_cleanup_failed"
                        ),
                    )
                elif output_identity is None or (
                    _candidate_output_identity(
                        target_descriptor,
                        output_name,
                        failure_code=(
                            "validation_candidate_output_cleanup_failed"
                        ),
                    )
                    != output_identity
                ):
                    raise AutopilotError(
                        "validation_candidate_output_cleanup_failed"
                    )
                if not shutil.rmtree.avoids_symlink_attacks:
                    raise OSError("descriptor-safe rmtree unavailable")
                shutil.rmtree(
                    output_name,
                    dir_fd=target_descriptor,
                )
            except OSError as error:
                cleanup_error = error
            except AutopilotError as error:
                cleanup_error = error
        if output_descriptor is not None:
            os.close(output_descriptor)
        if target_descriptor is not None:
            os.close(target_descriptor)
        if candidate_descriptor is not None:
            os.close(candidate_descriptor)
        if cleanup_error is not None:
            if isinstance(active_error, AutopilotError):
                active_error.add_related_error_code(
                    "validation_candidate_output_cleanup_failed"
                )
            elif active_error is not None:
                active_error.add_note(
                    "validation_candidate_output_cleanup_failed"
                )
            else:
                raise AutopilotError(
                    "validation_candidate_output_cleanup_failed"
                ) from cleanup_error


def _validation_profile_failure(
    repo_root: Path,
    candidate_output: PinnedCandidateOutput,
    *,
    profile: str,
    repository_head: str,
    repository_tree: str,
    failure: CommandFailure,
) -> AutopilotError:
    diagnostic_sha256 = write_validation_failure_diagnostic(
        repo_root,
        profile=profile,
        repository_head=repository_head,
        repository_tree=repository_tree,
        failure=failure,
    )
    result = AutopilotError(
        failure.code,
        diagnostic_sha256=diagnostic_sha256,
    )
    try:
        _verify_candidate_output_empty(
            candidate_output,
            changed=True,
        )
    except AutopilotError as related:
        result.add_related_error_code(related.code)
    return result


def _validate_candidate_output_path(
    candidate: Path,
    output: Path,
) -> None:
    expected_parent = candidate / "target"
    if (
        output.parent != expected_parent
        or CANDIDATE_OUTPUT_NAME_PATTERN.fullmatch(output.name) is None
    ):
        raise AutopilotError(
            "validation_candidate_output_invalid"
        )
    try:
        target_metadata = expected_parent.lstat()
        output_metadata = output.lstat()
    except OSError as error:
        raise AutopilotError(
            "validation_candidate_output_invalid"
        ) from error
    if (
        not stat.S_ISDIR(target_metadata.st_mode)
        or not stat.S_ISDIR(output_metadata.st_mode)
        or target_metadata.st_uid != os.getuid()
        or output_metadata.st_uid != os.getuid()
        or target_metadata.st_mode & 0o022
        or stat.S_IMODE(output_metadata.st_mode) != 0o700
    ):
        raise AutopilotError("validation_candidate_output_invalid")


def validation_sandbox_profile(
    repo_root: Path,
    worktree: Path,
    temporary_home: Path,
    candidate_output: Path,
    *,
    full_xcode: FullXcodeConfiguration | None = None,
    xcrun_proxy: Path | None = None,
) -> str:
    root = repo_root.resolve()
    candidate = worktree.resolve()
    home = real_home_directory()
    rustup_home = trusted_rustup_home()
    git_common_directory = repository_git_common_directory(root)
    trusted_makefile = root / "Makefile.toml"
    _validate_candidate_output_path(candidate, candidate_output)
    if (full_xcode is None) != (xcrun_proxy is None):
        raise AutopilotError("validation_sandbox_path_invalid")
    if full_xcode is not None and xcrun_proxy is not None:
        try:
            proxy_metadata = xcrun_proxy.lstat()
            proxy_directory_metadata = xcrun_proxy.parent.lstat()
        except OSError as error:
            raise AutopilotError(
                "validation_sandbox_path_invalid"
            ) from error
        if (
            xcrun_proxy.absolute()
            != temporary_home / "trusted-xcrun/xcrun"
            or not full_xcode.developer_dir.is_dir()
            or not full_xcode.metal_toolchain_root.is_dir()
            or not full_xcode.sdk_root.is_dir()
            or not stat.S_ISREG(proxy_metadata.st_mode)
            or proxy_metadata.st_uid != os.getuid()
            or proxy_metadata.st_nlink != 1
            or stat.S_IMODE(proxy_metadata.st_mode) != 0o500
            or not stat.S_ISDIR(proxy_directory_metadata.st_mode)
            or proxy_directory_metadata.st_uid != os.getuid()
            or stat.S_IMODE(proxy_directory_metadata.st_mode) != 0o700
        ):
            raise AutopilotError("validation_sandbox_path_invalid")
    if (
        not candidate.is_dir()
        or not root.is_dir()
        or not temporary_home.is_dir()
        or not rustup_home.is_dir()
        or trusted_makefile.is_symlink()
        or not trusted_makefile.is_file()
    ):
        raise AutopilotError("validation_sandbox_path_invalid")

    def literal(path: Path, *, resolve: bool = True) -> str:
        value = path.resolve(strict=False) if resolve else path.absolute()
        return json.dumps(str(value))

    readable = [
        candidate,
        git_common_directory,
        rustup_home / "toolchains",
        rustup_home / "settings.toml",
        temporary_home,
    ]
    if full_xcode is not None and xcrun_proxy is not None:
        readable.extend(
            (
                full_xcode.developer_dir,
                full_xcode.metal_toolchain_root,
                xcrun_proxy.parent,
            )
        )
    candidate_writable = (
        candidate_output,
        candidate / "site/.astro",
        candidate / "site/dist",
        candidate / "site/node_modules/.astro",
        candidate / "site/node_modules/.cache",
        candidate / "site/node_modules/.vite",
    )
    lines = [
        "(version 1)",
        "(deny default)",
        "(allow process*)",
        "(allow signal (target same-sandbox))",
        "(allow sysctl-read)",
        "(allow mach-lookup)",
        f"(allow network* (subpath {literal(temporary_home)}))",
        (
            "(deny mach-lookup "
            '(global-name "com.apple.securityd") '
            '(global-name "com.apple.securityd.system") '
            '(global-name "com.apple.securityd.xpc") '
            '(global-name "com.apple.securityd.general") '
            '(global-name "com.apple.securityd.sos") '
            '(global-name "com.apple.securityd.ckks") '
            '(global-name "com.apple.pboard") '
            '(global-name "com.apple.cfprefsd.agent") '
            '(global-name "com.apple.cfprefsd.daemon") '
            '(global-name "com.apple.security.XPCKeychainSandboxCheck"))'
        ),
    ]
    sensitive_read_roots = (
        home.parent,
        temporary_home.parent,
        Path("/Volumes"),
        Path("/Network"),
        Path("/cores"),
        Path("/private/tmp"),
        Path("/tmp"),
        Path("/private/var/tmp"),
        Path("/var/tmp"),
        Path("/private/var/root"),
        Path("/Library/Keychains"),
        Path("/private/var/db/dslocal"),
    )
    exclusions = " ".join(
        f"(require-not (subpath {literal(path)}))"
        for path in sensitive_read_roots
    )
    lines.append(f"(allow file-read* (require-all {exclusions}))")
    lines.append("(allow file-read-metadata)")
    lines.extend(
        f"(allow file-read* (subpath {literal(path)}))"
        for path in readable
        if path.exists()
    )
    lines.append(
        f"(allow file-read* (literal {literal(trusted_makefile)}))"
    )
    lines.append(
        f"(allow file-write* (subpath {literal(temporary_home)}))"
    )
    if xcrun_proxy is not None:
        lines.append(
            f"(deny file-write* (subpath {literal(xcrun_proxy.parent)}))"
        )
    lines.extend(
        f"(allow file-write* (subpath {literal(path, resolve=False)}))"
        for path in candidate_writable
    )
    lines.append('(allow file-write* (literal "/dev/null"))')
    cargo_home = temporary_home / "cargo-home"
    for path in (
        cargo_home / "git/checkouts",
        cargo_home / "git/db",
        cargo_home / "registry/cache",
        cargo_home / "registry/src",
    ):
        lines.append(f"(deny file-write* (subpath {literal(path)}))")
    return "\n".join(lines) + "\n"


def sandbox_evidence(
    profile: str,
    dependency_preparation: dict[str, Any],
    tools: dict[str, Path],
    formatter_tools: dict[str, Path],
) -> dict[str, str]:
    if set(formatter_tools) != {"cargo-fmt", "rustfmt"}:
        raise AutopilotError("validation_formatter_tool_set_invalid")
    sandbox_path = tools["sandbox-exec"].resolve(strict=True)
    return {
        "dependency_preparation_sha256": sha256_value(
            dependency_preparation
        ),
        "nightly_cargo_fmt_sha256": hash_file_bounded(
            formatter_tools["cargo-fmt"].resolve(strict=True)
        ),
        "nightly_rustfmt_sha256": hash_file_bounded(
            formatter_tools["rustfmt"].resolve(strict=True)
        ),
        "profile_sha256": sha256_value(profile),
        "sandbox_executable_sha256": hash_file_bounded(sandbox_path),
        "stable_cargo_sha256": hash_file_bounded(
            tools["cargo"].resolve(strict=True)
        ),
    }


def run_validation_profiles(
    repo_root: Path,
    worktree: Path,
    policy: dict[str, Any],
    *,
    role: str,
    candidate_kind: str,
    base_head: str,
    expected_head: str | None = None,
) -> dict[str, Any]:
    if role not in {"maintainer", "reviewer"}:
        raise AutopilotError("validation_receipt_role_mismatch")
    head, tree = repository_identity(worktree)
    if expected_head is not None and head != expected_head:
        raise AutopilotError("validation_receipt_head_mismatch")
    authority = validation_authority_identity(repo_root)
    changed_paths = changed_paths_between(
        worktree,
        base_head=base_head,
        head=head,
        policy=policy,
    )
    scope = classify_validation_scope(
        changed_paths,
        candidate_kind=candidate_kind,
        policy=policy,
    )
    task_graph_sha256 = sandbox_task_graph_sha256(repo_root)
    results: list[dict[str, Any]] = []
    profile_names = required_profile_names(scope["requires_full_gate"])
    full_xcode = (
        full_xcode_environment()
        if FULL_VALIDATION_PROFILE in profile_names
        else None
    )
    trusted_audit = (repo_root / "scripts/audit_node_lock.py").resolve()
    tool_paths, tool_evidence = validation_tools(repo_root)
    formatter_tools = trusted_formatter_tools(
        repo_root,
        tool_paths["rustup"],
    )
    formatter_environment = {
        "CARGO": str(tool_paths["cargo"]),
        "DECODEX_TRUSTED_NIGHTLY_CARGO_FMT": str(
            formatter_tools["cargo-fmt"]
        ),
        "RUSTFMT": str(formatter_tools["rustfmt"]),
    }
    with (
        validation_temporary_directory() as temporary,
        pinned_candidate_output_directory(worktree) as candidate_output,
    ):
        temporary_home = Path(temporary).resolve()
        initialize_validation_home(temporary_home)
        xcrun_proxy: Path | None = None
        xcrun_proxy_sha256: str | None = None
        if full_xcode is not None:
            xcrun_proxy, xcrun_proxy_sha256 = initialize_xcrun_proxy(
                temporary_home,
                full_xcode,
                tool_paths["python3"],
            )
        acquisition_environment = sanitized_validation_environment(
            temporary_home,
            tool_paths,
            cargo_home=temporary_home / "cargo-home",
        )
        runtime_preflight = run_command(
            [
                str(tool_paths["python3"]),
                str(trusted_audit),
                "--runtime-only",
            ],
            cwd=worktree,
            environment=acquisition_environment,
            inherit_environment=False,
            failure_code="validation_node_runtime_failed",
        )
        trusted_lock_provenance = run_command(
            [
                str(tool_paths["python3"]),
                str(trusted_audit),
                "--lock-only",
                "--site",
                str((worktree / "site").resolve()),
            ],
            cwd=worktree,
            environment=acquisition_environment,
            inherit_environment=False,
            failure_code="validation_node_lock_failed",
        )
        profile_environment, dependency_preparation = (
            prepare_dependency_cache(
                repo_root,
                worktree,
                temporary_home,
                tool_paths,
                trusted_audit,
            )
        )
        profile_environment.update(
            {
                **formatter_environment,
                CANDIDATE_OUTPUT_ENV: str(candidate_output.path),
            }
        )
        _verify_candidate_output_empty(
            candidate_output,
            changed=True,
        )
        profile = validation_sandbox_profile(
            repo_root,
            worktree,
            temporary_home,
            candidate_output.path,
            full_xcode=full_xcode,
            xcrun_proxy=xcrun_proxy,
        )
        profile_path = temporary_home / "validation.sb"
        try:
            with profile_path.open("x", encoding="utf-8") as handle:
                handle.write(profile)
                handle.flush()
                os.fsync(handle.fileno())
            os.chmod(profile_path, 0o400)
        except OSError as error:
            raise AutopilotError("validation_sandbox_profile_failed") from error
        current_sandbox_evidence = sandbox_evidence(
            profile,
            dependency_preparation,
            tool_paths,
            formatter_tools,
        )
        if validation_tool_evidence(tool_paths) != tool_evidence:
            raise AutopilotError("validation_tool_changed")
        for name in profile_names:
            command = policy["validation_profiles"][name]
            full_xcode_evidence = None
            current_environment = profile_environment
            if name == FULL_VALIDATION_PROFILE:
                if (
                    full_xcode is None
                    or xcrun_proxy is None
                    or xcrun_proxy_sha256 is None
                ):
                    raise AutopilotError("full_xcode_unavailable")
                full_xcode_evidence = {
                    **full_xcode.evidence,
                    "xcrun_proxy_sha256": xcrun_proxy_sha256,
                }
                current_environment = sanitized_validation_environment(
                    temporary_home,
                    tool_paths,
                    cargo_home=temporary_home / "cargo-home",
                    offline=True,
                    overrides={
                        **full_xcode.environment,
                        **formatter_environment,
                        CANDIDATE_OUTPUT_ENV: str(
                            candidate_output.path
                        ),
                        "PATH": os.pathsep.join(
                            (
                                str(xcrun_proxy.parent),
                                *_validation_path_entries(tool_paths),
                            )
                        ),
                    },
                )
            profile_command = trusted_profile_command(
                repo_root,
                name,
                cargo_executable=tool_paths["cargo"],
            )
            _verify_candidate_output_empty(
                candidate_output,
                changed=True,
            )
            try:
                output = run_command(
                    [
                        str(tool_paths["sandbox-exec"]),
                        "-f",
                        str(profile_path),
                        *profile_command,
                    ],
                    cwd=worktree,
                    environment=current_environment,
                    inherit_environment=False,
                    failure_code=f"validation_profile_{name}_failed",
                    timeout_seconds=3600,
                    capture_failure_diagnostic=True,
                )
            except CommandFailure as error:
                raise _validation_profile_failure(
                    repo_root,
                    candidate_output,
                    profile=name,
                    repository_head=head,
                    repository_tree=tree,
                    failure=error,
                ) from error
            _verify_candidate_output_empty(
                candidate_output,
                changed=True,
            )
            current_head, current_tree = repository_identity(worktree)
            if current_head != head or current_tree != tree:
                raise AutopilotError("validation_repository_changed")
            if validation_authority_identity(repo_root) != authority:
                raise AutopilotError("validation_authority_changed")
            if sandbox_task_graph_sha256(repo_root) != task_graph_sha256:
                raise AutopilotError("validation_task_graph_changed")
            if validation_tool_evidence(tool_paths) != tool_evidence:
                raise AutopilotError("validation_tool_changed")
            if (
                xcrun_proxy is not None
                and xcrun_proxy_sha256 is not None
                and hash_file_bounded(xcrun_proxy)
                != xcrun_proxy_sha256
            ):
                raise AutopilotError("validation_xcode_proxy_changed")
            if (
                name == FULL_VALIDATION_PROFILE
                and full_xcode_environment() != full_xcode
            ):
                raise AutopilotError("validation_xcode_changed")
            trusted_node_provenance = run_command(
                [
                    str(tool_paths["python3"]),
                    str(trusted_audit),
                    "--site",
                    str((worktree / "site").resolve()),
                ],
                cwd=worktree,
                environment=acquisition_environment,
                inherit_environment=False,
                failure_code="validation_node_provenance_failed",
            )
            if validation_authority_identity(repo_root) != authority:
                raise AutopilotError("validation_authority_changed")
            if validation_tool_evidence(tool_paths) != tool_evidence:
                raise AutopilotError("validation_tool_changed")
            if (
                sandbox_evidence(
                    profile,
                    dependency_preparation,
                    tool_paths,
                    formatter_tools,
                )
                != current_sandbox_evidence
            ):
                raise AutopilotError("validation_sandbox_changed")
            toolchain_evidence = {
                "validation_tools": tool_evidence,
                "full_xcode": full_xcode_evidence,
                "sandbox": current_sandbox_evidence,
            }
            results.append(
                {
                    "name": name,
                    "effective_task": SANDBOX_PROFILE_TASKS[name],
                    "command_sha256": profile_command_sha256(name),
                    "environment_sha256": sha256_value(
                        current_environment
                    ),
                    "exit_code": 0,
                    "output_sha256": sha256_value(
                        {
                            "command": command,
                            "effective_task": SANDBOX_PROFILE_TASKS[name],
                            "live_postgres_gate": LIVE_POSTGRES_GATE_STATUS,
                            "head": head,
                            "tree": tree,
                            "base_head": base_head,
                            "validation_authority": authority,
                            "runtime_preflight": runtime_preflight,
                            "trusted_lock_provenance": (
                                trusted_lock_provenance
                            ),
                            "dependency_preparation": (
                                dependency_preparation
                            ),
                            "trusted_node_provenance": (
                                trusted_node_provenance
                            ),
                            "sandbox_profile_sha256": (
                                current_sandbox_evidence["profile_sha256"]
                            ),
                            "toolchain_evidence": toolchain_evidence,
                            "stdout": output,
                        }
                    ),
                    "toolchain_evidence": toolchain_evidence,
                }
            )
    receipt = {
        "role": role,
        "base_head": base_head,
        "repository_head": head,
        "repository_tree": tree,
        **scope,
        "sandbox_task_graph_sha256": task_graph_sha256,
        "live_postgres_gate": LIVE_POSTGRES_GATE_STATUS,
        "validation_authority": authority,
        "profiles": results,
        "completed_at": utc_now(),
    }
    validate_receipt_against_policy(
        receipt,
        policy,
        role=role,
        expected_base_head=base_head,
        expected_head=head,
        expected_tree=tree,
    )
    return receipt


def assert_candidate_worktree(
    repo_root: Path,
    worktree: Path,
    policy: dict[str, Any],
    *,
    branch: str,
    head_sha: str,
) -> str:
    resolved = worktree.resolve()
    expected_parent = (repo_root / ".worktrees").resolve()
    try:
        resolved.relative_to(expected_parent)
    except ValueError as error:
        raise AutopilotError("candidate_worktree_outside_authority") from error
    root = Path(
        run_command(
            ["git", "rev-parse", "--show-toplevel"],
            cwd=resolved,
            failure_code="candidate_worktree_unavailable",
        )
    ).resolve()
    if root != resolved:
        raise AutopilotError("candidate_worktree_root_mismatch")
    common_dir_text = run_command(
        ["git", "rev-parse", "--git-common-dir"],
        cwd=resolved,
        failure_code="candidate_worktree_unavailable",
    )
    common_dir = Path(common_dir_text)
    if not common_dir.is_absolute():
        common_dir = resolved / common_dir
    if common_dir.resolve() != (repo_root / ".git").resolve():
        raise AutopilotError("candidate_worktree_repository_mismatch")
    current_branch = run_command(
        ["git", "branch", "--show-current"],
        cwd=resolved,
        failure_code="candidate_worktree_unavailable",
    )
    current_head, tree = repository_identity(resolved)
    if current_branch != branch or current_head != head_sha:
        raise AutopilotError("candidate_worktree_identity_mismatch")
    target_origin_urls(resolved, policy["target_repository"])
    return tree


def assert_detached_review_worktree(
    repo_root: Path,
    worktree: Path,
    policy: dict[str, Any],
    *,
    head_sha: str,
) -> str:
    resolved = worktree.resolve()
    expected_parent = (repo_root / ".worktrees").resolve()
    try:
        resolved.relative_to(expected_parent)
    except ValueError as error:
        raise AutopilotError("review_worktree_outside_authority") from error
    root = Path(
        run_command(
            ["git", "rev-parse", "--show-toplevel"],
            cwd=resolved,
            failure_code="review_worktree_unavailable",
        )
    ).resolve()
    common_dir_text = run_command(
        ["git", "rev-parse", "--git-common-dir"],
        cwd=resolved,
        failure_code="review_worktree_unavailable",
    )
    common_dir = Path(common_dir_text)
    if not common_dir.is_absolute():
        common_dir = resolved / common_dir
    branch = run_command(
        ["git", "branch", "--show-current"],
        cwd=resolved,
        failure_code="review_worktree_unavailable",
    )
    current_head, tree = repository_identity(resolved)
    if (
        root != resolved
        or common_dir.resolve() != (repo_root / ".git").resolve()
        or branch
        or current_head != head_sha
        or not command_succeeds(
            [
                "git",
                "merge-base",
                "--is-ancestor",
                head_sha,
                f"refs/remotes/origin/{policy['target_branch']}",
            ],
            cwd=repo_root,
            failure_code="review_head_containment_unavailable",
        )
    ):
        raise AutopilotError("review_worktree_identity_mismatch")
    target_origin_urls(resolved, policy["target_repository"])
    return tree


def assert_candidate_commit_worktree(
    repo_root: Path,
    worktree: Path,
    policy: dict[str, Any],
    *,
    branch: str,
) -> str:
    resolved = worktree.resolve()
    expected_parent = (repo_root / ".worktrees").resolve()
    try:
        resolved.relative_to(expected_parent)
    except ValueError as error:
        raise AutopilotError("candidate_worktree_outside_authority") from error
    root = Path(
        run_command(
            ["git", "rev-parse", "--show-toplevel"],
            cwd=resolved,
            failure_code="candidate_worktree_unavailable",
        )
    ).resolve()
    current_branch = run_command(
        ["git", "branch", "--show-current"],
        cwd=resolved,
        failure_code="candidate_worktree_unavailable",
    )
    head = run_command(
        ["git", "rev-parse", "HEAD"],
        cwd=resolved,
        failure_code="candidate_worktree_unavailable",
    )
    common_dir_text = run_command(
        ["git", "rev-parse", "--git-common-dir"],
        cwd=resolved,
        failure_code="candidate_worktree_unavailable",
    )
    common_dir = Path(common_dir_text)
    if not common_dir.is_absolute():
        common_dir = resolved / common_dir
    if (
        root != resolved
        or current_branch != branch
        or SHA_PATTERN.fullmatch(head) is None
        or common_dir.resolve() != (repo_root / ".git").resolve()
    ):
        raise AutopilotError("candidate_worktree_identity_mismatch")
    if not command_succeeds(
        ["git", "diff", "--quiet"],
        cwd=resolved,
        failure_code="candidate_worktree_diff_unavailable",
    ):
        raise AutopilotError("candidate_unstaged_changes")
    untracked = run_command(
        ["git", "ls-files", "--others", "--exclude-standard"],
        cwd=resolved,
        failure_code="candidate_worktree_diff_unavailable",
    )
    if untracked:
        raise AutopilotError("candidate_untracked_changes")
    if command_succeeds(
        ["git", "diff", "--cached", "--quiet"],
        cwd=resolved,
        failure_code="candidate_worktree_diff_unavailable",
    ):
        raise AutopilotError("candidate_staged_change_missing")
    target_origin_urls(resolved, policy["target_repository"])
    return head


def referenced_schema_evidence(state: dict[str, Any]) -> set[str]:
    values: set[str] = set()
    local_build = state.get("local_build")
    if isinstance(local_build, dict):
        for key in (
            "stable_schema_evidence_sha256",
            "experimental_schema_evidence_sha256",
        ):
            value = local_build.get(key)
            if is_sha256(value):
                values.add(value)
    for candidate in state.get("candidates", []):
        evidence = candidate.get("schema_evidence")
        if not isinstance(evidence, dict):
            continue
        for value in evidence.values():
            if is_sha256(value):
                values.add(value)
    return values
