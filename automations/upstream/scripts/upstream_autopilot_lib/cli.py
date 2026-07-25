"""Command-line orchestration for the Codex upstream autopilot."""

from __future__ import annotations

import argparse
from copy import deepcopy
import json
from pathlib import Path
from typing import Any

from . import (
    DEFAULT_POLICY_PATH,
    LAND_EFFECT_LEASE_BUDGET_SECONDS,
    REPO_ROOT,
    RESULT_SCHEMA,
    SIDE_EFFECT_LEASE_BUDGET_SECONDS,
    VALIDATION_LEASE_BUDGET_SECONDS,
    AutopilotError,
    advance_effect_phase,
    apply_observation,
    assert_candidate_commit_worktree,
    assert_candidate_worktree,
    assert_detached_review_worktree,
    assert_primary_clean_main,
    assert_primary_snapshot,
    atomic_write_json,
    begin_observation,
    block_candidate,
    check_lease_budget,
    classify_commit_entry,
    classify_land_entry,
    claim_candidate,
    collect_observation,
    commit_execution_receipt,
    decodex_identity,
    ensure_lease_budget,
    ensure_cache_root,
    ensure_remote_branch,
    find_candidate,
    find_or_create_pull_request,
    load_policy,
    land_execution_receipt,
    land_command_receipt,
    locked_state,
    managed_worktree_identity,
    observation_session_lock,
    prepare_effect,
    prepare_observation_plan,
    pull_request_readback,
    refresh_primary_snapshot,
    queue_automation_improvement,
    queue_automation_repair,
    queue_effectiveness_improvements,
    queue_needed_repairs,
    record_land_execution,
    record_land_command_execution,
    record_candidate_commit,
    referenced_schema_evidence,
    requeue_stale_decision,
    repository_identity,
    request_repair,
    resolve_candidate,
    resolve_primary_checkout,
    retire_candidate_pull_request,
    retire_pull_request,
    rewind_recorded_candidate_commit,
    rewind_unrecorded_decodex_commit,
    recover_expired_leases,
    recover_started_land_readback,
    remote_branch_head,
    renew_lease,
    run_command,
    run_decodex_commit,
    run_decodex_land,
    run_validation_profiles,
    save_state,
    sha256_value,
    state_health,
    submit_candidate,
    submit_decision,
    utc_now,
    validation_authority_identity,
    validation_receipt_is_current,
    verify_decodex_commit,
    verify_landed_change_record,
    verify_merge_parents,
    verify_merged_pull_request,
    verify_open_pull_request,
    verify_remote_main_contains,
)


def result_payload(status: str, **fields: Any) -> dict[str, Any]:
    return {"schema": RESULT_SCHEMA, "status": status, **fields}


def save_state_guarded(
    state: dict[str, Any],
    state_path: Path,
    now: int,
    *,
    repo_root: Path,
    policy: dict[str, Any],
    expected_head: str,
) -> None:
    assert_primary_snapshot(repo_root, policy, expected_head)
    save_state(state, state_path, now)


def add_common_state_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--json", action="store_true")


def add_leased_candidate_arguments(
    parser: argparse.ArgumentParser,
    *,
    role: bool = False,
    worktree: bool = False,
) -> None:
    add_common_state_arguments(parser)
    parser.add_argument("--candidate-id", required=True)
    if role:
        parser.add_argument("--role", choices=("maintainer", "reviewer"), required=True)
    parser.add_argument("--lease-token", required=True)
    if worktree:
        parser.add_argument("--worktree", type=Path, required=True)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    observe = subparsers.add_parser("observe")
    add_common_state_arguments(observe)

    claim = subparsers.add_parser("claim")
    add_common_state_arguments(claim)
    claim.add_argument("--role", choices=("maintainer", "reviewer"), required=True)

    renew = subparsers.add_parser("renew")
    add_leased_candidate_arguments(renew, role=True)

    commit = subparsers.add_parser("commit-candidate")
    add_leased_candidate_arguments(commit, worktree=True)

    publish = subparsers.add_parser("publish")
    add_leased_candidate_arguments(publish, worktree=True)

    retire = subparsers.add_parser("retire-pr")
    add_leased_candidate_arguments(retire, worktree=True)
    retire.add_argument("--reason-code", required=True)

    decision = subparsers.add_parser("submit-decision")
    add_leased_candidate_arguments(decision)
    decision.add_argument("--outcome", choices=("no_change", "rejected"), required=True)
    decision.add_argument("--reason-code", required=True)

    repair = subparsers.add_parser("request-repair")
    add_leased_candidate_arguments(repair)
    repair.add_argument("--finding-code", action="append", required=True)

    resolve = subparsers.add_parser("resolve-decision")
    add_leased_candidate_arguments(resolve, worktree=True)
    resolve.add_argument("--outcome", choices=("no_change", "rejected"), required=True)
    resolve.add_argument("--reason-code", required=True)

    land = subparsers.add_parser("land")
    add_leased_candidate_arguments(land, worktree=True)

    blocked = subparsers.add_parser("block")
    add_leased_candidate_arguments(blocked, role=True)
    blocked.add_argument("--reason-code", required=True)
    blocked.add_argument("--error-digest", required=True)

    escalate = subparsers.add_parser("escalate-repair")
    add_common_state_arguments(escalate)
    escalate.add_argument("--candidate-id", required=True)
    escalate.add_argument("--reason-code", required=True)

    improve = subparsers.add_parser("queue-improvement")
    add_common_state_arguments(improve)
    improve.add_argument(
        "--reason-code",
        choices=(
            "lead_time_sla_missed",
            "live_configuration_drift",
            "repeated_blocked_attempts",
            "repeated_review_repairs",
        ),
        required=True,
    )

    health = subparsers.add_parser("health")
    add_common_state_arguments(health)
    health.add_argument("--repair-expired", action="store_true")
    health.add_argument("--queue-repairs", action="store_true")
    health.add_argument("--queue-improvements", action="store_true")

    snapshot = subparsers.add_parser("snapshot")
    add_common_state_arguments(snapshot)

    return parser.parse_args()


