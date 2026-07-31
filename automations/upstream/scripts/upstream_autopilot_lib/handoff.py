"""Create and validate bounded, state-bound agent handoff receipts."""

from __future__ import annotations

from contextlib import contextmanager
import fcntl
import hashlib
import hmac
import json
import os
from pathlib import Path
import re
import secrets
import stat
from typing import Any, Iterator, Sequence

from .agent import AGENT_RESULT_SCHEMA, validate_agent_execution
from .core import (
    REASON_PATTERN,
    SHA_PATTERN,
    AutopilotError,
    bounded_string_list,
    canonical_json,
    ensure_cache_root,
    has_exact_keys,
    is_sha256,
    run_command,
    sha256_value,
)
from .validation import parse_name_status_paths


HANDOFF_RECEIPT_SCHEMA = "decodex/codex-upstream-handoff-receipt/4"
MAX_HANDOFF_RECEIPT_BYTES = 16 * 1024
MAX_HANDOFF_FINDING_CODES = 16
HANDOFF_CHALLENGE_PATTERN = re.compile(r"^[A-Za-z0-9_-]{32,128}$")
HANDOFF_RECEIPT_KEYS = {
    "schema",
    "candidate_id",
    "role",
    "action",
    "claim_generation",
    "challenge",
    "base_head",
    "repository_head",
    "repository_tree",
    "staged_paths_sha256",
    "patch_sha256",
    "disposition",
    "finding_codes",
    "agent_execution",
}
HANDOFF_PROVENANCE_KEYS = (
    HANDOFF_RECEIPT_KEYS - {"challenge"}
) | {"challenge_sha256", "receipt_sha256", "consumed_at"}


def handoff_receipt_path(
    cache_root: Path,
    *,
    candidate_id: str,
    role: str,
    generation: int,
) -> Path:
    if (
        re.fullmatch(r"[0-9a-f]{16}", candidate_id) is None
        or role not in {"maintainer", "reviewer"}
        or not isinstance(generation, int)
        or generation < 1
    ):
        raise AutopilotError("handoff_receipt_identity_invalid")
    return (
        cache_root
        / "handoffs"
        / f"{candidate_id}-{role}-{generation}.json"
    )


def _open_handoff_directory(cache_root: Path, *, create: bool) -> int:
    root = ensure_cache_root(cache_root)
    directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    root_descriptor: int | None = None
    handoff_descriptor: int | None = None
    try:
        root_descriptor = os.open(root, directory_flags)
        root_metadata = os.fstat(root_descriptor)
        if (
            not stat.S_ISDIR(root_metadata.st_mode)
            or root_metadata.st_uid != os.getuid()
            or root_metadata.st_mode & 0o077
        ):
            raise AutopilotError("handoff_directory_invalid")
        if create:
            try:
                os.mkdir("handoffs", mode=0o700, dir_fd=root_descriptor)
            except FileExistsError:
                pass
        handoff_descriptor = os.open(
            "handoffs",
            directory_flags,
            dir_fd=root_descriptor,
        )
        metadata = os.fstat(handoff_descriptor)
        if (
            not stat.S_ISDIR(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o700
        ):
            raise AutopilotError("handoff_directory_invalid")
        result = handoff_descriptor
        handoff_descriptor = None
        return result
    except AutopilotError:
        raise
    except OSError as error:
        raise AutopilotError("handoff_directory_unavailable") from error
    finally:
        if handoff_descriptor is not None:
            os.close(handoff_descriptor)
        if root_descriptor is not None:
            os.close(root_descriptor)


def ensure_handoff_receipt_path(
    cache_root: Path,
    *,
    candidate_id: str,
    role: str,
    generation: int,
) -> Path:
    path = handoff_receipt_path(
        cache_root,
        candidate_id=candidate_id,
        role=role,
        generation=generation,
    )
    descriptor = _open_handoff_directory(cache_root, create=True)
    os.close(descriptor)
    return path


def _open_handoff_lock(cache_root: Path) -> int:
    root = ensure_cache_root(cache_root)
    lock_path = root / "handoffs.lock"
    try:
        descriptor = os.open(
            lock_path,
            os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW,
            0o600,
        )
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_nlink != 1
        ):
            os.close(descriptor)
            raise AutopilotError("handoff_lock_invalid")
        return descriptor
    except AutopilotError:
        raise
    except OSError as error:
        raise AutopilotError("handoff_lock_unavailable") from error


