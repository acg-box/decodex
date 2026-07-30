"""Store bounded owner receipts for scheduled Codex task cleanup."""

from __future__ import annotations

from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
from typing import Any, Iterator

from .core import AutopilotError, atomic_write_json, utc_now


TASK_RETENTION_RECEIPT_SCHEMA = "decodex/codex-task-retention-receipt/2"
TASK_RETENTION_RECEIPT_ROOT = Path(
    ".agent/automations/upstream/cache/task-retention"
)
MAX_TASK_RETENTION_BATCH = 50
MAX_TASK_RETENTION_RECEIPTS = 512
MAX_SETTLED_RECEIPTS = 128
SETTLED_RECEIPT_MAX_AGE_SECONDS = 30 * 24 * 60 * 60
MAX_RECEIPT_BYTES = 2 * 1024
MAX_EVIDENCE_BYTES = 1024 * 1024
MAX_VALIDATOR_OUTPUT_BYTES = 4 * 1024
SOCIAL_VALIDATOR_RELATIVE_PATH = Path(
    "target/debug/decodex-publisher"
)
SOCIAL_VALIDATOR_OUTPUT_PATTERN = re.compile(
    rb"^validated [1-9][0-9]* social state file\(s\)\n$"
)
THREAD_ID_PATTERN = re.compile(
    r"^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$"
)
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
RESULT_CODE_PATTERN = re.compile(r"^[a-z0-9][a-z0-9_]{0,63}$")

MANAGED_TASKS = {
    "codex-upstream-maintainer": (
        "Codex Upstream Maintainer",
        "automations/upstream/prompts/maintainer.md",
    ),
    "codex-upstream-reviewer": (
        "Codex Upstream Reviewer And Lander",
        "automations/upstream/prompts/reviewer.md",
    ),
    "codex-upstream-health": (
        "Codex Upstream Automation Health",
        "automations/upstream/prompts/health.md",
    ),
    "decodex-content-manager": (
        "Decodex Content Manager",
        "automations/decodex/prompts/content-manager.md",
    ),
    "decodex-xurl-publisher": (
        "Decodex Xurl Publisher",
        "automations/decodex/prompts/xurl-publisher.md",
    ),
}

SUCCESS_RESULT_CODES = {
    "codex-upstream-maintainer": frozenset(
        {
            "no_candidate",
            "repair_queued",
            "role_busy",
            "review_pending",
        }
    ),
    "codex-upstream-reviewer": frozenset(
        {
            "no_candidate",
            "repair_queued",
            "role_busy",
            "no_change",
            "rejected",
            "landed",
            "repair_requested",
            "stale_decision_requeued",
        }
    ),
    "codex-upstream-health": frozenset({"pass"}),
    "decodex-content-manager": frozenset(
        {
            "candidate_recorded",
            "quality_skip_recorded",
            "strategy_recorded",
            "proven_no_op",
        }
    ),
    "decodex-xurl-publisher": frozenset(
        {
            "published",
            "outcome_observed",
            "quality_skip",
            "duplicate",
            "proven_no_op",
        }
    ),
}

EVIDENCE_COLLECTIONS = {
    "candidate": Path(
        ".agent/automations/decodex/cache/social/x/candidates"
    ),
    "strategy": Path(
        ".agent/automations/decodex/cache/manager/strategy"
    ),
    "post": Path(
        ".agent/automations/decodex/cache/social/x/posts"
    ),
    "outcome": Path(
        ".agent/automations/decodex/cache/social/x/outcomes"
    ),
}
RESULT_EVIDENCE_KINDS = {
    ("decodex-content-manager", "candidate_recorded"): "candidate",
    ("decodex-content-manager", "quality_skip_recorded"): "candidate",
    ("decodex-content-manager", "strategy_recorded"): "strategy",
    ("decodex-xurl-publisher", "published"): "post",
    ("decodex-xurl-publisher", "outcome_observed"): "outcome",
    ("decodex-xurl-publisher", "quality_skip"): "post",
}

