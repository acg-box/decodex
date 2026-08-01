"""Execute the only remote write effects authorized for the upstream autopilot."""

from __future__ import annotations

import json
from pathlib import Path
import re
import time
from typing import Any

from .core import (
    DEFAULT_POLICY_PATH,
    PR_PATTERN,
    SHA_PATTERN,
    AutopilotError,
    command_succeeds,
    hash_file_bounded,
    is_sha256,
    load_policy,
    resolve_executable,
    run_command,
    sha256_value,
    utc_now,
)
from .state import (
    pull_request_readback,
    verify_merge_parents,
    verify_open_pull_request,
)

LOCAL_GIT_TIMEOUT_SECONDS = 60
PULL_REQUEST_MERGE_READBACK_ATTEMPTS = 10
PULL_REQUEST_MERGE_READBACK_DELAY_SECONDS = 2


def managed_worktree_identity(repo_root: Path, worktree: Path) -> str:
    root = repo_root.resolve()
    try:
        resolved = worktree.resolve(strict=True)
        relative = resolved.relative_to(root)
    except (OSError, ValueError) as error:
        raise AutopilotError("managed_worktree_identity_invalid") from error
    if (
        len(relative.parts) < 2
        or relative.parts[0] != ".worktrees"
        or ".." in relative.parts
        or len(str(relative)) > 512
    ):
        raise AutopilotError("managed_worktree_identity_invalid")
    return str(relative)


def remote_branch_head(worktree: Path, branch: str) -> str | None:
    output = run_command(
        [
            "git",
            "ls-remote",
            "--heads",
            "origin",
            f"refs/heads/{branch}",
        ],
        cwd=worktree,
        failure_code="remote_branch_readback_failed",
    )
    if not output:
        return None
    lines = output.splitlines()
    if len(lines) != 1:
        raise AutopilotError("remote_branch_readback_invalid")
    fields = lines[0].split()
    if (
        len(fields) != 2
        or SHA_PATTERN.fullmatch(fields[0]) is None
        or fields[1] != f"refs/heads/{branch}"
    ):
        raise AutopilotError("remote_branch_readback_invalid")
    return fields[0]


def ensure_remote_branch(
    worktree: Path,
    *,
    branch: str,
    head_sha: str,
    expected_remote_head: str | None,
) -> None:
    remote_head = remote_branch_head(worktree, branch)
    if remote_head == head_sha:
        return
    if remote_head != expected_remote_head:
        raise AutopilotError("remote_branch_conflict")
    lease_value = expected_remote_head or ""
    run_command(
        [
            "git",
            "push",
            f"--force-with-lease=refs/heads/{branch}:{lease_value}",
            "origin",
            f"{head_sha}:refs/heads/{branch}",
        ],
        cwd=worktree,
        failure_code="candidate_push_failed",
        timeout_seconds=900,
    )
    if remote_branch_head(worktree, branch) != head_sha:
        raise AutopilotError("candidate_push_readback_mismatch")


def classify_commit_entry(current_head: str, base_head: str) -> str:
    if (
        SHA_PATTERN.fullmatch(current_head) is None
        or SHA_PATTERN.fullmatch(base_head) is None
    ):
        raise AutopilotError("candidate_commit_readback_failed")
    if current_head != base_head:
        raise AutopilotError("decodex_commit_execution_evidence_missing")
    return "execute"


def run_decodex_commit(
    worktree: Path,
    *,
    candidate_id: str,
    expected_identity: dict[str, str],
) -> dict[str, Any]:
    executable, identity = decodex_identity()
    if identity != expected_identity:
        raise AutopilotError("decodex_identity_changed")
    started_at = utc_now()
    output = run_command(
        [
            str(executable),
            "commit",
            f"Codex upstream candidate {candidate_id}",
            "--manual-authority",
        ],
        cwd=worktree,
        failure_code="decodex_commit_failed",
        timeout_seconds=900,
    )
    completed_at = utc_now()
    if hash_file_bounded(executable) != identity["executable_sha256"]:
        raise AutopilotError("decodex_identity_changed")
    return {
        "schema": "decodex/codex-upstream-commit-execution/1",
        "execution_mode": "command_completed",
        "decodex_version": identity["version"],
        "decodex_executable_sha256": identity["executable_sha256"],
        "started_at": started_at,
        "completed_at": completed_at,
        "stdout_sha256": sha256_value(output),
    }


