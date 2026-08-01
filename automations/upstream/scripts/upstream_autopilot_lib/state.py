"""Persist and transition the bounded upstream-autopilot state machine."""

from __future__ import annotations

from contextlib import contextmanager
from copy import deepcopy
import fcntl
import hashlib
import hmac
import json
import os
from pathlib import Path
import re
import secrets
import stat
from typing import Any, Iterator, Mapping, Sequence

from .core import (
    ALLOWED_CANDIDATE_KINDS,
    CANDIDATE_KEYS,
    CODEX_VERSION_PATTERN,
    CONTENT_DEGRADATION_CODES,
    LAND_EFFECT_LEASE_BUDGET_SECONDS,
    MAX_ACTIVE_SOURCE_CANDIDATES,
    MAX_EVENTS,
    MAX_LAND_RECOVERY_WORKTREES,
    MAX_METRIC_BUCKETS,
    MAX_SCHEMA_BYTES,
    MAX_SCHEMA_EVIDENCE_BYTES,
    MAX_SCHEMA_EVIDENCE_FILES,
    MAX_STATE_BYTES,
    MAX_STATE_CANDIDATES,
    METRIC_BUCKET_SECONDS,
    PR_PATTERN,
    PROACTIVE_IMPROVEMENT_REASON_CODES,
    REASON_PATTERN,
    SAFE_FACT_PATTERN,
    SHA_PATTERN,
    SIDE_EFFECT_LEASE_BUDGET_SECONDS,
    SOURCE_KEYS,
    STATE_SCHEMA,
    STATE_KEYS,
    TERMINAL_STATUSES,
    TAG_PATTERN,
    AutopilotError,
    Observation,
    atomic_write_json,
    bounded_string_list,
    canonical_json,
    command_succeeds,
    ensure_cache_root,
    has_exact_keys,
    is_sha256,
    run_command,
    sha256_value,
    utc_now,
    validate_candidate_result,
    validate_path_summary,
    validate_x_pricing_audit_evidence,
)
from .observation import mirror_arguments
from .effectiveness import (
    classify_lifetime_outcomes,
    effectiveness_improvement_reason,
    rolling_effectiveness,
)
from .validation import validate_validation_receipt
from .handoff import validate_handoff_provenance, validate_handoff_receipt


AGENT_RUN_KEYS = {
    "phase",
    "role",
    "generation",
    "base_head",
    "input_head",
    "repository_head",
    "input_tree",
    "repository_tree",
    "challenge_sha256",
    "started_at",
    "completed_at",
    "receipt_sha256",
    "receipt_file_sha256",
    "agent_execution_sha256",
    "disposition",
    "finding_codes",
}
STALE_REFRESH_KEYS = {
    "old_base_head",
    "old_head_sha",
    "target_base_head",
    "prepared_at",
    "updated_at",
}
PUBLISH_VALIDATION_REPAIR_REASON_CODES = frozenset(
    {
        "validation_diff_invalid",
        "validation_profile_cargo_make_check_failed",
        "validation_profile_cargo_make_check_upstream_automation_failed",
        "validation_profile_focused_tests_failed",
    }
)
LEGACY_STATE_NAMES = (
    "state.json",
    "state.recovery.json",
)
LEGACY_STATE_LOCK_NAME = "state.lock"


def validate_agent_run(value: Any) -> None:
    if (
        not has_exact_keys(value, AGENT_RUN_KEYS)
        or value.get("phase") not in {"prepared", "completed"}
        or value.get("role") not in {"maintainer", "reviewer"}
        or not isinstance(value.get("generation"), int)
        or value["generation"] < 1
        or any(
            SHA_PATTERN.fullmatch(str(value.get(key, ""))) is None
            for key in (
                "base_head",
                "input_head",
                "repository_head",
                "input_tree",
            )
        )
        or not is_sha256(value.get("challenge_sha256"))
        or not isinstance(value.get("started_at"), int)
    ):
        raise AutopilotError("candidate_agent_run_invalid")
    completed_fields = (
        value.get("completed_at"),
        value.get("receipt_sha256"),
        value.get("receipt_file_sha256"),
        value.get("agent_execution_sha256"),
        value.get("disposition"),
        value.get("finding_codes"),
    )
    if value["phase"] == "prepared":
        if (
            value.get("repository_tree") is not None
            or any(field is not None for field in completed_fields)
        ):
            raise AutopilotError("candidate_agent_run_invalid")
        return
    if (
        SHA_PATTERN.fullmatch(str(value.get("repository_tree", ""))) is None
        or not isinstance(value.get("completed_at"), int)
        or value["completed_at"] < value["started_at"]
        or not is_sha256(value.get("receipt_sha256"))
        or not is_sha256(value.get("receipt_file_sha256"))
        or not is_sha256(value.get("agent_execution_sha256"))
        or value.get("disposition")
        not in {"staged", "accept", "request_repair", "no_change", "rejected"}
        or not bounded_string_list(
            value.get("finding_codes"),
            pattern=REASON_PATTERN,
            maximum=16,
        )
        or (
            value["disposition"] == "request_repair"
        )
        != bool(value["finding_codes"])
        or (
            value["role"] == "maintainer"
            and (
                value["disposition"] != "staged"
                or value["finding_codes"]
            )
        )
        or (
            value["role"] == "reviewer"
            and value["disposition"] == "staged"
        )
    ):
        raise AutopilotError("candidate_agent_run_invalid")


def candidate_has_blocked_publish_validation(
    candidate: Mapping[str, Any],
) -> bool:
    result = candidate.get("result")
    return bool(
        isinstance(result, Mapping)
        and result.get("outcome") == "blocked"
        and result.get("reason_code")
        in PUBLISH_VALIDATION_REPAIR_REASON_CODES
    )


def candidate_has_pre_publish_stale_refresh(
    candidate: Mapping[str, Any],
) -> bool:
    stale_refresh = candidate.get("stale_refresh")
    return bool(
        isinstance(stale_refresh, Mapping)
        and candidate_has_blocked_publish_validation(candidate)
        and candidate.get("pull_request") is None
        and candidate.get("commit_receipt") is None
    )


def valid_owned_worktrees(value: Any) -> bool:
    if (
        not isinstance(value, list)
        or not 1 <= len(value) <= MAX_LAND_RECOVERY_WORKTREES
        or len(value) != len(set(value))
    ):
        return False
    for item in value:
        if not isinstance(item, str) or not 1 <= len(item) <= 512:
            return False
        path = Path(item)
        if (
            path.is_absolute()
            or ".." in path.parts
            or len(path.parts) < 2
            or path.parts[0] != ".worktrees"
        ):
            return False
    return True


def proposal_validation_receipt(candidate: dict[str, Any]) -> dict[str, Any]:
    pull_request = candidate.get("pull_request")
    decision = candidate.get("decision")
    if isinstance(pull_request, dict) and decision is None:
        receipt = pull_request.get("validation_receipt")
        if (
            not isinstance(receipt, dict)
            or receipt.get("repository_head") != pull_request.get("head_sha")
        ):
            raise AutopilotError("candidate_proposal_evidence_invalid")
        return receipt
    if isinstance(decision, dict) and pull_request is None:
        receipt = decision.get("maintainer_receipt")
        if not isinstance(receipt, dict):
            raise AutopilotError("candidate_proposal_evidence_invalid")
        return receipt
    raise AutopilotError("candidate_proposal_evidence_invalid")


def handoff_matches_validation_receipt(
    handoff: dict[str, Any],
    receipt: dict[str, Any],
) -> bool:
    return all(
        handoff.get(field) == receipt.get(field)
        for field in ("base_head", "repository_head", "repository_tree")
    )


def validate_reviewer_handoff_semantics(
    candidate: dict[str, Any],
    handoff: dict[str, Any],
    *,
    disposition: str,
    finding_codes: Sequence[str],
    receipt: dict[str, Any] | None = None,
) -> None:
    validate_handoff_provenance(handoff)
    proposal_receipt = proposal_validation_receipt(candidate)
    review_receipt = proposal_receipt if receipt is None else receipt
    if (
        handoff.get("candidate_id") != candidate.get("id")
        or handoff.get("role") != "reviewer"
        or handoff.get("action") != "independent_review"
        or handoff.get("disposition") != disposition
        or handoff.get("finding_codes") != sorted(set(finding_codes))
        or handoff.get("staged_paths_sha256") is not None
        or not handoff_matches_validation_receipt(handoff, review_receipt)
        or any(
            review_receipt.get(field) != proposal_receipt.get(field)
            for field in ("base_head", "repository_head", "repository_tree")
        )
    ):
        raise AutopilotError("reviewer_handoff_receipt_invalid")


def new_state(now: int) -> dict[str, Any]:
    return {
        "schema": STATE_SCHEMA,
        "persistence_generation": 0,
        "created_at": now,
        "updated_at": now,
        "last_observed_at": None,
        "source": {
            "observed_head_sha": None,
            "queued_head_sha": None,
            "cursor_sha": None,
            "cursor_sequence": 0,
            "next_sequence": 1,
            "next_discovery_sequence": 1,
            "next_lease_generation": 1,
            "observation_started_generation": 0,
            "observation_applied_generation": 0,
            "stable_tag": None,
            "stable_tag_sha": None,
            "prerelease_tag": None,
            "prerelease_tag_sha": None,
            "schema_fingerprints": {
                "upstream_main": None,
                "stable_release": None,
                "prerelease": None,
            },
        },
        "local_build": None,
        "candidates": [],
        "events": [],
        "metrics": {"buckets": []},
    }


def _load_state_file(path: Path) -> dict[str, Any]:
    if path.is_symlink():
        raise AutopilotError("state_path_symlink")
    try:
        if path.stat().st_size > MAX_STATE_BYTES:
            raise AutopilotError("state_budget_exceeded")
        state = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise AutopilotError("state_unavailable") from error
    validate_state(state)
    return state


def state_recovery_path(path: Path) -> Path:
    return path.with_name(f"{path.stem}.recovery{path.suffix}")


def load_state(path: Path) -> dict[str, Any]:
    recovery_path = state_recovery_path(path)
    existing = [
        candidate for candidate in (path, recovery_path) if candidate.exists()
    ]
    if not existing:
        return new_state(utc_now())
    valid: list[dict[str, Any]] = []
    for candidate in existing:
        try:
            valid.append(_load_state_file(candidate))
        except AutopilotError as error:
            if error.code == "state_path_symlink":
                raise
    if not valid:
        raise AutopilotError("state_unavailable")
    valid.sort(key=lambda value: value["persistence_generation"], reverse=True)
    if (
        len(valid) > 1
        and valid[0]["persistence_generation"]
        == valid[1]["persistence_generation"]
        and valid[0] != valid[1]
    ):
        raise AutopilotError("state_recovery_conflict")
    return valid[0]


@contextmanager
def locked_state(cache_root: Path) -> Iterator[tuple[dict[str, Any], Path]]:
    root = ensure_cache_root(cache_root)
    lock_path = root / "state-v4.lock"
    state_path = root / "state-v4.json"
    if lock_path.exists() and lock_path.is_symlink():
        raise AutopilotError("state_lock_symlink")
    try:
        with lock_path.open("a+", encoding="utf-8") as lock:
            os.chmod(lock_path, 0o600)
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
            legacy_lock_path = root / LEGACY_STATE_LOCK_NAME
            if (
                os.path.lexists(legacy_lock_path)
                and legacy_lock_path.is_symlink()
            ):
                raise AutopilotError("state_lock_symlink")
            with legacy_lock_path.open("a+", encoding="utf-8") as legacy_lock:
                os.chmod(legacy_lock_path, 0o600)
                try:
                    fcntl.flock(
                        legacy_lock.fileno(),
                        fcntl.LOCK_EX | fcntl.LOCK_NB,
                    )
                except BlockingIOError as error:
                    raise AutopilotError(
                        "legacy_state_process_active"
                    ) from error
                for name in LEGACY_STATE_NAMES:
                    legacy_path = root / name
                    if os.path.lexists(legacy_path):
                        raise AutopilotError(
                            "legacy_state_cutover_required"
                        )
                state = load_state(state_path)
                yield state, state_path
                fcntl.flock(legacy_lock.fileno(), fcntl.LOCK_UN)
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)
    except OSError as error:
        raise AutopilotError("state_lock_failed") from error