def execute(args: argparse.Namespace) -> dict[str, Any]:
    repo_root = resolve_primary_checkout(REPO_ROOT, "main")
    if REPO_ROOT.resolve() != repo_root:
        raise AutopilotError("state_tool_not_primary_authority")
    policy_path = repo_root / "automations/upstream/policy.json"
    if DEFAULT_POLICY_PATH.resolve() != policy_path.resolve():
        raise AutopilotError("policy_path_not_primary_authority")
    policy = load_policy(policy_path)
    cache_root = repo_root / ".agent/automations/upstream/cache"
    for protected_root in (
        repo_root / ".agent",
        repo_root / ".agent/automations",
        repo_root / ".agent/automations/upstream",
        cache_root,
    ):
        if protected_root.exists() and protected_root.is_symlink():
            raise AutopilotError("cache_root_symlink")
    preflight = assert_primary_clean_main(repo_root, policy)

    def persist(
        state: dict[str, Any],
        state_path: Path,
        *,
        expected_head: str = preflight["head"],
        at: int | None = None,
    ) -> None:
        save_state_guarded(
            state,
            state_path,
            utc_now() if at is None else at,
            repo_root=repo_root,
            policy=policy,
            expected_head=expected_head,
        )

    def reserve_lease_budget(
        *,
        candidate_id: str,
        role: str,
        token: str,
        minimum_seconds: int,
    ) -> int:
        now = utc_now()
        with locked_state(cache_root) as (state, state_path):
            expires_at = ensure_lease_budget(
                state,
                policy,
                candidate_id=candidate_id,
                role=role,
                token=token,
                minimum_seconds=minimum_seconds,
                now=now,
            )
            persist(state, state_path, at=now)
        return expires_at

    if args.command == "observe":
        with observation_session_lock(cache_root):
            started_at = utc_now()
            with locked_state(cache_root) as (state, state_path):
                generation = begin_observation(state, started_at)
                persist(state, state_path, at=started_at)
                snapshot = deepcopy(state)
                retained = referenced_schema_evidence(state)
            observation, mirror = collect_observation(
                cache_root,
                policy,
                "codex",
                retained_evidence=retained,
            )
            commits, references, summaries = prepare_observation_plan(
                snapshot,
                policy,
                observation,
                mirror,
            )
            completed_at = utc_now()
            with locked_state(cache_root) as (state, state_path):
                queued = apply_observation(
                    state,
                    policy,
                    observation,
                    now=completed_at,
                    observation_generation=generation,
                    commits=commits,
                    reference_observations=references,
                    path_summaries=summaries,
                )
                persist(state, state_path, at=completed_at)
                queued_head_sha = state["source"]["queued_head_sha"]
        return result_payload(
            "observed",
            preflight={
                "branch": preflight["branch"],
                "head": preflight["head"],
                "dirty": False,
                "primary_checkout": True,
            },
            observation_generation=generation,
            upstream_head_sha=observation.upstream_head_sha,
            queued_head_sha=queued_head_sha,
            stable_tag=observation.stable_tag,
            stable_tag_sha=observation.stable_tag_sha,
            prerelease_tag=observation.prerelease_tag,
            prerelease_tag_sha=observation.prerelease_tag_sha,
            codex_version=observation.codex_version,
            codex_executable_sha256=observation.codex_executable_sha256,
            contract_missing=observation.contract_missing,
            queued_candidate_ids=queued,
            cost={"x_api_calls": 0, "x_api_estimated_usd": 0, "github_api_calls": 0},
        )

    if args.command == "claim":
        now = utc_now()
        with locked_state(cache_root) as (state, state_path):
            claimed = claim_candidate(state, policy, args.role, now)
            persist(state, state_path, at=now)
        if claimed is None:
            return result_payload("no_candidate", role=args.role)
        if "busy" in claimed:
            return result_payload("role_busy", role=args.role, **claimed)
        return result_payload("claimed", role=args.role, **claimed)

    if args.command == "renew":
        now = utc_now()
        with locked_state(cache_root) as (state, state_path):
            expires_at = renew_lease(
                state,
                policy,
                candidate_id=args.candidate_id,
                role=args.role,
                token=args.lease_token,
                now=now,
            )
            persist(state, state_path, at=now)
        return result_payload(
            "lease_renewed",
            candidate_id=args.candidate_id,
            role=args.role,
            lease_expires_at=expires_at,
        )

    if args.command == "commit-candidate":
        with locked_state(cache_root) as (state, _state_path):
            candidate = deepcopy(find_candidate(state, args.candidate_id))
        effect = candidate.get("effect")
        if isinstance(effect, dict) and effect.get("kind") == "commit":
            base_head = effect["head_sha"]
            commit_decodex_identity = effect["decodex_identity"]
        elif isinstance(candidate.get("commit_receipt"), dict):
            base_head = candidate["commit_receipt"]["base_head"]
            _decodex_path, commit_decodex_identity = decodex_identity()
        else:
            base_head = assert_candidate_commit_worktree(
                repo_root,
                args.worktree,
                policy,
                branch=candidate["branch_name"],
            )
            _decodex_path, commit_decodex_identity = decodex_identity()
        with locked_state(cache_root) as (state, state_path):
            candidate = find_candidate(state, args.candidate_id)
            current_head = run_command(
                ["git", "rev-parse", "HEAD"],
                cwd=args.worktree,
                failure_code="candidate_worktree_unavailable",
            )
            if current_head == base_head:
                assert_candidate_commit_worktree(
                    repo_root,
                    args.worktree,
                    policy,
                    branch=candidate["branch_name"],
                )
            elif (
                isinstance(candidate.get("commit_receipt"), dict)
                and current_head == candidate["commit_receipt"]["head_sha"]
            ):
                assert_candidate_commit_worktree(
                    repo_root,
                    args.worktree,
                    policy,
                    branch=candidate["branch_name"],
                )
                allowed_remote_heads = {
                    None,
                    base_head,
                }
                existing_pr = candidate.get("pull_request")
                if isinstance(existing_pr, dict):
                    if (
                        existing_pr["branch"] != candidate["branch_name"]
                        or existing_pr["head_sha"]
                        != candidate["commit_receipt"]["head_sha"]
                    ):
                        raise AutopilotError(
                            "recorded_commit_remote_conflict"
                        )
                    allowed_remote_heads.add(existing_pr["head_sha"])
                rewind_recorded_candidate_commit(
                    args.worktree,
                    candidate_id=args.candidate_id,
                    branch=candidate["branch_name"],
                    commit_receipt=candidate["commit_receipt"],
                    allowed_remote_heads=allowed_remote_heads,
                )
                current_head = assert_candidate_commit_worktree(
                    repo_root,
                    args.worktree,
                    policy,
                    branch=candidate["branch_name"],
                )
            else:
                assert_candidate_worktree(
                    repo_root,
                    args.worktree,
                    policy,
                    branch=candidate["branch_name"],
                    head_sha=current_head,
                )
                rewind_unrecorded_decodex_commit(
                    args.worktree,
                    candidate_id=args.candidate_id,
                    branch=candidate["branch_name"],
                    base_head=base_head,
                )
                current_head = assert_candidate_commit_worktree(
                    repo_root,
                    args.worktree,
                    policy,
                    branch=candidate["branch_name"],
                )
            classify_commit_entry(current_head, base_head)
            now = utc_now()
            ensure_lease_budget(
                state,
                policy,
                candidate_id=args.candidate_id,
                role="maintainer",
                token=args.lease_token,
                minimum_seconds=SIDE_EFFECT_LEASE_BUDGET_SECONDS,
                now=now,
            )
            effect = prepare_effect(
                state,
                policy,
                candidate_id=args.candidate_id,
                role="maintainer",
                token=args.lease_token,
                kind="commit",
                branch=candidate["branch_name"],
                head_sha=base_head,
                pr_url=None,
                decodex_identity=commit_decodex_identity,
                now=now,
            )
            persist(state, state_path, at=now)
            process_evidence = run_decodex_commit(
                args.worktree,
                candidate_id=args.candidate_id,
                expected_identity=commit_decodex_identity,
            )
            evidence = verify_decodex_commit(
                args.worktree,
                candidate_id=args.candidate_id,
                base_head=base_head,
            )
            execution_receipt = commit_execution_receipt(
                intent_sha256=effect["intent_sha256"],
                process_evidence=process_evidence,
            )
            completed_at = utc_now()
            record_candidate_commit(
                state,
                candidate_id=args.candidate_id,
                token=args.lease_token,
                base_head=base_head,
                head_sha=evidence["head_sha"],
                tree_sha=evidence["tree_sha"],
                message_sha256=evidence["message_sha256"],
                execution_receipt=execution_receipt,
                now=completed_at,
            )
            persist(state, state_path, at=completed_at)
        return result_payload(
            "committed",
            candidate_id=args.candidate_id,
            head_sha=evidence["head_sha"],
            tree_sha=evidence["tree_sha"],
        )

    if args.command == "publish":
        reserve_lease_budget(
            candidate_id=args.candidate_id,
            role="maintainer",
            token=args.lease_token,
            minimum_seconds=VALIDATION_LEASE_BUDGET_SECONDS,
        )
        with locked_state(cache_root) as (state, _state_path):
            candidate = deepcopy(find_candidate(state, args.candidate_id))
        commit_receipt = candidate.get("commit_receipt")
        if not isinstance(commit_receipt, dict):
            raise AutopilotError("candidate_commit_receipt_missing")
        existing_effect = candidate.get("effect")
        if (
            isinstance(existing_effect, dict)
            and existing_effect.get("kind") == "publish"
        ):
            remote_head_before = existing_effect["remote_head_before"]
        else:
            remote_head_before = remote_branch_head(
                args.worktree,
                candidate["branch_name"],
            )
            allowed_remote_heads = {
                None,
                commit_receipt["base_head"],
            }
            existing_pr = candidate.get("pull_request")
            if isinstance(existing_pr, dict):
                allowed_remote_heads = {existing_pr["head_sha"]}
            if remote_head_before not in allowed_remote_heads:
                raise AutopilotError("remote_branch_conflict")
        tree = assert_candidate_worktree(
            repo_root,
            args.worktree,
            policy,
            branch=candidate["branch_name"],
            head_sha=commit_receipt["head_sha"],
        )
        if tree != commit_receipt["tree_sha"]:
            raise AutopilotError("candidate_commit_receipt_mismatch")
        validation_receipt = run_validation_profiles(
            repo_root,
            args.worktree,
            policy,
            role="maintainer",
            candidate_kind=candidate["kind"],
            base_head=commit_receipt["base_head"],
            expected_head=commit_receipt["head_sha"],
        )
        with locked_state(cache_root) as (state, state_path):
            candidate = find_candidate(state, args.candidate_id)
            existing_pr = candidate.get("pull_request")
            pr_url = existing_pr["url"] if isinstance(existing_pr, dict) else None
            now = utc_now()
            ensure_lease_budget(
                state,
                policy,
                candidate_id=args.candidate_id,
                role="maintainer",
                token=args.lease_token,
                minimum_seconds=SIDE_EFFECT_LEASE_BUDGET_SECONDS,
                now=now,
            )
            effect = prepare_effect(
                state,
                policy,
                candidate_id=args.candidate_id,
                role="maintainer",
                token=args.lease_token,
                kind="publish",
                branch=candidate["branch_name"],
                head_sha=commit_receipt["head_sha"],
                pr_url=pr_url,
                remote_head_before=remote_head_before,
                validation_receipt=validation_receipt,
                now=now,
            )
            persist(state, state_path, at=now)
            ensure_remote_branch(
                args.worktree,
                branch=candidate["branch_name"],
                head_sha=commit_receipt["head_sha"],
                expected_remote_head=effect["remote_head_before"],
            )
            if effect["phase"] == "prepared":
                pushed_at = utc_now()
                advance_effect_phase(
                    state,
                    candidate_id=args.candidate_id,
                    role="maintainer",
                    token=args.lease_token,
                    phase="pushed",
                    now=pushed_at,
                )
                persist(state, state_path, at=pushed_at)
            pr_url = find_or_create_pull_request(
                args.worktree,
                policy,
                candidate,
                head_sha=commit_receipt["head_sha"],
            )
            submitted_at = utc_now()
            advance_effect_phase(
                state,
                candidate_id=args.candidate_id,
                role="maintainer",
                token=args.lease_token,
                phase="pr_created",
                pr_url=pr_url,
                now=submitted_at,
            )
            persist(state, state_path, at=submitted_at)
            submit_candidate(
                state,
                policy,
                candidate_id=args.candidate_id,
                token=args.lease_token,
                branch=candidate["branch_name"],
                head_sha=commit_receipt["head_sha"],
                pr_url=pr_url,
                validation_receipt=validation_receipt,
                now=submitted_at,
            )
            persist(state, state_path, at=submitted_at)
        return result_payload(
            "review_pending",
            candidate_id=args.candidate_id,
            pr_url=pr_url,
            cost={"github_api_calls": 4, "x_api_calls": 0, "x_api_estimated_usd": 0},
        )

    if args.command == "retire-pr":
        with locked_state(cache_root) as (state, _state_path):
            candidate = deepcopy(find_candidate(state, args.candidate_id))
        pull_request = candidate.get("pull_request")
        if not isinstance(pull_request, dict):
            raise AutopilotError("pull_request_retirement_evidence_invalid")
        assert_candidate_worktree(
            repo_root,
            args.worktree,
            policy,
            branch=pull_request["branch"],
            head_sha=pull_request["head_sha"],
        )
        with locked_state(cache_root) as (state, state_path):
            now = utc_now()
            ensure_lease_budget(
                state,
                policy,
                candidate_id=args.candidate_id,
                role="maintainer",
                token=args.lease_token,
                minimum_seconds=SIDE_EFFECT_LEASE_BUDGET_SECONDS,
                now=now,
            )
            prepare_effect(
                state,
                policy,
                candidate_id=args.candidate_id,
                role="maintainer",
                token=args.lease_token,
                kind="retire_pr",
                branch=pull_request["branch"],
                head_sha=pull_request["head_sha"],
                pr_url=pull_request["url"],
                now=now,
            )
            persist(state, state_path, at=now)
            receipt_sha256 = retire_pull_request(
                args.worktree,
                policy,
                candidate_id=args.candidate_id,
                pr_url=pull_request["url"],
                branch=pull_request["branch"],
                base_head=pull_request["validation_receipt"]["base_head"],
                head_sha=pull_request["head_sha"],
            )
            retired_at = utc_now()
            retire_candidate_pull_request(
                state,
                candidate_id=args.candidate_id,
                token=args.lease_token,
                reason_code=args.reason_code,
                receipt_sha256=receipt_sha256,
                now=retired_at,
            )
            persist(state, state_path, at=retired_at)
        return result_payload(
            "pull_request_retired",
            candidate_id=args.candidate_id,
            pr_url=pull_request["url"],
            cost={"github_api_calls": 4, "x_api_calls": 0, "x_api_estimated_usd": 0},
        )

    if args.command == "submit-decision":
        reserve_lease_budget(
            candidate_id=args.candidate_id,
            role="maintainer",
            token=args.lease_token,
            minimum_seconds=VALIDATION_LEASE_BUDGET_SECONDS,
        )
        with locked_state(cache_root) as (state, _state_path):
            candidate = deepcopy(find_candidate(state, args.candidate_id))
        maintainer_receipt = run_validation_profiles(
            repo_root,
            repo_root,
            policy,
            role="maintainer",
            candidate_kind=candidate["kind"],
            base_head=preflight["head"],
            expected_head=preflight["head"],
        )
        now = utc_now()
        with locked_state(cache_root) as (state, state_path):
            refresh_primary_snapshot(
                repo_root,
                policy,
                maintainer_receipt["base_head"],
            )
            if not validation_receipt_is_current(
                maintainer_receipt,
                current_main_head=maintainer_receipt["base_head"],
                current_authority=validation_authority_identity(repo_root),
            ):
                raise AutopilotError("validation_authority_changed")
            ensure_lease_budget(
                state,
                policy,
                candidate_id=args.candidate_id,
                role="maintainer",
                token=args.lease_token,
                minimum_seconds=SIDE_EFFECT_LEASE_BUDGET_SECONDS,
                now=now,
            )
            submit_decision(
                state,
                candidate_id=args.candidate_id,
                token=args.lease_token,
                outcome=args.outcome,
                reason_code=args.reason_code,
                maintainer_receipt=maintainer_receipt,
                now=now,
            )
            persist(state, state_path, at=now)
        return result_payload(
            "review_pending",
            candidate_id=args.candidate_id,
            decision=args.outcome,
            validation_receipt_sha256=sha256_value(maintainer_receipt),
        )

    if args.command == "request-repair":
        now = utc_now()
        with locked_state(cache_root) as (state, state_path):
            ensure_lease_budget(
                state,
                policy,
                candidate_id=args.candidate_id,
                role="reviewer",
                token=args.lease_token,
                minimum_seconds=SIDE_EFFECT_LEASE_BUDGET_SECONDS,
                now=now,
            )
            request_repair(
                state,
                candidate_id=args.candidate_id,
                token=args.lease_token,
                finding_codes=args.finding_code,
                now=now,
            )
            persist(state, state_path, at=now)
        return result_payload("repair_requested", candidate_id=args.candidate_id)

    if args.command == "resolve-decision":
        with locked_state(cache_root) as (state, _state_path):
            candidate = deepcopy(find_candidate(state, args.candidate_id))
        decision = candidate.get("decision")
        if not isinstance(decision, dict) or decision.get("outcome") != args.outcome:
            raise AutopilotError("decision_evidence_missing")
        maintainer_receipt = decision["maintainer_receipt"]
        current_authority = validation_authority_identity(repo_root)
        if (
            maintainer_receipt["repository_head"] != preflight["head"]
            or not validation_receipt_is_current(
                maintainer_receipt,
                current_main_head=preflight["head"],
                current_authority=current_authority,
            )
        ):
            now = utc_now()
            with locked_state(cache_root) as (state, state_path):
                ensure_lease_budget(
                    state,
                    policy,
                    candidate_id=args.candidate_id,
                    role="reviewer",
                    token=args.lease_token,
                    minimum_seconds=SIDE_EFFECT_LEASE_BUDGET_SECONDS,
                    now=now,
                )
                requeue_stale_decision(
                    state,
                    candidate_id=args.candidate_id,
                    token=args.lease_token,
                    current_main_head=preflight["head"],
                    now=now,
                )
                persist(state, state_path, at=now)
            return result_payload(
                "stale_decision_requeued",
                candidate_id=args.candidate_id,
                reason_code="base_stale",
            )
        reserve_lease_budget(
            candidate_id=args.candidate_id,
            role="reviewer",
            token=args.lease_token,
            minimum_seconds=VALIDATION_LEASE_BUDGET_SECONDS,
        )
        review_tree = assert_detached_review_worktree(
            repo_root,
            args.worktree,
            policy,
            head_sha=maintainer_receipt["repository_head"],
        )
        if review_tree != maintainer_receipt["repository_tree"]:
            raise AutopilotError("decision_review_worktree_mismatch")
        reviewer_receipt = run_validation_profiles(
            repo_root,
            args.worktree,
            policy,
            role="reviewer",
            candidate_kind=candidate["kind"],
            base_head=maintainer_receipt["base_head"],
            expected_head=maintainer_receipt["repository_head"],
        )
        now = utc_now()
        with locked_state(cache_root) as (state, state_path):
            try:
                refresh_primary_snapshot(
                    repo_root,
                    policy,
                    reviewer_receipt["base_head"],
                )
            except AutopilotError as error:
                if error.code != "primary_snapshot_changed":
                    raise
                current_remote_head = run_command(
                    [
                        "git",
                        "rev-parse",
                        f"refs/remotes/origin/{policy['target_branch']}",
                    ],
                    cwd=repo_root,
                    failure_code="target_main_unavailable",
                )
                requeue_stale_decision(
                    state,
                    candidate_id=args.candidate_id,
                    token=args.lease_token,
                    current_main_head=current_remote_head,
                    now=now,
                )
                persist(state, state_path, at=now)
                return result_payload(
                    "stale_decision_requeued",
                    candidate_id=args.candidate_id,
                    reason_code="base_stale",
                )
            if not validation_receipt_is_current(
                reviewer_receipt,
                current_main_head=reviewer_receipt["base_head"],
                current_authority=validation_authority_identity(repo_root),
            ):
                raise AutopilotError("validation_authority_changed")
            ensure_lease_budget(
                state,
                policy,
                candidate_id=args.candidate_id,
                role="reviewer",
                token=args.lease_token,
                minimum_seconds=SIDE_EFFECT_LEASE_BUDGET_SECONDS,
                now=now,
            )
            resolve_candidate(
                state,
                candidate_id=args.candidate_id,
                role="reviewer",
                token=args.lease_token,
                outcome=args.outcome,
                reason_code=args.reason_code,
                merge_sha=None,
                land_intent_sha256=None,
                land_execution_receipt_sha256=None,
                reviewer_receipt=reviewer_receipt,
                now=now,
            )
            persist(state, state_path, at=now)
        return result_payload(
            args.outcome,
            candidate_id=args.candidate_id,
            validation_receipt_sha256=sha256_value(reviewer_receipt),
        )

    if args.command == "land":
        reserve_lease_budget(
            candidate_id=args.candidate_id,
            role="reviewer",
            token=args.lease_token,
            minimum_seconds=VALIDATION_LEASE_BUDGET_SECONDS,
        )
        with locked_state(cache_root) as (state, _state_path):
            candidate = deepcopy(find_candidate(state, args.candidate_id))
        pull_request = candidate.get("pull_request")
        if not isinstance(pull_request, dict):
            raise AutopilotError("landing_evidence_missing")
        existing_effect = candidate.get("effect")
        recovering_land = (
            isinstance(existing_effect, dict)
            and existing_effect.get("kind") == "land"
            and existing_effect.get("phase")
            in {
                "land_started",
                "land_command_completed",
                "land_completed",
            }
        )
        reviewer_receipt = (
            existing_effect.get("validation_receipt")
            if isinstance(existing_effect, dict)
            and existing_effect.get("kind") == "land"
            else None
        )
        if not isinstance(reviewer_receipt, dict):
            assert_candidate_worktree(
                repo_root,
                args.worktree,
                policy,
                branch=pull_request["branch"],
                head_sha=pull_request["head_sha"],
            )
            reviewer_receipt = run_validation_profiles(
                repo_root,
                args.worktree,
                policy,
                role="reviewer",
                candidate_kind=candidate["kind"],
                base_head=pull_request["validation_receipt"]["base_head"],
                expected_head=pull_request["head_sha"],
            )
        if (
            isinstance(existing_effect, dict)
            and existing_effect.get("kind") == "land"
        ):
            land_decodex_identity = existing_effect["decodex_identity"]
        else:
            _decodex_path, land_decodex_identity = decodex_identity()
        owned_worktrees = (
            existing_effect["owned_worktrees"]
            if isinstance(existing_effect, dict)
            and existing_effect.get("kind") == "land"
            else [managed_worktree_identity(repo_root, args.worktree)]
        )
        safe_stale_effect = (
            existing_effect is None
            or (
                isinstance(existing_effect, dict)
                and existing_effect.get("kind") == "land"
                and existing_effect.get("phase") in {"prepared", "land_started"}
                and existing_effect.get("command_receipt") is None
                and existing_effect.get("execution_receipt") is None
            )
        )
        merge_visibility_api_calls = 0
        with locked_state(cache_root) as (state, state_path):
            before = pull_request_readback(pull_request["url"])
            if (
                before.get("state") == "OPEN"
                and recovering_land
                and existing_effect.get("phase") == "land_started"
            ):
                before, merge_visibility_api_calls = (
                    recover_started_land_readback(
                        repo_root,
                        policy,
                        readback=before,
                        candidate_id=args.candidate_id,
                        intent_sha256=existing_effect["intent_sha256"],
                        base_head=reviewer_receipt["base_head"],
                        head_sha=pull_request["head_sha"],
                        pr_url=pull_request["url"],
                    )
                )
            if before.get("state") == "OPEN":
                current_base = reviewer_receipt["base_head"]
                current = True
                try:
                    refresh_primary_snapshot(
                        repo_root,
                        policy,
                        current_base,
                    )
                except AutopilotError as error:
                    if error.code != "primary_snapshot_changed":
                        raise
                    current = False
                current = current and validation_receipt_is_current(
                    reviewer_receipt,
                    current_main_head=current_base,
                    current_authority=validation_authority_identity(repo_root),
                )
                current = current and before.get("baseRefOid") == current_base
                if not current:
                    if not safe_stale_effect:
                        raise AutopilotError("land_base_stale_after_effect")
                    stale_at = utc_now()
                    request_repair(
                        state,
                        candidate_id=args.candidate_id,
                        token=args.lease_token,
                        finding_codes=["base_stale"],
                        now=stale_at,
                    )
                    persist(state, state_path, at=stale_at)
                    return result_payload(
                        "repair_requested",
                        candidate_id=args.candidate_id,
                        finding_codes=["base_stale"],
                    )
                verify_open_pull_request(
                    before,
                    policy,
                    pr_url=pull_request["url"],
                    branch=pull_request["branch"],
                    base_head=current_base,
                    head_sha=pull_request["head_sha"],
                )
            intent_at = utc_now()
            ensure_lease_budget(
                state,
                policy,
                candidate_id=args.candidate_id,
                role="reviewer",
                token=args.lease_token,
                minimum_seconds=LAND_EFFECT_LEASE_BUDGET_SECONDS,
                now=intent_at,
            )
            effect = prepare_effect(
                state,
                policy,
                candidate_id=args.candidate_id,
                role="reviewer",
                token=args.lease_token,
                kind="land",
                branch=pull_request["branch"],
                head_sha=pull_request["head_sha"],
                pr_url=pull_request["url"],
                owned_worktrees=owned_worktrees,
                validation_receipt=reviewer_receipt,
                decodex_identity=land_decodex_identity,
                now=intent_at,
            )
            persist(state, state_path, at=intent_at)
            process_evidence = effect.get("command_receipt")
            land_entry = classify_land_entry(
                before,
                recovering_land=recovering_land,
                effect_phase=effect["phase"],
            )
            if land_entry in {"execute", "recover_command"}:
                if land_entry == "execute":
                    assert_candidate_worktree(
                        repo_root,
                        args.worktree,
                        policy,
                        branch=pull_request["branch"],
                        head_sha=pull_request["head_sha"],
                    )
                    if effect["phase"] == "prepared":
                        started_at = utc_now()
                        check_lease_budget(
                            state,
                            policy,
                            candidate_id=args.candidate_id,
                            role="reviewer",
                            token=args.lease_token,
                            minimum_seconds=(
                                LAND_EFFECT_LEASE_BUDGET_SECONDS
                            ),
                            now=started_at,
                        )
                        advance_effect_phase(
                            state,
                            candidate_id=args.candidate_id,
                            role="reviewer",
                            token=args.lease_token,
                            phase="land_started",
                            now=started_at,
                        )
                        persist(state, state_path, at=started_at)
                        effect = deepcopy(
                            find_candidate(state, args.candidate_id)["effect"]
                        )
                else:
                    before_merge = before.get("mergeCommit")
                    observed_merge_sha = (
                        before_merge.get("oid")
                        if isinstance(before_merge, dict)
                        else None
                    )
                    if not isinstance(observed_merge_sha, str):
                        raise AutopilotError("landing_evidence_missing")
                    verify_merged_pull_request(
                        before,
                        policy,
                        pr_url=pull_request["url"],
                        branch=pull_request["branch"],
                        head_sha=pull_request["head_sha"],
                        merge_sha=observed_merge_sha,
                    )
                    verify_remote_main_contains(
                        repo_root,
                        policy,
                        pull_request["head_sha"],
                        observed_merge_sha,
                    )
                    verify_merge_parents(
                        repo_root,
                        merge_sha=observed_merge_sha,
                        base_head=reviewer_receipt["base_head"],
                        head_sha=pull_request["head_sha"],
                    )
                    verify_landed_change_record(
                        repo_root,
                        candidate_id=args.candidate_id,
                        intent_sha256=effect["intent_sha256"],
                        merge_sha=observed_merge_sha,
                    )

                command_worktree = (
                    args.worktree
                    if args.worktree.exists()
                    else repo_root
                )
                if command_worktree == args.worktree:
                    assert_candidate_worktree(
                        repo_root,
                        args.worktree,
                        policy,
                        branch=pull_request["branch"],
                        head_sha=pull_request["head_sha"],
                    )
                elif land_entry == "execute":
                    raise AutopilotError("candidate_worktree_unavailable")
                process_evidence = run_decodex_land(
                    command_worktree,
                    candidate_id=args.candidate_id,
                    intent_sha256=effect["intent_sha256"],
                    pr_url=pull_request["url"],
                    expected_base_oid=reviewer_receipt["base_head"],
                    expected_head_oid=pull_request["head_sha"],
                    expected_identity=land_decodex_identity,
                )
                merge_sha = process_evidence["reported_merge_sha"]
                command_receipt = land_command_receipt(
                    intent_sha256=effect["intent_sha256"],
                    process_evidence=process_evidence,
                )
                command_recorded_at = utc_now()
                post_command = assert_primary_clean_main(repo_root, policy)
                record_land_command_execution(
                    state,
                    candidate_id=args.candidate_id,
                    token=args.lease_token,
                    receipt=command_receipt,
                    now=command_recorded_at,
                )
                persist(
                    state,
                    state_path,
                    expected_head=post_command["head"],
                    at=command_recorded_at,
                )
                effect = deepcopy(
                    find_candidate(state, args.candidate_id)["effect"]
                )
                process_evidence = effect["command_receipt"]
            after = pull_request_readback(pull_request["url"])
            merge_commit = after.get("mergeCommit")
            merge_sha = (
                merge_commit.get("oid")
                if isinstance(merge_commit, dict)
                else None
            )
            if not isinstance(merge_sha, str):
                raise AutopilotError("landing_evidence_missing")
            verify_merged_pull_request(
                after,
                policy,
                pr_url=pull_request["url"],
                branch=pull_request["branch"],
                head_sha=pull_request["head_sha"],
                merge_sha=merge_sha,
            )
            verify_remote_main_contains(
                repo_root,
                policy,
                pull_request["head_sha"],
                merge_sha,
            )
            verify_merge_parents(
                repo_root,
                merge_sha=merge_sha,
                base_head=reviewer_receipt["base_head"],
                head_sha=pull_request["head_sha"],
            )
            landed_record_sha256 = verify_landed_change_record(
                repo_root,
                candidate_id=args.candidate_id,
                intent_sha256=effect["intent_sha256"],
                merge_sha=merge_sha,
            )
            post_land = assert_primary_clean_main(repo_root, policy)
            candidate = find_candidate(state, args.candidate_id)
            current_effect = candidate["effect"]
            if current_effect["phase"] != "land_completed":
                receipt = land_execution_receipt(
                    intent_sha256=effect["intent_sha256"],
                    decodex=land_decodex_identity,
                    merge_sha=merge_sha,
                    landed_record_sha256=landed_record_sha256,
                    process_evidence=process_evidence,
                    intent_started_at=effect["started_at"],
                    completed_at=utc_now(),
                )
                execution_recorded_at = utc_now()
                record_land_execution(
                    state,
                    candidate_id=args.candidate_id,
                    token=args.lease_token,
                    receipt=receipt,
                    now=execution_recorded_at,
                )
                persist(
                    state,
                    state_path,
                    expected_head=post_land["head"],
                    at=execution_recorded_at,
                )
                current_effect = find_candidate(
                    state,
                    args.candidate_id,
                )["effect"]
            execution_receipt_sha256 = sha256_value(
                current_effect["execution_receipt"]
            )
            resolved_at = utc_now()
            resolve_candidate(
                state,
                candidate_id=args.candidate_id,
                role="reviewer",
                token=args.lease_token,
                outcome="landed",
                reason_code="independent_review_passed",
                merge_sha=merge_sha,
                land_intent_sha256=effect["intent_sha256"],
                land_execution_receipt_sha256=execution_receipt_sha256,
                reviewer_receipt=reviewer_receipt,
                now=resolved_at,
            )
            persist(
                state,
                state_path,
                expected_head=post_land["head"],
                at=resolved_at,
            )
        return result_payload(
            "landed",
            candidate_id=args.candidate_id,
            merge_sha=merge_sha,
            cost={
                "github_api_calls": 3 + merge_visibility_api_calls,
                "x_api_calls": 0,
                "x_api_estimated_usd": 0,
            },
        )

    if args.command == "block":
        now = utc_now()
        with locked_state(cache_root) as (state, state_path):
            ensure_lease_budget(
                state,
                policy,
                candidate_id=args.candidate_id,
                role=args.role,
                token=args.lease_token,
                minimum_seconds=SIDE_EFFECT_LEASE_BUDGET_SECONDS,
                now=now,
            )
            block_candidate(
                state,
                policy,
                candidate_id=args.candidate_id,
                role=args.role,
                token=args.lease_token,
                reason_code=args.reason_code,
                error_digest=args.error_digest,
                now=now,
            )
            status = find_candidate(state, args.candidate_id)["status"]
            persist(state, state_path, at=now)
        return result_payload(status, candidate_id=args.candidate_id)

    if args.command == "escalate-repair":
        now = utc_now()
        with locked_state(cache_root) as (state, state_path):
            repair = queue_automation_repair(
                state,
                policy,
                blocked_candidate_id=args.candidate_id,
                reason_code=args.reason_code,
                repository_head=preflight["head"],
                now=now,
            )
            persist(state, state_path, at=now)
        return result_payload(
            "repair_queued",
            candidate_id=repair["id"],
            repair_of=args.candidate_id,
        )

    if args.command == "queue-improvement":
        now = utc_now()
        with locked_state(cache_root) as (state, state_path):
            before = len(state["candidates"])
            improvement = queue_automation_improvement(
                state,
                policy,
                reason_code=args.reason_code,
                repository_head=preflight["head"],
                now=now,
            )
            persist(state, state_path, at=now)
            created = len(state["candidates"]) > before
        return result_payload(
            "improvement_queued" if created else "improvement_already_recorded",
            candidate_id=improvement["id"],
            reason_code=args.reason_code,
        )

    if args.command == "health":
        now = utc_now()
        mirror = cache_root / "mirror/openai-codex.git"
        if not mirror.exists():
            mirror = None
        with locked_state(cache_root) as (state, state_path):
            recovered = (
                recover_expired_leases(state, policy, now)
                if args.repair_expired
                else []
            )
            queued_repairs = (
                queue_needed_repairs(
                    state,
                    policy,
                    repository_head=preflight["head"],
                    now=now,
                )
                if args.queue_repairs
                else []
            )
            queued_improvements = (
                queue_effectiveness_improvements(
                    state,
                    policy,
                    repository_head=preflight["head"],
                    now=now,
                )
                if args.queue_improvements
                else []
            )
            persist(state, state_path, at=now)
            snapshot = deepcopy(state)
        health = state_health(
            snapshot,
            mirror,
            now,
            recovered,
            queued_repairs,
            queued_improvements,
        )
        assert_primary_snapshot(repo_root, policy, preflight["head"])
        atomic_write_json(ensure_cache_root(cache_root) / "health/latest.json", health)
        return health

    if args.command == "snapshot":
        with locked_state(cache_root) as (state, _state_path):
            assert_primary_snapshot(repo_root, policy, preflight["head"])
            return result_payload("snapshot", state=state)

    raise AutopilotError("command_unknown")


def main() -> int:
    args = parse_args()
    try:
        payload = execute(args)
    except AutopilotError as error:
        payload = result_payload("failed", error_code=error.code)
        print(
            json.dumps(
                payload,
                indent=2 if getattr(args, "json", False) else None,
                sort_keys=True,
            )
        )
        return 1
    except Exception:
        payload = result_payload("failed", error_code="unclassified_failure")
        print(
            json.dumps(
                payload,
                indent=2 if getattr(args, "json", False) else None,
                sort_keys=True,
            )
        )
        return 1
    print(
        json.dumps(
            payload,
            indent=2 if getattr(args, "json", False) else None,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