def commit_execution_receipt(
    *,
    intent_sha256: str,
    process_evidence: dict[str, Any],
) -> dict[str, Any]:
    return {
        **process_evidence,
        "intent_sha256": intent_sha256,
    }


def verify_decodex_commit(
    worktree: Path,
    *,
    candidate_id: str,
    base_head: str,
    require_clean: bool = True,
) -> dict[str, str]:
    head_sha = run_command(
        ["git", "rev-parse", "HEAD"],
        cwd=worktree,
        failure_code="candidate_commit_readback_failed",
    )
    parent = run_command(
        ["git", "rev-parse", "HEAD^"],
        cwd=worktree,
        failure_code="candidate_commit_readback_failed",
    )
    tree_sha = run_command(
        ["git", "rev-parse", "HEAD^{tree}"],
        cwd=worktree,
        failure_code="candidate_commit_readback_failed",
    )
    status = run_command(
        ["git", "status", "--porcelain=v1"],
        cwd=worktree,
        failure_code="candidate_commit_readback_failed",
    )
    subject = run_command(
        ["git", "show", "-s", "--format=%s", "HEAD"],
        cwd=worktree,
        failure_code="candidate_commit_readback_failed",
    )
    try:
        message = json.loads(subject)
    except json.JSONDecodeError as error:
        raise AutopilotError("candidate_commit_message_invalid") from error
    if (
        SHA_PATTERN.fullmatch(head_sha) is None
        or SHA_PATTERN.fullmatch(tree_sha) is None
        or parent != base_head
        or (require_clean and status)
        or not isinstance(message, dict)
        or message.get("schema") != "decodex/commit/2"
        or message.get("authority") != "manual"
        or message.get("impact") != "compatible"
        or message.get("change") != f"Codex upstream candidate {candidate_id}"
    ):
        raise AutopilotError("candidate_commit_readback_mismatch")
    verify_commit_signature(
        worktree,
        "HEAD",
        failure_code="candidate_commit_signature_invalid",
    )
    return {
        "head_sha": head_sha,
        "tree_sha": tree_sha,
        "message_sha256": sha256_value(message),
    }


def verify_commit_signature(
    worktree: Path,
    ref: str,
    *,
    failure_code: str,
) -> None:
    if not command_succeeds(
        ["git", "verify-commit", ref],
        cwd=worktree,
        failure_code=failure_code,
    ):
        raise AutopilotError(failure_code)


def staged_replacement_tree(worktree: Path) -> str:
    if not command_succeeds(
        ["git", "diff", "--quiet"],
        cwd=worktree,
        failure_code="recorded_commit_rewind_failed",
    ):
        raise AutopilotError("candidate_unstaged_changes")
    untracked = run_command(
        ["git", "ls-files", "--others", "--exclude-standard"],
        cwd=worktree,
        failure_code="recorded_commit_rewind_failed",
    )
    if untracked:
        raise AutopilotError("candidate_untracked_changes")
    if command_succeeds(
        ["git", "diff", "--cached", "--quiet"],
        cwd=worktree,
        failure_code="recorded_commit_rewind_failed",
    ):
        raise AutopilotError("candidate_staged_change_missing")
    tree = run_command(
        ["git", "write-tree"],
        cwd=worktree,
        failure_code="recorded_commit_rewind_failed",
    )
    if SHA_PATTERN.fullmatch(tree) is None:
        raise AutopilotError("recorded_commit_rewind_failed")
    return tree


def rewind_unrecorded_decodex_commit(
    worktree: Path,
    *,
    candidate_id: str,
    branch: str,
    base_head: str,
) -> None:
    evidence = verify_decodex_commit(
        worktree,
        candidate_id=candidate_id,
        base_head=base_head,
    )
    remote_head = remote_branch_head(worktree, branch)
    if remote_head not in {None, base_head}:
        raise AutopilotError("unrecorded_commit_remote_conflict")
    run_command(
        ["git", "reset", "--soft", base_head],
        cwd=worktree,
        failure_code="unrecorded_commit_rewind_failed",
    )
    restored_head = run_command(
        ["git", "rev-parse", "HEAD"],
        cwd=worktree,
        failure_code="unrecorded_commit_rewind_failed",
    )
    restored_tree = run_command(
        ["git", "write-tree"],
        cwd=worktree,
        failure_code="unrecorded_commit_rewind_failed",
    )
    if restored_head != base_head or restored_tree != evidence["tree_sha"]:
        raise AutopilotError("unrecorded_commit_rewind_mismatch")