def validate_state(state: dict[str, Any]) -> None:
    if not has_exact_keys(state, STATE_KEYS):
        raise AutopilotError("state_shape_invalid")
    if state.get("schema") != STATE_SCHEMA:
        raise AutopilotError("state_schema_invalid")
    source = state.get("source")
    candidates = state.get("candidates")
    events = state.get("events")
    metrics = state.get("metrics")
    if (
        not has_exact_keys(source, SOURCE_KEYS)
        or not isinstance(candidates, list)
        or not isinstance(events, list)
        or not has_exact_keys(metrics, {"buckets"})
        or not isinstance(metrics["buckets"], list)
    ):
        raise AutopilotError("state_shape_invalid")
    if (
        len(candidates) > MAX_STATE_CANDIDATES
        or len(events) > MAX_EVENTS
        or len(metrics["buckets"]) > MAX_METRIC_BUCKETS
    ):
        raise AutopilotError("state_budget_exceeded")
    if len(canonical_json(state)) > MAX_STATE_BYTES:
        raise AutopilotError("state_budget_exceeded")
    for field in ("created_at", "updated_at"):
        if not isinstance(state.get(field), int):
            raise AutopilotError("state_timestamp_invalid")
    if (
        not isinstance(state.get("persistence_generation"), int)
        or state["persistence_generation"] < 0
    ):
        raise AutopilotError("state_persistence_generation_invalid")
    if state.get("last_observed_at") is not None and not isinstance(
        state["last_observed_at"], int
    ):
        raise AutopilotError("state_timestamp_invalid")
    for field in ("observed_head_sha", "queued_head_sha", "cursor_sha"):
        value = source.get(field)
        if value is not None and (
            not isinstance(value, str) or SHA_PATTERN.fullmatch(value) is None
        ):
            raise AutopilotError("state_source_sha_invalid")
    if (
        not isinstance(source.get("cursor_sequence"), int)
        or source["cursor_sequence"] < 0
        or not isinstance(source.get("next_sequence"), int)
        or source["next_sequence"] <= source["cursor_sequence"]
        or not isinstance(source.get("next_discovery_sequence"), int)
        or source["next_discovery_sequence"] < 1
        or not isinstance(source.get("next_lease_generation"), int)
        or source["next_lease_generation"] < 1
        or not isinstance(source.get("observation_started_generation"), int)
        or source["observation_started_generation"] < 0
        or not isinstance(source.get("observation_applied_generation"), int)
        or source["observation_applied_generation"] < 0
        or source["observation_applied_generation"]
        > source["observation_started_generation"]
        or (source["cursor_sequence"] == 0) != (source["cursor_sha"] is None)
    ):
        raise AutopilotError("state_source_sequence_invalid")
    if (source["observed_head_sha"] is None) != (source["queued_head_sha"] is None):
        raise AutopilotError("state_source_sequence_invalid")
    for tag_field, sha_field in (
        ("stable_tag", "stable_tag_sha"),
        ("prerelease_tag", "prerelease_tag_sha"),
    ):
        value = source.get(tag_field)
        tag_sha = source.get(sha_field)
        if (
            (value is None) != (tag_sha is None)
            or (
                value is not None
                and (
                    not isinstance(value, str)
                    or TAG_PATTERN.fullmatch(value) is None
                    or not isinstance(tag_sha, str)
                    or SHA_PATTERN.fullmatch(tag_sha) is None
                )
            )
        ):
            raise AutopilotError("state_release_tag_invalid")
    source_fingerprints = source.get("schema_fingerprints")
    if not has_exact_keys(
        source_fingerprints,
        {"upstream_main", "stable_release", "prerelease"},
    ):
        raise AutopilotError("state_source_schema_invalid")
    for value in source_fingerprints.values():
        if value is not None and not is_sha256(value):
            raise AutopilotError("state_source_schema_invalid")
    local_build = state.get("local_build")
    if local_build is not None:
        if (
            not has_exact_keys(
                local_build,
                {
                    "codex_version",
                    "codex_executable_sha256",
                    "policy_fingerprint",
                    "accepted_marker_fingerprint",
                    "stable_schema_fingerprint",
                    "experimental_schema_fingerprint",
                    "stable_schema_evidence_sha256",
                    "experimental_schema_evidence_sha256",
                    "contract_missing",
                    "observed_at",
                },
            )
            or CODEX_VERSION_PATTERN.fullmatch(str(local_build.get("codex_version", "")))
            is None
            or not isinstance(local_build.get("observed_at"), int)
            or not bounded_string_list(
                local_build.get("contract_missing"),
                pattern=SAFE_FACT_PATTERN,
                maximum=512,
            )
        ):
            raise AutopilotError("state_local_build_invalid")
        for field in ("stable_schema_fingerprint", "experimental_schema_fingerprint"):
            if not is_sha256(local_build.get(field)):
                raise AutopilotError("state_local_build_invalid")
        for field in (
            "codex_executable_sha256",
            "policy_fingerprint",
            "accepted_marker_fingerprint",
            "stable_schema_evidence_sha256",
            "experimental_schema_evidence_sha256",
        ):
            if not is_sha256(local_build.get(field)):
                raise AutopilotError("state_local_build_invalid")
    seen_ids: set[str] = set()
    seen_sequences: set[int] = set()
    seen_discovery_sequences: set[int] = set()
    seen_handoff_challenges: set[str] = set()
    seen_handoff_receipts: set[str] = set()
    for candidate in candidates:
        if not has_exact_keys(candidate, CANDIDATE_KEYS):
            raise AutopilotError("candidate_shape_invalid")
        identifier = candidate.get("id")
        status = candidate.get("status")
        if not isinstance(identifier, str) or not re.fullmatch(r"[0-9a-f]{16}", identifier):
            raise AutopilotError("candidate_id_invalid")
        if identifier in seen_ids:
            raise AutopilotError("candidate_id_duplicate")
        seen_ids.add(identifier)
        discovery_sequence = candidate.get("discovery_sequence")
        if (
            not isinstance(discovery_sequence, int)
            or discovery_sequence < 1
            or discovery_sequence >= source["next_discovery_sequence"]
            or discovery_sequence in seen_discovery_sequences
        ):
            raise AutopilotError("candidate_discovery_sequence_invalid")
        seen_discovery_sequences.add(discovery_sequence)
        if status not in {
            "queued",
            "implementing",
            "review_pending",
            "reviewing",
            "repair_requested",
            "repair_pending",
            "retry_wait",
            "needs_attention",
            *TERMINAL_STATUSES,
        }:
            raise AutopilotError("candidate_status_invalid")
        if candidate.get("kind") not in ALLOWED_CANDIDATE_KINDS:
            raise AutopilotError("candidate_kind_invalid")
        if candidate.get("priority") not in {"critical", "normal"}:
            raise AutopilotError("candidate_priority_invalid")
        if candidate.get("branch_name") != f"xv/codex-upstream-{identifier}":
            raise AutopilotError("candidate_branch_invalid")
        if CODEX_VERSION_PATTERN.fullmatch(str(candidate.get("codex_version", ""))) is None:
            raise AutopilotError("candidate_codex_version_invalid")
        for field in (
            "codex_executable_sha256",
            "policy_fingerprint",
            "accepted_marker_fingerprint",
        ):
            if not is_sha256(candidate.get(field)):
                raise AutopilotError("candidate_observation_identity_invalid")
        if not bounded_string_list(
            candidate.get("contract_missing"),
            pattern=SAFE_FACT_PATTERN,
            maximum=512,
        ):
            raise AutopilotError("candidate_contract_invalid")
        fingerprints = candidate.get("schema_fingerprints")
        if not has_exact_keys(
            fingerprints,
            {
                "stable",
                "experimental",
                "upstream_main",
                "upstream_stable_release",
                "upstream_prerelease",
            },
        ):
            raise AutopilotError("candidate_schema_fingerprint_invalid")
        for field in ("stable", "experimental", "upstream_main"):
            if not is_sha256(fingerprints.get(field)):
                raise AutopilotError("candidate_schema_fingerprint_invalid")
        for field in ("upstream_stable_release", "upstream_prerelease"):
            value = fingerprints.get(field)
            if value is not None and not is_sha256(value):
                raise AutopilotError("candidate_schema_fingerprint_invalid")
        schema_evidence = candidate.get("schema_evidence")
        if (
            not has_exact_keys(schema_evidence, {"stable", "experimental"})
            or any(
                value is not None and not is_sha256(value)
                for value in schema_evidence.values()
            )
            or (
                status not in TERMINAL_STATUSES
                and any(value is None for value in schema_evidence.values())
            )
        ):
            raise AutopilotError("candidate_schema_evidence_invalid")
        for field in ("from_sha", "to_sha"):
            value = candidate.get(field)
            if value is not None and (
                not isinstance(value, str) or SHA_PATTERN.fullmatch(value) is None
            ):
                raise AutopilotError("candidate_source_sha_invalid")
        release_tag = candidate.get("release_tag")
        if release_tag is not None and (
            not isinstance(release_tag, str) or TAG_PATTERN.fullmatch(release_tag) is None
        ):
            raise AutopilotError("candidate_release_tag_invalid")
        repair_of = candidate.get("repair_of")
        if repair_of is not None and re.fullmatch(r"[0-9a-f]{16}", str(repair_of)) is None:
            raise AutopilotError("candidate_repair_target_invalid")
        if candidate["kind"] != "automation_repair" and repair_of is not None:
            raise AutopilotError("candidate_repair_target_invalid")
        if candidate["kind"] == "automation_repair" and status == "rejected":
            raise AutopilotError("automation_repair_rejected_invalid")
        validate_path_summary(candidate)
        attempts = candidate.get("attempts")
        if (
            not isinstance(attempts, dict)
            or set(attempts) != {"maintainer", "reviewer"}
            or any(
                not isinstance(attempts[role], int) or not 0 <= attempts[role] <= 10
                for role in ("maintainer", "reviewer")
            )
        ):
            raise AutopilotError("candidate_attempts_invalid")
        for field in ("created_at", "updated_at"):
            if not isinstance(candidate.get(field), int):
                raise AutopilotError("candidate_timestamp_invalid")
        next_retry_at = candidate.get("next_retry_at")
        retry_role = candidate.get("retry_role")
        if next_retry_at is not None and not isinstance(next_retry_at, int):
            raise AutopilotError("candidate_retry_invalid")
        if retry_role is not None and retry_role not in {"maintainer", "reviewer"}:
            raise AutopilotError("candidate_retry_invalid")
        if status == "retry_wait":
            if next_retry_at is None or retry_role is None:
                raise AutopilotError("candidate_retry_invalid")
        elif status in {"needs_attention", "repair_pending"}:
            if next_retry_at is not None or retry_role is None:
                raise AutopilotError("candidate_retry_invalid")
        elif next_retry_at is not None or retry_role is not None:
            raise AutopilotError("candidate_retry_invalid")
        sequence = candidate.get("source_sequence")
        if sequence is not None:
            if not isinstance(sequence, int) or sequence <= 0 or sequence in seen_sequences:
                raise AutopilotError("candidate_sequence_invalid")
            seen_sequences.add(sequence)
        kind = candidate["kind"]
        if kind == "bootstrap" and not (
            sequence is not None
            and candidate["from_sha"] is None
            and candidate["to_sha"] is not None
            and release_tag is None
        ):
            raise AutopilotError("candidate_source_shape_invalid")
        if kind == "upstream_range" and not (
            sequence is not None
            and candidate["from_sha"] is not None
            and candidate["to_sha"] is not None
            and release_tag is None
        ):
            raise AutopilotError("candidate_source_shape_invalid")
        if kind in {"stable_release", "prerelease_release"} and not (
            sequence is None
            and candidate["from_sha"] is None
            and candidate["to_sha"] is not None
            and release_tag is not None
        ):
            raise AutopilotError("candidate_source_shape_invalid")
        if kind == "stable_release" and TAG_PATTERN.fullmatch(release_tag).group(
            "label"
        ) is not None:
            raise AutopilotError("candidate_source_shape_invalid")
        if kind == "prerelease_release" and TAG_PATTERN.fullmatch(release_tag).group(
            "label"
        ) is None:
            raise AutopilotError("candidate_source_shape_invalid")
        if kind == "local_build" and not (
            sequence is None
            and candidate["from_sha"] is None
            and candidate["to_sha"] is not None
            and release_tag is None
        ):
            raise AutopilotError("candidate_source_shape_invalid")
        if kind == "automation_repair" and sequence is not None:
            raise AutopilotError("candidate_source_shape_invalid")
        lease = candidate.get("lease")
        if lease is not None:
            if (
                not has_exact_keys(
                    lease,
                    {
                        "role",
                        "generation",
                        "token_sha256",
                        "issued_at",
                        "expires_at",
                        "renewals",
                    },
                )
                or lease.get("role") not in {"maintainer", "reviewer"}
                or not isinstance(lease.get("generation"), int)
                or not 1 <= lease["generation"] < source["next_lease_generation"]
                or not is_sha256(lease.get("token_sha256"))
                or not isinstance(lease.get("issued_at"), int)
                or not isinstance(lease.get("expires_at"), int)
                or lease["expires_at"] <= lease["issued_at"]
                or not isinstance(lease.get("renewals"), int)
                or not 0 <= lease["renewals"] <= 12
            ):
                raise AutopilotError("candidate_lease_invalid")
        expected_lease_role = {
            "implementing": "maintainer",
            "reviewing": "reviewer",
        }.get(status)
        if (
            (expected_lease_role is None and lease is not None)
            or (
                expected_lease_role is not None
                and (lease is None or lease["role"] != expected_lease_role)
            )
        ):
            raise AutopilotError("candidate_lease_invalid")
        handoff = candidate.get("handoff")
        if handoff is not None:
            if (
                not has_exact_keys(
                    handoff,
                    {
                        "role",
                        "generation",
                        "challenge_sha256",
                        "issued_at",
                        "consumed",
                        "agent_run",
                    },
                )
                or handoff.get("role") not in {"maintainer", "reviewer"}
                or not isinstance(handoff.get("generation"), int)
                or handoff["generation"] < 1
                or not is_sha256(handoff.get("challenge_sha256"))
                or not isinstance(handoff.get("issued_at"), int)
                or handoff["challenge_sha256"] in seen_handoff_challenges
                or (
                    lease is not None
                    and (
                        handoff["role"] != lease["role"]
                        or handoff["generation"] != lease["generation"]
                    )
                )
            ):
                raise AutopilotError("candidate_handoff_invalid")
            seen_handoff_challenges.add(handoff["challenge_sha256"])
            agent_run = handoff["agent_run"]
            if agent_run is not None:
                validate_agent_run(agent_run)
                if (
                    agent_run["role"] != handoff["role"]
                    or agent_run["generation"] != handoff["generation"]
                    or agent_run["challenge_sha256"]
                    != handoff["challenge_sha256"]
                ):
                    raise AutopilotError("candidate_handoff_invalid")
            consumed = handoff["consumed"]
            if consumed is not None:
                validate_handoff_provenance(consumed)
                if (
                    consumed["candidate_id"] != candidate["id"]
                    or consumed["role"] != handoff["role"]
                    or consumed["claim_generation"] != handoff["generation"]
                    or consumed["challenge_sha256"]
                    != handoff["challenge_sha256"]
                    or consumed["receipt_sha256"] in seen_handoff_receipts
                    or (
                        isinstance(agent_run, dict)
                        and agent_run["phase"] == "completed"
                        and (
                            consumed["receipt_sha256"]
                            != agent_run["receipt_sha256"]
                            or consumed["agent_execution"][
                                "execution_sha256"
                            ]
                            != agent_run["agent_execution_sha256"]
                        )
                    )
                ):
                    raise AutopilotError("candidate_handoff_invalid")
                seen_handoff_receipts.add(consumed["receipt_sha256"])
            if lease is None:
                recovery_role = {
                    "queued": "maintainer",
                    "repair_requested": "maintainer",
                    "review_pending": "reviewer",
                }.get(status)
                if not (
                    recovery_role == handoff["role"]
                    and consumed is None
                    and isinstance(agent_run, dict)
                    and agent_run["phase"] == "completed"
                ):
                    raise AutopilotError("candidate_handoff_invalid")
        effect = candidate.get("effect")
        if effect is not None:
            if (
                not has_exact_keys(
                    effect,
                    {
                        "kind",
                        "lease_generation",
                        "active_lease_generation",
                        "intent_sha256",
                        "phase",
                        "branch",
                        "head_sha",
                        "remote_head_before",
                        "owned_worktrees",
                        "pr_url",
                        "validation_receipt",
                        "handoff_receipt",
                        "decodex_identity",
                        "command_receipt",
                        "execution_receipt",
                        "started_at",
                        "updated_at",
                    },
                )
                or effect.get("kind")
                not in {"commit", "publish", "retire_pr", "land"}
                or not isinstance(effect.get("lease_generation"), int)
                or effect["lease_generation"] < 1
                or not isinstance(effect.get("active_lease_generation"), int)
                or effect["active_lease_generation"] < 1
                or effect["active_lease_generation"]
                >= source["next_lease_generation"]
                or (
                    lease is not None
                    and effect["active_lease_generation"]
                    != lease["generation"]
                )
                or not is_sha256(effect.get("intent_sha256"))
                or effect.get("phase")
                not in {
                    "prepared",
                    "pushed",
                    "pr_created",
                    "land_started",
                    "land_command_completed",
                    "land_completed",
                }
                or effect.get("branch") != candidate["branch_name"]
                or SHA_PATTERN.fullmatch(str(effect.get("head_sha", ""))) is None
                or (
                    effect.get("remote_head_before") is not None
                    and SHA_PATTERN.fullmatch(
                        str(effect["remote_head_before"])
                    )
                    is None
                )
                or (
                    effect.get("pr_url") is not None
                    and PR_PATTERN.fullmatch(str(effect["pr_url"])) is None
                )
                or (
                    effect["kind"] == "land"
                    and not valid_owned_worktrees(effect.get("owned_worktrees"))
                )
                or (
                    effect["kind"] != "land"
                    and effect.get("owned_worktrees") is not None
                )
                or not isinstance(effect.get("started_at"), int)
                or not isinstance(effect.get("updated_at"), int)
                or effect["updated_at"] < effect["started_at"]
                or status in TERMINAL_STATUSES
                or status == "review_pending"
            ):
                raise AutopilotError("candidate_effect_invalid")
            effect_receipt = effect["validation_receipt"]
            if effect["kind"] in {"publish", "land"}:
                validate_validation_receipt(
                    effect_receipt,
                    role=(
                        "maintainer"
                        if effect["kind"] == "publish"
                        else "reviewer"
                    ),
                    expected_head=effect["head_sha"],
                )
            elif effect_receipt is not None:
                raise AutopilotError("candidate_effect_invalid")
            effect_handoff = effect["handoff_receipt"]
            if effect["kind"] in {"commit", "land"}:
                validate_handoff_provenance(effect_handoff)
                expected_action = (
                    "worker_staged"
                    if effect["kind"] == "commit"
                    else "independent_review"
                )
                if (
                    effect_handoff["candidate_id"] != candidate["id"]
                    or effect_handoff["action"] != expected_action
                    or effect_handoff["claim_generation"]
                    != effect["lease_generation"]
                ):
                    raise AutopilotError("candidate_effect_invalid")
                if effect["kind"] == "commit" and (
                    effect_handoff["role"] != "maintainer"
                    or effect_handoff["disposition"] != "staged"
                    or effect_handoff["finding_codes"]
                    or effect_handoff["base_head"] != effect["head_sha"]
                    or effect_handoff["repository_head"] != effect["head_sha"]
                    or effect_handoff["staged_paths_sha256"] is None
                ):
                    raise AutopilotError("candidate_effect_invalid")
                if effect["kind"] == "land":
                    try:
                        validate_reviewer_handoff_semantics(
                            candidate,
                            effect_handoff,
                            disposition="accept",
                            finding_codes=[],
                            receipt=effect_receipt,
                        )
                    except AutopilotError as error:
                        raise AutopilotError("candidate_effect_invalid") from error
            elif effect_handoff is not None:
                raise AutopilotError("candidate_effect_invalid")
            decodex_identity = effect["decodex_identity"]
            if effect["kind"] in {"commit", "land"}:
                if (
                    not has_exact_keys(
                        decodex_identity,
                        {"version", "executable_sha256"},
                    )
                    or not isinstance(decodex_identity.get("version"), str)
                    or not 1 <= len(decodex_identity["version"]) <= 256
                    or "\n" in decodex_identity["version"]
                    or "\r" in decodex_identity["version"]
                    or not is_sha256(
                        decodex_identity.get("executable_sha256")
                    )
                ):
                    raise AutopilotError("candidate_effect_invalid")
            elif decodex_identity is not None:
                raise AutopilotError("candidate_effect_invalid")
            execution_receipt = effect["execution_receipt"]
            command_receipt = effect["command_receipt"]
            if command_receipt is not None:
                validate_land_command_receipt(
                    command_receipt,
                    intent_sha256=effect["intent_sha256"],
                    decodex_identity=decodex_identity,
                    intent_started_at=effect["started_at"],
                    observed_at=effect["updated_at"],
                )
            if execution_receipt is not None:
                validate_land_execution_receipt(
                    execution_receipt,
                    intent_sha256=effect["intent_sha256"],
                    merge_sha=None,
                    decodex_identity=decodex_identity,
                    intent_started_at=effect["started_at"],
                    observed_at=effect["updated_at"],
                )
            if (
                effect["kind"] != "land"
                and (
                    command_receipt is not None
                    or execution_receipt is not None
                )
            ):
                raise AutopilotError("candidate_effect_invalid")
            allowed_effect_phases = {
                "commit": {"prepared"},
                "publish": {"prepared", "pushed", "pr_created"},
                "retire_pr": {"prepared"},
                "land": {
                    "prepared",
                    "land_started",
                    "land_command_completed",
                    "land_completed",
                },
            }
            effect_pull_request = candidate.get("pull_request")
            commit_receipt = candidate.get("commit_receipt")
            allowed_remote_heads: set[str | None] = {None}
            if effect["kind"] == "publish":
                if isinstance(effect_pull_request, dict):
                    allowed_remote_heads = {effect_pull_request["head_sha"]}
                elif isinstance(commit_receipt, dict):
                    allowed_remote_heads.add(commit_receipt["base_head"])
            if (
                effect["phase"] not in allowed_effect_phases[effect["kind"]]
                or (
                    effect["kind"] == "commit"
                    and effect["pr_url"] is not None
                )
                or (
                    effect["kind"] != "publish"
                    and effect["remote_head_before"] is not None
                )
                or effect["remote_head_before"] not in allowed_remote_heads
                or (
                    (effect["phase"] == "land_completed")
                    != (execution_receipt is not None)
                )
                or (
                    (
                        effect["phase"]
                        in {"land_command_completed", "land_completed"}
                    )
                    != (command_receipt is not None)
                )
                or (
                    effect["kind"] in {"retire_pr", "land"}
                    and effect["pr_url"] is None
                )
            ):
                raise AutopilotError("candidate_effect_invalid")
        commit_receipt = candidate.get("commit_receipt")
        if commit_receipt is not None:
            if (
                not has_exact_keys(
                    commit_receipt,
                    {
                        "base_head",
                        "head_sha",
                        "tree_sha",
                        "message_sha256",
                        "intent_sha256",
                        "execution_receipt",
                        "execution_receipt_sha256",
                        "worker_handoff",
                        "committed_at",
                    },
                )
                or SHA_PATTERN.fullmatch(str(commit_receipt.get("base_head", "")))
                is None
                or SHA_PATTERN.fullmatch(str(commit_receipt.get("head_sha", "")))
                is None
                or SHA_PATTERN.fullmatch(str(commit_receipt.get("tree_sha", "")))
                is None
                or commit_receipt["base_head"] == commit_receipt["head_sha"]
                or not is_sha256(commit_receipt.get("message_sha256"))
                or not is_sha256(commit_receipt.get("intent_sha256"))
                or not is_sha256(
                    commit_receipt.get("execution_receipt_sha256")
                )
                or not isinstance(commit_receipt.get("committed_at"), int)
            ):
                raise AutopilotError("candidate_commit_receipt_invalid")
            validate_commit_execution_receipt(
                commit_receipt["execution_receipt"],
                intent_sha256=commit_receipt["intent_sha256"],
                decodex_identity=None,
                observed_at=commit_receipt["committed_at"],
            )
            if (
                sha256_value(commit_receipt["execution_receipt"])
                != commit_receipt["execution_receipt_sha256"]
            ):
                raise AutopilotError("candidate_commit_receipt_invalid")
            validate_handoff_provenance(commit_receipt["worker_handoff"])
            if (
                commit_receipt["worker_handoff"]["candidate_id"]
                != candidate["id"]
                or commit_receipt["worker_handoff"]["role"] != "maintainer"
                or commit_receipt["worker_handoff"]["action"]
                != "worker_staged"
                or commit_receipt["worker_handoff"]["disposition"] != "staged"
                or commit_receipt["worker_handoff"]["finding_codes"]
                or commit_receipt["worker_handoff"]["base_head"]
                != commit_receipt["base_head"]
                or commit_receipt["worker_handoff"]["repository_head"]
                != commit_receipt["base_head"]
                or commit_receipt["worker_handoff"]["repository_tree"]
                != commit_receipt["tree_sha"]
                or commit_receipt["worker_handoff"]["staged_paths_sha256"]
                is None
            ):
                raise AutopilotError("candidate_commit_receipt_invalid")
        pull_request = candidate.get("pull_request")
        if pull_request is not None:
            if (
                not has_exact_keys(
                    pull_request,
                    {
                        "url",
                        "branch",
                        "head_sha",
                        "validation_receipt",
                        "submitted_at",
                    },
                )
                or PR_PATTERN.fullmatch(str(pull_request.get("url", ""))) is None
                or pull_request.get("branch") != candidate["branch_name"]
                or SHA_PATTERN.fullmatch(str(pull_request.get("head_sha", ""))) is None
                or not isinstance(pull_request.get("submitted_at"), int)
            ):
                raise AutopilotError("candidate_pull_request_invalid")
            validate_validation_receipt(
                pull_request["validation_receipt"],
                role="maintainer",
                expected_head=pull_request["head_sha"],
            )
        retired_pull_requests = candidate.get("retired_pull_requests")
        if (
            not isinstance(retired_pull_requests, list)
            or len(retired_pull_requests) > 10
        ):
            raise AutopilotError("candidate_retired_pull_requests_invalid")
        retired_urls: set[str] = set()
        for retired in retired_pull_requests:
            if (
                not has_exact_keys(
                    retired,
                    {
                        "url",
                        "branch",
                        "head_sha",
                        "reason_code",
                        "receipt_sha256",
                        "retired_at",
                    },
                )
                or PR_PATTERN.fullmatch(str(retired.get("url", ""))) is None
                or retired["url"] in retired_urls
                or retired.get("branch") != candidate["branch_name"]
                or SHA_PATTERN.fullmatch(str(retired.get("head_sha", ""))) is None
                or REASON_PATTERN.fullmatch(str(retired.get("reason_code", "")))
                is None
                or not is_sha256(retired.get("receipt_sha256"))
                or not isinstance(retired.get("retired_at"), int)
            ):
                raise AutopilotError("candidate_retired_pull_requests_invalid")
            retired_urls.add(retired["url"])
        decision = candidate.get("decision")
        if decision is not None:
            if (
                not has_exact_keys(
                    decision,
                    {
                        "outcome",
                        "reason_code",
                        "maintainer_receipt",
                        "submitted_at",
                    },
                )
                or decision.get("outcome") not in {"no_change", "rejected"}
                or REASON_PATTERN.fullmatch(str(decision.get("reason_code", ""))) is None
                or not isinstance(decision.get("submitted_at"), int)
            ):
                raise AutopilotError("candidate_decision_invalid")
            validate_validation_receipt(
                decision["maintainer_receipt"],
                role="maintainer",
            )
        if pull_request is not None and decision is not None:
            raise AutopilotError("candidate_evidence_ambiguous")
        if (
            candidate["kind"] == "automation_repair"
            and isinstance(decision, dict)
            and decision.get("outcome") == "rejected"
        ):
            raise AutopilotError("automation_repair_rejected_invalid")
        if status in {"review_pending", "reviewing"} and (
            (pull_request is None) == (decision is None)
        ):
            raise AutopilotError("candidate_review_evidence_invalid")
        validate_candidate_result(candidate)
        result = candidate.get("result")
        stale_refresh = candidate.get("stale_refresh")
        stale_credit = candidate.get("stale_refresh_credit")
        if stale_credit is not None:
            credit_generation = (
                stale_credit.get("generation")
                if isinstance(stale_credit, dict)
                else None
            )
            credit_incremented = (
                stale_credit.get("attempt_incremented")
                if isinstance(stale_credit, dict)
                else None
            )
            lease = candidate.get("lease")
            if (
                not has_exact_keys(
                    stale_credit,
                    {"generation", "attempt_incremented"},
                )
                or (
                    credit_generation is not None
                    and (
                        not isinstance(credit_generation, int)
                        or credit_generation < 1
                    )
                )
                or not isinstance(credit_incremented, bool)
                or (
                    credit_generation is None
                    and credit_incremented
                )
                or not isinstance(result, dict)
                or result.get("outcome") != "repair_requested"
                or result.get("finding_codes") != ["base_stale"]
                or (
                    credit_generation is not None
                    and (
                        not isinstance(lease, dict)
                        or lease.get("role") != "maintainer"
                        or lease.get("generation") != credit_generation
                        or status != "implementing"
                    )
                )
            ):
                raise AutopilotError(
                    "candidate_stale_refresh_credit_invalid"
                )
        if stale_refresh is not None:
            if (
                not has_exact_keys(stale_refresh, STALE_REFRESH_KEYS)
                or any(
                    SHA_PATTERN.fullmatch(
                        str(stale_refresh.get(key, ""))
                    )
                    is None
                    for key in (
                        "old_base_head",
                        "old_head_sha",
                        "target_base_head",
                    )
                )
                or stale_refresh["old_base_head"]
                == stale_refresh["target_base_head"]
                or not isinstance(stale_refresh.get("prepared_at"), int)
                or not isinstance(stale_refresh.get("updated_at"), int)
                or stale_refresh["updated_at"]
                < stale_refresh["prepared_at"]
            ):
                raise AutopilotError("candidate_stale_refresh_invalid")
            if not candidate_has_pre_publish_stale_refresh(candidate):
                if (
                    status
                    not in {
                        "repair_requested",
                        "implementing",
                        "retry_wait",
                        "needs_attention",
                        "repair_pending",
                    }
                    or not isinstance(pull_request, dict)
                    or not isinstance(result, dict)
                    or result.get("outcome") != "repair_requested"
                    or "base_stale"
                    not in result.get("finding_codes", [])
                    or stale_refresh["old_base_head"]
                    != pull_request["validation_receipt"]["base_head"]
                    or stale_refresh["old_head_sha"]
                    != pull_request["head_sha"]
                    or commit_receipt is not None
                    or decision is not None
                ):
                    raise AutopilotError("candidate_stale_refresh_invalid")
            else:
                handoff = candidate.get("handoff")
                agent_run = (
                    handoff.get("agent_run")
                    if isinstance(handoff, dict)
                    else None
                )
                effect = candidate.get("effect")
                pre_publish_commit_effect = bool(
                    isinstance(effect, Mapping)
                    and effect.get("kind") == "commit"
                    and effect.get("phase") == "prepared"
                    and effect.get("head_sha")
                    == stale_refresh["target_base_head"]
                    and isinstance(handoff, Mapping)
                    and isinstance(agent_run, Mapping)
                    and agent_run.get("phase") == "completed"
                    and handoff.get("consumed")
                    == effect.get("handoff_receipt")
                )
                if (
                    status != "implementing"
                    or result["at"] > stale_refresh["prepared_at"]
                    or candidate["attempts"]["maintainer"] < 2
                    or not isinstance(agent_run, Mapping)
                    or pull_request is not None
                    or decision is not None
                    or (
                        effect is not None
                        and not pre_publish_commit_effect
                    )
                    or commit_receipt is not None
                    or stale_credit is not None
                    or agent_run["base_head"]
                    != stale_refresh["target_base_head"]
                    or agent_run["input_head"]
                    != stale_refresh["target_base_head"]
                    or agent_run["repository_head"]
                    != stale_refresh["target_base_head"]
                ):
                    raise AutopilotError("candidate_stale_refresh_invalid")
        if isinstance(result, dict) and result.get("outcome") == "repair_requested":
            codes = result["finding_codes"]
            disposition = result["reviewer_handoff"]["disposition"]
            if not (
                disposition == "request_repair"
                or (disposition == "accept" and codes == ["base_stale"])
            ):
                raise AutopilotError("candidate_result_invalid")
            expected_codes = [] if disposition == "accept" else codes
            try:
                validate_reviewer_handoff_semantics(
                    candidate,
                    result["reviewer_handoff"],
                    disposition=disposition,
                    finding_codes=expected_codes,
                )
            except AutopilotError as error:
                raise AutopilotError("candidate_result_invalid") from error
        if status in TERMINAL_STATUSES:
            if not isinstance(candidate["result"], dict):
                raise AutopilotError("candidate_result_invalid")
            validate_validation_receipt(
                candidate["result"]["reviewer_receipt"],
                role="reviewer",
            )
            try:
                validate_reviewer_handoff_semantics(
                    candidate,
                    candidate["result"]["reviewer_handoff"],
                    disposition=("accept" if status == "landed" else status),
                    finding_codes=[],
                    receipt=candidate["result"]["reviewer_receipt"],
                )
            except AutopilotError as error:
                raise AutopilotError("candidate_result_invalid") from error
            if status == "landed":
                if not isinstance(pull_request, dict):
                    raise AutopilotError("candidate_terminal_evidence_invalid")
                validate_land_execution_receipt(
                    candidate["result"]["land_execution_receipt"],
                    intent_sha256=candidate["result"][
                        "land_intent_sha256"
                    ],
                    merge_sha=candidate["result"]["merge_sha"],
                    observed_at=candidate["result"]["resolved_at"],
                )
                validate_validation_receipt(
                    pull_request["validation_receipt"],
                    role="maintainer",
                    expected_head=pull_request["head_sha"],
                )
                if (
                    candidate["result"]["reviewer_receipt"]["repository_head"]
                    != pull_request["head_sha"]
                    or candidate["result"]["reviewer_receipt"]["repository_tree"]
                    != pull_request["validation_receipt"]["repository_tree"]
                    or candidate["result"]["reviewer_receipt"]["base_head"]
                    != pull_request["validation_receipt"]["base_head"]
                ):
                    raise AutopilotError("candidate_review_receipt_mismatch")
            else:
                if not isinstance(decision, dict):
                    raise AutopilotError("candidate_terminal_evidence_invalid")
                maintainer_receipt = decision["maintainer_receipt"]
                reviewer_receipt = candidate["result"]["reviewer_receipt"]
                if (
                    candidate["result"]["decision_receipt_sha256"]
                    != sha256_value(decision)
                    or reviewer_receipt["repository_head"]
                    != maintainer_receipt["repository_head"]
                    or reviewer_receipt["repository_tree"]
                    != maintainer_receipt["repository_tree"]
                    or reviewer_receipt["base_head"]
                    != maintainer_receipt["base_head"]
                ):
                    raise AutopilotError("candidate_review_receipt_mismatch")
        if status in TERMINAL_STATUSES and (
            not isinstance(candidate["result"], dict)
            or candidate["result"].get("outcome") != status
        ):
            raise AutopilotError("candidate_result_invalid")
        if status == "landed" and (
            pull_request is None or decision is not None
        ):
            raise AutopilotError("candidate_terminal_evidence_invalid")
        if status in {"no_change", "rejected"} and (
            pull_request is not None
            or not isinstance(decision, dict)
            or decision.get("outcome") != status
        ):
            raise AutopilotError("candidate_terminal_evidence_invalid")
        if status == "repair_requested" and (
            not isinstance(candidate["result"], dict)
            or candidate["result"].get("outcome") != "repair_requested"
        ):
            raise AutopilotError("candidate_result_invalid")
        if status in {"retry_wait", "needs_attention", "repair_pending"}:
            retry_result = candidate.get("result")
            preserves_stale_repair = bool(
                isinstance(retry_result, dict)
                and retry_result.get("outcome") == "repair_requested"
                and retry_result.get("finding_codes") == ["base_stale"]
                and isinstance(candidate.get("stale_refresh"), dict)
            )
            if (
                not isinstance(retry_result, dict)
                or retry_result.get("outcome") != "blocked"
            ) and not preserves_stale_repair:
                raise AutopilotError("candidate_result_invalid")
    candidates_by_id = {candidate["id"]: candidate for candidate in candidates}
    active_repairs_by_target: dict[str, list[dict[str, Any]]] = {}
    for candidate in candidates:
        repair_of = candidate.get("repair_of")
        if repair_of is None:
            continue
        if repair_of == candidate["id"] or repair_of not in candidates_by_id:
            raise AutopilotError("candidate_repair_target_invalid")
        if candidate["status"] not in TERMINAL_STATUSES:
            active_repairs_by_target.setdefault(repair_of, []).append(candidate)
    for candidate in candidates:
        visited: set[str] = set()
        current = candidate
        while current.get("repair_of") is not None:
            if current["id"] in visited:
                raise AutopilotError("candidate_repair_cycle_invalid")
            visited.add(current["id"])
            current = candidates_by_id[current["repair_of"]]
    for candidate in candidates:
        active_owners = active_repairs_by_target.get(candidate["id"], [])
        if candidate["status"] == "repair_pending":
            if len(active_owners) != 1:
                raise AutopilotError("candidate_repair_ownership_invalid")
        elif active_owners:
            raise AutopilotError("candidate_repair_ownership_invalid")
    source_candidates = {
        candidate["source_sequence"]: candidate
        for candidate in candidates
        if candidate.get("source_sequence") is not None
    }
    cursor_sequence = source["cursor_sequence"]
    if any(
        sequence <= cursor_sequence
        and candidate["status"] not in TERMINAL_STATUSES
        for sequence, candidate in source_candidates.items()
    ):
        raise AutopilotError("state_source_continuity_invalid")
    expected_sequences = set(range(cursor_sequence + 1, source["next_sequence"]))
    actual_sequences = {
        sequence
        for sequence in source_candidates
        if sequence > cursor_sequence
    }
    if actual_sequences != expected_sequences:
        raise AutopilotError("state_source_continuity_invalid")
    previous_sha = source["cursor_sha"]
    for sequence in sorted(expected_sequences):
        candidate = source_candidates[sequence]
        if candidate["from_sha"] != previous_sha:
            raise AutopilotError("state_source_continuity_invalid")
        previous_sha = candidate["to_sha"]
    if source["queued_head_sha"] != previous_sha:
        raise AutopilotError("state_source_continuity_invalid")
    for event in events:
        if not isinstance(event, dict) or not {
            "event",
            "at",
        }.issubset(event) or not set(event).issubset(
            {"event", "at", "candidate_id", "reason_code"}
        ):
            raise AutopilotError("state_event_invalid")
        if (
            REASON_PATTERN.fullmatch(str(event.get("event", ""))) is None
            or not isinstance(event.get("at"), int)
            or (
                event.get("candidate_id") is not None
                and re.fullmatch(
                    r"[0-9a-f]{16}",
                    str(event["candidate_id"]),
                )
                is None
            )
            or (
                event.get("reason_code") is not None
                and REASON_PATTERN.fullmatch(str(event["reason_code"])) is None
            )
        ):
            raise AutopilotError("state_event_invalid")
    previous_bucket: int | None = None
    for bucket in metrics["buckets"]:
        if (
            not has_exact_keys(
                bucket,
                {
                    "start",
                    "events",
                    "outcomes",
                    "lead_time_seconds_total",
                    "lead_time_count",
                },
            )
            or not isinstance(bucket.get("start"), int)
            or bucket["start"] % METRIC_BUCKET_SECONDS != 0
            or (
                previous_bucket is not None
                and bucket["start"] <= previous_bucket
            )
            or not has_exact_keys(
                bucket.get("events"),
                {
                    "candidate_blocked",
                    "repair_requested",
                    "automation_repair_queued",
                    "automation_improvement_queued",
                },
            )
            or not has_exact_keys(
                bucket.get("outcomes"),
                {"landed", "no_change", "rejected"},
            )
            or any(
                not isinstance(value, int) or value < 0
                for value in [
                    *bucket["events"].values(),
                    *bucket["outcomes"].values(),
                    bucket.get("lead_time_seconds_total"),
                    bucket.get("lead_time_count"),
                ]
            )
        ):
            raise AutopilotError("state_metrics_invalid")
        previous_bucket = bucket["start"]