RECEIPT_KEYS = {
    "schema",
    "automation_id",
    "thread_id",
    "terminal_result_code",
    "evidence_kind",
    "evidence_sha256",
    "timestamp",
    "status",
}
PENDING_STATUS = "pending_archive"
ARCHIVED_STATUS = "archived_readback_confirmed"
KEEP_VISIBLE_PREFIX = "keep_visible:"


def _valid_thread_id(value: Any) -> bool:
    return isinstance(value, str) and THREAD_ID_PATTERN.fullmatch(value) is not None


def _valid_result_code(value: Any) -> bool:
    return (
        isinstance(value, str)
        and RESULT_CODE_PATTERN.fullmatch(value) is not None
    )


def _valid_status(value: Any) -> bool:
    if value in {PENDING_STATUS, ARCHIVED_STATUS}:
        return True
    if not isinstance(value, str) or not value.startswith(KEEP_VISIBLE_PREFIX):
        return False
    return _valid_result_code(value.removeprefix(KEEP_VISIBLE_PREFIX))


def _valid_evidence_projection(
    *,
    automation_id: str,
    terminal_result_code: str,
    evidence_kind: Any,
    evidence_sha256: Any,
    status: str,
) -> bool:
    expected_kind = RESULT_EVIDENCE_KINDS.get(
        (automation_id, terminal_result_code)
    )
    has_evidence = (
        isinstance(evidence_kind, str)
        and evidence_kind in EVIDENCE_COLLECTIONS
        and isinstance(evidence_sha256, str)
        and DIGEST_PATTERN.fullmatch(evidence_sha256) is not None
    )
    has_no_evidence = evidence_kind is None and evidence_sha256 is None
    successful = terminal_result_code in SUCCESS_RESULT_CODES[automation_id]
    if status.startswith(KEEP_VISIBLE_PREFIX):
        return has_no_evidence or (
            successful and expected_kind == evidence_kind and has_evidence
        )
    return (
        successful
        and (
            expected_kind == evidence_kind
            if expected_kind is not None
            else has_no_evidence
        )
        and (has_evidence if expected_kind is not None else True)
    )


def _validate_receipt(value: Any) -> dict[str, Any]:
    if (
        not isinstance(value, dict)
        or set(value) != RECEIPT_KEYS
        or value.get("schema") != TASK_RETENTION_RECEIPT_SCHEMA
        or value.get("automation_id") not in MANAGED_TASKS
        or not _valid_thread_id(value.get("thread_id"))
        or not _valid_result_code(value.get("terminal_result_code"))
        or not isinstance(value.get("timestamp"), int)
        or isinstance(value.get("timestamp"), bool)
        or value["timestamp"] < 0
        or not _valid_status(value.get("status"))
        or not _valid_evidence_projection(
            automation_id=value["automation_id"],
            terminal_result_code=value["terminal_result_code"],
            evidence_kind=value.get("evidence_kind"),
            evidence_sha256=value.get("evidence_sha256"),
            status=value["status"],
        )
    ):
        raise AutopilotError("task_retention_receipt_invalid")
    raw = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    if len(raw) > MAX_RECEIPT_BYTES:
        raise AutopilotError("task_retention_receipt_invalid")
    return value


def _receipt_root(repo_root: Path, *, create: bool) -> Path:
    root = repo_root / TASK_RETENTION_RECEIPT_ROOT
    try:
        if create:
            root.mkdir(parents=True, exist_ok=True, mode=0o700)
            os.chmod(root, 0o700)
        metadata = root.lstat()
    except FileNotFoundError as error:
        raise AutopilotError("task_retention_store_unavailable") from error
    except OSError as error:
        raise AutopilotError("task_retention_store_invalid") from error
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        raise AutopilotError("task_retention_store_invalid")
    return root