def rewind_recorded_candidate_commit(
    worktree: Path,
    *,
    candidate_id: str,
    branch: str,
    commit_receipt: dict[str, Any],
    allowed_remote_heads: set[str | None],
) -> None:
    base_head = commit_receipt.get("base_head")
    old_head = commit_receipt.get("head_sha")
    old_tree = commit_receipt.get("tree_sha")
    old_message = commit_receipt.get("message_sha256")
    if (
        any(
            SHA_PATTERN.fullmatch(str(value)) is None
            for value in (base_head, old_head, old_tree)
        )
        or not is_sha256(old_message)
        or not allowed_remote_heads
        or any(
            value is not None and SHA_PATTERN.fullmatch(value) is None
            for value in allowed_remote_heads
        )
    ):
        raise AutopilotError("recorded_commit_rewind_identity_invalid")
    replacement_tree = staged_replacement_tree(worktree)
    evidence = verify_decodex_commit(
        worktree,
        candidate_id=candidate_id,
        base_head=base_head,
        require_clean=False,
    )
    if (
        evidence["head_sha"] != old_head
        or evidence["tree_sha"] != old_tree
        or evidence["message_sha256"] != old_message
    ):
        raise AutopilotError("recorded_commit_rewind_evidence_mismatch")
    if (
        staged_replacement_tree(worktree) != replacement_tree
        or replacement_tree == old_tree
    ):
        raise AutopilotError("candidate_staged_change_missing")
    remote_head = remote_branch_head(worktree, branch)
    if remote_head not in allowed_remote_heads:
        raise AutopilotError("recorded_commit_remote_conflict")
    run_command(
        ["git", "reset", "--soft", base_head],
        cwd=worktree,
        failure_code="recorded_commit_rewind_failed",
    )
    restored_head = run_command(
        ["git", "rev-parse", "HEAD"],
        cwd=worktree,
        failure_code="recorded_commit_rewind_failed",
    )
    restored_tree = run_command(
        ["git", "write-tree"],
        cwd=worktree,
        failure_code="recorded_commit_rewind_failed",
    )
    if restored_head != base_head or restored_tree != replacement_tree:
        raise AutopilotError("recorded_commit_rewind_mismatch")


def candidate_pr_body(candidate: dict[str, Any]) -> str:
    source = (
        f"{candidate.get('from_sha') or 'none'}..{candidate.get('to_sha') or 'none'}"
    )
    missing = candidate.get("contract_missing") or []
    visible_missing = missing[:32]
    missing_text = ", ".join(visible_missing) if visible_missing else "none"
    if len(missing) > len(visible_missing):
        missing_text += f" (+{len(missing) - len(visible_missing)} more)"
    return (
        "## Summary\n\n"
        f"Autonomous Codex upstream adaptation for `{candidate['kind']}` "
        f"candidate `{candidate['id']}`.\n\n"
        "## Source\n\n"
        f"- Range: `{source}`\n"
        f"- Release: `{candidate.get('release_tag') or 'none'}`\n"
        f"- Codex: `{candidate['codex_version']}`\n"
        f"- Contract gaps at discovery: `{missing_text}`\n\n"
        "## Verification\n\n"
        "Every validation profile required by the trusted changed-path "
        "classification passed for the exact PR head. The independent reviewer "
        "repeats the same required profile set before Decodex land is allowed.\n"
    )


def find_or_create_pull_request(
    worktree: Path,
    policy: dict[str, Any],
    candidate: dict[str, Any],
    *,
    base_head: str,
    head_sha: str,
) -> str:
    if SHA_PATTERN.fullmatch(base_head) is None:
        raise AutopilotError("pull_request_base_invalid")
    branch = candidate["branch_name"]
    output = run_command(
        [
            "gh",
            "pr",
            "list",
            "--repo",
            policy["target_repository"],
            "--head",
            branch,
            "--state",
            "all",
            "--limit",
            "10",
            "--json",
            "url",
        ],
        cwd=worktree,
        failure_code="pull_request_lookup_failed",
    )
    try:
        matches = json.loads(output)
    except json.JSONDecodeError as error:
        raise AutopilotError("pull_request_lookup_invalid") from error
    if not isinstance(matches, list) or any(
        not isinstance(value, dict)
        or PR_PATTERN.fullmatch(str(value.get("url", ""))) is None
        for value in matches
    ):
        raise AutopilotError("pull_request_lookup_invalid")
    if len(matches) > 1:
        raise AutopilotError("pull_request_lookup_ambiguous")
    if matches:
        pr_url = matches[0]["url"]
    else:
        title = f"Adapt Codex upstream {candidate['kind']} {candidate['id']}"
        pr_url = run_command(
            [
                "gh",
                "pr",
                "create",
                "--repo",
                policy["target_repository"],
                "--base",
                policy["target_branch"],
                "--head",
                branch,
                "--title",
                title,
                "--body",
                candidate_pr_body(candidate),
            ],
            cwd=worktree,
            failure_code="pull_request_creation_failed",
        )
    value = pull_request_readback(pr_url)
    verify_open_pull_request(
        worktree,
        value,
        policy,
        pr_url=pr_url,
        branch=branch,
        base_head=base_head,
        head_sha=head_sha,
    )
    return pr_url