def metric_bucket(state: dict[str, Any], now: int) -> dict[str, Any]:
    start = now - (now % METRIC_BUCKET_SECONDS)
    buckets = state["metrics"]["buckets"]
    for existing in buckets:
        if existing["start"] == start:
            return existing
    bucket = {
        "start": start,
        "events": {
            "candidate_blocked": 0,
            "repair_requested": 0,
            "automation_repair_queued": 0,
            "automation_improvement_queued": 0,
        },
        "outcomes": {"landed": 0, "no_change": 0, "rejected": 0},
        "lead_time_seconds_total": 0,
        "lead_time_count": 0,
    }
    buckets.append(bucket)
    buckets.sort(key=lambda value: value["start"])
    state["metrics"]["buckets"] = buckets[-MAX_METRIC_BUCKETS:]
    if bucket not in state["metrics"]["buckets"]:
        raise AutopilotError("state_metrics_outside_retention")
    return bucket


def append_event(
    state: dict[str, Any],
    event: str,
    now: int,
    *,
    candidate_id: str | None = None,
    reason_code: str | None = None,
) -> None:
    record: dict[str, Any] = {"event": event, "at": now}
    if candidate_id is not None:
        record["candidate_id"] = candidate_id
    if reason_code is not None:
        if not REASON_PATTERN.fullmatch(reason_code):
            raise AutopilotError("reason_code_invalid")
        record["reason_code"] = reason_code
    state["events"].append(record)
    state["events"] = state["events"][-MAX_EVENTS:]
    if event in {
        "candidate_blocked",
        "repair_requested",
        "automation_repair_queued",
        "automation_improvement_queued",
    }:
        metric_bucket(state, now)["events"][event] += 1


def record_terminal_metrics(
    state: dict[str, Any],
    *,
    outcome: str,
    lead_time_seconds: int,
    now: int,
) -> None:
    if outcome not in TERMINAL_STATUSES or lead_time_seconds < 0:
        raise AutopilotError("state_metrics_invalid")
    bucket = metric_bucket(state, now)
    bucket["outcomes"][outcome] += 1
    bucket["lead_time_seconds_total"] += lead_time_seconds
    bucket["lead_time_count"] += 1


def save_state(state: dict[str, Any], path: Path, now: int) -> None:
    state["updated_at"] = now
    state["persistence_generation"] += 1
    prune_state(state)
    validate_state(state)
    atomic_write_json(state_recovery_path(path), state)
    atomic_write_json(path, state)


def prune_state(state: dict[str, Any]) -> None:
    state["metrics"]["buckets"] = state["metrics"]["buckets"][-MAX_METRIC_BUCKETS:]
    candidates = state["candidates"]
    if len(candidates) > MAX_STATE_CANDIDATES:
        candidates_by_id = {
            candidate["id"]: candidate for candidate in candidates
        }
        adjacent: dict[str, set[str]] = {
            candidate["id"]: set() for candidate in candidates
        }
        for candidate in candidates:
            repair_of = candidate.get("repair_of")
            if repair_of is None:
                continue
            if (
                repair_of == candidate["id"]
                or repair_of not in candidates_by_id
            ):
                raise AutopilotError("candidate_repair_target_invalid")
            adjacent[candidate["id"]].add(repair_of)
            adjacent[repair_of].add(candidate["id"])
        removable_components: list[list[dict[str, Any]]] = []
        visited: set[str] = set()
        for candidate in candidates:
            if candidate["id"] in visited:
                continue
            component_ids: list[str] = []
            pending = [candidate["id"]]
            while pending:
                identifier = pending.pop()
                if identifier in visited:
                    continue
                visited.add(identifier)
                component_ids.append(identifier)
                pending.extend(sorted(adjacent[identifier], reverse=True))
            component = [candidates_by_id[value] for value in component_ids]
            if all(
                value["status"] in TERMINAL_STATUSES
                and (
                    value.get("source_sequence") is None
                    or value["source_sequence"]
                    <= state["source"]["cursor_sequence"]
                )
                for value in component
            ):
                removable_components.append(component)
        remove_count = len(candidates) - MAX_STATE_CANDIDATES
        remove_ids: set[str] = set()
        for component in removable_components:
            remove_ids.update(candidate["id"] for candidate in component)
            if len(remove_ids) >= remove_count:
                break
        state["candidates"] = [
            candidate for candidate in candidates if candidate["id"] not in remove_ids
        ]
    if len(state["candidates"]) > MAX_STATE_CANDIDATES:
        raise AutopilotError("state_candidate_capacity")
    prune_terminal_schema_evidence(state)


def prune_terminal_schema_evidence(state: dict[str, Any]) -> None:
    reserve = 2
    limit = min(
        MAX_SCHEMA_EVIDENCE_FILES - reserve,
        (
            MAX_SCHEMA_EVIDENCE_BYTES - reserve * MAX_SCHEMA_BYTES
        )
        // MAX_SCHEMA_BYTES,
    )
    if limit < 0:
        raise AutopilotError("schema_evidence_reserve_invalid")
    protected: set[str] = set()
    local_build = state.get("local_build")
    if isinstance(local_build, dict):
        protected.update(
            value
            for value in (
                local_build["stable_schema_evidence_sha256"],
                local_build["experimental_schema_evidence_sha256"],
            )
            if is_sha256(value)
        )
    terminal: list[dict[str, Any]] = []
    all_references = set(protected)
    for candidate in state["candidates"]:
        evidence = candidate["schema_evidence"]
        values = {value for value in evidence.values() if is_sha256(value)}
        all_references.update(values)
        if candidate["status"] in TERMINAL_STATUSES:
            terminal.append(candidate)
        else:
            protected.update(values)
    if len(protected) > limit:
        raise AutopilotError("active_schema_evidence_capacity")
    if len(all_references) <= limit:
        return
    terminal.sort(
        key=lambda candidate: (
            candidate["updated_at"],
            candidate["created_at"],
            candidate["id"],
        )
    )
    for candidate in terminal:
        candidate["schema_evidence"] = {
            "stable": None,
            "experimental": None,
        }
        retained = set(protected)
        for value in state["candidates"]:
            retained.update(
                evidence
                for evidence in value["schema_evidence"].values()
                if is_sha256(evidence)
            )
        if len(retained) <= limit:
            return
    raise AutopilotError("schema_evidence_reference_capacity")


def candidate_identity(kind: str, fields: dict[str, Any]) -> str:
    return hashlib.sha256(canonical_json({"kind": kind, **fields})).hexdigest()[:16]


def allocate_discovery_sequence(state: dict[str, Any]) -> int:
    sequence = state["source"]["next_discovery_sequence"]
    state["source"]["next_discovery_sequence"] += 1
    return sequence


def queue_candidate(
    state: dict[str, Any],
    policy: dict[str, Any],
    *,
    kind: str,
    now: int,
    source_sequence: int | None,
    from_sha: str | None,
    to_sha: str | None,
    observation: Observation,
    release_tag: str | None = None,
    path_summary: dict[str, Any] | None = None,
) -> dict[str, Any]:
    discovery_sequence = allocate_discovery_sequence(state)
    identity_fields = {
        "discovery_sequence": discovery_sequence,
        "from_sha": from_sha,
        "to_sha": to_sha,
        "release_tag": release_tag,
        "codex_version": observation.codex_version,
        "stable_schema_fingerprint": observation.stable_schema_fingerprint,
        "experimental_schema_fingerprint": observation.experimental_schema_fingerprint,
        "upstream_main_schema_fingerprint": observation.upstream_main_schema_fingerprint,
        "stable_release_schema_fingerprint": observation.stable_release_schema_fingerprint,
        "prerelease_schema_fingerprint": observation.prerelease_schema_fingerprint,
    }
    identifier = candidate_identity(kind, identity_fields)
    contract_missing = observation.contract_missing_for(kind)
    candidate = {
        "id": identifier,
        "discovery_sequence": discovery_sequence,
        "kind": kind,
        "status": "queued",
        "priority": "critical" if contract_missing else "normal",
        "source_sequence": source_sequence,
        "from_sha": from_sha,
        "to_sha": to_sha,
        "release_tag": release_tag,
        "codex_version": observation.codex_version,
        "codex_executable_sha256": observation.codex_executable_sha256,
        "policy_fingerprint": observation.policy_fingerprint,
        "accepted_marker_fingerprint": observation.accepted_marker_fingerprint,
        "schema_fingerprints": {
            "stable": observation.stable_schema_fingerprint,
            "experimental": observation.experimental_schema_fingerprint,
            "upstream_main": observation.upstream_main_schema_fingerprint,
            "upstream_stable_release": observation.stable_release_schema_fingerprint,
            "upstream_prerelease": observation.prerelease_schema_fingerprint,
        },
        "schema_evidence": {
            "stable": observation.stable_schema_evidence_sha256,
            "experimental": observation.experimental_schema_evidence_sha256,
        },
        "contract_missing": contract_missing,
        "path_summary": path_summary,
        "repair_of": None,
        "branch_name": f"{policy['branch_prefix']}{identifier}",
        "attempts": {"maintainer": 0, "reviewer": 0},
        "created_at": now,
        "updated_at": now,
        "next_retry_at": None,
        "retry_role": None,
        "lease": None,
        "handoff": None,
        "effect": None,
        "commit_receipt": None,
        "stale_refresh": None,
        "stale_refresh_credit": None,
        "pull_request": None,
        "retired_pull_requests": [],
        "decision": None,
        "result": None,
    }
    state["candidates"].append(candidate)
    append_event(state, "candidate_queued", now, candidate_id=identifier)
    return candidate