def _cleanup_handoff_temps(directory_descriptor: int) -> None:
    removed = False
    try:
        names = os.listdir(directory_descriptor)
        for name in names:
            if re.fullmatch(
                r"\.handoff-[1-9][0-9]*-[0-9a-f]{16}\.tmp",
                name,
            ) is None:
                continue
            descriptor = os.open(
                name,
                os.O_RDONLY | os.O_NOFOLLOW,
                dir_fd=directory_descriptor,
            )
            try:
                metadata = os.fstat(descriptor)
                if (
                    not stat.S_ISREG(metadata.st_mode)
                    or metadata.st_uid != os.getuid()
                    or stat.S_IMODE(metadata.st_mode) != 0o600
                    or metadata.st_nlink not in {1, 2}
                    or metadata.st_size > MAX_HANDOFF_RECEIPT_BYTES
                ):
                    raise AutopilotError("handoff_receipt_path_invalid")
            finally:
                os.close(descriptor)
            os.unlink(name, dir_fd=directory_descriptor)
            removed = True
        if removed:
            os.fsync(directory_descriptor)
    except AutopilotError:
        raise
    except OSError as error:
        raise AutopilotError("handoff_receipt_cleanup_failed") from error


@contextmanager
def _locked_handoff_directory(
    cache_root: Path,
    *,
    create: bool,
) -> Iterator[int]:
    lock_descriptor: int | None = None
    directory_descriptor: int | None = None
    try:
        lock_descriptor = _open_handoff_lock(cache_root)
        fcntl.flock(lock_descriptor, fcntl.LOCK_EX)
        directory_descriptor = _open_handoff_directory(
            cache_root,
            create=create,
        )
        _cleanup_handoff_temps(directory_descriptor)
    except AutopilotError:
        if directory_descriptor is not None:
            os.close(directory_descriptor)
        if lock_descriptor is not None:
            os.close(lock_descriptor)
        raise
    except OSError as error:
        if directory_descriptor is not None:
            os.close(directory_descriptor)
        if lock_descriptor is not None:
            os.close(lock_descriptor)
        raise AutopilotError("handoff_lock_unavailable") from error
    try:
        yield directory_descriptor
    finally:
        if directory_descriptor is not None:
            os.close(directory_descriptor)
        if lock_descriptor is not None:
            try:
                fcntl.flock(lock_descriptor, fcntl.LOCK_UN)
            finally:
                os.close(lock_descriptor)


def _receipt_location(path: Path, expected_path: Path) -> tuple[Path, str]:
    if (
        path.absolute() != expected_path.absolute()
        or expected_path.parent.name != "handoffs"
        or not expected_path.name.endswith(".json")
        or "/" in expected_path.name
    ):
        raise AutopilotError("handoff_receipt_path_invalid")
    return expected_path.parent.parent, expected_path.name


def _read_handoff_at(
    directory_descriptor: int,
    receipt_name: str,
) -> tuple[dict[str, Any], bytes]:
    receipt_descriptor: int | None = None
    try:
        receipt_descriptor = os.open(
            receipt_name,
            os.O_RDONLY | os.O_NOFOLLOW,
            dir_fd=directory_descriptor,
        )
        metadata = os.fstat(receipt_descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_nlink != 1
            or not 1 <= metadata.st_size <= MAX_HANDOFF_RECEIPT_BYTES
        ):
            raise AutopilotError("handoff_receipt_path_invalid")
        payload = bytearray()
        while len(payload) <= MAX_HANDOFF_RECEIPT_BYTES:
            chunk = os.read(
                receipt_descriptor,
                min(4096, MAX_HANDOFF_RECEIPT_BYTES + 1 - len(payload)),
            )
            if not chunk:
                break
            payload.extend(chunk)
        if len(payload) != metadata.st_size:
            raise AutopilotError("handoff_receipt_path_invalid")
        raw = bytes(payload)
        receipt = json.loads(raw.decode("utf-8"))
    finally:
        if receipt_descriptor is not None:
            os.close(receipt_descriptor)
    if len(canonical_json(receipt)) > MAX_HANDOFF_RECEIPT_BYTES:
        raise AutopilotError("handoff_receipt_budget_exceeded")
    return receipt, raw