def verify_retired_pull_request(
    worktree: Path,
    value: dict[str, Any],
    policy: dict[str, Any],
    *,
    pr_url: str,
    branch: str,
    base_head: str,
    head_sha: str,
) -> None:
    recorded_base = value.get("baseRefOid")
    if (
        value.get("state") != "CLOSED"
        or value.get("url") != pr_url
        or value.get("isCrossRepository") is not False
        or value.get("baseRefName") != policy["target_branch"]
        or SHA_PATTERN.fullmatch(str(recorded_base)) is None
        or value.get("headRefName") != branch
        or value.get("headRefOid") != head_sha
        or value.get("mergeCommit") is not None
    ):
        raise AutopilotError("pull_request_retirement_mismatch")
    if recorded_base != base_head and not command_succeeds(
        ["git", "merge-base", "--is-ancestor", recorded_base, base_head],
        cwd=worktree,
        failure_code="pull_request_retirement_mismatch",
    ):
        raise AutopilotError("pull_request_retirement_mismatch")


def retire_pull_request(
    worktree: Path,
    policy: dict[str, Any],
    *,
    candidate_id: str,
    pr_url: str,
    branch: str,
    base_head: str,
    head_sha: str,
) -> str:
    before = pull_request_readback(pr_url)
    if before.get("state") == "OPEN":
        verify_open_pull_request(
            worktree,
            before,
            policy,
            pr_url=pr_url,
            branch=branch,
            base_head=base_head,
            head_sha=head_sha,
        )
        run_command(
            [
                "gh",
                "pr",
                "close",
                pr_url,
                "--delete-branch",
                "--comment",
                (
                    "Closed by the autonomous upstream reviewer workflow before an "
                    f"independently verified terminal decision for `{candidate_id}`."
                ),
            ],
            cwd=worktree,
            failure_code="pull_request_retirement_failed",
        )
    else:
        verify_retired_pull_request(
            worktree,
            before,
            policy,
            pr_url=pr_url,
            branch=branch,
            base_head=base_head,
            head_sha=head_sha,
        )
    after = pull_request_readback(pr_url)
    verify_retired_pull_request(
        worktree,
        after,
        policy,
        pr_url=pr_url,
        branch=branch,
        base_head=base_head,
        head_sha=head_sha,
    )
    remote_head = remote_branch_head(worktree, branch)
    if remote_head is not None:
        if remote_head != head_sha:
            raise AutopilotError("pull_request_branch_retirement_conflict")
        run_command(
            [
                "git",
                "push",
                f"--force-with-lease=refs/heads/{branch}:{head_sha}",
                "origin",
                f":refs/heads/{branch}",
            ],
            cwd=worktree,
            failure_code="pull_request_branch_retirement_failed",
            timeout_seconds=900,
        )
    if remote_branch_head(worktree, branch) is not None:
        raise AutopilotError("pull_request_branch_retirement_mismatch")
    return sha256_value(after)