@contextmanager
def _locked_receipts(repo_root: Path) -> Iterator[Path]:
    root = _receipt_root(repo_root, create=True)
    lock_path = root / ".lock"
    try:
        flags = os.O_CREAT | os.O_RDWR | os.O_CLOEXEC
        if hasattr(os, "O_NOFOLLOW"):
            flags |= os.O_NOFOLLOW
        descriptor = os.open(
            lock_path,
            flags,
            0o600,
        )
        os.fchmod(descriptor, 0o600)
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_nlink != 1
        ):
            os.close(descriptor)
            raise AutopilotError("task_retention_store_invalid")
        fcntl.flock(descriptor, fcntl.LOCK_EX)
    except OSError as error:
        raise AutopilotError("task_retention_store_invalid") from error
    try:
        yield root
    finally:
        fcntl.flock(descriptor, fcntl.LOCK_UN)
        os.close(descriptor)


def _receipt_path(root: Path, thread_id: str) -> Path:
    if not _valid_thread_id(thread_id):
        raise AutopilotError("task_retention_thread_id_invalid")
    return root / f"{thread_id}.json"


def _read_receipt(path: Path) -> dict[str, Any]:
    try:
        metadata = path.lstat()
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_nlink != 1
            or metadata.st_size > MAX_RECEIPT_BYTES
        ):
            raise AutopilotError("task_retention_receipt_invalid")
        value = json.loads(path.read_text(encoding="utf-8"))
    except AutopilotError:
        raise
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise AutopilotError("task_retention_receipt_invalid") from error
    receipt = _validate_receipt(value)
    if path.name != f"{receipt['thread_id']}.json":
        raise AutopilotError("task_retention_receipt_invalid")
    return receipt


def _scan_receipts(root: Path) -> list[tuple[Path, dict[str, Any]]]:
    try:
        entries = list(root.iterdir())
    except OSError as error:
        raise AutopilotError("task_retention_store_invalid") from error
    receipt_paths = sorted(
        (entry for entry in entries if entry.name != ".lock"),
        key=lambda path: path.name,
    )
    if (
        len(receipt_paths) > MAX_TASK_RETENTION_RECEIPTS
        or any(
            path.suffix != ".json"
            or not _valid_thread_id(path.stem)
            for path in receipt_paths
        )
    ):
        raise AutopilotError("task_retention_store_capacity_exceeded")
    return [(path, _read_receipt(path)) for path in receipt_paths]


def _prune_settled(
    records: list[tuple[Path, dict[str, Any]]],
    *,
    now: int,
) -> int:
    settled = sorted(
        (
            (path, receipt)
            for path, receipt in records
            if receipt["status"] != PENDING_STATUS
        ),
        key=lambda item: (item[1]["timestamp"], item[1]["thread_id"]),
        reverse=True,
    )
    keep = {
        receipt["thread_id"]
        for _path, receipt in settled[:MAX_SETTLED_RECEIPTS]
        if now - receipt["timestamp"] <= SETTLED_RECEIPT_MAX_AGE_SECONDS
    }
    pruned = 0
    for path, receipt in settled:
        if receipt["thread_id"] in keep:
            continue
        try:
            path.unlink()
        except OSError as error:
            raise AutopilotError("task_retention_store_invalid") from error
        pruned += 1
    if pruned:
        descriptor: int | None = None
        try:
            descriptor = os.open(path.parent, os.O_RDONLY | os.O_CLOEXEC)
            os.fsync(descriptor)
        except OSError as error:
            raise AutopilotError("task_retention_store_invalid") from error
        finally:
            if descriptor is not None:
                os.close(descriptor)
    return pruned


def _decode_evidence_json(raw: bytes) -> Any:
    def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        value: dict[str, Any] = {}
        for key, item in pairs:
            if key in value:
                raise ValueError("duplicate JSON key")
            value[key] = item
        return value

    def reject_constant(_value: str) -> None:
        raise ValueError("non-finite JSON value")

    try:
        return json.loads(
            raw,
            object_pairs_hook=unique_object,
            parse_constant=reject_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError) as error:
        raise AutopilotError("task_retention_evidence_invalid") from error