def queue_automation_repair(
    state: dict[str, Any],
    policy: dict[str, Any],
    *,
    blocked_candidate_id: str,
    reason_code: str,
    repository_head: str,
    now: int,
) -> dict[str, Any]:
    if REASON_PATTERN.fullmatch(reason_code) is None:
        raise AutopilotError("reason_code_invalid")
    if not SHA_PATTERN.fullmatch(repository_head):
        raise AutopilotError("head_invalid")
    blocked = find_candidate(state, blocked_candidate_id)
    if blocked["status"] != "needs_attention":
        raise AutopilotError("repair_target_not_needs_attention")
    if has_unresolved_external_effect(blocked):
        raise AutopilotError("repair_target_external_effect_unresolved")
    existing = next(
        (
            candidate
            for candidate in state["candidates"]
            if candidate.get("repair_of") == blocked_candidate_id
            and candidate["status"] not in TERMINAL_STATUSES
        ),
        None,
    )
    if existing is not None:
        blocked["status"] = "repair_pending"
        blocked["updated_at"] = now
        return existing
    evidence_sha256 = sha256_value(
        {
            "blocked_candidate_id": blocked_candidate_id,
            "reason_code": reason_code,
            "repository_head": repository_head,
            "attempts": blocked["attempts"],
        }
    )
    discovery_sequence = allocate_discovery_sequence(state)
    identifier = candidate_identity(
        "automation_repair",
        {
            "discovery_sequence": discovery_sequence,
            "evidence_sha256": evidence_sha256,
        },
    )
    candidate = {
        "id": identifier,
        "discovery_sequence": discovery_sequence,
        "kind": "automation_repair",
        "status": "queued",
        "priority": "critical",
        "source_sequence": None,
        "from_sha": blocked.get("from_sha"),
        "to_sha": blocked.get("to_sha"),
        "release_tag": blocked.get("release_tag"),
        "codex_version": blocked["codex_version"],
        "codex_executable_sha256": blocked["codex_executable_sha256"],
        "policy_fingerprint": blocked["policy_fingerprint"],
        "accepted_marker_fingerprint": blocked["accepted_marker_fingerprint"],
        "schema_fingerprints": deepcopy(blocked["schema_fingerprints"]),
        "schema_evidence": deepcopy(blocked["schema_evidence"]),
        "contract_missing": [],
        "path_summary": {
            "repair_of": blocked_candidate_id,
            "reason_code": reason_code,
            "evidence_sha256": evidence_sha256,
        },
        "repair_of": blocked_candidate_id,
        "branch_name": f"{policy['branch_prefix']}{identifier}",
        "attempts": {"maintainer": 0, "reviewer": 0},
        "created_at": now,
        "updated_at": now,
        "next_retry_at": None,
        "retry_role": None,
        "lease": None,
        "handoff": None,
        "effect": None,
        "commit_receipt": None,
        "stale_refresh": None,
        "stale_refresh_credit": None,
        "pull_request": None,
        "retired_pull_requests": [],
        "decision": None,
        "result": None,
    }
    state["candidates"].append(candidate)
    append_event(
        state,
        "automation_repair_queued",
        now,
        candidate_id=identifier,
        reason_code=reason_code,
    )
    blocked["status"] = "repair_pending"
    blocked["updated_at"] = now
    return candidate


def queue_automation_improvement(
    state: dict[str, Any],
    policy: dict[str, Any],
    *,
    reason_code: str,
    repository_head: str,
    now: int,
    degradation_codes: Sequence[str] = (),
    pricing_audit: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    if reason_code not in PROACTIVE_IMPROVEMENT_REASON_CODES:
        raise AutopilotError("improvement_reason_invalid")
    if not SHA_PATTERN.fullmatch(repository_head):
        raise AutopilotError("head_invalid")
    normalized_degradation_codes = tuple(sorted(degradation_codes))
    if (
        len(normalized_degradation_codes) != len(set(normalized_degradation_codes))
        or any(
            code not in CONTENT_DEGRADATION_CODES
            for code in normalized_degradation_codes
        )
    ):
        raise AutopilotError("content_degradation_evidence_invalid")
    if reason_code == "content_loop_degraded":
        if not normalized_degradation_codes:
            raise AutopilotError("content_degradation_evidence_missing")
    elif normalized_degradation_codes:
        raise AutopilotError("content_degradation_evidence_not_applicable")
    normalized_pricing_audit = (
        deepcopy(dict(pricing_audit))
        if pricing_audit is not None
        else None
    )
    if reason_code == "x_pricing_contract_drift":
        if normalized_pricing_audit is None:
            raise AutopilotError("x_pricing_audit_evidence_missing")
        validate_x_pricing_audit_evidence(normalized_pricing_audit)
    elif normalized_pricing_audit is not None:
        raise AutopilotError("x_pricing_audit_evidence_not_applicable")
    active_candidates = [
        candidate
        for candidate in state["candidates"]
        if candidate["kind"] == "automation_repair"
        and candidate.get("repair_of") is None
        and candidate.get("path_summary", {}).get("reason_code") == reason_code
        and candidate["status"] not in TERMINAL_STATUSES
    ]
    merge_target = None
    require_successor = False
    if active_candidates:
        if reason_code not in {
            "content_loop_degraded",
            "x_pricing_contract_drift",
        }:
            return active_candidates[0]
        merge_target = next(
            (
                candidate
                for candidate in active_candidates
                if candidate["status"] == "queued"
            ),
            None,
        )
        if merge_target is None:
            require_successor = True
        elif reason_code == "content_loop_degraded":
            existing_codes = tuple(
                merge_target.get("path_summary", {}).get(
                    "degradation_codes", []
                )
            )
            merged_codes = tuple(
                sorted(set(existing_codes) | set(normalized_degradation_codes))
            )
            if merged_codes == existing_codes:
                return merge_target
            normalized_degradation_codes = merged_codes
    local_build = state.get("local_build")
    source = state["source"]
    source_fingerprints = source["schema_fingerprints"]
    if (
        not isinstance(local_build, dict)
        or source["observed_head_sha"] is None
        or source_fingerprints["upstream_main"] is None
    ):
        raise AutopilotError("improvement_evidence_missing")
    evidence_sha256 = sha256_value(
        {
            "reason_code": reason_code,
            "degradation_codes": normalized_degradation_codes,
            **(
                {"pricing_audit": normalized_pricing_audit}
                if reason_code == "x_pricing_contract_drift"
                else {}
            ),
            "repository_head": repository_head,
            "codex_version": local_build["codex_version"],
            "codex_executable_sha256": local_build["codex_executable_sha256"],
            "policy_fingerprint": local_build["policy_fingerprint"],
            "accepted_marker_fingerprint": local_build[
                "accepted_marker_fingerprint"
            ],
            "stable_schema_fingerprint": local_build[
                "stable_schema_fingerprint"
            ],
            "experimental_schema_fingerprint": local_build[
                "experimental_schema_fingerprint"
            ],
            "trigger_bucket": now // 21600,
            "metrics_sha256": sha256_value(state["metrics"]["buckets"]),
        }
    )
    if merge_target is not None:
        if reason_code == "content_loop_degraded":
            merge_target["path_summary"]["degradation_codes"] = list(
                normalized_degradation_codes
            )
        else:
            if (
                merge_target["path_summary"].get("pricing_audit")
                == normalized_pricing_audit
            ):
                return merge_target
            merge_target["path_summary"][
                "pricing_audit"
            ] = normalized_pricing_audit
        merge_target["path_summary"]["evidence_sha256"] = evidence_sha256
        merge_target["updated_at"] = now
        append_event(
            state,
            "automation_improvement_evidence_extended",
            now,
            candidate_id=merge_target["id"],
            reason_code=reason_code,
        )
        return merge_target
    same_evidence = next(
        (
            candidate
            for candidate in state["candidates"]
            if candidate["kind"] == "automation_repair"
            and candidate.get("repair_of") is None
            and candidate.get("path_summary", {}).get("evidence_sha256")
            == evidence_sha256
        ),
        None,
    )
    if same_evidence is not None and not require_successor:
        return same_evidence
    discovery_sequence = allocate_discovery_sequence(state)
    identifier = candidate_identity(
        "automation_repair",
        {
            "discovery_sequence": discovery_sequence,
            "evidence_sha256": evidence_sha256,
        },
    )
    candidate = {
        "id": identifier,
        "discovery_sequence": discovery_sequence,
        "kind": "automation_repair",
        "status": "queued",
        "priority": (
            "critical"
            if reason_code
            in {
                "live_configuration_drift",
                "task_retention_contract_drift",
                "x_pricing_contract_drift",
            }
            else "normal"
        ),
        "source_sequence": None,
        "from_sha": None,
        "to_sha": source["observed_head_sha"],
        "release_tag": None,
        "codex_version": local_build["codex_version"],
        "codex_executable_sha256": local_build["codex_executable_sha256"],
        "policy_fingerprint": local_build["policy_fingerprint"],
        "accepted_marker_fingerprint": local_build[
            "accepted_marker_fingerprint"
        ],
        "schema_fingerprints": {
            "stable": local_build["stable_schema_fingerprint"],
            "experimental": local_build["experimental_schema_fingerprint"],
            "upstream_main": source_fingerprints["upstream_main"],
            "upstream_stable_release": source_fingerprints["stable_release"],
            "upstream_prerelease": source_fingerprints["prerelease"],
        },
        "schema_evidence": {
            "stable": local_build["stable_schema_evidence_sha256"],
            "experimental": local_build[
                "experimental_schema_evidence_sha256"
            ],
        },
        "contract_missing": [],
        "path_summary": {
            "repair_of": None,
            "reason_code": reason_code,
            "evidence_sha256": evidence_sha256,
            **(
                {"degradation_codes": list(normalized_degradation_codes)}
                if reason_code == "content_loop_degraded"
                else {}
            ),
            **(
                {"pricing_audit": normalized_pricing_audit}
                if reason_code == "x_pricing_contract_drift"
                else {}
            ),
        },
        "repair_of": None,
        "branch_name": f"{policy['branch_prefix']}{identifier}",
        "attempts": {"maintainer": 0, "reviewer": 0},
        "created_at": now,
        "updated_at": now,
        "next_retry_at": None,
        "retry_role": None,
        "lease": None,
        "handoff": None,
        "effect": None,
        "commit_receipt": None,
        "stale_refresh": None,
        "stale_refresh_credit": None,
        "pull_request": None,
        "retired_pull_requests": [],
        "decision": None,
        "result": None,
    }
    state["candidates"].append(candidate)
    append_event(
        state,
        "automation_improvement_queued",
        now,
        candidate_id=identifier,
        reason_code=reason_code,
    )
    return candidate


def begin_observation(state: dict[str, Any], now: int) -> int:
    validate_state(state)
    source = state["source"]
    generation = source["observation_started_generation"] + 1
    source["observation_started_generation"] = generation
    append_event(state, "observation_started", now)
    return generation


def apply_observation(
    state: dict[str, Any],
    policy: dict[str, Any],
    observation: Observation,
    *,
    now: int,
    observation_generation: int,
    commits: Sequence[str],
    reference_observations: dict[str, Observation],
    path_summaries: dict[str, dict[str, Any]],
) -> list[str]:
    validate_state(state)
    source = state["source"]
    if (
        observation_generation != source["observation_started_generation"]
        or observation_generation <= source["observation_applied_generation"]
    ):
        raise AutopilotError("observation_generation_stale")
    queued: list[str] = []
    previous_head = source["observed_head_sha"]
    queued_head = source["queued_head_sha"]
    if queued_head is None:
        sequence = source["next_sequence"]
        candidate = queue_candidate(
            state,
            policy,
            kind="bootstrap",
            now=now,
            source_sequence=sequence,
            from_sha=None,
            to_sha=observation.upstream_head_sha,
            observation=observation,
        )
        source["next_sequence"] += 1
        source["queued_head_sha"] = observation.upstream_head_sha
        queued.append(candidate["id"])
    elif queued_head != observation.upstream_head_sha:
        if not commits or commits[-1] != observation.upstream_head_sha:
            raise AutopilotError("observation_plan_incomplete")
        batch_size = int(policy["max_batch_commits"])
        active_source_candidates = sum(
            candidate.get("source_sequence") is not None
            and candidate["status"] not in TERMINAL_STATUSES
            for candidate in state["candidates"]
        )
        available_batches = max(
            0,
            MAX_ACTIVE_SOURCE_CANDIDATES - active_source_candidates,
        )
        lower = queued_head
        for offset in range(
            0,
            min(len(commits), available_batches * batch_size),
            batch_size,
        ):
            upper = commits[min(offset + batch_size, len(commits)) - 1]
            sequence = source["next_sequence"]
            batch_observation = reference_observations.get(upper)
            if batch_observation is None:
                raise AutopilotError("observation_plan_incomplete")
            summary_key = f"{lower}:{upper}"
            summary = path_summaries.get(summary_key)
            if not isinstance(summary, dict):
                raise AutopilotError("observation_plan_incomplete")
            candidate = queue_candidate(
                state,
                policy,
                kind="upstream_range",
                now=now,
                source_sequence=sequence,
                from_sha=lower,
                to_sha=upper,
                observation=batch_observation,
                path_summary=summary,
            )
            source["next_sequence"] += 1
            queued.append(candidate["id"])
            lower = upper
        source["queued_head_sha"] = lower

    if (
        observation.stable_tag is not None
        and (
            previous_head is None
            or source["stable_tag"] != observation.stable_tag
            or source["stable_tag_sha"] != observation.stable_tag_sha
            or source["schema_fingerprints"]["stable_release"]
            != observation.stable_release_schema_fingerprint
        )
    ):
        candidate = queue_candidate(
            state,
            policy,
            kind="stable_release",
            now=now,
            source_sequence=None,
            from_sha=None,
            to_sha=observation.stable_tag_sha,
            observation=observation,
            release_tag=observation.stable_tag,
        )
        queued.append(candidate["id"])
    if (
        observation.prerelease_tag is not None
        and (
            previous_head is None
            or source["prerelease_tag"] != observation.prerelease_tag
            or source["prerelease_tag_sha"] != observation.prerelease_tag_sha
            or source["schema_fingerprints"]["prerelease"]
            != observation.prerelease_schema_fingerprint
        )
    ):
        candidate = queue_candidate(
            state,
            policy,
            kind="prerelease_release",
            now=now,
            source_sequence=None,
            from_sha=None,
            to_sha=observation.prerelease_tag_sha,
            observation=observation,
            release_tag=observation.prerelease_tag,
        )
        queued.append(candidate["id"])

    previous_build = state.get("local_build")
    build_changed = previous_build is not None and (
        previous_build.get("codex_version") != observation.codex_version
        or previous_build.get("codex_executable_sha256")
        != observation.codex_executable_sha256
        or previous_build.get("policy_fingerprint")
        != observation.policy_fingerprint
        or previous_build.get("accepted_marker_fingerprint")
        != observation.accepted_marker_fingerprint
        or previous_build.get("stable_schema_fingerprint")
        != observation.stable_schema_fingerprint
        or previous_build.get("experimental_schema_fingerprint")
        != observation.experimental_schema_fingerprint
    )
    if build_changed:
        candidate = queue_candidate(
            state,
            policy,
            kind="local_build",
            now=now,
            source_sequence=None,
            from_sha=None,
            to_sha=observation.upstream_head_sha,
            observation=observation,
        )
        queued.append(candidate["id"])

    source["observed_head_sha"] = observation.upstream_head_sha
    source["stable_tag"] = observation.stable_tag
    source["stable_tag_sha"] = observation.stable_tag_sha
    source["prerelease_tag"] = observation.prerelease_tag
    source["prerelease_tag_sha"] = observation.prerelease_tag_sha
    source["schema_fingerprints"] = {
        "upstream_main": observation.upstream_main_schema_fingerprint,
        "stable_release": observation.stable_release_schema_fingerprint,
        "prerelease": observation.prerelease_schema_fingerprint,
    }
    state["local_build"] = {
        "codex_version": observation.codex_version,
        "codex_executable_sha256": observation.codex_executable_sha256,
        "policy_fingerprint": observation.policy_fingerprint,
        "accepted_marker_fingerprint": observation.accepted_marker_fingerprint,
        "stable_schema_fingerprint": observation.stable_schema_fingerprint,
        "experimental_schema_fingerprint": observation.experimental_schema_fingerprint,
        "stable_schema_evidence_sha256": (
            observation.stable_schema_evidence_sha256
        ),
        "experimental_schema_evidence_sha256": (
            observation.experimental_schema_evidence_sha256
        ),
        "contract_missing": observation.contract_missing,
        "observed_at": now,
    }
    state["last_observed_at"] = now
    source["observation_applied_generation"] = observation_generation
    append_event(state, "observation_completed", now)
    return queued


def find_candidate(state: dict[str, Any], candidate_id: str) -> dict[str, Any]:
    candidate = next(
        (value for value in state["candidates"] if value["id"] == candidate_id),
        None,
    )
    if candidate is None:
        raise AutopilotError("candidate_not_found")
    return candidate


def has_unresolved_external_effect(candidate: dict[str, Any]) -> bool:
    effect = candidate.get("effect")
    return isinstance(effect, dict) and effect.get("kind") in {
        "publish",
        "retire_pr",
        "land",
    }


def preserved_finding_codes(candidate: Mapping[str, Any]) -> list[str]:
    result = candidate.get("result")
    finding_codes = (
        result.get("finding_codes")
        if isinstance(result, Mapping)
        else None
    )
    return list(finding_codes) if isinstance(finding_codes, list) else []


def external_effect_recovery_role(candidate: Mapping[str, Any]) -> str | None:
    effect = candidate.get("effect")
    if not isinstance(effect, Mapping):
        return None
    if effect.get("kind") == "land":
        return "reviewer"
    if effect.get("kind") in {"publish", "retire_pr"}:
        return "maintainer"
    return None


def lease_matches(candidate: dict[str, Any], role: str, token: str, now: int) -> None:
    lease = candidate.get("lease")
    if lease is None or lease.get("role") != role:
        raise AutopilotError("lease_missing")
    if lease["expires_at"] <= now:
        raise AutopilotError("lease_expired")
    actual = hashlib.sha256(token.encode("utf-8")).hexdigest()
    if not hmac.compare_digest(actual, lease["token_sha256"]):
        raise AutopilotError("lease_token_invalid")


def prepare_agent_run(
    state: dict[str, Any],
    *,
    candidate_id: str,
    role: str,
    token: str,
    challenge_sha256: str,
    base_head: str,
    repository_head: str,
    input_tree: str,
    now: int,
    input_head: str | None = None,
) -> dict[str, Any]:
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, role, token, now)
    handoff = candidate.get("handoff")
    resolved_input_head = (
        repository_head if input_head is None else input_head
    )
    if (
        not isinstance(handoff, dict)
        or handoff.get("role") != role
        or handoff.get("generation") != candidate["lease"]["generation"]
        or not hmac.compare_digest(
            str(handoff.get("challenge_sha256", "")),
            challenge_sha256,
        )
        or any(
            SHA_PATTERN.fullmatch(value) is None
            for value in (
                base_head,
                resolved_input_head,
                repository_head,
                input_tree,
            )
        )
    ):
        raise AutopilotError("agent_run_context_invalid")
    intended = {
        "phase": "prepared",
        "role": role,
        "generation": candidate["lease"]["generation"],
        "base_head": base_head,
        "input_head": resolved_input_head,
        "repository_head": repository_head,
        "input_tree": input_tree,
        "repository_tree": None,
        "challenge_sha256": challenge_sha256,
        "started_at": now,
        "completed_at": None,
        "receipt_sha256": None,
        "receipt_file_sha256": None,
        "agent_execution_sha256": None,
        "disposition": None,
        "finding_codes": None,
    }
    existing = handoff.get("agent_run")
    if existing is not None:
        validate_agent_run(existing)
        immutable_keys = {
            "role",
            "generation",
            "base_head",
            "input_head",
            "repository_head",
            "input_tree",
            "challenge_sha256",
        }
        if any(existing[key] != intended[key] for key in immutable_keys):
            if (
                existing["phase"] != "prepared"
                or existing["role"] != role
                or existing["generation"] != intended["generation"]
                or not hmac.compare_digest(
                    existing["challenge_sha256"],
                    challenge_sha256,
                )
            ):
                raise AutopilotError("agent_run_context_conflict")
            handoff["agent_run"] = intended
            candidate["updated_at"] = now
            append_event(
                state,
                "agent_run_context_retargeted",
                now,
                candidate_id=candidate_id,
            )
            return deepcopy(intended)
        return deepcopy(existing)
    handoff["agent_run"] = intended
    candidate["updated_at"] = now
    append_event(state, "agent_run_prepared", now, candidate_id=candidate_id)
    return deepcopy(intended)