def decodex_identity() -> tuple[Path, dict[str, str]]:
    executable, executable_sha256 = resolve_executable("decodex")
    policy = load_policy(DEFAULT_POLICY_PATH)
    if executable_sha256 != policy["decodex_executable_sha256"]:
        raise AutopilotError("decodex_identity_not_approved")
    version = run_command(
        [str(executable), "--version"],
        failure_code="decodex_version_unavailable",
    )
    land_help = run_command(
        [str(executable), "land", "--help"],
        failure_code="decodex_capability_unavailable",
    )
    commit_help = run_command(
        [str(executable), "commit", "--help"],
        failure_code="decodex_capability_unavailable",
    )
    if (
        re.fullmatch(r"decodex [0-9A-Za-z][0-9A-Za-z.+_-]{0,255}", version)
        is None
        or hash_file_bounded(executable) != executable_sha256
    ):
        raise AutopilotError("decodex_identity_invalid")
    if (
        any(
            marker not in land_help
            for marker in (
                "without contacting the Decodex server",
                "--manual-authority",
                "--expected-base-oid",
                "--expected-head-oid",
            )
        )
        or any(
            marker not in commit_help
            for marker in (
                "without contacting the Decodex server",
                "--manual-authority",
            )
        )
    ):
        raise AutopilotError("decodex_capability_incompatible")
    return executable, {
        "version": version,
        "executable_sha256": executable_sha256,
    }


def classify_land_entry(
    readback: dict[str, Any],
    *,
    recovering_land: bool,
    effect_phase: str,
) -> str:
    state = readback.get("state")
    if state == "OPEN":
        if effect_phase in {"land_command_completed", "land_completed"}:
            raise AutopilotError("land_execution_state_conflict")
        return "execute"
    if state == "MERGED":
        if (
            not recovering_land
            or effect_phase
            not in {
                "land_started",
                "land_command_completed",
                "land_completed",
            }
        ):
            raise AutopilotError("external_merge_detected")
        if effect_phase == "land_started":
            return "recover_command"
        return "recover"
    raise AutopilotError("landing_state_invalid")


def land_change_summary(candidate_id: str, intent_sha256: str) -> str:
    if (
        re.fullmatch(r"[0-9a-f]{16}", candidate_id) is None
        or not is_sha256(intent_sha256)
    ):
        raise AutopilotError("land_intent_invalid")
    return f"Codex upstream candidate {candidate_id} intent {intent_sha256}"


def expected_landed_change_record(
    candidate_id: str,
    intent_sha256: str,
) -> dict[str, str]:
    return {
        "schema": "decodex/commit/2",
        "change": f"Land {land_change_summary(candidate_id, intent_sha256)}",
        "authority": "manual",
        "impact": "compatible",
    }


def verify_land_merge_commit(
    repo_root: Path,
    *,
    candidate_id: str,
    intent_sha256: str,
    merge_sha: str,
    base_head: str,
    head_sha: str,
) -> None:
    if not land_merge_candidate_matches_structure(
        repo_root,
        candidate_id=candidate_id,
        intent_sha256=intent_sha256,
        merge_sha=merge_sha,
        base_head=base_head,
        head_sha=head_sha,
    ):
        raise AutopilotError("land_merge_structure_mismatch")
    verify_commit_signature(
        repo_root,
        merge_sha,
        failure_code="land_merge_signature_invalid",
    )


def land_merge_candidate_matches_structure(
    repo_root: Path,
    *,
    candidate_id: str,
    intent_sha256: str,
    merge_sha: str,
    base_head: str,
    head_sha: str,
) -> bool:
    if SHA_PATTERN.fullmatch(merge_sha) is None:
        raise AutopilotError("land_merge_commit_invalid")
    try:
        verify_merge_parents(
            repo_root,
            merge_sha=merge_sha,
            base_head=base_head,
            head_sha=head_sha,
        )
    except AutopilotError as error:
        if error.code == "landing_parent_mismatch":
            return False
        raise
    head_tree = run_command(
        ["git", "rev-parse", f"{head_sha}^{{tree}}"],
        cwd=repo_root,
        failure_code="land_merge_tree_readback_failed",
        timeout_seconds=LOCAL_GIT_TIMEOUT_SECONDS,
    )
    merge_tree = run_command(
        ["git", "rev-parse", f"{merge_sha}^{{tree}}"],
        cwd=repo_root,
        failure_code="land_merge_tree_readback_failed",
        timeout_seconds=LOCAL_GIT_TIMEOUT_SECONDS,
    )
    if head_tree != merge_tree or SHA_PATTERN.fullmatch(merge_tree) is None:
        return False
    try:
        verify_landed_change_record(
            repo_root,
            candidate_id=candidate_id,
            intent_sha256=intent_sha256,
            merge_sha=merge_sha,
        )
    except AutopilotError as error:
        if error.code in {
            "landed_commit_record_invalid",
            "landed_commit_record_mismatch",
        }:
            return False
        raise
    return True


