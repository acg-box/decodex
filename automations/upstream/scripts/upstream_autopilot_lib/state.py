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
from typing import Any, Iterator, Sequence

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
)
from .observation import mirror_arguments
from .validation import validate_validation_receipt
from .handoff import validate_handoff_provenance, validate_handoff_receipt


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
    lock_path = root / "state.lock"
    state_path = root / "state.json"
    if lock_path.exists() and lock_path.is_symlink():
        raise AutopilotError("state_lock_symlink")
    try:
        with lock_path.open("a+", encoding="utf-8") as lock:
            os.chmod(lock_path, 0o600)
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
            state = load_state(state_path)
            yield state, state_path
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
                ):
                    raise AutopilotError("candidate_handoff_invalid")
                seen_handoff_receipts.add(consumed["receipt_sha256"])
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
        if status in {"retry_wait", "needs_attention", "repair_pending"} and (
            not isinstance(candidate["result"], dict)
            or candidate["result"].get("outcome") != "blocked"
        ):
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
) -> dict[str, Any]:
    if reason_code not in {
        "content_loop_degraded",
        "lead_time_sla_missed",
        "live_configuration_drift",
        "repeated_blocked_attempts",
        "repeated_review_repairs",
    }:
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
    active = next(
        (
            candidate
            for candidate in state["candidates"]
            if candidate["kind"] == "automation_repair"
            and candidate.get("repair_of") is None
            and candidate.get("path_summary", {}).get("reason_code") == reason_code
            and candidate["status"] not in TERMINAL_STATUSES
        ),
        None,
    )
    if active is not None:
        return active
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
    if same_evidence is not None:
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
            if reason_code == "live_configuration_drift"
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


def lease_matches(candidate: dict[str, Any], role: str, token: str, now: int) -> None:
    lease = candidate.get("lease")
    if lease is None or lease.get("role") != role:
        raise AutopilotError("lease_missing")
    if lease["expires_at"] <= now:
        raise AutopilotError("lease_expired")
    actual = hashlib.sha256(token.encode("utf-8")).hexdigest()
    if not hmac.compare_digest(actual, lease["token_sha256"]):
        raise AutopilotError("lease_token_invalid")


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
        disposition=disposition,
        finding_codes=finding_codes,
        consumed_at=consumed_at,
    )
    existing = handoff.get("consumed")
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