def read_handoff_receipt(path: Path, *, expected_path: Path) -> dict[str, Any]:
    try:
        cache_root, receipt_name = _receipt_location(path, expected_path)
        with _locked_handoff_directory(
            cache_root,
            create=False,
        ) as directory_descriptor:
            receipt, _raw = _read_handoff_at(
                directory_descriptor,
                receipt_name,
            )
    except AutopilotError:
        raise
    except (
        OSError,
        UnicodeDecodeError,
        json.JSONDecodeError,
    ) as error:
        raise AutopilotError("handoff_receipt_unavailable") from error
    return receipt


def write_handoff_receipt(
    path: Path,
    *,
    expected_path: Path,
    receipt: dict[str, Any],
) -> str:
    """Create one private canonical receipt, or accept an exact retry."""

    payload = canonical_json(receipt) + b"\n"
    if not 1 <= len(payload) <= MAX_HANDOFF_RECEIPT_BYTES:
        raise AutopilotError("handoff_receipt_budget_exceeded")

    temporary_name: str | None = None
    temporary_descriptor: int | None = None
    try:
        cache_root, receipt_name = _receipt_location(path, expected_path)
        with _locked_handoff_directory(
            cache_root,
            create=True,
        ) as directory_descriptor:
            try:
                _existing_receipt, existing_payload = _read_handoff_at(
                    directory_descriptor,
                    receipt_name,
                )
            except FileNotFoundError:
                existing_payload = None
            if existing_payload is not None:
                if existing_payload != payload:
                    raise AutopilotError("handoff_receipt_conflict")
            else:
                temporary_name = (
                    f".handoff-{os.getpid()}-{secrets.token_hex(8)}.tmp"
                )
                temporary_descriptor = os.open(
                    temporary_name,
                    os.O_WRONLY
                    | os.O_CREAT
                    | os.O_EXCL
                    | os.O_NOFOLLOW,
                    0o600,
                    dir_fd=directory_descriptor,
                )
                offset = 0
                while offset < len(payload):
                    written = os.write(
                        temporary_descriptor,
                        payload[offset:],
                    )
                    if written <= 0:
                        raise AutopilotError("handoff_receipt_write_failed")
                    offset += written
                os.fsync(temporary_descriptor)
                os.close(temporary_descriptor)
                temporary_descriptor = None
                try:
                    os.link(
                        temporary_name,
                        receipt_name,
                        src_dir_fd=directory_descriptor,
                        dst_dir_fd=directory_descriptor,
                        follow_symlinks=False,
                    )
                except FileExistsError:
                    _existing_receipt, existing_payload = _read_handoff_at(
                        directory_descriptor,
                        receipt_name,
                    )
                    if existing_payload != payload:
                        raise AutopilotError("handoff_receipt_conflict")
                os.unlink(
                    temporary_name,
                    dir_fd=directory_descriptor,
                )
                temporary_name = None
                os.fsync(directory_descriptor)
                _created_receipt, created_payload = _read_handoff_at(
                    directory_descriptor,
                    receipt_name,
                )
                if created_payload != payload:
                    raise AutopilotError("handoff_receipt_write_failed")
    except AutopilotError:
        raise
    except OSError as error:
        raise AutopilotError("handoff_receipt_write_failed") from error
    finally:
        if temporary_descriptor is not None:
            os.close(temporary_descriptor)
        if temporary_name is not None:
            try:
                cache_root, _receipt_name = _receipt_location(
                    path,
                    expected_path,
                )
                with _locked_handoff_directory(
                    cache_root,
                    create=True,
                ) as directory_descriptor:
                    try:
                        os.unlink(
                            temporary_name,
                            dir_fd=directory_descriptor,
                        )
                        os.fsync(directory_descriptor)
                    except FileNotFoundError:
                        pass
            except AutopilotError:
                pass

    loaded = read_handoff_receipt(path, expected_path=expected_path)
    if loaded != receipt:
        raise AutopilotError("handoff_receipt_write_failed")
    return hashlib.sha256(payload).hexdigest()