def _complete_agent_run_record(
    state: dict[str, Any],
    candidate: dict[str, Any],
    *,
    candidate_id: str,
    role: str,
    generation: int,
    receipt: dict[str, Any],
    receipt_file_sha256: str,
    now: int,
) -> dict[str, Any]:
    handoff = candidate.get("handoff")
    agent_run = (
        handoff.get("agent_run") if isinstance(handoff, dict) else None
    )
    if (
        not isinstance(agent_run, dict)
        or agent_run.get("role") != role
        or agent_run.get("generation") != generation
        or not is_sha256(receipt_file_sha256)
    ):
        raise AutopilotError("agent_run_missing")
    validate_agent_run(agent_run)
    disposition = receipt.get("disposition")
    finding_codes = receipt.get("finding_codes")
    if not isinstance(disposition, str) or not isinstance(finding_codes, list):
        raise AutopilotError("agent_run_receipt_invalid")
    action = "worker_staged" if role == "maintainer" else "independent_review"
    provenance = validate_handoff_receipt(
        receipt,
        candidate_id=candidate_id,
        role=role,
        action=action,
        generation=agent_run["generation"],
        challenge_sha256=agent_run["challenge_sha256"],
        base_head=agent_run["base_head"],
        repository_head=agent_run["repository_head"],
        repository_tree=receipt.get("repository_tree"),
        staged_paths_sha256=receipt.get("staged_paths_sha256"),
        patch_sha256=receipt.get("patch_sha256"),
        disposition=disposition,
        finding_codes=finding_codes,
        consumed_at=now,
    )
    actual_file_sha256 = hashlib.sha256(
        canonical_json(receipt) + b"\n"
    ).hexdigest()
    if not hmac.compare_digest(actual_file_sha256, receipt_file_sha256):
        raise AutopilotError("agent_run_receipt_invalid")
    completed = {
        **{
            key: agent_run[key]
            for key in (
                "role",
                "generation",
                "base_head",
                "input_head",
                "repository_head",
                "input_tree",
                "challenge_sha256",
                "started_at",
            )
        },
        "phase": "completed",
        "repository_tree": receipt["repository_tree"],
        "completed_at": now,
        "receipt_sha256": provenance["receipt_sha256"],
        "receipt_file_sha256": receipt_file_sha256,
        "agent_execution_sha256": receipt["agent_execution"][
            "execution_sha256"
        ],
        "disposition": disposition,
        "finding_codes": list(finding_codes),
    }
    validate_agent_run(completed)
    if agent_run["phase"] == "completed":
        comparable = dict(completed)
        comparable["completed_at"] = agent_run["completed_at"]
        if agent_run != comparable:
            raise AutopilotError("agent_run_completion_conflict")
        return deepcopy(agent_run)
    stale_credit = candidate.get("stale_refresh_credit")
    if (
        role == "maintainer"
        and isinstance(candidate.get("stale_refresh"), dict)
        and isinstance(stale_credit, dict)
        and stale_credit.get("generation") == generation
    ):
        if stale_credit.get("attempt_incremented"):
            candidate["attempts"]["maintainer"] = max(
                0,
                candidate["attempts"]["maintainer"] - 1,
            )
        candidate["stale_refresh_credit"] = None
        append_event(
            state,
            "stale_refresh_credit_refunded",
            now,
            candidate_id=candidate_id,
            reason_code="base_stale",
        )
    handoff["agent_run"] = completed
    candidate["updated_at"] = now
    append_event(state, "agent_run_completed", now, candidate_id=candidate_id)
    return deepcopy(completed)


def complete_agent_run(
    state: dict[str, Any],
    *,
    candidate_id: str,
    role: str,
    token: str,
    receipt: dict[str, Any],
    receipt_file_sha256: str,
    now: int,
) -> dict[str, Any]:
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, role, token, now)
    return _complete_agent_run_record(
        state,
        candidate,
        candidate_id=candidate_id,
        role=role,
        generation=candidate["lease"]["generation"],
        receipt=receipt,
        receipt_file_sha256=receipt_file_sha256,
        now=now,
    )


def complete_expired_agent_run(
    state: dict[str, Any],
    *,
    candidate_id: str,
    role: str,
    receipt: dict[str, Any],
    receipt_file_sha256: str,
    now: int,
) -> dict[str, Any]:
    """Promote one canonical receipt before its expired lease is recovered."""

    candidate = find_candidate(state, candidate_id)
    lease = candidate.get("lease")
    handoff = candidate.get("handoff")
    agent_run = (
        handoff.get("agent_run") if isinstance(handoff, dict) else None
    )
    if (
        not isinstance(lease, dict)
        or lease.get("role") != role
        or lease["expires_at"] > now
        or not isinstance(agent_run, dict)
        or agent_run.get("phase") != "prepared"
    ):
        raise AutopilotError("expired_agent_run_recovery_invalid")
    completed = _complete_agent_run_record(
        state,
        candidate,
        candidate_id=candidate_id,
        role=role,
        generation=lease["generation"],
        receipt=receipt,
        receipt_file_sha256=receipt_file_sha256,
        now=now,
    )
    append_event(
        state,
        "expired_agent_run_receipt_promoted",
        now,
        candidate_id=candidate_id,
    )
    return completed


def consume_handoff_receipt(
    state: dict[str, Any],
    *,
    candidate_id: str,
    role: str,
    token: str,
    receipt: dict[str, Any],
    action: str,
    base_head: str,
    repository_head: str,
    repository_tree: str,
    staged_paths_sha256: str | None,
    disposition: str,
    finding_codes: Sequence[str],
    now: int,
) -> dict[str, Any]:
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, role, token, now)
    handoff = candidate.get("handoff")
    lease = candidate["lease"]
    if (
        not isinstance(handoff, dict)
        or handoff.get("role") != role
        or handoff.get("generation") != lease["generation"]
    ):
        raise AutopilotError("handoff_challenge_missing")
    consumed_at = (
        handoff["consumed"]["consumed_at"]
        if isinstance(handoff.get("consumed"), dict)
        else now
    )
    provenance = validate_handoff_receipt(
        receipt,
        candidate_id=candidate_id,
        role=role,
        action=action,
        generation=lease["generation"],
        challenge_sha256=handoff["challenge_sha256"],
        base_head=base_head,
        repository_head=repository_head,
        repository_tree=repository_tree,
        staged_paths_sha256=staged_paths_sha256,
        patch_sha256=(
            receipt.get("patch_sha256")
            if isinstance(receipt, dict)
            else None
        ),
        disposition=disposition,
        finding_codes=finding_codes,
        consumed_at=consumed_at,
    )
    existing = handoff.get("consumed")
    agent_run = handoff.get("agent_run")
    if not isinstance(agent_run, dict):
        raise AutopilotError("agent_run_missing")
    validate_agent_run(agent_run)
    if (
        agent_run["phase"] != "completed"
        or agent_run["receipt_sha256"] != provenance["receipt_sha256"]
        or agent_run["agent_execution_sha256"]
        != provenance["agent_execution"]["execution_sha256"]
    ):
        raise AutopilotError("agent_run_receipt_mismatch")
    if existing is not None and existing != provenance:
        raise AutopilotError("handoff_receipt_replayed")
    if existing == provenance:
        return deepcopy(provenance)
    for other in state["candidates"]:
        other_handoff = other.get("handoff")
        other_consumed = (
            other_handoff.get("consumed")
            if isinstance(other_handoff, dict)
            else None
        )
        if (
            isinstance(other_consumed, dict)
            and other_consumed["receipt_sha256"] == provenance["receipt_sha256"]
            and (
                other["id"] != candidate_id
                or other_handoff["generation"] != lease["generation"]
            )
        ):
            raise AutopilotError("handoff_receipt_replayed")
    handoff["consumed"] = deepcopy(provenance)
    candidate["updated_at"] = now
    append_event(state, "handoff_receipt_consumed", now, candidate_id=candidate_id)
    return provenance


def retry_delay(policy: dict[str, Any], attempt: int) -> int:
    values = [int(value) for value in policy["retry_backoff_seconds"]]
    return values[min(max(attempt - 1, 0), len(values) - 1)]


def recoverable_completed_agent_run(
    candidate: dict[str, Any],
    *,
    role: str,
) -> dict[str, Any] | None:
    """Return one completed, unconsumed run that a new lease can reclaim."""

    handoff = candidate.get("handoff")
    agent_run = (
        handoff.get("agent_run") if isinstance(handoff, dict) else None
    )
    if (
        isinstance(handoff, dict)
        and handoff.get("role") == role
        and handoff.get("consumed") is None
        and isinstance(agent_run, dict)
        and agent_run.get("phase") == "completed"
        and agent_run.get("role") == role
        and agent_run.get("generation") == handoff.get("generation")
    ):
        validate_agent_run(agent_run)
        return handoff
    return None


def recover_expired_leases(
    state: dict[str, Any],
    policy: dict[str, Any],
    now: int,
    *,
    prepared_agent_runs_reconciled: bool = False,
) -> list[str]:
    recovered: list[str] = []
    for candidate in state["candidates"]:
        lease = candidate.get("lease")
        if lease is None or lease["expires_at"] > now:
            continue
        role = lease["role"]
        pre_publish_stale_refresh = bool(
            role == "maintainer"
            and candidate_has_pre_publish_stale_refresh(candidate)
        )
        handoff = candidate.get("handoff")
        agent_run = (
            handoff.get("agent_run")
            if isinstance(handoff, dict)
            else None
        )
        if (
            isinstance(agent_run, dict)
            and agent_run.get("phase") == "prepared"
            and not prepared_agent_runs_reconciled
        ):
            raise AutopilotError(
                "expired_agent_run_reconciliation_required"
            )
        completed_handoff = recoverable_completed_agent_run(
            candidate,
            role=role,
        )
        if pre_publish_stale_refresh:
            candidate["stale_refresh"] = None
        if completed_handoff is not None:
            candidate["lease"] = None
            candidate["updated_at"] = now
            if role == "reviewer":
                candidate["status"] = "review_pending"
                candidate["next_retry_at"] = None
                candidate["retry_role"] = None
            else:
                result = candidate.get("result")
                if (
                    isinstance(result, dict)
                    and result.get("outcome") == "repair_requested"
                ):
                    candidate["status"] = "repair_requested"
                else:
                    candidate["status"] = "queued"
                    candidate["result"] = None
                candidate["next_retry_at"] = None
                candidate["retry_role"] = None
            append_event(
                state,
                "completed_agent_run_recovery_pending",
                now,
                candidate_id=candidate["id"],
            )
            recovered.append(candidate["id"])
            continue
        attempts = candidate["attempts"][role]
        stale_base_retry = bool(
            role == "maintainer"
            and isinstance(candidate.get("result"), dict)
            and candidate["result"].get("outcome") == "repair_requested"
            and candidate["result"].get("finding_codes") == ["base_stale"]
        )
        stale_credit = candidate.get("stale_refresh_credit")
        if (
            isinstance(stale_credit, dict)
            and stale_credit.get("generation") == lease["generation"]
        ):
            candidate["stale_refresh_credit"] = None
        effect = candidate.get("effect")
        if (
            isinstance(effect, dict)
            and effect.get("kind") == "land"
            and effect.get("phase") == "prepared"
            and effect.get("command_receipt") is None
            and effect.get("execution_receipt") is None
        ):
            candidate["effect"] = None
        candidate["lease"] = None
        candidate["handoff"] = None
        candidate["updated_at"] = now
        recovery_role = external_effect_recovery_role(candidate)
        if recovery_role == role:
            candidate["status"] = "retry_wait"
            candidate["next_retry_at"] = now
            candidate["retry_role"] = role
            if not stale_base_retry:
                candidate["result"] = {
                    "outcome": "blocked",
                    "reason_code": "lease_expired",
                    "error_digest": sha256_value(
                        {"reason_code": "lease_expired", "role": role}
                    ),
                    "finding_codes": preserved_finding_codes(candidate),
                    "at": now,
                }
        elif attempts >= int(policy["max_attempts"]):
            candidate["status"] = "needs_attention"
            candidate["next_retry_at"] = None
            candidate["retry_role"] = role
            if not stale_base_retry:
                candidate["result"] = {
                    "outcome": "blocked",
                    "reason_code": "lease_expired",
                    "error_digest": sha256_value(
                        {"reason_code": "lease_expired", "role": role}
                    ),
                    "finding_codes": preserved_finding_codes(candidate),
                    "at": now,
                }
        elif role == "reviewer":
            candidate["status"] = "review_pending"
            candidate["next_retry_at"] = None
            candidate["retry_role"] = None
        else:
            candidate["status"] = "retry_wait"
            candidate["next_retry_at"] = now
            candidate["retry_role"] = "maintainer"
            if not stale_base_retry:
                candidate["result"] = {
                    "outcome": "blocked",
                    "reason_code": "lease_expired",
                    "error_digest": sha256_value(
                        {"reason_code": "lease_expired", "role": role}
                    ),
                    "finding_codes": preserved_finding_codes(candidate),
                    "at": now,
                }
        append_event(
            state,
            "lease_expired_recovered",
            now,
            candidate_id=candidate["id"],
        )
        recovered.append(candidate["id"])
    return recovered


def abandon_missing_completed_agent_run(
    state: dict[str, Any],
    *,
    candidate_id: str,
    role: str,
    token: str,
    now: int,
) -> None:
    """Replace a completed state record whose canonical receipt is missing."""

    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, role, token, now)
    if recoverable_completed_agent_run(candidate, role=role) is None:
        raise AutopilotError("agent_run_recovery_invalid")
    stale_base_recovery = bool(
        role == "maintainer"
        and isinstance(candidate.get("result"), dict)
        and candidate["result"].get("outcome") == "repair_requested"
        and candidate["result"].get("finding_codes") == ["base_stale"]
    )
    pre_publish_stale_refresh = bool(
        role == "maintainer"
        and candidate_has_pre_publish_stale_refresh(candidate)
    )
    if not stale_base_recovery and not pre_publish_stale_refresh:
        candidate["attempts"][role] = max(
            0,
            candidate["attempts"][role] - 1,
        )
    else:
        candidate["stale_refresh_credit"] = None
    candidate["lease"] = None
    candidate["handoff"] = None
    if pre_publish_stale_refresh:
        candidate["stale_refresh"] = None
    candidate["updated_at"] = now
    if role == "reviewer":
        candidate["status"] = "review_pending"
        candidate["next_retry_at"] = None
        candidate["retry_role"] = None
    else:
        result = candidate.get("result")
        if (
            isinstance(result, dict)
            and result.get("outcome") == "repair_requested"
        ):
            candidate["status"] = "repair_requested"
        else:
            candidate["status"] = "queued"
            candidate["result"] = None
        candidate["next_retry_at"] = None
        candidate["retry_role"] = None
    append_event(
        state,
        "completed_agent_run_receipt_missing",
        now,
        candidate_id=candidate_id,
    )


def candidate_is_claimable(candidate: dict[str, Any], role: str, now: int) -> bool:
    if candidate.get("lease") is not None:
        return False
    if role == "maintainer":
        if candidate["status"] in {"queued", "repair_requested"}:
            return True
        return (
            candidate["status"] == "retry_wait"
            and candidate.get("retry_role") == role
            and int(candidate.get("next_retry_at") or 0) <= now
        )
    if candidate["status"] == "review_pending":
        return True
    return (
        candidate["status"] == "retry_wait"
        and candidate.get("retry_role") == role
        and int(candidate.get("next_retry_at") or 0) <= now
    )


def earliest_unresolved_source_candidate(
    state: dict[str, Any],
) -> dict[str, Any] | None:
    return min(
        (
            candidate
            for candidate in state["candidates"]
            if candidate["kind"] != "automation_repair"
            and candidate["status"] not in TERMINAL_STATUSES
        ),
        key=lambda candidate: candidate["discovery_sequence"],
        default=None,
    )


def claim_candidate(
    state: dict[str, Any],
    policy: dict[str, Any],
    role: str,
    now: int,
    *,
    prepared_agent_runs_reconciled: bool = False,
) -> dict[str, Any] | None:
    recover_expired_leases(
        state,
        policy,
        now,
        prepared_agent_runs_reconciled=prepared_agent_runs_reconciled,
    )
    active = next(
        (
            candidate
            for candidate in state["candidates"]
            if candidate.get("lease") is not None
            and candidate["lease"].get("role") == role
        ),
        None,
    )
    if active is not None:
        return {
            "busy": {
                "candidate_id": active["id"],
                "lease_expires_at": active["lease"]["expires_at"],
            }
        }
    candidates = [
        candidate
        for candidate in state["candidates"]
        if candidate_is_claimable(candidate, role, now)
    ]
    if role == "maintainer":
        source_predecessor = earliest_unresolved_source_candidate(state)
        if source_predecessor is not None:
            candidates = [
                candidate
                for candidate in candidates
                if candidate["kind"] == "automation_repair"
                or candidate["id"] == source_predecessor["id"]
            ]
            repairs = [
                candidate
                for candidate in candidates
                if candidate["kind"] == "automation_repair"
            ]
            if source_predecessor["status"] in {
                "retry_wait",
                "repair_pending",
                "needs_attention",
            } and repairs:
                candidates = repairs
    if not candidates:
        return None
    candidates.sort(
        key=lambda value: (
            0 if value["priority"] == "critical" else 1,
            0
            if value["kind"] == "automation_repair"
            and value.get("repair_of") is not None
            else 1,
            value.get("source_sequence")
            if value.get("source_sequence") is not None
            else 1_000_000_000,
            value["created_at"],
            value["id"],
        )
    )
    candidate = candidates[0]
    completed_handoff = recoverable_completed_agent_run(
        candidate,
        role=role,
    )
    effect = candidate.get("effect")
    if isinstance(effect, dict):
        effect_role = (
            "reviewer" if effect.get("kind") == "land" else "maintainer"
        )
        if effect_role != role:
            raise AutopilotError("candidate_effect_role_mismatch")
    external_effect_recovery = bool(
        external_effect_recovery_role(candidate) == role
    )
    attempts = candidate["attempts"][role]
    stale_credit_available = bool(
        role == "maintainer"
        and isinstance(candidate.get("result"), dict)
        and candidate["result"].get("outcome") == "repair_requested"
        and candidate["result"].get("finding_codes") == ["base_stale"]
        and candidate.get("stale_refresh_credit")
        == {"generation": None, "attempt_incremented": False}
    )
    if (
        completed_handoff is None
        and not stale_credit_available
        and not external_effect_recovery
        and attempts >= int(policy["max_attempts"])
    ):
        stale_base_retry = bool(
            role == "maintainer"
            and isinstance(candidate.get("result"), dict)
            and candidate["result"].get("outcome") == "repair_requested"
            and candidate["result"].get("finding_codes") == ["base_stale"]
        )
        candidate["status"] = "needs_attention"
        candidate["next_retry_at"] = None
        candidate["retry_role"] = role
        if not stale_base_retry:
            candidate["result"] = {
                "outcome": "blocked",
                "reason_code": "attempt_budget_exhausted",
                "error_digest": sha256_value(
                    {
                        "reason_code": "attempt_budget_exhausted",
                        "role": role,
                        "attempts": attempts,
                    }
                ),
                "finding_codes": preserved_finding_codes(candidate),
                "at": now,
            }
        candidate["updated_at"] = now
        append_event(
            state,
            "attempt_budget_exhausted",
            now,
            candidate_id=candidate["id"],
        )
        return None
    raw_token = secrets.token_urlsafe(32)
    if completed_handoff is not None:
        raw_challenge: str | None = None
        generation = completed_handoff["generation"]
    else:
        used_challenges = {
            value["handoff"]["challenge_sha256"]
            for value in state["candidates"]
            if isinstance(value.get("handoff"), dict)
        }
        raw_challenge = ""
        challenge_sha256 = ""
        for _attempt in range(8):
            raw_challenge = secrets.token_urlsafe(32)
            challenge_sha256 = hashlib.sha256(
                raw_challenge.encode("utf-8")
            ).hexdigest()
            if challenge_sha256 not in used_challenges:
                break
        else:
            raise AutopilotError("handoff_challenge_generation_failed")
        generation = state["source"]["next_lease_generation"]
        state["source"]["next_lease_generation"] += 1
        if stale_credit_available:
            incremented = attempts < 10
            if incremented:
                candidate["attempts"][role] += 1
            candidate["stale_refresh_credit"] = {
                "generation": generation,
                "attempt_incremented": incremented,
            }
        elif not external_effect_recovery:
            candidate["attempts"][role] += 1
    candidate["status"] = "implementing" if role == "maintainer" else "reviewing"
    candidate["next_retry_at"] = None
    candidate["retry_role"] = None
    candidate["lease"] = {
        "role": role,
        "generation": generation,
        "token_sha256": hashlib.sha256(raw_token.encode("utf-8")).hexdigest(),
        "issued_at": now,
        "expires_at": now + int(policy["lease_seconds"]),
        "renewals": 0,
    }
    if completed_handoff is None:
        candidate["handoff"] = {
            "role": role,
            "generation": generation,
            "challenge_sha256": challenge_sha256,
            "issued_at": now,
            "consumed": None,
            "agent_run": None,
        }
    if isinstance(effect, dict):
        effect["active_lease_generation"] = generation
        effect["updated_at"] = now
    candidate["updated_at"] = now
    append_event(state, "candidate_claimed", now, candidate_id=candidate["id"])
    public_candidate = deepcopy(candidate)
    public_candidate["lease"] = {
        "role": role,
        "generation": generation,
        "expires_at": candidate["lease"]["expires_at"],
        "renewals": 0,
    }
    public_candidate["handoff"] = {
        "role": role,
        "generation": generation,
        "issued_at": candidate["handoff"]["issued_at"],
        "consumed": False,
        "agent_run": (
            None
            if completed_handoff is None
            else {"phase": "completed"}
        ),
    }
    return {
        "candidate": public_candidate,
        "lease_token": raw_token,
        "handoff_challenge": raw_challenge,
        "completed_agent_run_recovery": completed_handoff is not None,
    }