def recover_exact_land_merge(
    repo_root: Path,
    policy: dict[str, Any],
    *,
    candidate_id: str,
    intent_sha256: str,
    base_head: str,
    head_sha: str,
) -> str:
    remote_main = remote_branch_head(repo_root, policy["target_branch"])
    if remote_main in {None, base_head}:
        raise AutopilotError("land_base_compare_and_swap_failed")
    run_command(
        [
            "git",
            "fetch",
            "--quiet",
            "origin",
            (
                f"refs/heads/{policy['target_branch']}:"
                f"refs/remotes/origin/{policy['target_branch']}"
            ),
        ],
        cwd=repo_root,
        failure_code="land_merge_readback_failed",
        timeout_seconds=900,
    )
    remote_ref = f"refs/remotes/origin/{policy['target_branch']}"
    output = run_command(
        [
            "git",
            "log",
            "--format=%H",
            "--fixed-strings",
            f"--grep={intent_sha256}",
            "--max-count=17",
            f"{base_head}..{remote_ref}",
        ],
        cwd=repo_root,
        failure_code="land_merge_readback_failed",
        timeout_seconds=LOCAL_GIT_TIMEOUT_SECONDS,
    )
    candidates = output.splitlines() if output else []
    if len(candidates) >= 17:
        raise AutopilotError("land_merge_search_ambiguous")
    exact_matches: list[str] = []
    for merge_sha in candidates:
        if SHA_PATTERN.fullmatch(merge_sha) is None:
            raise AutopilotError("land_merge_readback_failed")
        if not land_merge_candidate_matches_structure(
            repo_root,
            candidate_id=candidate_id,
            intent_sha256=intent_sha256,
            merge_sha=merge_sha,
            base_head=base_head,
            head_sha=head_sha,
        ):
            continue
        verify_commit_signature(
            repo_root,
            merge_sha,
            failure_code="land_merge_signature_invalid",
        )
        if command_succeeds(
            ["git", "merge-base", "--is-ancestor", merge_sha, remote_ref],
            cwd=repo_root,
            failure_code="land_merge_readback_failed",
        ):
            exact_matches.append(merge_sha)
    if not exact_matches:
        raise AutopilotError("land_base_compare_and_swap_failed")
    if len(exact_matches) != 1:
        raise AutopilotError("land_merge_search_ambiguous")
    return exact_matches[0]


def recover_started_land_readback(
    repo_root: Path,
    policy: dict[str, Any],
    *,
    readback: dict[str, Any],
    candidate_id: str,
    intent_sha256: str,
    base_head: str,
    head_sha: str,
    pr_url: str,
) -> tuple[dict[str, Any], int]:
    if readback.get("state") != "OPEN":
        raise AutopilotError("landing_state_invalid")
    current_remote_main = remote_branch_head(
        repo_root,
        policy["target_branch"],
    )
    if current_remote_main in {None, base_head}:
        return readback, 0
    try:
        merge_sha = recover_exact_land_merge(
            repo_root,
            policy,
            candidate_id=candidate_id,
            intent_sha256=intent_sha256,
            base_head=base_head,
            head_sha=head_sha,
        )
    except AutopilotError as error:
        if error.code == "land_base_compare_and_swap_failed":
            return readback, 0
        raise
    return wait_for_pull_request_merge_readback(
        pr_url,
        merge_sha=merge_sha,
    )


def wait_for_pull_request_merge_readback(
    pr_url: str,
    *,
    merge_sha: str,
) -> tuple[dict[str, Any], int]:
    if PR_PATTERN.fullmatch(pr_url) is None or SHA_PATTERN.fullmatch(
        merge_sha
    ) is None:
        raise AutopilotError("landing_evidence_missing")
    for attempt in range(1, PULL_REQUEST_MERGE_READBACK_ATTEMPTS + 1):
        readback = pull_request_readback(pr_url)
        merge_commit = readback.get("mergeCommit")
        observed_merge = (
            merge_commit.get("oid")
            if isinstance(merge_commit, dict)
            else None
        )
        if readback.get("state") == "MERGED":
            if observed_merge != merge_sha:
                raise AutopilotError("land_merge_readback_mismatch")
            return readback, attempt
        if readback.get("state") != "OPEN":
            raise AutopilotError("landing_state_invalid")
        if attempt < PULL_REQUEST_MERGE_READBACK_ATTEMPTS:
            time.sleep(PULL_REQUEST_MERGE_READBACK_DELAY_SECONDS)
    raise AutopilotError("land_merge_visibility_pending")