def remove_handoff_receipt(
    path: Path,
    *,
    expected_path: Path,
    missing_ok: bool = False,
) -> None:
    receipt_descriptor: int | None = None
    try:
        cache_root, receipt_name = _receipt_location(path, expected_path)
        with _locked_handoff_directory(
            cache_root,
            create=False,
        ) as directory_descriptor:
            try:
                receipt_descriptor = os.open(
                    receipt_name,
                    os.O_RDONLY | os.O_NOFOLLOW,
                    dir_fd=directory_descriptor,
                )
            except FileNotFoundError:
                if missing_ok:
                    return
                raise
            metadata = os.fstat(receipt_descriptor)
            if (
                not stat.S_ISREG(metadata.st_mode)
                or metadata.st_uid != os.getuid()
                or stat.S_IMODE(metadata.st_mode) != 0o600
                or metadata.st_nlink != 1
            ):
                raise AutopilotError("handoff_receipt_cleanup_failed")
            current = os.stat(
                receipt_name,
                dir_fd=directory_descriptor,
                follow_symlinks=False,
            )
            if (current.st_dev, current.st_ino) != (
                metadata.st_dev,
                metadata.st_ino,
            ):
                raise AutopilotError("handoff_receipt_cleanup_failed")
            os.close(receipt_descriptor)
            receipt_descriptor = None
            os.unlink(receipt_name, dir_fd=directory_descriptor)
            os.fsync(directory_descriptor)
    except AutopilotError:
        raise
    except OSError as error:
        raise AutopilotError("handoff_receipt_cleanup_failed") from error
    finally:
        if receipt_descriptor is not None:
            os.close(receipt_descriptor)


def reconcile_handoff_receipts(
    cache_root: Path,
    state: dict[str, Any],
) -> list[str]:
    """Remove canonical receipts that no live state handoff can consume."""

    keep: set[str] = set()
    candidates = state.get("candidates") if isinstance(state, dict) else None
    if not isinstance(candidates, list):
        raise AutopilotError("handoff_reconciliation_state_invalid")
    for candidate in candidates:
        if not isinstance(candidate, dict):
            raise AutopilotError("handoff_reconciliation_state_invalid")
        handoff = candidate.get("handoff")
        lease = candidate.get("lease")
        if handoff is None:
            continue
        agent_run = (
            handoff.get("agent_run") if isinstance(handoff, dict) else None
        )
        completed_recovery = bool(
            lease is None
            and isinstance(handoff, dict)
            and handoff.get("consumed") is None
            and isinstance(agent_run, dict)
            and agent_run.get("phase") == "completed"
            and candidate.get("status")
            in {"queued", "repair_requested", "review_pending"}
        )
        if (
            not isinstance(handoff, dict)
            or not isinstance(candidate.get("id"), str)
            or re.fullmatch(r"[0-9a-f]{16}", candidate["id"]) is None
            or handoff.get("role") not in {"maintainer", "reviewer"}
            or (
                not completed_recovery
                and (
                    not isinstance(lease, dict)
                    or handoff.get("role") != lease.get("role")
                    or handoff.get("generation") != lease.get("generation")
                )
            )
        ):
            raise AutopilotError("handoff_reconciliation_state_invalid")
        keep.add(
            handoff_receipt_path(
                cache_root,
                candidate_id=candidate.get("id"),
                role=handoff["role"],
                generation=handoff["generation"],
            ).name
        )

    directory = cache_root / "handoffs"
    try:
        directory.lstat()
    except FileNotFoundError:
        return []
    except OSError as error:
        raise AutopilotError("handoff_directory_unavailable") from error
    removed: list[str] = []
    try:
        with _locked_handoff_directory(
            cache_root,
            create=False,
        ) as directory_descriptor:
            for name in sorted(os.listdir(directory_descriptor)):
                if name in keep:
                    continue
                if re.fullmatch(
                    r"[0-9a-f]{16}-(?:maintainer|reviewer)-"
                    r"[1-9][0-9]*\.json",
                    name,
                ) is None:
                    raise AutopilotError(
                        "handoff_receipt_path_invalid"
                    )
                receipt_descriptor = os.open(
                    name,
                    os.O_RDONLY | os.O_NOFOLLOW,
                    dir_fd=directory_descriptor,
                )
                try:
                    metadata = os.fstat(receipt_descriptor)
                    if (
                        not stat.S_ISREG(metadata.st_mode)
                        or metadata.st_uid != os.getuid()
                        or stat.S_IMODE(metadata.st_mode) != 0o600
                        or metadata.st_nlink != 1
                        or not 1
                        <= metadata.st_size
                        <= MAX_HANDOFF_RECEIPT_BYTES
                    ):
                        raise AutopilotError(
                            "handoff_receipt_cleanup_failed"
                        )
                    current = os.stat(
                        name,
                        dir_fd=directory_descriptor,
                        follow_symlinks=False,
                    )
                    if (current.st_dev, current.st_ino) != (
                        metadata.st_dev,
                        metadata.st_ino,
                    ):
                        raise AutopilotError(
                            "handoff_receipt_cleanup_failed"
                        )
                finally:
                    os.close(receipt_descriptor)
                os.unlink(name, dir_fd=directory_descriptor)
                removed.append(name)
            if removed:
                os.fsync(directory_descriptor)
    except AutopilotError:
        raise
    except OSError as error:
        raise AutopilotError("handoff_receipt_cleanup_failed") from error
    return removed


