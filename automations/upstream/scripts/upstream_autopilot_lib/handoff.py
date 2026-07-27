"""Validate bounded, state-bound subagent handoff receipts."""

from __future__ import annotations

import hashlib
import hmac
import json
import os
from pathlib import Path
import re
import stat
from typing import Any, Sequence

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


HANDOFF_RECEIPT_SCHEMA = "decodex/codex-upstream-handoff-receipt/1"
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
    "disposition",
    "finding_codes",
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


def _receipt_location(path: Path, expected_path: Path) -> tuple[Path, str]:
    if (
        path.absolute() != expected_path.absolute()
        or expected_path.parent.name != "handoffs"
        or not expected_path.name.endswith(".json")
        or "/" in expected_path.name
    ):
        raise AutopilotError("handoff_receipt_path_invalid")
    return expected_path.parent.parent, expected_path.name


def read_handoff_receipt(path: Path, *, expected_path: Path) -> dict[str, Any]:
    directory_descriptor: int | None = None
    receipt_descriptor: int | None = None
    try:
        cache_root, receipt_name = _receipt_location(path, expected_path)
        directory_descriptor = _open_handoff_directory(
            cache_root,
            create=False,
        )
        receipt_descriptor = os.open(
            receipt_name,
            os.O_RDONLY | os.O_NOFOLLOW,
            dir_fd=directory_descriptor,
        )
        metadata = os.fstat(receipt_descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or metadata.st_mode & 0o077
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
        receipt = json.loads(bytes(payload).decode("utf-8"))
    except AutopilotError:
        raise
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise AutopilotError("handoff_receipt_unavailable") from error
    finally:
        if receipt_descriptor is not None:
            os.close(receipt_descriptor)
        if directory_descriptor is not None:
            os.close(directory_descriptor)
    if len(canonical_json(receipt)) > MAX_HANDOFF_RECEIPT_BYTES:
        raise AutopilotError("handoff_receipt_budget_exceeded")
    return receipt


def remove_handoff_receipt(
    path: Path,
    *,
    expected_path: Path,
    missing_ok: bool = False,
) -> None:
    directory_descriptor: int | None = None
    receipt_descriptor: int | None = None
    try:
        cache_root, receipt_name = _receipt_location(path, expected_path)
        directory_descriptor = _open_handoff_directory(
            cache_root,
            create=False,
        )
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
            or metadata.st_mode & 0o077
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
        os.unlink(receipt_name, dir_fd=directory_descriptor)
    except AutopilotError:
        raise
    except OSError as error:
        raise AutopilotError("handoff_receipt_cleanup_failed") from error
    finally:
        if receipt_descriptor is not None:
            os.close(receipt_descriptor)
        if directory_descriptor is not None:
            os.close(directory_descriptor)


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
        or not isinstance(consumed_at, int)
    ):
        raise AutopilotError("handoff_receipt_invalid")
    actual_challenge_sha256 = hashlib.sha256(
        receipt["challenge"].encode("utf-8")
    ).hexdigest()
    if not hmac.compare_digest(actual_challenge_sha256, challenge_sha256):
        raise AutopilotError("handoff_challenge_invalid")
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
    if value["action"] == "worker_staged":
        if (
            value["role"] != "maintainer"
            or value["disposition"] != "staged"
            or value["staged_paths_sha256"] is None
            or value["finding_codes"]
        ):
            raise AutopilotError("handoff_provenance_invalid")
    elif value["role"] != "reviewer" or value["staged_paths_sha256"] is not None:
        raise AutopilotError("handoff_provenance_invalid")
