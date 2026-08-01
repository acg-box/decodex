"""Evaluate bounded automation outcomes without persistence or external effects."""

from __future__ import annotations

import re
from typing import Any

from .core import METRIC_BUCKET_SECONDS, TERMINAL_STATUSES, is_sha256


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


def _has_validated_landed_diff(candidate: dict[str, Any]) -> bool:
    commit = candidate.get("commit_receipt")
    pull_request = candidate.get("pull_request")
    result = candidate.get("result")
    if not all(
        isinstance(value, dict)
        for value in (commit, pull_request, result)
    ):
        return False
    maintainer = pull_request.get("validation_receipt")
    reviewer = result.get("reviewer_receipt")
    if not isinstance(maintainer, dict) or not isinstance(reviewer, dict):
        return False
    base_head = commit.get("base_head")
    head_sha = commit.get("head_sha")
    tree_sha = commit.get("tree_sha")
    return bool(
        re.fullmatch(r"[0-9a-f]{40}", str(base_head)) is not None
        and re.fullmatch(r"[0-9a-f]{40}", str(head_sha)) is not None
        and re.fullmatch(r"[0-9a-f]{40}", str(tree_sha)) is not None
        and base_head != head_sha
        and pull_request.get("head_sha") == head_sha
        and maintainer.get("base_head") == base_head
        and maintainer.get("repository_head") == head_sha
        and maintainer.get("repository_tree") == tree_sha
        and reviewer.get("base_head") == base_head
        and reviewer.get("repository_head") == head_sha
        and reviewer.get("repository_tree") == tree_sha
        and re.fullmatch(
            r"[0-9a-f]{40}",
            str(result.get("merge_sha")),
        )
        is not None
        and is_sha256(result.get("land_intent_sha256"))
        and is_sha256(result.get("land_execution_receipt_sha256"))
    )


def classify_lifetime_outcomes(state: dict[str, Any]) -> dict[str, int]:
    """Separate real contract adaptation from assessment-only activity."""

    contract_adaptation_landed_count = 0
    automation_repair_landed_count = 0
    assessment_only_landed_count = 0
    validated_no_change_count = 0
    validated_rejected_count = 0
    active_contract_gap_count = 0
    for candidate in state["candidates"]:
        if (
            candidate["status"] not in TERMINAL_STATUSES
            and candidate["contract_missing"]
        ):
            active_contract_gap_count = active_contract_gap_count + 1
        result = candidate.get("result")
        if not isinstance(result, dict):
            continue
        outcome = result.get("outcome")
        if outcome == "no_change":
            validated_no_change_count = validated_no_change_count + 1
        elif outcome == "rejected":
            validated_rejected_count = validated_rejected_count + 1
        elif outcome == "landed":
            if candidate["kind"] == "automation_repair":
                automation_repair_landed_count = (
                    automation_repair_landed_count + 1
                )
            elif (
                candidate["contract_missing"]
                and _has_validated_landed_diff(candidate)
            ):
                contract_adaptation_landed_count = (
                    contract_adaptation_landed_count + 1
                )
            else:
                assessment_only_landed_count = (
                    assessment_only_landed_count + 1
                )
    return {
        "contract_adaptation_landed_count": (
            contract_adaptation_landed_count
        ),
        "automation_repair_landed_count": automation_repair_landed_count,
        "assessment_only_landed_count": assessment_only_landed_count,
        "validated_no_change_count": validated_no_change_count,
        "validated_rejected_count": validated_rejected_count,
        "active_contract_gap_count": active_contract_gap_count,
    }


def effectiveness_improvement_reason(
    state: dict[str, Any],
    *,
    now: int,
) -> str | None:
    metrics = rolling_effectiveness(
        state,
        now=now,
        window_seconds=604800,
    )
    assessment_only_landings = sum(
        candidate["status"] == "landed"
        and isinstance(candidate.get("result"), dict)
        and candidate["result"].get("outcome") == "landed"
        and int(candidate["result"].get("resolved_at") or 0)
        >= now - 604800
        and not (
            candidate["contract_missing"]
            and _has_validated_landed_diff(candidate)
        )
        and candidate["kind"] != "automation_repair"
        for candidate in state["candidates"]
    )
    if assessment_only_landings >= 2:
        return "assessment_only_churn"
    if metrics["repair_request_count"] >= 2:
        return "repeated_review_repairs"
    if (
        metrics["terminal_count"] >= 3
        and metrics["average_lead_time_seconds"] is not None
        and metrics["average_lead_time_seconds"] > 21600
    ):
        return "lead_time_sla_missed"
    return None