def renew_lease(
    state: dict[str, Any],
    policy: dict[str, Any],
    *,
    candidate_id: str,
    role: str,
    token: str,
    now: int,
) -> int:
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, role, token, now)
    lease = candidate["lease"]
    if lease["renewals"] >= int(policy["max_lease_renewals"]):
        raise AutopilotError("lease_renewal_budget_exhausted")
    lease["renewals"] += 1
    lease["expires_at"] = now + int(policy["lease_seconds"])
    candidate["updated_at"] = now
    append_event(state, "lease_renewed", now, candidate_id=candidate_id)
    return lease["expires_at"]


def ensure_lease_budget(
    state: dict[str, Any],
    policy: dict[str, Any],
    *,
    candidate_id: str,
    role: str,
    token: str,
    minimum_seconds: int,
    now: int,
) -> int:
    lease_seconds = int(policy["lease_seconds"])
    write_guard = int(policy["lease_write_guard_seconds"])
    if not write_guard <= minimum_seconds <= lease_seconds:
        raise AutopilotError("lease_budget_invalid")
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, role, token, now)
    if candidate["lease"]["expires_at"] - now < minimum_seconds:
        return renew_lease(
            state,
            policy,
            candidate_id=candidate_id,
            role=role,
            token=token,
            now=now,
        )
    return candidate["lease"]["expires_at"]


def check_lease_write_guard(
    state: dict[str, Any],
    policy: dict[str, Any],
    *,
    candidate_id: str,
    role: str,
    token: str,
    now: int,
) -> int:
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, role, token, now)
    remaining = candidate["lease"]["expires_at"] - now
    if remaining < int(policy["lease_write_guard_seconds"]):
        raise AutopilotError("lease_write_guard_insufficient")
    return candidate["lease"]["expires_at"]


def check_lease_budget(
    state: dict[str, Any],
    policy: dict[str, Any],
    *,
    candidate_id: str,
    role: str,
    token: str,
    minimum_seconds: int,
    now: int,
) -> int:
    lease_seconds = int(policy["lease_seconds"])
    write_guard = int(policy["lease_write_guard_seconds"])
    if not write_guard <= minimum_seconds <= lease_seconds:
        raise AutopilotError("lease_budget_invalid")
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, role, token, now)
    if candidate["lease"]["expires_at"] - now < minimum_seconds:
        raise AutopilotError("effect_lease_budget_insufficient")
    return candidate["lease"]["expires_at"]


def prepare_effect(
    state: dict[str, Any],
    policy: dict[str, Any],
    *,
    candidate_id: str,
    role: str,
    token: str,
    kind: str,
    branch: str,
    head_sha: str,
    pr_url: str | None,
    remote_head_before: str | None = None,
    owned_worktrees: list[str] | None = None,
    validation_receipt: dict[str, Any] | None = None,
    handoff_receipt: dict[str, Any] | None = None,
    decodex_identity: dict[str, Any] | None = None,
    now: int,
) -> dict[str, Any]:
    expected_role = {
        "commit": "maintainer",
        "publish": "maintainer",
        "retire_pr": "maintainer",
        "land": "reviewer",
    }.get(kind)
    if expected_role != role:
        raise AutopilotError("effect_role_invalid")
    if SHA_PATTERN.fullmatch(head_sha) is None or (
        pr_url is not None and PR_PATTERN.fullmatch(pr_url) is None
    ) or (
        remote_head_before is not None
        and SHA_PATTERN.fullmatch(remote_head_before) is None
    ):
        raise AutopilotError("effect_identity_invalid")
    if (kind == "land") != (owned_worktrees is not None):
        raise AutopilotError("effect_worktree_ownership_invalid")
    if owned_worktrees is not None and not valid_owned_worktrees(
        owned_worktrees
    ):
        raise AutopilotError("effect_worktree_ownership_invalid")
    if kind != "publish" and remote_head_before is not None:
        raise AutopilotError("effect_remote_head_invalid")
    candidate = find_candidate(state, candidate_id)
    required_budget = (
        LAND_EFFECT_LEASE_BUDGET_SECONDS
        if kind == "land"
        else SIDE_EFFECT_LEASE_BUDGET_SECONDS
    )
    check_lease_budget(
        state,
        policy,
        candidate_id=candidate_id,
        role=role,
        token=token,
        minimum_seconds=required_budget,
        now=now,
    )
    expected_status = "implementing" if role == "maintainer" else "reviewing"
    if candidate["status"] != expected_status:
        raise AutopilotError("candidate_role_status_mismatch")
    if branch != candidate["branch_name"]:
        raise AutopilotError("candidate_branch_mismatch")
    if kind in {"publish", "land"}:
        validate_validation_receipt(
            validation_receipt,
            role="maintainer" if kind == "publish" else "reviewer",
            expected_head=head_sha,
        )
    elif validation_receipt is not None:
        raise AutopilotError("effect_validation_receipt_invalid")
    effect = candidate.get("effect")
    recovering_handoff = (
        effect.get("handoff_receipt")
        if isinstance(effect, dict) and effect.get("kind") == kind
        else None
    )
    if recovering_handoff is not None:
        if handoff_receipt is not None and handoff_receipt != recovering_handoff:
            raise AutopilotError("effect_recovery_conflict")
        handoff_receipt = recovering_handoff
    if kind in {"commit", "land"}:
        validate_handoff_provenance(handoff_receipt)
        expected_action = (
            "worker_staged" if kind == "commit" else "independent_review"
        )
        active_handoff = candidate.get("handoff")
        if (
            handoff_receipt["candidate_id"] != candidate_id
            or handoff_receipt["role"] != role
            or handoff_receipt["action"] != expected_action
            or (
                kind == "commit"
                and handoff_receipt["disposition"] != "staged"
            )
            or (
                kind == "land"
                and handoff_receipt["disposition"] != "accept"
            )
            or (
                recovering_handoff is None
                and handoff_receipt["claim_generation"]
                != candidate["lease"]["generation"]
            )
            or (
                recovering_handoff is None
                and (
                    not isinstance(active_handoff, dict)
                    or active_handoff.get("consumed") != handoff_receipt
                )
            )
        ):
            raise AutopilotError("effect_handoff_receipt_invalid")
        if kind == "commit" and (
            handoff_receipt["base_head"] != head_sha
            or handoff_receipt["repository_head"] != head_sha
            or handoff_receipt["finding_codes"]
            or handoff_receipt["staged_paths_sha256"] is None
        ):
            raise AutopilotError("effect_handoff_receipt_invalid")
        if kind == "land":
            try:
                validate_reviewer_handoff_semantics(
                    candidate,
                    handoff_receipt,
                    disposition="accept",
                    finding_codes=[],
                    receipt=validation_receipt,
                )
            except AutopilotError as error:
                raise AutopilotError("effect_handoff_receipt_invalid") from error
    elif handoff_receipt is not None:
        raise AutopilotError("effect_handoff_receipt_invalid")
    if kind in {"commit", "land"}:
        if (
            not has_exact_keys(
                decodex_identity,
                {"version", "executable_sha256"},
            )
            or not isinstance(decodex_identity.get("version"), str)
            or not 1 <= len(decodex_identity["version"]) <= 256
            or "\n" in decodex_identity["version"]
            or "\r" in decodex_identity["version"]
            or not is_sha256(decodex_identity.get("executable_sha256"))
        ):
            raise AutopilotError("effect_decodex_identity_invalid")
    elif decodex_identity is not None:
        raise AutopilotError("effect_decodex_identity_invalid")
    pull_request = candidate.get("pull_request")
    if kind == "publish" and (
        (
            isinstance(pull_request, dict)
            and pull_request.get("url") != pr_url
        )
        or (pull_request is None and pr_url is not None)
    ):
        raise AutopilotError("effect_pr_mismatch")
    if kind == "publish" and (
        not isinstance(candidate.get("commit_receipt"), dict)
        or candidate["commit_receipt"]["head_sha"] != head_sha
    ):
        raise AutopilotError("candidate_commit_receipt_missing")
    if kind in {"retire_pr", "land"} and (
        not isinstance(pull_request, dict)
        or pull_request.get("url") != pr_url
        or pull_request.get("branch") != branch
        or pull_request.get("head_sha") != head_sha
    ):
        raise AutopilotError("effect_pr_mismatch")
    lease_generation = candidate["lease"]["generation"]
    if effect is not None:
        if (
            effect["kind"] != kind
            or effect["branch"] != branch
            or effect["head_sha"] != head_sha
            or effect["remote_head_before"] != remote_head_before
            or effect["owned_worktrees"] != owned_worktrees
            or effect["pr_url"] != pr_url
            or effect["decodex_identity"] != decodex_identity
            or effect["handoff_receipt"] != handoff_receipt
        ):
            raise AutopilotError("effect_recovery_conflict")
        effect["active_lease_generation"] = lease_generation
        if validation_receipt is not None:
            if (
                effect["validation_receipt"] is not None
                and effect["validation_receipt"]["repository_tree"]
                != validation_receipt["repository_tree"]
            ):
                raise AutopilotError("effect_validation_receipt_mismatch")
            effect["validation_receipt"] = deepcopy(validation_receipt)
        effect["updated_at"] = now
        append_event(state, "effect_adopted", now, candidate_id=candidate_id)
        return deepcopy(effect)
    effect = {
        "kind": kind,
        "lease_generation": lease_generation,
        "active_lease_generation": lease_generation,
        "intent_sha256": secrets.token_hex(32),
        "phase": "prepared",
        "branch": branch,
        "head_sha": head_sha,
        "remote_head_before": remote_head_before,
        "owned_worktrees": deepcopy(owned_worktrees),
        "pr_url": pr_url,
        "validation_receipt": deepcopy(validation_receipt),
        "handoff_receipt": deepcopy(handoff_receipt),
        "decodex_identity": deepcopy(decodex_identity),
        "command_receipt": None,
        "execution_receipt": None,
        "started_at": now,
        "updated_at": now,
    }
    candidate["effect"] = effect
    candidate["updated_at"] = now
    append_event(state, "effect_prepared", now, candidate_id=candidate_id)
    return deepcopy(effect)


def advance_effect_phase(
    state: dict[str, Any],
    *,
    candidate_id: str,
    role: str,
    token: str,
    phase: str,
    pr_url: str | None = None,
    now: int,
) -> None:
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, role, token, now)
    effect = candidate.get("effect")
    if (
        not isinstance(effect, dict)
        or effect["active_lease_generation"]
        != candidate["lease"]["generation"]
    ):
        raise AutopilotError("effect_permit_stale")
    allowed = {
        "commit": set(),
        "publish": {"pushed", "pr_created"},
        "land": {"land_started"},
        "retire_pr": set(),
    }[effect["kind"]]
    if phase not in allowed:
        raise AutopilotError("effect_phase_invalid")
    if effect["kind"] == "publish":
        rank = {"prepared": 0, "pushed": 1, "pr_created": 2}
        if rank[phase] < rank[effect["phase"]]:
            raise AutopilotError("effect_phase_regression")
    if (
        effect["kind"] == "land"
        and effect["phase"] in {"land_command_completed", "land_completed"}
    ):
        raise AutopilotError("effect_phase_regression")
    if pr_url is not None:
        if (
            effect["kind"] != "publish"
            or phase != "pr_created"
            or PR_PATTERN.fullmatch(pr_url) is None
            or effect["pr_url"] not in {None, pr_url}
        ):
            raise AutopilotError("effect_pr_mismatch")
        effect["pr_url"] = pr_url
    effect["phase"] = phase
    effect["updated_at"] = now
    candidate["updated_at"] = now
    append_event(state, "effect_advanced", now, candidate_id=candidate_id)


def validate_land_command_receipt(
    receipt: Any,
    *,
    intent_sha256: str,
    decodex_identity: dict[str, Any] | None = None,
    intent_started_at: int | None = None,
    observed_at: int | None = None,
) -> None:
    if (
        not has_exact_keys(
            receipt,
            {
                "schema",
                "intent_sha256",
                "execution_mode",
                "decodex_version",
                "decodex_executable_sha256",
                "started_at",
                "completed_at",
                "stdout_sha256",
                "reported_merge_sha",
            },
        )
        or receipt.get("schema")
        != "decodex/codex-upstream-land-command/1"
        or receipt.get("intent_sha256") != intent_sha256
        or receipt.get("execution_mode") != "command_completed"
        or not isinstance(receipt.get("decodex_version"), str)
        or not 1 <= len(receipt["decodex_version"]) <= 256
        or "\n" in receipt["decodex_version"]
        or "\r" in receipt["decodex_version"]
        or not is_sha256(receipt.get("decodex_executable_sha256"))
        or not isinstance(receipt.get("started_at"), int)
        or not isinstance(receipt.get("completed_at"), int)
        or receipt["completed_at"] < receipt["started_at"]
        or not is_sha256(receipt.get("stdout_sha256"))
        or SHA_PATTERN.fullmatch(
            str(receipt.get("reported_merge_sha", ""))
        )
        is None
    ):
        raise AutopilotError("land_command_receipt_invalid")
    if decodex_identity is not None and (
        receipt["decodex_version"] != decodex_identity["version"]
        or receipt["decodex_executable_sha256"]
        != decodex_identity["executable_sha256"]
    ):
        raise AutopilotError("land_command_receipt_mismatch")
    if (
        intent_started_at is not None
        and receipt["started_at"] < intent_started_at
    ):
        raise AutopilotError("land_command_receipt_mismatch")
    if observed_at is not None and receipt["completed_at"] > observed_at:
        raise AutopilotError("land_command_receipt_mismatch")


def validate_land_execution_receipt(
    receipt: Any,
    *,
    intent_sha256: str,
    merge_sha: str | None,
    decodex_identity: dict[str, Any] | None = None,
    intent_started_at: int | None = None,
    observed_at: int | None = None,
) -> None:
    if (
        not has_exact_keys(
            receipt,
            {
                "schema",
                "intent_sha256",
                "execution_mode",
                "decodex_version",
                "decodex_executable_sha256",
                "started_at",
                "completed_at",
                "stdout_sha256",
                "reported_merge_sha",
                "landed_record_sha256",
            },
        )
        or receipt.get("schema")
        != "decodex/codex-upstream-land-execution/1"
        or receipt.get("intent_sha256") != intent_sha256
        or receipt.get("execution_mode") != "command_completed"
        or not isinstance(receipt.get("decodex_version"), str)
        or not 1 <= len(receipt["decodex_version"]) <= 256
        or "\n" in receipt["decodex_version"]
        or "\r" in receipt["decodex_version"]
        or not is_sha256(receipt.get("decodex_executable_sha256"))
        or not isinstance(receipt.get("started_at"), int)
        or not isinstance(receipt.get("completed_at"), int)
        or receipt["completed_at"] < receipt["started_at"]
        or not SHA_PATTERN.fullmatch(
            str(receipt.get("reported_merge_sha", ""))
        )
        or not is_sha256(receipt.get("landed_record_sha256"))
    ):
        raise AutopilotError("land_execution_receipt_invalid")
    if not is_sha256(receipt.get("stdout_sha256")):
        raise AutopilotError("land_execution_receipt_invalid")
    if merge_sha is not None and receipt["reported_merge_sha"] != merge_sha:
        raise AutopilotError("land_execution_receipt_mismatch")
    if decodex_identity is not None and (
        receipt["decodex_version"] != decodex_identity["version"]
        or receipt["decodex_executable_sha256"]
        != decodex_identity["executable_sha256"]
    ):
        raise AutopilotError("land_execution_receipt_mismatch")
    if (
        intent_started_at is not None
        and receipt["started_at"] < intent_started_at
    ):
        raise AutopilotError("land_execution_receipt_mismatch")
    if observed_at is not None and receipt["completed_at"] > observed_at:
        raise AutopilotError("land_execution_receipt_mismatch")


def record_land_command_execution(
    state: dict[str, Any],
    *,
    candidate_id: str,
    token: str,
    receipt: dict[str, Any],
    now: int,
) -> None:
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, "reviewer", token, now)
    effect = candidate.get("effect")
    if (
        candidate["status"] != "reviewing"
        or not isinstance(effect, dict)
        or effect["kind"] != "land"
        or effect["phase"] != "land_started"
        or effect["active_lease_generation"]
        != candidate["lease"]["generation"]
    ):
        raise AutopilotError("landing_effect_evidence_missing")
    validate_land_command_receipt(
        receipt,
        intent_sha256=effect["intent_sha256"],
        decodex_identity=effect["decodex_identity"],
        intent_started_at=effect["started_at"],
        observed_at=now,
    )
    effect["command_receipt"] = deepcopy(receipt)
    effect["phase"] = "land_command_completed"
    effect["updated_at"] = now
    candidate["updated_at"] = now
    append_event(
        state,
        "land_command_execution_recorded",
        now,
        candidate_id=candidate_id,
    )


def record_land_execution(
    state: dict[str, Any],
    *,
    candidate_id: str,
    token: str,
    receipt: dict[str, Any],
    now: int,
) -> None:
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, "reviewer", token, now)
    effect = candidate.get("effect")
    if (
        candidate["status"] != "reviewing"
        or not isinstance(effect, dict)
        or effect["kind"] != "land"
        or effect["phase"] != "land_command_completed"
        or effect["active_lease_generation"]
        != candidate["lease"]["generation"]
        or not isinstance(effect.get("command_receipt"), dict)
    ):
        raise AutopilotError("landing_effect_evidence_missing")
    validate_land_execution_receipt(
        receipt,
        intent_sha256=effect["intent_sha256"],
        merge_sha=receipt.get("reported_merge_sha"),
        decodex_identity=effect["decodex_identity"],
        intent_started_at=effect["started_at"],
        observed_at=now,
    )
    if receipt["reported_merge_sha"] != effect["command_receipt"][
        "reported_merge_sha"
    ]:
        raise AutopilotError("land_execution_receipt_mismatch")
    effect["execution_receipt"] = deepcopy(receipt)
    effect["phase"] = "land_completed"
    effect["updated_at"] = now
    candidate["updated_at"] = now
    append_event(state, "land_execution_recorded", now, candidate_id=candidate_id)


def pull_request_readback(pr_url: str) -> dict[str, Any]:
    if PR_PATTERN.fullmatch(pr_url) is None:
        raise AutopilotError("candidate_pr_invalid")
    output = run_command(
        [
            "gh",
            "pr",
            "view",
            pr_url,
            "--json",
            (
                "state,isDraft,isCrossRepository,baseRefName,baseRefOid,"
                "headRefName,headRefOid,url,mergeCommit"
            ),
        ],
        failure_code="pull_request_readback_failed",
    )
    try:
        value = json.loads(output)
    except json.JSONDecodeError as error:
        raise AutopilotError("pull_request_readback_invalid") from error
    if not isinstance(value, dict):
        raise AutopilotError("pull_request_readback_invalid")
    return value