def run_decodex_land(
    worktree: Path,
    *,
    candidate_id: str,
    intent_sha256: str,
    pr_url: str,
    expected_base_oid: str,
    expected_head_oid: str,
    expected_identity: dict[str, str],
) -> dict[str, Any]:
    if any(
        SHA_PATTERN.fullmatch(value) is None
        for value in (expected_base_oid, expected_head_oid)
    ):
        raise AutopilotError("land_base_identity_invalid")
    executable, identity = decodex_identity()
    if identity != expected_identity:
        raise AutopilotError("decodex_identity_changed")
    started_at = utc_now()
    output = run_command(
        [
            str(executable),
            "land",
            land_change_summary(candidate_id, intent_sha256),
            "--manual-authority",
            "--pr",
            pr_url,
            "--expected-base-oid",
            expected_base_oid,
            "--expected-head-oid",
            expected_head_oid,
        ],
        cwd=worktree,
        failure_code="decodex_land_failed",
        timeout_seconds=3600,
    )
    completed_at = utc_now()
    if hash_file_bounded(executable) != identity["executable_sha256"]:
        raise AutopilotError("decodex_identity_changed")
    match = re.fullmatch(
        (
            rf"land ok: pr={re.escape(pr_url)} "
            r"merge_commit=((?:[0-9a-f]{40}|[0-9a-f]{64})) "
            r"default_branch=main local_default_branch_synced=true"
        ),
        output,
    )
    if match is None:
        raise AutopilotError("decodex_land_output_invalid")
    return {
        "execution_mode": "command_completed",
        "decodex_version": identity["version"],
        "decodex_executable_sha256": identity["executable_sha256"],
        "started_at": started_at,
        "completed_at": completed_at,
        "stdout_sha256": sha256_value(output),
        "reported_merge_sha": match.group(1),
    }


def land_command_receipt(
    *,
    intent_sha256: str,
    process_evidence: dict[str, Any],
) -> dict[str, Any]:
    return {
        "schema": "decodex/codex-upstream-land-command/1",
        "intent_sha256": intent_sha256,
        **process_evidence,
    }


def verify_landed_change_record(
    repo_root: Path,
    *,
    candidate_id: str,
    intent_sha256: str,
    merge_sha: str,
) -> str:
    subject = run_command(
        ["git", "show", "-s", "--format=%s", merge_sha],
        cwd=repo_root,
        failure_code="landed_commit_subject_unavailable",
        max_output_bytes=64 * 1024,
    )
    try:
        record = json.loads(subject)
    except json.JSONDecodeError as error:
        raise AutopilotError("landed_commit_record_invalid") from error
    expected = expected_landed_change_record(candidate_id, intent_sha256)
    if record != expected:
        raise AutopilotError("landed_commit_record_mismatch")
    return sha256_value(record)


def land_execution_receipt(
    *,
    intent_sha256: str,
    decodex: dict[str, str],
    merge_sha: str,
    landed_record_sha256: str,
    process_evidence: dict[str, Any],
    intent_started_at: int,
    completed_at: int,
) -> dict[str, Any]:
    if (
        process_evidence.get("schema")
        != "decodex/codex-upstream-land-command/1"
        or process_evidence.get("intent_sha256") != intent_sha256
        or process_evidence.get("reported_merge_sha") != merge_sha
        or process_evidence.get("decodex_version") != decodex["version"]
        or process_evidence.get("decodex_executable_sha256")
        != decodex["executable_sha256"]
        or process_evidence.get("started_at", 0) < intent_started_at
        or process_evidence.get("completed_at", completed_at) > completed_at
    ):
        raise AutopilotError("land_execution_receipt_mismatch")
    return {
        "schema": "decodex/codex-upstream-land-execution/1",
        "intent_sha256": intent_sha256,
        "execution_mode": process_evidence["execution_mode"],
        "decodex_version": process_evidence["decodex_version"],
        "decodex_executable_sha256": process_evidence[
            "decodex_executable_sha256"
        ],
        "started_at": process_evidence["started_at"],
        "completed_at": process_evidence["completed_at"],
        "stdout_sha256": process_evidence["stdout_sha256"],
        "reported_merge_sha": process_evidence["reported_merge_sha"],
        "landed_record_sha256": landed_record_sha256,
    }