def staged_handoff_identity(worktree: Path) -> dict[str, str]:
    head = run_command(
        ["git", "rev-parse", "HEAD"],
        cwd=worktree,
        failure_code="handoff_worktree_identity_unavailable",
    )
    tree = run_command(
        ["git", "write-tree"],
        cwd=worktree,
        failure_code="handoff_worktree_identity_unavailable",
    )
    names = run_command(
        [
            "git",
            "diff",
            "--cached",
            "--find-renames",
            "--find-copies",
            "--name-status",
            "-z",
        ],
        cwd=worktree,
        failure_code="handoff_worktree_identity_unavailable",
        max_output_bytes=8 * 1024 * 1024,
    )
    paths = parse_name_status_paths(names)
    if (
        SHA_PATTERN.fullmatch(head) is None
        or SHA_PATTERN.fullmatch(tree) is None
        or not paths
        or len(paths) > 4096
    ):
        raise AutopilotError("handoff_worktree_identity_invalid")
    return {
        "repository_head": head,
        "repository_tree": tree,
        "staged_paths_sha256": hashlib.sha256(
            names.encode("utf-8")
        ).hexdigest(),
    }


def validate_handoff_receipt(
    receipt: Any,
    *,
    candidate_id: str,
    role: str,
    action: str,
    generation: int,
    challenge_sha256: str,
    base_head: str,
    repository_head: str,
    repository_tree: str,
    staged_paths_sha256: str | None,
    patch_sha256: str | None,
    disposition: str,
    finding_codes: Sequence[str],
    consumed_at: int,
) -> dict[str, Any]:
    normalized_codes = sorted(set(finding_codes))
    if (
        not has_exact_keys(receipt, HANDOFF_RECEIPT_KEYS)
        or receipt.get("schema") != HANDOFF_RECEIPT_SCHEMA
        or receipt.get("candidate_id") != candidate_id
        or receipt.get("role") != role
        or receipt.get("action") != action
        or receipt.get("claim_generation") != generation
        or not isinstance(receipt.get("challenge"), str)
        or HANDOFF_CHALLENGE_PATTERN.fullmatch(receipt["challenge"]) is None
        or receipt.get("base_head") != base_head
        or receipt.get("repository_head") != repository_head
        or receipt.get("repository_tree") != repository_tree
        or receipt.get("staged_paths_sha256") != staged_paths_sha256
        or receipt.get("patch_sha256") != patch_sha256
        or receipt.get("disposition") != disposition
        or receipt.get("finding_codes") != normalized_codes
        or not bounded_string_list(
            normalized_codes,
            pattern=REASON_PATTERN,
            maximum=MAX_HANDOFF_FINDING_CODES,
        )
        or any(
            SHA_PATTERN.fullmatch(value) is None
            for value in (base_head, repository_head, repository_tree)
        )
        or (
            staged_paths_sha256 is not None
            and not is_sha256(staged_paths_sha256)
        )
        or (patch_sha256 is not None and not is_sha256(patch_sha256))
        or not isinstance(consumed_at, int)
    ):
        raise AutopilotError("handoff_receipt_invalid")
    actual_challenge_sha256 = hashlib.sha256(
        receipt["challenge"].encode("utf-8")
    ).hexdigest()
    if not hmac.compare_digest(actual_challenge_sha256, challenge_sha256):
        raise AutopilotError("handoff_challenge_invalid")
    validate_agent_execution(
        receipt["agent_execution"],
        candidate_id=candidate_id,
        role=role,
        generation=generation,
        result={
            "schema": AGENT_RESULT_SCHEMA,
            "role": role,
            "disposition": disposition,
            "finding_codes": normalized_codes,
            "patch_sha256": patch_sha256,
        },
    )
    receipt_sha256 = sha256_value(receipt)
    return {
        key: receipt[key]
        for key in HANDOFF_RECEIPT_KEYS
        if key != "challenge"
    } | {
        "challenge_sha256": actual_challenge_sha256,
        "receipt_sha256": receipt_sha256,
        "consumed_at": consumed_at,
    }