def verify_open_pull_request(
    value: dict[str, Any],
    policy: dict[str, Any],
    *,
    pr_url: str,
    branch: str,
    base_head: str,
    head_sha: str,
) -> None:
    if (
        value.get("url") != pr_url
        or value.get("state") != "OPEN"
        or value.get("isDraft") is not False
        or value.get("isCrossRepository") is not False
        or value.get("baseRefName") != policy["target_branch"]
        or value.get("baseRefOid") != base_head
        or value.get("headRefName") != branch
        or value.get("headRefOid") != head_sha
    ):
        raise AutopilotError("pull_request_submission_mismatch")


def verify_merged_pull_request(
    value: dict[str, Any],
    policy: dict[str, Any],
    *,
    pr_url: str,
    branch: str,
    head_sha: str,
    merge_sha: str,
) -> None:
    merge_commit = value.get("mergeCommit")
    if (
        value.get("url") != pr_url
        or value.get("state") != "MERGED"
        or value.get("isCrossRepository") is not False
        or value.get("baseRefName") != policy["target_branch"]
        or value.get("headRefName") != branch
        or value.get("headRefOid") != head_sha
        or not isinstance(merge_commit, dict)
        or merge_commit.get("oid") != merge_sha
    ):
        raise AutopilotError("pull_request_landing_mismatch")


def verify_remote_main_contains(
    repo_root: Path,
    policy: dict[str, Any],
    *commits: str,
) -> None:
    run_command(
        [
            "git",
            "fetch",
            "--quiet",
            "origin",
            f"refs/heads/{policy['target_branch']}:refs/remotes/origin/{policy['target_branch']}",
        ],
        cwd=repo_root,
        failure_code="target_main_fetch_failed",
    )
    remote_main = f"refs/remotes/origin/{policy['target_branch']}"
    for commit in commits:
        if not command_succeeds(
            ["git", "merge-base", "--is-ancestor", commit, remote_main],
            cwd=repo_root,
            failure_code="landing_containment_unavailable",
        ):
            raise AutopilotError("landing_not_contained")


def verify_merge_parents(
    repo_root: Path,
    *,
    merge_sha: str,
    base_head: str,
    head_sha: str,
) -> None:
    if any(
        SHA_PATTERN.fullmatch(value) is None
        for value in (merge_sha, base_head, head_sha)
    ):
        raise AutopilotError("landing_parent_identity_invalid")
    parents = run_command(
        ["git", "show", "-s", "--format=%P", merge_sha],
        cwd=repo_root,
        failure_code="landing_parent_readback_failed",
    ).split()
    if parents != [base_head, head_sha]:
        raise AutopilotError("landing_parent_mismatch")


def submit_candidate(
    state: dict[str, Any],
    policy: dict[str, Any],
    *,
    candidate_id: str,
    token: str,
    branch: str,
    head_sha: str,
    pr_url: str,
    validation_receipt: dict[str, Any],
    now: int,
) -> None:
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, "maintainer", token, now)
    if candidate["status"] != "implementing":
        raise AutopilotError("candidate_not_implementing")
    if branch != candidate["branch_name"] or not branch.startswith(policy["branch_prefix"]):
        raise AutopilotError("candidate_branch_mismatch")
    if not SHA_PATTERN.fullmatch(head_sha):
        raise AutopilotError("candidate_head_invalid")
    if not PR_PATTERN.fullmatch(pr_url):
        raise AutopilotError("candidate_pr_invalid")
    validate_validation_receipt(
        validation_receipt,
        role="maintainer",
        expected_base_head=candidate["commit_receipt"]["base_head"],
        expected_head=head_sha,
    )
    effect = candidate.get("effect")
    if (
        not isinstance(effect, dict)
        or effect["kind"] != "publish"
        or effect["phase"] != "pr_created"
        or effect["active_lease_generation"]
        != candidate["lease"]["generation"]
        or effect["branch"] != branch
        or effect["head_sha"] != head_sha
        or effect["pr_url"] != pr_url
        or effect["validation_receipt"] != validation_receipt
    ):
        raise AutopilotError("publish_effect_evidence_missing")
    candidate["pull_request"] = {
        "url": pr_url,
        "branch": branch,
        "head_sha": head_sha,
        "validation_receipt": deepcopy(validation_receipt),
        "submitted_at": now,
    }
    candidate["decision"] = None
    candidate["result"] = None
    candidate["status"] = "review_pending"
    candidate["lease"] = None
    candidate["handoff"] = None
    candidate["effect"] = None
    candidate["stale_refresh"] = None
    candidate["stale_refresh_credit"] = None
    candidate["updated_at"] = now
    append_event(state, "candidate_submitted", now, candidate_id=candidate_id)


def validate_commit_execution_receipt(
    receipt: Any,
    *,
    intent_sha256: str,
    decodex_identity: dict[str, Any] | None,
    observed_at: int | None,
) -> None:
    if (
        not has_exact_keys(
            receipt,
            {
                "schema",
                "intent_sha256",
                "execution_mode",
                "decodex_version",
                "decodex_executable_sha256",
                "started_at",
                "completed_at",
                "stdout_sha256",
            },
        )
        or receipt.get("schema")
        != "decodex/codex-upstream-commit-execution/1"
        or receipt.get("intent_sha256") != intent_sha256
        or receipt.get("execution_mode") != "command_completed"
        or not isinstance(receipt.get("decodex_version"), str)
        or not 1 <= len(receipt["decodex_version"]) <= 256
        or "\n" in receipt["decodex_version"]
        or "\r" in receipt["decodex_version"]
        or not is_sha256(receipt.get("decodex_executable_sha256"))
        or not isinstance(receipt.get("started_at"), int)
        or not isinstance(receipt.get("completed_at"), int)
        or receipt["completed_at"] < receipt["started_at"]
        or not is_sha256(receipt.get("stdout_sha256"))
    ):
        raise AutopilotError("commit_execution_receipt_invalid")
    if decodex_identity is not None and (
        receipt["decodex_version"] != decodex_identity["version"]
        or receipt["decodex_executable_sha256"]
        != decodex_identity["executable_sha256"]
    ):
        raise AutopilotError("commit_execution_receipt_mismatch")
    if observed_at is not None and receipt["completed_at"] > observed_at:
        raise AutopilotError("commit_execution_receipt_mismatch")


def record_candidate_commit(
    state: dict[str, Any],
    *,
    candidate_id: str,
    token: str,
    base_head: str,
    head_sha: str,
    tree_sha: str,
    message_sha256: str,
    execution_receipt: dict[str, Any],
    now: int,
) -> None:
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, "maintainer", token, now)
    effect = candidate.get("effect")
    if (
        candidate["status"] != "implementing"
        or not isinstance(effect, dict)
        or effect["kind"] != "commit"
        or effect["phase"] != "prepared"
        or effect["active_lease_generation"]
        != candidate["lease"]["generation"]
        or effect["head_sha"] != base_head
        or any(
            SHA_PATTERN.fullmatch(value) is None
            for value in (base_head, head_sha, tree_sha)
        )
        or base_head == head_sha
        or not is_sha256(message_sha256)
    ):
        raise AutopilotError("candidate_commit_evidence_invalid")
    validate_commit_execution_receipt(
        execution_receipt,
        intent_sha256=effect["intent_sha256"],
        decodex_identity=effect["decodex_identity"],
        observed_at=now,
    )
    stale_refresh = candidate.get("stale_refresh")
    if isinstance(stale_refresh, dict) and (
        base_head != stale_refresh["target_base_head"]
    ):
        raise AutopilotError("candidate_commit_evidence_invalid")
    candidate["commit_receipt"] = {
        "base_head": base_head,
        "head_sha": head_sha,
        "tree_sha": tree_sha,
        "message_sha256": message_sha256,
        "intent_sha256": effect["intent_sha256"],
        "execution_receipt": deepcopy(execution_receipt),
        "execution_receipt_sha256": sha256_value(execution_receipt),
        "worker_handoff": deepcopy(effect["handoff_receipt"]),
        "committed_at": now,
    }
    candidate["stale_refresh"] = None
    candidate["stale_refresh_credit"] = None
    candidate["effect"] = None
    candidate["updated_at"] = now
    append_event(state, "candidate_committed", now, candidate_id=candidate_id)


def retire_candidate_pull_request(
    state: dict[str, Any],
    *,
    candidate_id: str,
    token: str,
    reason_code: str,
    receipt_sha256: str,
    now: int,
) -> None:
    if (
        REASON_PATTERN.fullmatch(reason_code) is None
        or not is_sha256(receipt_sha256)
    ):
        raise AutopilotError("pull_request_retirement_evidence_invalid")
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, "maintainer", token, now)
    if candidate["status"] != "implementing":
        raise AutopilotError("candidate_not_implementing")
    pull_request = candidate.get("pull_request")
    effect = candidate.get("effect")
    if (
        not isinstance(pull_request, dict)
        or not isinstance(effect, dict)
        or effect["kind"] != "retire_pr"
        or effect["phase"] != "prepared"
        or effect["active_lease_generation"]
        != candidate["lease"]["generation"]
        or effect["pr_url"] != pull_request["url"]
        or effect["head_sha"] != pull_request["head_sha"]
    ):
        raise AutopilotError("pull_request_retirement_effect_missing")
    candidate["retired_pull_requests"].append(
        {
            "url": pull_request["url"],
            "branch": pull_request["branch"],
            "head_sha": pull_request["head_sha"],
            "reason_code": reason_code,
            "receipt_sha256": receipt_sha256,
            "retired_at": now,
        }
    )
    candidate["pull_request"] = None
    candidate["effect"] = None
    candidate["result"] = None
    candidate["updated_at"] = now
    append_event(
        state,
        "pull_request_retired",
        now,
        candidate_id=candidate_id,
        reason_code=reason_code,
    )


def submit_decision(
    state: dict[str, Any],
    *,
    candidate_id: str,
    token: str,
    outcome: str,
    reason_code: str,
    maintainer_receipt: dict[str, Any],
    now: int,
) -> None:
    if outcome not in {"no_change", "rejected"}:
        raise AutopilotError("decision_outcome_invalid")
    if REASON_PATTERN.fullmatch(reason_code) is None:
        raise AutopilotError("reason_code_invalid")
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, "maintainer", token, now)
    if candidate["status"] != "implementing":
        raise AutopilotError("candidate_not_implementing")
    if candidate["contract_missing"]:
        raise AutopilotError("missing_contract_cannot_close")
    if candidate["kind"] == "automation_repair" and outcome == "rejected":
        raise AutopilotError("automation_repair_cannot_reject")
    if candidate.get("pull_request") is not None:
        raise AutopilotError("decision_has_pull_request")
    if candidate.get("effect") is not None:
        raise AutopilotError("decision_effect_unresolved")
    validate_validation_receipt(maintainer_receipt, role="maintainer")
    candidate["decision"] = {
        "outcome": outcome,
        "reason_code": reason_code,
        "maintainer_receipt": deepcopy(maintainer_receipt),
        "submitted_at": now,
    }
    candidate["result"] = None
    candidate["status"] = "review_pending"
    candidate["lease"] = None
    candidate["handoff"] = None
    candidate["updated_at"] = now
    append_event(state, "decision_submitted", now, candidate_id=candidate_id)


def reviewer_handoff_has_state_authority(
    candidate: dict[str, Any],
    reviewer_handoff: dict[str, Any],
) -> bool:
    lease = candidate.get("lease")
    if not isinstance(lease, dict):
        return False
    active_handoff = candidate.get("handoff")
    if (
        reviewer_handoff["claim_generation"] == lease["generation"]
        and isinstance(active_handoff, dict)
        and active_handoff.get("consumed") == reviewer_handoff
    ):
        return True
    effect = candidate.get("effect")
    return bool(
        isinstance(effect, dict)
        and effect.get("kind") == "land"
        and effect.get("handoff_receipt") == reviewer_handoff
        and reviewer_handoff["claim_generation"]
        == effect.get("lease_generation")
        and effect.get("active_lease_generation") == lease["generation"]
    )


def request_repair(
    state: dict[str, Any],
    *,
    candidate_id: str,
    token: str,
    finding_codes: Sequence[str],
    now: int,
    reviewer_handoff: dict[str, Any] | None = None,
    stale_target_base_head: str | None = None,
) -> None:
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, "reviewer", token, now)
    if candidate["status"] != "reviewing":
        raise AutopilotError("candidate_not_reviewing")
    codes = sorted(set(finding_codes))
    if not codes or len(codes) > 16 or any(
        REASON_PATTERN.fullmatch(value) is None for value in codes
    ):
        raise AutopilotError("finding_codes_invalid")
    stale_base = codes == ["base_stale"]
    if (
        ("base_stale" in codes and not stale_base)
        or (
            stale_base
            and (
                not isinstance(stale_target_base_head, str)
                or SHA_PATTERN.fullmatch(stale_target_base_head) is None
            )
        )
        or (not stale_base and stale_target_base_head is not None)
    ):
        raise AutopilotError("finding_codes_invalid")
    effect = candidate.get("effect")
    if effect is not None:
        if not (
            effect["kind"] == "land"
            and effect["phase"] == "prepared"
            and effect["command_receipt"] is None
            and effect["execution_receipt"] is None
        ):
            raise AutopilotError("repair_effect_not_reversible")
    validate_handoff_provenance(reviewer_handoff)
    allowed_disposition = (
        reviewer_handoff["disposition"] == "accept"
        if stale_base
        else reviewer_handoff["disposition"] == "request_repair"
    )
    if (
        reviewer_handoff["candidate_id"] != candidate_id
        or reviewer_handoff["role"] != "reviewer"
        or reviewer_handoff["action"] != "independent_review"
        or not allowed_disposition
        or (
            reviewer_handoff["disposition"] == "request_repair"
            and reviewer_handoff["finding_codes"] != codes
        )
        or (
            reviewer_handoff["disposition"] == "accept"
            and reviewer_handoff["finding_codes"]
        )
        or not reviewer_handoff_has_state_authority(
            candidate,
            reviewer_handoff,
        )
    ):
        raise AutopilotError("reviewer_handoff_receipt_invalid")
    proposal_receipt = proposal_validation_receipt(candidate)
    if not handoff_matches_validation_receipt(
        reviewer_handoff,
        proposal_receipt,
    ):
        raise AutopilotError("reviewer_handoff_receipt_invalid")
    if effect is not None:
        candidate["effect"] = None
    if stale_base:
        pull_request = candidate.get("pull_request")
        commit_receipt = candidate.get("commit_receipt")
        if (
            not isinstance(pull_request, dict)
            or candidate.get("decision") is not None
            or not isinstance(commit_receipt, dict)
            or commit_receipt.get("base_head")
            != pull_request["validation_receipt"]["base_head"]
            or commit_receipt.get("head_sha") != pull_request["head_sha"]
            or stale_target_base_head
            == pull_request["validation_receipt"]["base_head"]
        ):
            raise AutopilotError("stale_pull_request_refresh_invalid")
        candidate["commit_receipt"] = None
        candidate["stale_refresh"] = {
            "old_base_head": pull_request["validation_receipt"]["base_head"],
            "old_head_sha": pull_request["head_sha"],
            "target_base_head": stale_target_base_head,
            "prepared_at": now,
            "updated_at": now,
        }
        candidate["attempts"]["reviewer"] = max(
            0,
            candidate["attempts"]["reviewer"] - 1,
        )
        candidate["stale_refresh_credit"] = {
            "generation": None,
            "attempt_incremented": False,
        }
        append_event(
            state,
            "stale_pull_request_refresh_prepared",
            now,
            candidate_id=candidate_id,
            reason_code="base_stale",
        )
    else:
        candidate["stale_refresh"] = None
        candidate["stale_refresh_credit"] = None
    candidate["status"] = "repair_requested"
    candidate["lease"] = None
    candidate["handoff"] = None
    candidate["next_retry_at"] = None
    candidate["retry_role"] = None
    candidate["result"] = {
        "outcome": "repair_requested",
        "finding_codes": codes,
        "reviewer_handoff": deepcopy(reviewer_handoff),
        "at": now,
    }
    candidate["updated_at"] = now
    append_event(state, "repair_requested", now, candidate_id=candidate_id)


def requeue_stale_decision(
    state: dict[str, Any],
    *,
    candidate_id: str,
    token: str,
    current_main_head: str,
    now: int,
) -> None:
    if SHA_PATTERN.fullmatch(current_main_head) is None:
        raise AutopilotError("current_main_head_invalid")
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, "reviewer", token, now)
    decision = candidate.get("decision")
    active_handoff = candidate.get("handoff")
    if (
        candidate["status"] != "reviewing"
        or not isinstance(decision, dict)
        or candidate.get("pull_request") is not None
        or candidate.get("effect") is not None
    ):
        raise AutopilotError("stale_decision_requeue_invalid")
    receipt = decision["maintainer_receipt"]
    if (
        receipt["base_head"] == current_main_head
        and receipt["repository_head"] == current_main_head
    ):
        raise AutopilotError("decision_not_stale")
    candidate["decision"] = None
    candidate["attempts"]["reviewer"] = max(
        0,
        candidate["attempts"]["reviewer"] - 1,
    )
    candidate["status"] = "queued"
    candidate["lease"] = None
    candidate["handoff"] = None
    candidate["next_retry_at"] = None
    candidate["retry_role"] = None
    candidate["result"] = None
    candidate["updated_at"] = now
    append_event(
        state,
        "stale_decision_requeued",
        now,
        candidate_id=candidate_id,
        reason_code="base_stale",
    )


def prepare_pre_publish_stale_refresh(
    state: dict[str, Any],
    *,
    candidate_id: str,
    token: str,
    challenge_sha256: str,
    current_main_head: str,
    current_main_tree: str,
    commit_receipt_sha256: str,
    now: int,
) -> None:
    if (
        SHA_PATTERN.fullmatch(current_main_head) is None
        or SHA_PATTERN.fullmatch(current_main_tree) is None
        or not is_sha256(challenge_sha256)
        or not is_sha256(commit_receipt_sha256)
    ):
        raise AutopilotError("pre_publish_stale_refresh_invalid")
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, "maintainer", token, now)
    result = candidate.get("result")
    handoff = candidate.get("handoff")
    agent_run = (
        handoff.get("agent_run") if isinstance(handoff, dict) else None
    )
    commit_receipt = candidate.get("commit_receipt")
    prepared_old_commit_run = bool(
        isinstance(agent_run, dict)
        and agent_run.get("phase") == "prepared"
        and agent_run.get("role") == "maintainer"
        and agent_run.get("generation") == handoff.get("generation")
        and hmac.compare_digest(
            str(agent_run.get("challenge_sha256", "")),
            challenge_sha256,
        )
        and isinstance(commit_receipt, dict)
        and agent_run.get("base_head") == commit_receipt.get("base_head")
        and agent_run.get("input_head") == commit_receipt.get("head_sha")
        and agent_run.get("repository_head")
        == commit_receipt.get("base_head")
        and agent_run.get("input_tree") == commit_receipt.get("tree_sha")
    )
    if (
        candidate["status"] != "implementing"
        or candidate["attempts"]["maintainer"] < 2
        or not candidate_has_blocked_publish_validation(candidate)
        or not isinstance(commit_receipt, dict)
        or not hmac.compare_digest(
            sha256_value(commit_receipt),
            commit_receipt_sha256,
        )
        or result["at"] < commit_receipt["committed_at"]
        or current_main_head
        in {commit_receipt["base_head"], commit_receipt["head_sha"]}
        or not isinstance(handoff, dict)
        or handoff.get("role") != "maintainer"
        or handoff.get("generation") != candidate["lease"]["generation"]
        or handoff.get("consumed") is not None
        or not hmac.compare_digest(
            str(handoff.get("challenge_sha256", "")),
            challenge_sha256,
        )
        or candidate.get("pull_request") is not None
        or candidate.get("decision") is not None
        or candidate.get("effect") is not None
        or candidate.get("stale_refresh") is not None
        or candidate.get("stale_refresh_credit") is not None
        or (agent_run is not None and not prepared_old_commit_run)
    ):
        raise AutopilotError("pre_publish_stale_refresh_invalid")

    prepare_agent_run(
        state,
        candidate_id=candidate_id,
        role="maintainer",
        token=token,
        challenge_sha256=challenge_sha256,
        base_head=current_main_head,
        input_head=current_main_head,
        repository_head=current_main_head,
        input_tree=current_main_tree,
        now=now,
    )
    candidate["commit_receipt"] = None
    candidate["stale_refresh"] = {
        "old_base_head": commit_receipt["base_head"],
        "old_head_sha": commit_receipt["head_sha"],
        "target_base_head": current_main_head,
        "prepared_at": now,
        "updated_at": now,
    }
    append_event(
        state,
        "pre_publish_stale_refresh_prepared",
        now,
        candidate_id=candidate_id,
        reason_code=result["reason_code"],
    )