def _read_evidence_bytes(repo_root: Path, relative_path: Path) -> bytes:
    directory_descriptor: int | None = None
    file_descriptor: int | None = None
    try:
        directory_flags = os.O_RDONLY | os.O_CLOEXEC
        if hasattr(os, "O_DIRECTORY"):
            directory_flags |= os.O_DIRECTORY
        if hasattr(os, "O_NOFOLLOW"):
            directory_flags |= os.O_NOFOLLOW
        directory_descriptor = os.open(repo_root, directory_flags)
        for component in relative_path.parts[:-1]:
            next_descriptor = os.open(
                component,
                directory_flags,
                dir_fd=directory_descriptor,
            )
            os.close(directory_descriptor)
            directory_descriptor = next_descriptor

        file_flags = os.O_RDONLY | os.O_CLOEXEC
        if hasattr(os, "O_NOFOLLOW"):
            file_flags |= os.O_NOFOLLOW
        file_descriptor = os.open(
            relative_path.name,
            file_flags,
            dir_fd=directory_descriptor,
        )
        before = os.fstat(file_descriptor)
        if (
            not stat.S_ISREG(before.st_mode)
            or before.st_uid != os.getuid()
            or stat.S_IMODE(before.st_mode) != 0o600
            or before.st_nlink != 1
            or before.st_size > MAX_EVIDENCE_BYTES
        ):
            raise AutopilotError("task_retention_evidence_invalid")
        chunks = []
        remaining = MAX_EVIDENCE_BYTES + 1
        while remaining:
            chunk = os.read(file_descriptor, min(64 * 1024, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        raw = b"".join(chunks)
        after = os.fstat(file_descriptor)
        if (
            len(raw) > MAX_EVIDENCE_BYTES
            or (
                before.st_dev,
                before.st_ino,
                before.st_size,
                before.st_mtime_ns,
            )
            != (
                after.st_dev,
                after.st_ino,
                after.st_size,
                after.st_mtime_ns,
            )
            or len(raw) != before.st_size
        ):
            raise AutopilotError("task_retention_evidence_invalid")
        return raw
    except AutopilotError:
        raise
    except OSError as error:
        raise AutopilotError("task_retention_evidence_invalid") from error
    finally:
        if file_descriptor is not None:
            os.close(file_descriptor)
        if directory_descriptor is not None:
            os.close(directory_descriptor)


def _validate_evidence_semantics(
    *,
    value: Any,
    evidence_kind: str,
    automation_id: str,
    terminal_result_code: str,
    thread_id: str,
    filename: str,
) -> None:
    if not isinstance(value, dict):
        raise AutopilotError("task_retention_evidence_invalid")
    if automation_id == "decodex-content-manager":
        if filename != f"{thread_id}.json":
            raise AutopilotError("task_retention_evidence_invalid")
        if evidence_kind == "strategy":
            valid = (
                terminal_result_code == "strategy_recorded"
                and value.get("schema") == "social_strategy/v1"
            )
        else:
            worthiness = value.get("decision")
            worthiness = (
                worthiness.get("worthiness")
                if isinstance(worthiness, dict)
                else None
            )
            expected_worthiness = (
                "publish"
                if terminal_result_code == "candidate_recorded"
                else "skip"
            )
            valid = (
                value.get("schema") == "social_candidate/v1"
                and worthiness == expected_worthiness
            )
    elif evidence_kind == "outcome":
        valid = (
            filename == f"{thread_id}.json"
            and value.get("schema") == "social_outcome/v1"
        )
    else:
        valid = (
            value.get("schema") == "social_post/v1"
            and value.get("status")
            == (
                "published"
                if terminal_result_code == "published"
                else "skipped"
            )
            and (
                terminal_result_code != "published"
                or filename == f"{thread_id}.json"
            )
        )
    if not valid:
        raise AutopilotError("task_retention_evidence_invalid")
    if automation_id == "decodex-xurl-publisher":
        owner = value.get("owner")
        if (
            not isinstance(owner, dict)
            or owner.get("automation_id") != automation_id
            or owner.get("run_id") != thread_id
        ):
            raise AutopilotError("task_retention_evidence_invalid")


def _validate_social_store(repo_root: Path) -> None:
    validator = repo_root / SOCIAL_VALIDATOR_RELATIVE_PATH
    try:
        metadata = validator.lstat()
    except OSError as error:
        raise AutopilotError(
            "task_retention_evidence_validation_failed"
        ) from error
    if (
        not stat.S_ISREG(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or metadata.st_nlink != 1
        or metadata.st_mode & 0o022
        or not metadata.st_mode & stat.S_IXUSR
    ):
        raise AutopilotError(
            "task_retention_evidence_validation_failed"
        )
    try:
        completed = subprocess.run(
            [str(validator), "validate-social"],
            cwd=repo_root,
            env={
                "LANG": "C",
                "LC_ALL": "C",
                "PATH": "/usr/bin:/bin",
            },
            capture_output=True,
            check=False,
            timeout=60,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise AutopilotError(
            "task_retention_evidence_validation_failed"
        ) from error
    if (
        completed.returncode != 0
        or completed.stderr
        or len(completed.stdout) > MAX_VALIDATOR_OUTPUT_BYTES
        or SOCIAL_VALIDATOR_OUTPUT_PATTERN.fullmatch(
            completed.stdout
        )
        is None
    ):
        raise AutopilotError(
            "task_retention_evidence_validation_failed"
        )


def _validated_evidence(
    *,
    repo_root: Path,
    thread_id: str,
    automation_id: str,
    terminal_result_code: str,
    evidence_path: str | None,
) -> tuple[str | None, str | None]:
    expected_kind = RESULT_EVIDENCE_KINDS.get(
        (automation_id, terminal_result_code)
    )
    if expected_kind is None:
        if evidence_path is not None:
            raise AutopilotError("task_retention_evidence_unexpected")
        return None, None
    if not isinstance(evidence_path, str) or not 1 <= len(evidence_path) <= 512:
        raise AutopilotError("task_retention_evidence_required")
    if "\x00" in evidence_path:
        raise AutopilotError("task_retention_evidence_path_invalid")
    relative_path = Path(evidence_path)
    collection = EVIDENCE_COLLECTIONS[expected_kind]
    if (
        relative_path.is_absolute()
        or evidence_path != relative_path.as_posix()
        or relative_path.parent != collection
        or relative_path.suffix != ".json"
    ):
        raise AutopilotError("task_retention_evidence_path_invalid")
    raw = _read_evidence_bytes(repo_root, relative_path)
    value = _decode_evidence_json(raw)
    _validate_evidence_semantics(
        value=value,
        evidence_kind=expected_kind,
        automation_id=automation_id,
        terminal_result_code=terminal_result_code,
        thread_id=thread_id,
        filename=relative_path.name,
    )
    _validate_social_store(repo_root)
    if _read_evidence_bytes(repo_root, relative_path) != raw:
        raise AutopilotError("task_retention_evidence_invalid")
    return expected_kind, hashlib.sha256(raw).hexdigest()


def seal_task_retention(
    *,
    repo_root: Path,
    thread_id: str,
    automation_id: str,
    terminal_result_code: str,
    evidence_path: str | None,
    keep_visible_reason: str | None,
    now: int | None = None,
) -> dict[str, Any]:
    """Create one owner receipt for the current app-provided task ID."""

    if (
        automation_id not in MANAGED_TASKS
        or not _valid_thread_id(thread_id)
        or not _valid_result_code(terminal_result_code)
        or (
            keep_visible_reason is not None
            and not _valid_result_code(keep_visible_reason)
        )
    ):
        raise AutopilotError("task_retention_seal_invalid")
    timestamp = utc_now() if now is None else now
    status = (
        PENDING_STATUS
        if keep_visible_reason is None
        else f"{KEEP_VISIBLE_PREFIX}{keep_visible_reason}"
    )
    if status == PENDING_STATUS:
        evidence_kind, evidence_sha256 = _validated_evidence(
            repo_root=repo_root,
            thread_id=thread_id,
            automation_id=automation_id,
            terminal_result_code=terminal_result_code,
            evidence_path=evidence_path,
        )
    else:
        if evidence_path is not None:
            raise AutopilotError("task_retention_evidence_unexpected")
        evidence_kind, evidence_sha256 = None, None
    receipt = _validate_receipt(
        {
            "schema": TASK_RETENTION_RECEIPT_SCHEMA,
            "automation_id": automation_id,
            "thread_id": thread_id,
            "terminal_result_code": terminal_result_code,
            "evidence_kind": evidence_kind,
            "evidence_sha256": evidence_sha256,
            "timestamp": timestamp,
            "status": status,
        }
    )
    with _locked_receipts(repo_root) as root:
        path = _receipt_path(root, thread_id)
        if path.exists():
            existing = _read_receipt(path)
            stable_keys = RECEIPT_KEYS - {"timestamp", "status"}
            if any(existing[key] != receipt[key] for key in stable_keys):
                raise AutopilotError("task_retention_receipt_conflict")
            return existing
        if len(_scan_receipts(root)) >= MAX_TASK_RETENTION_RECEIPTS:
            raise AutopilotError("task_retention_store_capacity_exceeded")
        atomic_write_json(path, receipt)
    return receipt


def plan_task_retention(
    *,
    repo_root: Path,
    current_thread_id: str,
    now: int | None = None,
) -> dict[str, Any]:
    """Return bound pending tasks; native task reads remain the manager's job."""

    if not _valid_thread_id(current_thread_id):
        raise AutopilotError("task_retention_manager_thread_id_invalid")
    timestamp = utc_now() if now is None else now
    with _locked_receipts(repo_root) as root:
        records = _scan_receipts(root)
        pruned = _prune_settled(records, now=timestamp)
        pending = sorted(
            (
                receipt
                for _path, receipt in records
                if receipt["status"] == PENDING_STATUS
                and receipt["thread_id"] != current_thread_id
            ),
            key=lambda receipt: (
                receipt["timestamp"],
                receipt["thread_id"],
            ),
        )
    selected = pending[:MAX_TASK_RETENTION_BATCH]
    return {
        "active_manager_thread_id": current_thread_id,
        "pending_tasks": [
            {
                key: receipt[key]
                for key in (
                    "thread_id",
                    "automation_id",
                    "terminal_result_code",
                    "evidence_kind",
                    "evidence_sha256",
                )
            }
            for receipt in selected
        ],
        "pending_count": len(pending),
        "has_more": len(pending) > len(selected),
        "pruned_settled_count": pruned,
    }


def settle_task_retention(
    *,
    repo_root: Path,
    current_thread_id: str,
    thread_id: str,
    result: str,
    reason: str | None,
    now: int | None = None,
) -> dict[str, Any]:
    """Record the Health manager's exact native readback result."""

    if (
        not _valid_thread_id(current_thread_id)
        or not _valid_thread_id(thread_id)
        or current_thread_id == thread_id
        or result not in {"archived", "keep-visible"}
        or (
            result == "archived"
            and reason is not None
        )
        or (
            result == "keep-visible"
            and not _valid_result_code(reason)
        )
    ):
        raise AutopilotError("task_retention_settle_invalid")
    timestamp = utc_now() if now is None else now
    with _locked_receipts(repo_root) as root:
        path = _receipt_path(root, thread_id)
        if not path.exists():
            raise AutopilotError("task_retention_receipt_missing")
        receipt = _read_receipt(path)
        if receipt["status"] != PENDING_STATUS:
            raise AutopilotError("task_retention_receipt_not_pending")
        receipt["timestamp"] = timestamp
        receipt["status"] = (
            ARCHIVED_STATUS
            if result == "archived"
            else f"{KEEP_VISIBLE_PREFIX}{reason}"
        )
        _validate_receipt(receipt)
        atomic_write_json(path, receipt)
        records = _scan_receipts(root)
        pruned = _prune_settled(records, now=timestamp)
    return {
        "thread_id": thread_id,
        "status": receipt["status"],
        "settled": True,
        "pruned_settled_count": pruned,
    }