def validate_handoff_provenance(value: Any) -> None:
    if (
        not has_exact_keys(value, HANDOFF_PROVENANCE_KEYS)
        or value.get("schema") != HANDOFF_RECEIPT_SCHEMA
        or re.fullmatch(r"[0-9a-f]{16}", str(value.get("candidate_id", "")))
        is None
        or value.get("role") not in {"maintainer", "reviewer"}
        or value.get("action") not in {"worker_staged", "independent_review"}
        or not isinstance(value.get("claim_generation"), int)
        or value["claim_generation"] < 1
        or any(
            SHA_PATTERN.fullmatch(str(value.get(key, ""))) is None
            for key in ("base_head", "repository_head", "repository_tree")
        )
        or (
            value.get("staged_paths_sha256") is not None
            and not is_sha256(value["staged_paths_sha256"])
        )
        or (
            value.get("patch_sha256") is not None
            and not is_sha256(value["patch_sha256"])
        )
        or value.get("disposition")
        not in {"staged", "accept", "request_repair", "no_change", "rejected"}
        or not bounded_string_list(
            value.get("finding_codes"),
            pattern=REASON_PATTERN,
            maximum=MAX_HANDOFF_FINDING_CODES,
        )
        or not is_sha256(value.get("challenge_sha256"))
        or not is_sha256(value.get("receipt_sha256"))
        or not isinstance(value.get("consumed_at"), int)
    ):
        raise AutopilotError("handoff_provenance_invalid")
    try:
        validate_agent_execution(
            value["agent_execution"],
            candidate_id=value["candidate_id"],
            role=value["role"],
            generation=value["claim_generation"],
            result={
                "schema": AGENT_RESULT_SCHEMA,
                "role": value["role"],
                "disposition": value["disposition"],
                "finding_codes": value["finding_codes"],
                "patch_sha256": value["patch_sha256"],
            },
        )
    except AutopilotError as error:
        raise AutopilotError("handoff_provenance_invalid") from error
    if value["action"] == "worker_staged":
        if (
            value["role"] != "maintainer"
            or value["disposition"] != "staged"
            or value["staged_paths_sha256"] is None
            or value["patch_sha256"] is None
            or value["finding_codes"]
        ):
            raise AutopilotError("handoff_provenance_invalid")
    elif (
        value["role"] != "reviewer"
        or value["staged_paths_sha256"] is not None
        or value["patch_sha256"] is not None
    ):
        raise AutopilotError("handoff_provenance_invalid")