def prepare_stale_pull_request_refresh(
    state: dict[str, Any],
    *,
    candidate_id: str,
    token: str,
    current_main_head: str,
    now: int,
) -> dict[str, Any]:
    if SHA_PATTERN.fullmatch(current_main_head) is None:
        raise AutopilotError("current_main_head_invalid")
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, "maintainer", token, now)
    pull_request = candidate.get("pull_request")
    result = candidate.get("result")
    commit_receipt = candidate.get("commit_receipt")
    if (
        candidate["status"] != "implementing"
        or not isinstance(pull_request, dict)
        or not isinstance(result, dict)
        or result.get("outcome") != "repair_requested"
        or "base_stale" not in result.get("finding_codes", [])
        or candidate.get("decision") is not None
        or candidate.get("effect") is not None
        or pull_request["validation_receipt"]["base_head"]
        == current_main_head
    ):
        raise AutopilotError("stale_pull_request_refresh_invalid")
    if commit_receipt is not None and (
        not isinstance(commit_receipt, dict)
        or commit_receipt["head_sha"] != pull_request["head_sha"]
        or commit_receipt["base_head"]
        != pull_request["validation_receipt"]["base_head"]
        or commit_receipt["tree_sha"]
        != pull_request["validation_receipt"]["repository_tree"]
    ):
        raise AutopilotError("stale_pull_request_refresh_invalid")
    stale_refresh = candidate.get("stale_refresh")
    if stale_refresh is not None and (
        not has_exact_keys(stale_refresh, STALE_REFRESH_KEYS)
        or stale_refresh["old_base_head"]
        != pull_request["validation_receipt"]["base_head"]
        or stale_refresh["old_head_sha"] != pull_request["head_sha"]
    ):
        raise AutopilotError("stale_pull_request_refresh_invalid")
    previous_target = (
        None
        if stale_refresh is None
        else stale_refresh["target_base_head"]
    )
    changed = (
        commit_receipt is not None
        or stale_refresh is None
        or previous_target != current_main_head
    )
    if changed:
        if commit_receipt is not None:
            candidate["commit_receipt"] = None
        candidate["stale_refresh"] = {
            "old_base_head": pull_request["validation_receipt"]["base_head"],
            "old_head_sha": pull_request["head_sha"],
            "target_base_head": current_main_head,
            "prepared_at": (
                now if stale_refresh is None else stale_refresh["prepared_at"]
            ),
            "updated_at": now,
        }
        candidate["updated_at"] = now
        append_event(
            state,
            (
                "stale_pull_request_refresh_retargeted"
                if previous_target is not None
                and previous_target != current_main_head
                else "stale_pull_request_refresh_prepared"
            ),
            now,
            candidate_id=candidate_id,
            reason_code="base_stale",
        )
    return {
        "branch": pull_request["branch"],
        "pr_url": pull_request["url"],
        "old_base_head": pull_request["validation_receipt"]["base_head"],
        "old_head_sha": pull_request["head_sha"],
        "new_base_head": candidate["stale_refresh"]["target_base_head"],
        "prepared": commit_receipt is not None,
        "retargeted": (
            previous_target is not None
            and previous_target != current_main_head
        ),
    }


def advance_source_cursor(state: dict[str, Any]) -> None:
    source = state["source"]
    next_sequence = int(source["cursor_sequence"]) + 1
    by_sequence = {
        candidate["source_sequence"]: candidate
        for candidate in state["candidates"]
        if candidate.get("source_sequence") is not None
    }
    while True:
        candidate = by_sequence.get(next_sequence)
        if candidate is None or candidate["status"] not in TERMINAL_STATUSES:
            break
        source["cursor_sha"] = candidate["to_sha"]
        source["cursor_sequence"] = next_sequence
        next_sequence += 1


def resolve_candidate(
    state: dict[str, Any],
    *,
    candidate_id: str,
    role: str,
    token: str,
    outcome: str,
    reason_code: str,
    merge_sha: str | None,
    land_intent_sha256: str | None,
    land_execution_receipt_sha256: str | None,
    reviewer_receipt: dict[str, Any],
    now: int,
    reviewer_handoff: dict[str, Any] | None = None,
) -> None:
    if REASON_PATTERN.fullmatch(reason_code) is None:
        raise AutopilotError("reason_code_invalid")
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, role, token, now)
    allowed = {
        "maintainer": set(),
        "reviewer": {"landed", "no_change", "rejected"},
    }
    if role not in allowed:
        raise AutopilotError("candidate_role_invalid")
    if outcome not in allowed[role]:
        raise AutopilotError("outcome_not_authorized")
    if candidate["kind"] == "automation_repair" and outcome == "rejected":
        raise AutopilotError("automation_repair_cannot_reject")
    expected_status = "implementing" if role == "maintainer" else "reviewing"
    if candidate["status"] != expected_status:
        raise AutopilotError("candidate_role_status_mismatch")
    if outcome in {"no_change", "rejected"} and candidate["contract_missing"]:
        raise AutopilotError("missing_contract_cannot_close")
    validate_validation_receipt(reviewer_receipt, role="reviewer")
    expected_disposition = "accept" if outcome == "landed" else outcome
    try:
        validate_reviewer_handoff_semantics(
            candidate,
            reviewer_handoff,
            disposition=expected_disposition,
            finding_codes=[],
            receipt=reviewer_receipt,
        )
    except AutopilotError as error:
        raise AutopilotError("reviewer_handoff_receipt_invalid") from error
    if not reviewer_handoff_has_state_authority(candidate, reviewer_handoff):
        raise AutopilotError("reviewer_handoff_receipt_invalid")
    if outcome == "landed":
        if (
            candidate.get("pull_request") is None
            or merge_sha is None
            or not is_sha256(land_intent_sha256)
            or not is_sha256(land_execution_receipt_sha256)
        ):
            raise AutopilotError("landing_evidence_missing")
        if not SHA_PATTERN.fullmatch(merge_sha):
            raise AutopilotError("merge_sha_invalid")
        pull_request = candidate["pull_request"]
        if (
            reviewer_receipt["repository_head"] != pull_request["head_sha"]
            or reviewer_receipt["repository_tree"]
            != pull_request["validation_receipt"]["repository_tree"]
            or reviewer_receipt["base_head"]
            != pull_request["validation_receipt"]["base_head"]
        ):
            raise AutopilotError("candidate_review_receipt_mismatch")
        effect = candidate.get("effect")
        if (
            not isinstance(effect, dict)
            or effect["kind"] != "land"
            or effect["phase"] != "land_completed"
            or effect["active_lease_generation"]
            != candidate["lease"]["generation"]
            or effect["pr_url"] != pull_request["url"]
            or effect["head_sha"] != pull_request["head_sha"]
            or effect["validation_receipt"] != reviewer_receipt
            or effect["intent_sha256"] != land_intent_sha256
        ):
            raise AutopilotError("landing_effect_evidence_missing")
        terminal_execution_receipt = deepcopy(
            effect["execution_receipt"]
        )
        validate_land_execution_receipt(
            terminal_execution_receipt,
            intent_sha256=effect["intent_sha256"],
            merge_sha=merge_sha,
            decodex_identity=effect["decodex_identity"],
            intent_started_at=effect["started_at"],
            observed_at=effect["updated_at"],
        )
        if (
            sha256_value(terminal_execution_receipt)
            != land_execution_receipt_sha256
        ):
            raise AutopilotError("landing_effect_evidence_missing")
        decision_receipt_sha256 = None
    else:
        decision = candidate.get("decision")
        if (
            merge_sha is not None
            or land_intent_sha256 is not None
            or land_execution_receipt_sha256 is not None
            or not isinstance(decision, dict)
            or decision.get("outcome") != outcome
        ):
            raise AutopilotError("decision_evidence_missing")
        maintainer_receipt = decision["maintainer_receipt"]
        if (
            reviewer_receipt["repository_head"]
            != maintainer_receipt["repository_head"]
            or reviewer_receipt["repository_tree"]
            != maintainer_receipt["repository_tree"]
            or reviewer_receipt["base_head"] != maintainer_receipt["base_head"]
        ):
            raise AutopilotError("candidate_review_receipt_mismatch")
        decision_receipt_sha256 = sha256_value(decision)
        terminal_execution_receipt = None
    candidate["status"] = outcome
    candidate["lease"] = None
    candidate["handoff"] = None
    candidate["effect"] = None
    candidate["stale_refresh"] = None
    candidate["stale_refresh_credit"] = None
    candidate["result"] = {
        "outcome": outcome,
        "reason_code": reason_code,
        "merge_sha": merge_sha,
        "land_intent_sha256": land_intent_sha256,
        "land_execution_receipt": terminal_execution_receipt,
        "land_execution_receipt_sha256": land_execution_receipt_sha256,
        "decision_receipt_sha256": decision_receipt_sha256,
        "reviewer_receipt": deepcopy(reviewer_receipt),
        "reviewer_handoff": deepcopy(reviewer_handoff),
        "resolved_at": now,
    }
    candidate["updated_at"] = now
    append_event(
        state,
        "candidate_resolved",
        now,
        candidate_id=candidate_id,
        reason_code=reason_code,
    )
    record_terminal_metrics(
        state,
        outcome=outcome,
        lead_time_seconds=now - int(candidate["created_at"]),
        now=now,
    )
    if outcome in {"landed", "no_change"} and candidate.get("repair_of") is not None:
        repaired = find_candidate(state, candidate["repair_of"])
        if repaired["status"] in {"needs_attention", "repair_pending"}:
            repair_target_findings = preserved_finding_codes(repaired)
            blocked_role = repaired["retry_role"]
            resumed_role = blocked_role
            if resumed_role == "reviewer":
                if outcome == "landed" and repaired.get("decision") is not None:
                    resumed_role = "maintainer"
                    repaired["status"] = "queued"
                    repaired["attempts"]["maintainer"] = 0
                    repaired["decision"] = None
                    repaired["effect"] = None
                else:
                    repaired["status"] = "review_pending"
                    repaired["attempts"]["reviewer"] = 0
                    effect = repaired.get("effect")
                    if not (
                        isinstance(effect, dict)
                        and effect.get("kind") == "land"
                        and effect.get("phase") == "land_completed"
                    ):
                        repaired["effect"] = None
            elif resumed_role == "maintainer":
                repaired["status"] = "queued"
                repaired["attempts"]["maintainer"] = 0
                repaired["effect"] = None
            else:
                raise AutopilotError("repair_resume_role_missing")
            repaired["next_retry_at"] = None
            repaired["retry_role"] = None
            repaired["lease"] = None
            repaired["handoff"] = None
            repaired["stale_refresh"] = None
            repaired["stale_refresh_credit"] = None
            repaired["result"] = {
                "outcome": "automation_repair_resolved",
                "repair_candidate_id": candidate_id,
                "merge_sha": merge_sha,
                "repair_outcome": outcome,
                "blocked_role": blocked_role,
                "resumed_role": resumed_role,
                "finding_codes": repair_target_findings,
                "at": now,
            }
            repaired["updated_at"] = now
            append_event(
                state,
                "blocked_candidate_requeued",
                now,
                candidate_id=repaired["id"],
            )
    advance_source_cursor(state)


def block_candidate(
    state: dict[str, Any],
    policy: dict[str, Any],
    *,
    candidate_id: str,
    role: str,
    token: str,
    reason_code: str,
    error_digest: str,
    now: int,
) -> None:
    if REASON_PATTERN.fullmatch(reason_code) is None:
        raise AutopilotError("reason_code_invalid")
    if not re.fullmatch(r"[0-9a-f]{64}", error_digest):
        raise AutopilotError("error_digest_invalid")
    candidate = find_candidate(state, candidate_id)
    lease_matches(candidate, role, token, now)
    attempt = candidate["attempts"][role]
    stale_base_retry = bool(
        role == "maintainer"
        and isinstance(candidate.get("result"), dict)
        and candidate["result"].get("outcome") == "repair_requested"
        and candidate["result"].get("finding_codes") == ["base_stale"]
    )
    stale_credit = candidate.get("stale_refresh_credit")
    pre_publish_stale_refresh = bool(
        role == "maintainer"
        and candidate_has_pre_publish_stale_refresh(candidate)
    )
    if (
        isinstance(stale_credit, dict)
        and stale_credit.get("generation")
        == candidate["lease"]["generation"]
    ):
        candidate["stale_refresh_credit"] = None
    effect = candidate.get("effect")
    if (
        isinstance(effect, dict)
        and effect.get("kind") == "land"
        and effect.get("phase") == "prepared"
    ):
        candidate["effect"] = None
    candidate["lease"] = None
    candidate["handoff"] = None
    if pre_publish_stale_refresh:
        candidate["stale_refresh"] = None
    candidate["retry_role"] = role
    if not stale_base_retry:
        finding_codes = preserved_finding_codes(candidate)
        candidate["result"] = {
            "outcome": "blocked",
            "reason_code": reason_code,
            "error_digest": error_digest,
            "finding_codes": finding_codes,
            "at": now,
        }
    if external_effect_recovery_role(candidate) == role:
        candidate["status"] = "retry_wait"
        candidate["next_retry_at"] = now + retry_delay(policy, attempt)
    elif attempt >= int(policy["max_attempts"]):
        candidate["status"] = "needs_attention"
        candidate["next_retry_at"] = None
    else:
        candidate["status"] = "retry_wait"
        candidate["next_retry_at"] = now + retry_delay(policy, attempt)
    candidate["updated_at"] = now
    append_event(
        state,
        "candidate_blocked",
        now,
        candidate_id=candidate_id,
        reason_code=reason_code,
    )


def queue_needed_repairs(
    state: dict[str, Any],
    policy: dict[str, Any],
    *,
    repository_head: str,
    now: int,
) -> list[str]:
    queued: list[str] = []
    for blocked in list(state["candidates"]):
        if blocked["status"] != "needs_attention":
            continue
        if has_unresolved_external_effect(blocked):
            recovery_role = external_effect_recovery_role(blocked)
            if recovery_role is not None:
                blocked["status"] = "retry_wait"
                blocked["next_retry_at"] = now
                blocked["retry_role"] = recovery_role
                blocked["updated_at"] = now
                append_event(
                    state,
                    "external_effect_recovery_requeued",
                    now,
                    candidate_id=blocked["id"],
                )
            continue
        result = blocked.get("result")
        reason_code = result.get("reason_code") if isinstance(result, dict) else None
        if (
            not isinstance(reason_code, str)
            or REASON_PATTERN.fullmatch(reason_code) is None
        ):
            reason_code = "attempt_budget_exhausted"
        before = len(state["candidates"])
        repair = queue_automation_repair(
            state,
            policy,
            blocked_candidate_id=blocked["id"],
            reason_code=reason_code,
            repository_head=repository_head,
            now=now,
        )
        if len(state["candidates"]) > before:
            queued.append(repair["id"])
    return queued


def queue_effectiveness_improvements(
    state: dict[str, Any],
    policy: dict[str, Any],
    *,
    repository_head: str,
    now: int,
) -> list[str]:
    reason_code = effectiveness_improvement_reason(
        state,
        now=now,
    )
    if reason_code is None:
        return []
    if any(
        candidate["kind"] == "automation_repair"
        and candidate.get("repair_of") is None
        and candidate.get("path_summary", {}).get("reason_code") == reason_code
        and candidate["status"] not in TERMINAL_STATUSES
        for candidate in state["candidates"]
    ):
        return []
    before = len(state["candidates"])
    improvement = queue_automation_improvement(
        state,
        policy,
        reason_code=reason_code,
        repository_head=repository_head,
        now=now,
    )
    return [improvement["id"]] if len(state["candidates"]) > before else []


def state_health(
    state: dict[str, Any],
    mirror: Path | None,
    now: int,
    recovered: Sequence[str],
    queued_repairs: Sequence[str] = (),
    queued_improvements: Sequence[str] = (),
) -> dict[str, Any]:
    counts: dict[str, int] = {}
    for candidate in state["candidates"]:
        counts[candidate["status"]] = counts.get(candidate["status"], 0) + 1
    active = [
        candidate
        for candidate in state["candidates"]
        if candidate["status"] not in TERMINAL_STATUSES
    ]
    oldest_age = max((now - candidate["updated_at"] for candidate in active), default=0)
    stale_pull_requests = sorted(
        candidate["pull_request"]["url"]
        for candidate in active
        if isinstance(candidate.get("pull_request"), dict)
        and now - int(candidate["pull_request"]["submitted_at"]) > 21600
    )
    unresolved_external_effects = sorted(
        (
            {
                "candidate_id": candidate["id"],
                "kind": candidate["effect"]["kind"],
                "phase": candidate["effect"]["phase"],
            }
            for candidate in active
            if has_unresolved_external_effect(candidate)
        ),
        key=lambda value: value["candidate_id"],
    )
    observation_age = (
        None
        if state["last_observed_at"] is None
        else max(0, now - int(state["last_observed_at"]))
    )
    source = state["source"]
    lag_commits: int | None = None
    unqueued_commits: int | None = None
    if (
        mirror is not None
        and source["cursor_sha"] is not None
        and source["observed_head_sha"] is not None
    ):
        output = run_command(
            mirror_arguments(
                mirror,
                "rev-list",
                "--count",
                "--first-parent",
                f"{source['cursor_sha']}..{source['observed_head_sha']}",
            ),
            failure_code="upstream_lag_unavailable",
        )
        try:
            lag_commits = int(output)
        except ValueError as error:
            raise AutopilotError("upstream_lag_invalid") from error
    if (
        mirror is not None
        and source["queued_head_sha"] is not None
        and source["observed_head_sha"] is not None
    ):
        output = run_command(
            mirror_arguments(
                mirror,
                "rev-list",
                "--count",
                "--first-parent",
                f"{source['queued_head_sha']}..{source['observed_head_sha']}",
            ),
            failure_code="upstream_lag_unavailable",
        )
        try:
            unqueued_commits = int(output)
        except ValueError as error:
            raise AutopilotError("upstream_lag_invalid") from error
    blockers = []
    if observation_age is None or observation_age > 7200:
        blockers.append("observation_stale")
    if (state.get("local_build") or {}).get("contract_missing"):
        blockers.append("required_protocol_missing")
    if counts.get("needs_attention", 0):
        blockers.append("attempt_budget_exhausted")
    if unresolved_external_effects:
        blockers.append("external_effect_unresolved")
    if oldest_age > 21600:
        blockers.append("candidate_stale")
    if stale_pull_requests:
        blockers.append("pull_request_stale")
    if blockers:
        status = "blocked"
    elif (
        active
        or (lag_commits is not None and lag_commits > 0)
        or (unqueued_commits is not None and unqueued_commits > 0)
    ):
        status = "degraded"
    else:
        status = "pass"
    return {
        "schema": "decodex/codex-upstream-health/1",
        "status": status,
        "observed_at": now,
        "observation_age_seconds": observation_age,
        "source": {
            "observed_head_sha": source["observed_head_sha"],
            "queued_head_sha": source["queued_head_sha"],
            "cursor_sha": source["cursor_sha"],
            "cursor_contiguous": True,
            "lag_commits": lag_commits,
            "unqueued_commits": unqueued_commits,
            "stable_tag": source["stable_tag"],
            "prerelease_tag": source["prerelease_tag"],
        },
        "local_build": state["local_build"],
        "candidate_counts": dict(sorted(counts.items())),
        "oldest_nonterminal_age_seconds": oldest_age,
        "expired_leases_recovered": list(recovered),
        "automation_repairs_queued": list(queued_repairs),
        "automation_improvements_queued": list(queued_improvements),
        "open_pull_requests": sorted(
            candidate["pull_request"]["url"]
            for candidate in active
            if candidate.get("pull_request") is not None
        ),
        "stale_pull_requests": stale_pull_requests,
        "unresolved_external_effects": unresolved_external_effects,
        "blockers": blockers,
        "effectiveness": {
            "lifetime_outcome_classes": classify_lifetime_outcomes(state),
            "rolling_24_hours": rolling_effectiveness(
                state,
                now=now,
                window_seconds=86400,
            ),
            "rolling_7_days": rolling_effectiveness(
                state,
                now=now,
                window_seconds=604800,
            ),
        },
    }