def recover_expired_leases(
    state: dict[str, Any],
    policy: dict[str, Any],
    now: int,
) -> list[str]:
    recovered: list[str] = []
    for candidate in state["candidates"]:
        lease = candidate.get("lease")
        if lease is None or lease["expires_at"] > now:
            continue
        role = lease["role"]
        attempts = candidate["attempts"][role]
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
        if attempts >= int(policy["max_attempts"]):
            candidate["status"] = "needs_attention"
            candidate["next_retry_at"] = None
            candidate["retry_role"] = role
            candidate["result"] = {
                "outcome": "blocked",
                "reason_code": "lease_expired",
                "error_digest": sha256_value(
                    {"reason_code": "lease_expired", "role": role}
                ),
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
            candidate["result"] = {
                "outcome": "blocked",
                "reason_code": "lease_expired",
                "error_digest": sha256_value(
                    {"reason_code": "lease_expired", "role": role}
                ),
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


def claim_candidate(
    state: dict[str, Any],
    policy: dict[str, Any],
    role: str,
    now: int,
) -> dict[str, Any] | None:
    recover_expired_leases(state, policy, now)
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
    effect = candidate.get("effect")
    if isinstance(effect, dict):
        effect_role = (
            "reviewer" if effect.get("kind") == "land" else "maintainer"
        )
        if effect_role != role:
            raise AutopilotError("candidate_effect_role_mismatch")
    attempts = candidate["attempts"][role]
    if attempts >= int(policy["max_attempts"]):
        candidate["status"] = "needs_attention"
        candidate["next_retry_at"] = None
        candidate["retry_role"] = role
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
    candidate["handoff"] = {
        "role": role,
        "generation": generation,
        "challenge_sha256": challenge_sha256,
        "issued_at": now,
        "consumed": None,
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
        "issued_at": now,
        "consumed": False,
    }
    return {
        "candidate": public_candidate,
        "lease_token": raw_token,
        "handoff_challenge": raw_challenge,
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
    allowed_disposition = reviewer_handoff["disposition"] == "request_repair" or (
        reviewer_handoff["disposition"] == "accept" and codes == ["base_stale"]
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
            repaired["result"] = {
                "outcome": "automation_repair_resolved",
                "repair_candidate_id": candidate_id,
                "merge_sha": merge_sha,
                "repair_outcome": outcome,
                "blocked_role": blocked_role,
                "resumed_role": resumed_role,
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
    effect = candidate.get("effect")
    if (
        isinstance(effect, dict)
        and effect.get("kind") == "land"
        and effect.get("phase") == "prepared"
    ):
        candidate["effect"] = None
    candidate["lease"] = None
    candidate["handoff"] = None
    candidate["retry_role"] = role
    candidate["result"] = {
        "outcome": "blocked",
        "reason_code": reason_code,
        "error_digest": error_digest,
        "at": now,
    }
    if attempt >= int(policy["max_attempts"]):
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


def rolling_effectiveness(
    state: dict[str, Any],
    *,
    now: int,
    window_seconds: int,
) -> dict[str, Any]:
    cutoff = now - window_seconds
    coverage_start = cutoff - (cutoff % METRIC_BUCKET_SECONDS)
    buckets = [
        bucket
        for bucket in state["metrics"]["buckets"]
        if int(bucket["start"]) >= coverage_start
    ]
    outcome_counts = {
        outcome: sum(bucket["outcomes"][outcome] for bucket in buckets)
        for outcome in sorted(TERMINAL_STATUSES)
    }
    terminal_count = sum(outcome_counts.values())
    lead_time_count = sum(bucket["lead_time_count"] for bucket in buckets)
    lead_time_total = sum(
        bucket["lead_time_seconds_total"] for bucket in buckets
    )
    event_counts = {
        event: sum(bucket["events"][event] for bucket in buckets)
        for event in (
            "candidate_blocked",
            "repair_requested",
            "automation_repair_queued",
            "automation_improvement_queued",
        )
    }
    return {
        "window_seconds": window_seconds,
        "bucket_seconds": METRIC_BUCKET_SECONDS,
        "coverage_start": coverage_start,
        "terminal_count": terminal_count,
        "outcome_counts": outcome_counts,
        "landed_rate_basis_points": (
            outcome_counts["landed"] * 10_000 // terminal_count
            if terminal_count
            else None
        ),
        "average_lead_time_seconds": (
            lead_time_total // lead_time_count if lead_time_count else None
        ),
        "blocked_attempt_count": event_counts["candidate_blocked"],
        "repair_request_count": event_counts["repair_requested"],
        "automation_repair_queued_count": event_counts[
            "automation_repair_queued"
        ],
        "automation_improvement_queued_count": event_counts[
            "automation_improvement_queued"
        ],
    }


def queue_effectiveness_improvements(
    state: dict[str, Any],
    policy: dict[str, Any],
    *,
    repository_head: str,
    now: int,
) -> list[str]:
    metrics = rolling_effectiveness(
        state,
        now=now,
        window_seconds=604800,
    )
    reason_code: str | None = None
    if metrics["repair_request_count"] >= 2:
        reason_code = "repeated_review_repairs"
    elif metrics["blocked_attempt_count"] >= 3:
        reason_code = "repeated_blocked_attempts"
    elif (
        metrics["terminal_count"] >= 3
        and metrics["average_lead_time_seconds"] is not None
        and metrics["average_lead_time_seconds"] > 21600
    ):
        reason_code = "lead_time_sla_missed"
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
        "cost": {
            "x_api_calls": 0,
            "x_api_estimated_usd": 0,
            "github_api_calls": 0,
        },
    }
