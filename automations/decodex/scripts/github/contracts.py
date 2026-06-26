#!/usr/bin/env python3
"""Shared contract helpers for GitHub bundle and signal-entry tooling."""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

BUNDLE_SCHEMA = "github_change_bundle/v1"
SIGNAL_SCHEMA = "signal_entry/v1"
RELEASE_DELTA_SCHEMA = "release_delta/v1"
UPSTREAM_REVIEW_QUEUE_SCHEMA = "upstream_review_queue/v1"
UPSTREAM_REVIEW_SCHEMA = "upstream_review/v1"
SOCIAL_CANDIDATE_SCHEMA = "social_candidate/v1"
SOCIAL_POST_SCHEMA = "social_post/v1"
SOCIAL_PUBLISH_RESERVATION_SCHEMA = "social_publish_reservation/v1"
CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA = "control_plane_upgrade_candidate/v1"
ANALYSIS_MODES = {"pr_first", "commit_only"}
SIGNAL_KINDS = {"capability", "behavior_change", "try_now"}
SIGNAL_CONFIDENCE = {"confirmed", "likely", "weak"}
SIGNAL_IMPACT = {"low", "medium", "high"}
SOURCE_ITEM_KINDS = {"pull_request", "commit"}
UPSTREAM_SUBJECT_KINDS = {"commit", "pr"}
UPSTREAM_REVIEW_PRIORITIES = {"critical", "high", "normal", "low"}
UPSTREAM_REVIEW_NEXT_STEPS = {"ai_review_required"}
UPSTREAM_SOURCE_STATES = {"open", "closed", "merged", "commit_only"}
UPSTREAM_REVIEW_ACTION_TYPES = {
    "none",
    "upstream_impact",
    "signal_entry",
    "social_candidate",
    "control_plane_upgrade_candidate",
}
CONTROL_PLANE_UPGRADE_IMPACTS = {"adopt_now", "candidate", "compat_risk"}
CONTROL_PLANE_UPGRADE_PATHS = {"adopt_now", "compat_risk_mitigation", "discovery"}
CONTROL_PLANE_UPGRADE_STATUSES = {"blocked", "deferred", "proposed", "superseded"}
CODEX_COMPATIBILITY_STATUSES = {
    "compatible",
    "incompatible",
    "needs_review",
    "not_tested",
    "unknown",
}
CODEX_TARGET_CHANNELS = {"main", "preview", "stable"}
SOCIAL_POST_MODES = {
    "release_pulse",
    "release_rollup",
    "practical_explainer",
    "operator_impact",
    "thread",
    "watch_note",
}
SOCIAL_POST_STATUSES = {"published", "blocked", "failed", "skipped"}
SOCIAL_POST_PRIORITIES = {"critical", "high", "normal", "low"}
SOCIAL_POST_WORTHINESS = {"publish", "skip", "block"}
SOCIAL_POST_LIFECYCLE_STATES = {
    "deleted_by_operator",
    "live",
    "superseded_failed_attempt",
    "superseded_published",
    "superseded_text_only",
}
SOCIAL_PUBLISH_RESERVATION_STATUSES = {"active", "canceled", "consumed", "expired"}
SOCIAL_BLOCK_REASONS = {
    "daily_cap_exceeded",
    "duplicate",
    "policy_block",
    "insufficient_evidence",
}
ISSUE_REF_RE = re.compile(r"(?:^|[^\w])((?:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#\d+)")
FLAG_RE = re.compile(
    r"(?<![\w-])(--[a-zA-Z0-9][\w-]*|[A-Z][A-Z0-9_]{2,}(?:=[^\s,`]+)?)"
)
GENERIC_COMMIT_TITLES = {
    "update",
    "fix",
    "fix.",
    "fix tests",
    "fix tests.",
    "merge fixes",
    "flaky syntax",
}


@dataclass
class ValidationResult:
    ok: bool
    errors: list[str]


def load_json(path: str | Path) -> Any:
    return json.loads(Path(path).read_text(encoding="utf-8"))


def dump_json(path: str | Path, payload: Any) -> None:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def utc_now_iso() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def slugify(value: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", value.lower()).strip("-")
    return slug or "signal"


def first_line(value: str) -> str:
    return value.strip().splitlines()[0] if value.strip() else ""


def truncate_patch(value: str | None, limit: int = 900) -> str | None:
    if not value:
        return None
    compact = value.strip()
    return compact[:limit] + "..." if len(compact) > limit else compact


def collect_issue_refs(*texts: str) -> list[str]:
    found: list[str] = []
    for text in texts:
        for match in ISSUE_REF_RE.findall(text or ""):
            if match not in found:
                found.append(match)
    return found


def collect_flags(*texts: str) -> list[str]:
    found: list[str] = []
    for text in texts:
        for match in FLAG_RE.findall(text or ""):
            if match not in found:
                found.append(match)
    return found


def validate_bundle(bundle: dict[str, Any]) -> ValidationResult:
    errors: list[str] = []

    if bundle.get("schema") != BUNDLE_SCHEMA:
        errors.append(f"schema must be {BUNDLE_SCHEMA}")

    if not isinstance(bundle.get("repo"), str) or "/" not in bundle["repo"]:
        errors.append("repo must be owner/name")

    if bundle.get("analysis_mode") not in ANALYSIS_MODES:
        errors.append(f"analysis_mode must be one of {sorted(ANALYSIS_MODES)}")

    if not isinstance(bundle.get("default_branch"), str) or not bundle["default_branch"]:
        errors.append("default_branch must be a non-empty string")

    commits = bundle.get("commits")
    if not isinstance(commits, list) or not commits:
        errors.append("commits must be a non-empty list")
    else:
        for index, commit in enumerate(commits):
            if not isinstance(commit, dict):
                errors.append(f"commits[{index}] must be an object")
                continue
            for field in ("sha", "message", "url"):
                if not isinstance(commit.get(field), str) or not commit[field]:
                    errors.append(f"commits[{index}].{field} must be a non-empty string")

    files = bundle.get("files")
    if not isinstance(files, list) or not files:
        errors.append("files must be a non-empty list")
    else:
        for index, item in enumerate(files):
            if not isinstance(item, dict):
                errors.append(f"files[{index}] must be an object")
                continue
            for field in ("path", "status", "additions", "deletions"):
                if field not in item:
                    errors.append(f"files[{index}].{field} is required")

    if bundle.get("analysis_mode") == "pr_first":
        pr = bundle.get("primary_pr")
        if not isinstance(pr, dict):
            errors.append("primary_pr is required when analysis_mode is pr_first")
        else:
            for field in ("number", "title", "body", "state", "labels", "url"):
                if field not in pr:
                    errors.append(f"primary_pr.{field} is required")

    return ValidationResult(ok=not errors, errors=errors)


def validate_analysis_draft(draft: dict[str, Any]) -> ValidationResult:
    errors: list[str] = []
    for field in ("kind", "title", "summary", "why_it_matters", "confidence", "impact", "proof_points"):
        if field not in draft:
            errors.append(f"{field} is required in analysis draft")

    if draft.get("kind") not in SIGNAL_KINDS:
        errors.append(f"kind must be one of {sorted(SIGNAL_KINDS)}")
    if draft.get("confidence") not in SIGNAL_CONFIDENCE:
        errors.append(f"confidence must be one of {sorted(SIGNAL_CONFIDENCE)}")
    if draft.get("impact") not in SIGNAL_IMPACT:
        errors.append(f"impact must be one of {sorted(SIGNAL_IMPACT)}")

    proof_points = draft.get("proof_points")
    if not isinstance(proof_points, list) or not proof_points:
        errors.append("proof_points must be a non-empty list")

    how_to_try = draft.get("how_to_try")
    if draft.get("kind") == "try_now" and not how_to_try:
        errors.append("how_to_try is required when kind is try_now")
    if how_to_try and not draft.get("expected_effect"):
        errors.append("expected_effect is required when how_to_try is present")

    return ValidationResult(ok=not errors, errors=errors)


def validate_signal(entry: dict[str, Any]) -> ValidationResult:
    errors: list[str] = []
    if entry.get("schema") != SIGNAL_SCHEMA:
        errors.append(f"schema must be {SIGNAL_SCHEMA}")

    if entry.get("lane") != "github":
        errors.append("lane must be github for the MVP")

    if entry.get("kind") not in SIGNAL_KINDS:
        errors.append(f"kind must be one of {sorted(SIGNAL_KINDS)}")

    if entry.get("confidence") not in SIGNAL_CONFIDENCE:
        errors.append(f"confidence must be one of {sorted(SIGNAL_CONFIDENCE)}")

    if entry.get("impact") not in SIGNAL_IMPACT:
        errors.append(f"impact must be one of {sorted(SIGNAL_IMPACT)}")

    for field in ("slug", "title", "published_at", "summary", "why_it_matters"):
        if not isinstance(entry.get(field), str) or not entry[field]:
            errors.append(f"{field} must be a non-empty string")

    proof_points = entry.get("proof_points")
    if not isinstance(proof_points, list) or not proof_points:
        errors.append("proof_points must be a non-empty list")

    config_flags = entry.get("config_flags", [])
    if config_flags is None:
        config_flags = []
    if not isinstance(config_flags, list):
        errors.append("config_flags must be a list when present")
        config_flags = []

    if (entry.get("kind") == "try_now" or config_flags) and not entry.get("how_to_try"):
        errors.append("how_to_try is required for try_now or flag-backed entries")

    if entry.get("how_to_try") and not entry.get("expected_effect"):
        errors.append("expected_effect is required when how_to_try is present")

    caveats = entry.get("caveats", [])
    if caveats is None:
        caveats = []
    if not isinstance(caveats, list) or not all(isinstance(item, str) and item for item in caveats):
        errors.append("caveats must be a list of non-empty strings when present")

    watch_state = entry.get("watch_state")
    if watch_state is not None and (not isinstance(watch_state, str) or not watch_state):
        errors.append("watch_state must be a non-empty string when present")

    refs = entry.get("source_refs")
    if not isinstance(refs, dict):
        errors.append("source_refs must be an object")
    else:
        repo = refs.get("repo")
        if not isinstance(repo, str) or "/" not in repo:
            errors.append("source_refs.repo must be owner/name")
        items = refs.get("items", [])
        if items and (
            not isinstance(items, list)
            or not all(
                isinstance(item, dict)
                and item.get("kind") in SOURCE_ITEM_KINDS
                and isinstance(item.get("title"), str)
                and item["title"]
                and isinstance(item.get("url"), str)
                and item["url"].startswith("https://")
                and ("meta" not in item or isinstance(item.get("meta"), str))
                for item in items
            )
        ):
            errors.append("source_refs.items must be a list of titled source entries")
        pr_url = refs.get("pr_url")
        commit_urls = refs.get("commit_urls", [])
        if pr_url is None and not commit_urls and not items:
            errors.append("source_refs must include pr_url, commit URLs, or source_refs.items")
        if pr_url is not None and (not isinstance(pr_url, str) or not pr_url.startswith("https://")):
            errors.append("source_refs.pr_url must be an https URL when present")
        if commit_urls and (
            not isinstance(commit_urls, list)
            or not all(isinstance(url, str) and url.startswith("https://") for url in commit_urls)
        ):
            errors.append("source_refs.commit_urls must be a list of https URLs")

    return ValidationResult(ok=not errors, errors=errors)


def validate_release_delta(entry: dict[str, Any]) -> ValidationResult:
    errors: list[str] = []

    if entry.get("schema") != RELEASE_DELTA_SCHEMA:
        errors.append(f"schema must be {RELEASE_DELTA_SCHEMA}")

    repo = entry.get("repo")
    if not isinstance(repo, str) or "/" not in repo:
        errors.append("repo must be owner/name")

    tag_prefix = entry.get("tag_prefix")
    if not isinstance(tag_prefix, str) or not tag_prefix:
        errors.append("tag_prefix must be a non-empty string")

    generated_at = entry.get("generated_at")
    if not isinstance(generated_at, str) or not generated_at:
        errors.append("generated_at must be a non-empty string")

    for field_name, expect_prerelease in (
        ("stable_release", False),
        ("prerelease", True),
    ):
        release = entry.get(field_name)
        if not isinstance(release, dict):
            errors.append(f"{field_name} must be an object")
            continue
        for field in ("tag_name", "name", "published_at", "url"):
            if not isinstance(release.get(field), str) or not release[field]:
                errors.append(f"{field_name}.{field} must be a non-empty string")
        tag_name = release.get("tag_name")
        if isinstance(tag_name, str) and isinstance(tag_prefix, str) and not tag_name.startswith(tag_prefix):
            errors.append(f"{field_name}.tag_name must start with tag_prefix")
        prerelease = release.get("prerelease")
        if prerelease is not expect_prerelease:
            expected = "true" if expect_prerelease else "false"
            errors.append(f"{field_name}.prerelease must be {expected}")

    compare = entry.get("compare")
    if not isinstance(compare, dict):
        errors.append("compare must be an object")
    else:
        status = compare.get("status")
        if not isinstance(status, str) or not status:
            errors.append("compare.status must be a non-empty string")
        for field in ("ahead_by", "total_commits"):
            if not isinstance(compare.get(field), int):
                errors.append(f"compare.{field} must be an integer")
        url = compare.get("url")
        if not isinstance(url, str) or not url.startswith("https://"):
            errors.append("compare.url must be an https URL")
        commit_shas = compare.get("commit_shas", [])
        if commit_shas and (
            not isinstance(commit_shas, list)
            or not all(isinstance(sha, str) and sha for sha in commit_shas)
        ):
            errors.append("compare.commit_shas must be a list of non-empty strings")
        pr_numbers = compare.get("pr_numbers", [])
        if pr_numbers and (
            not isinstance(pr_numbers, list)
            or not all(isinstance(number, int) and number > 0 for number in pr_numbers)
        ):
            errors.append("compare.pr_numbers must be a list of positive integers")

    tracked_signal_slugs = entry.get("tracked_signal_slugs")
    if not isinstance(tracked_signal_slugs, list):
        errors.append("tracked_signal_slugs must be a list")
    elif not all(isinstance(slug, str) and slug for slug in tracked_signal_slugs):
        errors.append("tracked_signal_slugs must contain only non-empty strings")

    release_options = entry.get("release_options")
    stable_tags: set[str] = set()
    preview_tags: set[str] = set()
    if not isinstance(release_options, dict):
        errors.append("release_options must be an object")
    else:
        stable_options = release_options.get("stable")
        preview_options = release_options.get("preview")
        if not isinstance(stable_options, list) or not stable_options:
            errors.append("release_options.stable must be a non-empty list")
        else:
            for index, release in enumerate(stable_options):
                if not isinstance(release, dict):
                    errors.append(f"release_options.stable[{index}] must be an object")
                    continue
                tag_name = release.get("tag_name")
                if not isinstance(tag_name, str) or not tag_name:
                    errors.append(f"release_options.stable[{index}].tag_name must be a non-empty string")
                else:
                    stable_tags.add(tag_name)
                if release.get("prerelease") is not False:
                    errors.append(f"release_options.stable[{index}].prerelease must be false")
        if not isinstance(preview_options, list) or not preview_options:
            errors.append("release_options.preview must be a non-empty list")
        else:
            for index, release in enumerate(preview_options):
                if not isinstance(release, dict):
                    errors.append(f"release_options.preview[{index}] must be an object")
                    continue
                tag_name = release.get("tag_name")
                if not isinstance(tag_name, str) or not tag_name:
                    errors.append(f"release_options.preview[{index}].tag_name must be a non-empty string")
                else:
                    preview_tags.add(tag_name)
                if release.get("prerelease") is not True:
                    errors.append(f"release_options.preview[{index}].prerelease must be true")

    comparisons = entry.get("comparisons")
    has_default_comparison = False
    stable_release = entry.get("stable_release") if isinstance(entry.get("stable_release"), dict) else {}
    prerelease = entry.get("prerelease") if isinstance(entry.get("prerelease"), dict) else {}
    if not isinstance(comparisons, list) or not comparisons:
        errors.append("comparisons must be a non-empty list")
    else:
        for index, comparison in enumerate(comparisons):
            if not isinstance(comparison, dict):
                errors.append(f"comparisons[{index}] must be an object")
                continue
            stable_tag_name = comparison.get("stable_tag_name")
            preview_tag_name = comparison.get("prerelease_tag_name")
            if not isinstance(stable_tag_name, str) or not stable_tag_name:
                errors.append(f"comparisons[{index}].stable_tag_name must be a non-empty string")
            elif stable_tags and stable_tag_name not in stable_tags:
                errors.append(f"comparisons[{index}].stable_tag_name must exist in release_options.stable")
            if not isinstance(preview_tag_name, str) or not preview_tag_name:
                errors.append(f"comparisons[{index}].prerelease_tag_name must be a non-empty string")
            elif preview_tags and preview_tag_name not in preview_tags:
                errors.append(f"comparisons[{index}].prerelease_tag_name must exist in release_options.preview")
            if (
                stable_tag_name == stable_release.get("tag_name")
                and preview_tag_name == prerelease.get("tag_name")
            ):
                has_default_comparison = True

            comparison_compare = comparison.get("compare")
            if not isinstance(comparison_compare, dict):
                errors.append(f"comparisons[{index}].compare must be an object")
            else:
                status = comparison_compare.get("status")
                if not isinstance(status, str) or not status:
                    errors.append(f"comparisons[{index}].compare.status must be a non-empty string")
                for field in ("ahead_by", "total_commits"):
                    if not isinstance(comparison_compare.get(field), int):
                        errors.append(f"comparisons[{index}].compare.{field} must be an integer")
                url = comparison_compare.get("url")
                if not isinstance(url, str) or not url.startswith("https://"):
                    errors.append(f"comparisons[{index}].compare.url must be an https URL")
                commit_shas = comparison_compare.get("commit_shas", [])
                if commit_shas and (
                    not isinstance(commit_shas, list)
                    or not all(isinstance(sha, str) and sha for sha in commit_shas)
                ):
                    errors.append(f"comparisons[{index}].compare.commit_shas must be a list of non-empty strings")
                pr_numbers = comparison_compare.get("pr_numbers", [])
                if pr_numbers and (
                    not isinstance(pr_numbers, list)
                    or not all(isinstance(number, int) and number > 0 for number in pr_numbers)
                ):
                    errors.append(f"comparisons[{index}].compare.pr_numbers must be a list of positive integers")

            comparison_slugs = comparison.get("tracked_signal_slugs")
            if not isinstance(comparison_slugs, list):
                errors.append(f"comparisons[{index}].tracked_signal_slugs must be a list")
            elif not all(isinstance(slug, str) and slug for slug in comparison_slugs):
                errors.append(f"comparisons[{index}].tracked_signal_slugs must contain only non-empty strings")

    if comparisons and not has_default_comparison:
        errors.append("comparisons must include the default stable/prerelease pair")

    return ValidationResult(ok=not errors, errors=errors)


def validate_upstream_review_queue(entry: dict[str, Any]) -> ValidationResult:
    errors: list[str] = []

    if entry.get("schema") != UPSTREAM_REVIEW_QUEUE_SCHEMA:
        errors.append(f"schema must be {UPSTREAM_REVIEW_QUEUE_SCHEMA}")

    repo = entry.get("repo")
    if not isinstance(repo, str) or "/" not in repo:
        errors.append("repo must be owner/name")

    if not isinstance(entry.get("generated_at"), str) or not entry["generated_at"]:
        errors.append("generated_at must be a non-empty string")

    source = entry.get("source")
    if not isinstance(source, dict):
        errors.append("source must be an object")
    else:
        if not isinstance(source.get("default_branch"), str) or not source["default_branch"]:
            errors.append("source.default_branch must be a non-empty string")
        if not isinstance(source.get("search_limit"), int) or source["search_limit"] < 1:
            errors.append("source.search_limit must be a positive integer")

    subjects = entry.get("subjects")
    if not isinstance(subjects, list):
        errors.append("subjects must be a list")
        subjects = []

    seen: set[tuple[str, str]] = set()
    for index, subject in enumerate(subjects):
        if not isinstance(subject, dict):
            errors.append(f"subjects[{index}] must be an object")
            continue
        subject_kind = subject.get("subject_kind")
        subject_id = subject.get("subject_id")
        if subject_kind not in UPSTREAM_SUBJECT_KINDS:
            errors.append(f"subjects[{index}].subject_kind must be one of {sorted(UPSTREAM_SUBJECT_KINDS)}")
        if not isinstance(subject_id, str) or not subject_id:
            errors.append(f"subjects[{index}].subject_id must be a non-empty string")
        if isinstance(subject_kind, str) and isinstance(subject_id, str):
            key = (subject_kind, subject_id)
            if key in seen:
                errors.append(f"subjects[{index}] duplicates {subject_kind}:{subject_id}")
            seen.add(key)

        for field in ("title", "url", "review_reason"):
            if not isinstance(subject.get(field), str) or not subject[field]:
                errors.append(f"subjects[{index}].{field} must be a non-empty string")
        if isinstance(subject.get("url"), str) and not subject["url"].startswith("https://"):
            errors.append(f"subjects[{index}].url must be an https URL")
        if subject.get("source_state") not in UPSTREAM_SOURCE_STATES:
            errors.append(f"subjects[{index}].source_state must be one of {sorted(UPSTREAM_SOURCE_STATES)}")
        if subject.get("review_priority") not in UPSTREAM_REVIEW_PRIORITIES:
            errors.append(f"subjects[{index}].review_priority must be one of {sorted(UPSTREAM_REVIEW_PRIORITIES)}")
        if subject.get("next_step") not in UPSTREAM_REVIEW_NEXT_STEPS:
            errors.append(f"subjects[{index}].next_step must be one of {sorted(UPSTREAM_REVIEW_NEXT_STEPS)}")

        commit_shas = subject.get("commit_shas")
        if (
            not isinstance(commit_shas, list)
            or not commit_shas
            or not all(isinstance(item, str) and item for item in commit_shas)
        ):
            errors.append(f"subjects[{index}].commit_shas must be a non-empty list of strings")

        for list_field in ("surface_hints", "attention_flags", "sample_paths"):
            values = subject.get(list_field)
            if values is not None and (
                not isinstance(values, list)
                or not all(isinstance(item, str) and item for item in values)
            ):
                errors.append(f"subjects[{index}].{list_field} must be a list of non-empty strings")

        changed_file_count = subject.get("changed_file_count")
        if not isinstance(changed_file_count, int) or changed_file_count < 0:
            errors.append(f"subjects[{index}].changed_file_count must be a non-negative integer")

    counts = entry.get("counts")
    if not isinstance(counts, dict):
        errors.append("counts must be an object")
    else:
        queued = counts.get("subjects_queued")
        if not isinstance(queued, int) or queued != len(subjects):
            errors.append("counts.subjects_queued must equal len(subjects)")
        for field in ("recent_commits_scanned", "published_subjects_seen", "critical", "high", "normal", "low"):
            value = counts.get(field)
            if not isinstance(value, int) or value < 0:
                errors.append(f"counts.{field} must be a non-negative integer")

    return ValidationResult(ok=not errors, errors=errors)


def validate_upstream_review(entry: dict[str, Any]) -> ValidationResult:
    errors: list[str] = []

    if entry.get("schema") != UPSTREAM_REVIEW_SCHEMA:
        errors.append(f"schema must be {UPSTREAM_REVIEW_SCHEMA}")

    for field in ("slug", "repo", "reviewed_at", "observed_change"):
        if not isinstance(entry.get(field), str) or not entry[field]:
            errors.append(f"{field} must be a non-empty string")
    repo = entry.get("repo")
    if isinstance(repo, str) and "/" not in repo:
        errors.append("repo must be owner/name")

    subject = entry.get("subject")
    if not isinstance(subject, dict):
        errors.append("subject must be an object")
    else:
        if subject.get("subject_kind") not in UPSTREAM_SUBJECT_KINDS:
            errors.append(f"subject.subject_kind must be one of {sorted(UPSTREAM_SUBJECT_KINDS)}")
        if not isinstance(subject.get("subject_id"), str) or not subject["subject_id"]:
            errors.append("subject.subject_id must be a non-empty string")
        commit_shas = subject.get("commit_shas")
        if commit_shas is not None and (
            not isinstance(commit_shas, list)
            or not all(isinstance(item, str) and item for item in commit_shas)
        ):
            errors.append("subject.commit_shas must be a list of non-empty strings when present")

    refs = entry.get("source_refs")
    if not isinstance(refs, dict):
        errors.append("source_refs must be an object")
    else:
        items = refs.get("items")
        if (
            not isinstance(items, list)
            or not items
            or not all(
                isinstance(item, dict)
                and isinstance(item.get("kind"), str)
                and isinstance(item.get("title"), str)
                and item["title"]
                and isinstance(item.get("url"), str)
                and item["url"].startswith("https://")
                for item in items
            )
        ):
            errors.append("source_refs.items must be a non-empty list of titled https source entries")

    for list_field in ("changed_surfaces", "evidence"):
        values = entry.get(list_field)
        if (
            not isinstance(values, list)
            or not values
            or not all(isinstance(item, str) and item for item in values)
        ):
            errors.append(f"{list_field} must be a non-empty list of strings")

    for optional_field in (
        "user_visible_path",
        "control_plane_relevance",
        "compatibility_risk",
        "adoption_opportunity",
        "community_value",
        "deprecated_or_breaking_notes",
        "caveats",
    ):
        value = entry.get(optional_field)
        if value is not None and not isinstance(value, str):
            errors.append(f"{optional_field} must be a string when present")

    if entry.get("confidence") not in SIGNAL_CONFIDENCE:
        errors.append(f"confidence must be one of {sorted(SIGNAL_CONFIDENCE)}")

    next_actions = entry.get("next_actions")
    if not isinstance(next_actions, list) or not next_actions:
        errors.append("next_actions must be a non-empty list")
    else:
        for index, action in enumerate(next_actions):
            if not isinstance(action, dict):
                errors.append(f"next_actions[{index}] must be an object")
                continue
            if action.get("type") not in UPSTREAM_REVIEW_ACTION_TYPES:
                errors.append(f"next_actions[{index}].type must be one of {sorted(UPSTREAM_REVIEW_ACTION_TYPES)}")
            if not isinstance(action.get("reason"), str) or not action["reason"]:
                errors.append(f"next_actions[{index}].reason must be a non-empty string")

    return ValidationResult(ok=not errors, errors=errors)


def validate_control_plane_upgrade_candidate(entry: dict[str, Any]) -> ValidationResult:
    errors: list[str] = []

    if entry.get("schema") != CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA:
        errors.append(f"schema must be {CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA}")

    for field in ("slug", "repo", "observed_change", "reason"):
        if not isinstance(entry.get(field), str) or not entry[field]:
            errors.append(f"{field} must be a non-empty string")

    repo = entry.get("repo")
    if isinstance(repo, str) and "/" not in repo:
        errors.append("repo must be owner/name")

    if entry.get("status") not in CONTROL_PLANE_UPGRADE_STATUSES:
        errors.append(f"status must be one of {sorted(CONTROL_PLANE_UPGRADE_STATUSES)}")
    if entry.get("control_plane_impact") not in CONTROL_PLANE_UPGRADE_IMPACTS:
        errors.append(
            f"control_plane_impact must be one of {sorted(CONTROL_PLANE_UPGRADE_IMPACTS)}"
        )
    if entry.get("upgrade_path") not in CONTROL_PLANE_UPGRADE_PATHS:
        errors.append(f"upgrade_path must be one of {sorted(CONTROL_PLANE_UPGRADE_PATHS)}")

    refs = entry.get("source_refs")
    if not isinstance(refs, dict):
        errors.append("source_refs must be an object")
    else:
        present = [
            field
            for field in ("upstream_reviews", "upstream_impacts", "release_deltas", "urls")
            if isinstance(refs.get(field), list) and refs[field]
        ]
        if not present:
            errors.append(
                "source_refs must include upstream_reviews, upstream_impacts, release_deltas, or urls"
            )
        for field in ("upstream_reviews", "upstream_impacts", "release_deltas"):
            values = refs.get(field, [])
            if values and (
                not isinstance(values, list)
                or not all(isinstance(item, str) and item for item in values)
            ):
                errors.append(f"source_refs.{field} must be a list of non-empty strings")
        urls = refs.get("urls", [])
        if urls and (
            not isinstance(urls, list)
            or not all(isinstance(url, str) and url.startswith("https://") for url in urls)
        ):
            errors.append("source_refs.urls must be a list of https URLs")

    target = entry.get("target_codex")
    if not isinstance(target, dict):
        errors.append("target_codex must be an object")
    else:
        if target.get("channel") not in CODEX_TARGET_CHANNELS:
            errors.append(
                f"target_codex.channel must be one of {sorted(CODEX_TARGET_CHANNELS)}"
            )
        if not any(
            isinstance(target.get(field), str) and target[field]
            for field in ("version", "tag", "commit_sha", "release_url")
        ):
            errors.append("target_codex must include version, tag, commit_sha, or release_url")
        release_url = target.get("release_url")
        if release_url is not None and (
            not isinstance(release_url, str) or not release_url.startswith("https://")
        ):
            errors.append("target_codex.release_url must be an https URL when present")
        compatibility_status = target.get("compatibility_status")
        if (
            compatibility_status is not None
            and compatibility_status not in CODEX_COMPATIBILITY_STATUSES
        ):
            errors.append(
                "target_codex.compatibility_status must be one of "
                f"{sorted(CODEX_COMPATIBILITY_STATUSES)}"
            )
        for field in ("version", "tag", "commit_sha", "matrix_ref", "probe_evidence"):
            value = target.get(field)
            if value is not None and (not isinstance(value, str) or not value):
                errors.append(f"target_codex.{field} must be non-empty when present")

    authority = entry.get("authority")
    if not isinstance(authority, dict):
        errors.append("authority must be an object")
    else:
        if authority.get("decision_contract_required") is not True:
            errors.append("authority.decision_contract_required must be true")
        if authority.get("program_intake_required") is not True:
            errors.append("authority.program_intake_required must be true")
        if authority.get("mutation_allowed") is not False:
            errors.append("authority.mutation_allowed must be false")
        for field in ("objective_id", "objective_version", "policy_ref"):
            value = authority.get(field)
            if value is not None and (not isinstance(value, str) or not value):
                errors.append(f"authority.{field} must be non-empty when present")

    for list_field in ("affected_surfaces", "validation_gates", "stop_conditions"):
        values = entry.get(list_field)
        if (
            not isinstance(values, list)
            or not values
            or not all(isinstance(item, str) and item for item in values)
        ):
            errors.append(f"{list_field} must be a non-empty list of strings")

    for list_field in ("acceptance_criteria", "caveats", "next_steps"):
        values = entry.get(list_field, [])
        if values is not None and (
            not isinstance(values, list)
            or not all(isinstance(item, str) and item for item in values)
        ):
            errors.append(f"{list_field} must be a list of non-empty strings when present")

    return ValidationResult(ok=not errors, errors=errors)


def validate_social_text_list(value: Any, field: str, errors: list[str]) -> None:
    if not isinstance(value, list) or not value:
        errors.append(f"{field} must be a non-empty list of X-sized strings")
        return

    for index, item in enumerate(value):
        if not isinstance(item, str):
            errors.append(f"{field}[{index}] must be a string")
            continue
        validate_social_text_item(item, f"{field}[{index}]", errors)


def validate_social_text_item(text: str, label: str, errors: list[str]) -> None:
    if not text or len(text) > 280:
        errors.append(f"{label} must be a non-empty X-sized string")
    if "Automated by @hackink" in text:
        errors.append(f"{label} must not include automation attribution")
    if len(text) > 260 and "https://" not in text:
        errors.append(
            f"{label} longer than 260 characters must include an unavoidable direct source URL"
        )

    normalized = text.strip().lower()
    if (
        normalized == "watching this"
        or normalized.startswith("watching this.")
        or normalized.startswith("tracking this.")
        or "new release available" in normalized
    ):
        errors.append(
            f"{label} must name a concrete source-backed release, PR, protocol surface, "
            "workflow impact, or operator action"
        )


def validate_social_candidate(entry: dict[str, Any]) -> ValidationResult:
    errors: list[str] = []

    if entry.get("schema") != SOCIAL_CANDIDATE_SCHEMA:
        errors.append(f"schema must be {SOCIAL_CANDIDATE_SCHEMA}")

    for field in ("slug", "repo", "audience"):
        if not isinstance(entry.get(field), str) or not entry[field]:
            errors.append(f"{field} must be a non-empty string")

    if isinstance(entry.get("repo"), str) and "/" not in entry["repo"]:
        errors.append("repo must be owner/name")
    if entry.get("channel") != "x":
        errors.append("channel must be x")
    if entry.get("target_account") != "decodexspace":
        errors.append("target_account must be decodexspace")
    if entry.get("mode") not in SOCIAL_POST_MODES:
        errors.append(f"mode must be one of {sorted(SOCIAL_POST_MODES)}")
    if entry.get("priority") not in SOCIAL_POST_PRIORITIES:
        errors.append(f"priority must be one of {sorted(SOCIAL_POST_PRIORITIES)}")

    validate_social_text_list(entry.get("candidate_text"), "candidate_text", errors)

    refs = entry.get("source_refs")
    if not isinstance(refs, dict):
        errors.append("source_refs must be an object")
    else:
        present = [
            name
            for name in ("signals", "upstream_impacts", "upstream_reviews", "release_deltas", "urls")
            if isinstance(refs.get(name), list) and refs[name]
        ]
        if not present:
            errors.append(
                "source_refs must include signals, upstream_impacts, upstream_reviews, release_deltas, or urls"
            )
        urls = refs.get("urls", [])
        if urls and (
            not isinstance(urls, list)
            or not all(isinstance(url, str) and url.startswith("https://") for url in urls)
        ):
            errors.append("source_refs.urls must be a list of https URLs")
    for list_field in ("evidence_notes", "claims"):
        values = entry.get(list_field)
        if not isinstance(values, list) or not values:
            errors.append(f"{list_field} must be a non-empty list")

    claims = entry.get("claims")
    if isinstance(claims, list):
        for index, claim in enumerate(claims):
            if not isinstance(claim, dict):
                errors.append(f"claims[{index}] must be an object")
                continue
            for field in ("text", "evidence"):
                if not isinstance(claim.get(field), str) or not claim[field]:
                    errors.append(f"claims[{index}].{field} must be a non-empty string")
            if claim.get("confidence") not in SIGNAL_CONFIDENCE:
                errors.append(f"claims[{index}].confidence must be one of {sorted(SIGNAL_CONFIDENCE)}")

    decision = entry.get("decision")
    if not isinstance(decision, dict):
        errors.append("decision must be an object")
    else:
        if decision.get("worthiness") not in {"publish", "defer", "skip"}:
            errors.append("decision.worthiness must be one of ['defer', 'publish', 'skip']")
        for field in ("idempotency_key", "reason"):
            if not isinstance(decision.get(field), str) or not decision[field]:
                errors.append(f"decision.{field} must be a non-empty string")

    caveats = entry.get("caveats", [])
    if caveats is not None and (
        not isinstance(caveats, list) or not all(isinstance(item, str) and item for item in caveats)
    ):
        errors.append("caveats must be a list of non-empty strings when present")

    next_steps = entry.get("next_steps", [])
    if next_steps is not None and (
        not isinstance(next_steps, list) or not all(isinstance(item, str) and item for item in next_steps)
    ):
        errors.append("next_steps must be a list of non-empty strings when present")

    return ValidationResult(ok=not errors, errors=errors)


def validate_social_publish_reservation(entry: dict[str, Any]) -> ValidationResult:
    errors: list[str] = []

    if entry.get("schema") != SOCIAL_PUBLISH_RESERVATION_SCHEMA:
        errors.append(f"schema must be {SOCIAL_PUBLISH_RESERVATION_SCHEMA}")

    for field in ("slug", "idempotency_key", "reserved_at", "expires_at", "day", "timezone"):
        if not isinstance(entry.get(field), str) or not entry[field]:
            errors.append(f"{field} must be a non-empty string")

    if entry.get("channel") != "x":
        errors.append("channel must be x")
    if entry.get("target_account") != "decodexspace":
        errors.append("target_account must be decodexspace")
    if entry.get("controller_account") != "hackink":
        errors.append("controller_account must be hackink")
    if entry.get("mode") not in SOCIAL_POST_MODES:
        errors.append(f"mode must be one of {sorted(SOCIAL_POST_MODES)}")
    if entry.get("status") not in SOCIAL_PUBLISH_RESERVATION_STATUSES:
        errors.append(f"status must be one of {sorted(SOCIAL_PUBLISH_RESERVATION_STATUSES)}")

    refs = entry.get("candidate_refs")
    if not isinstance(refs, dict):
        errors.append("candidate_refs must be an object")
    else:
        present = [
            name
            for name in ("social_candidates", "urls")
            if isinstance(refs.get(name), list) and refs[name]
        ]
        if not present:
            errors.append("candidate_refs must include social_candidates or urls")
        social_candidates = refs.get("social_candidates", [])
        if social_candidates and (
            not isinstance(social_candidates, list)
            or not all(isinstance(item, str) and item for item in social_candidates)
        ):
            errors.append("candidate_refs.social_candidates must be a list of non-empty strings")
        urls = refs.get("urls", [])
        if urls and (
            not isinstance(urls, list)
            or not all(isinstance(url, str) and url.startswith("https://") for url in urls)
        ):
            errors.append("candidate_refs.urls must be a list of https URLs")

    duplicate_keys = entry.get("duplicate_keys")
    if (
        not isinstance(duplicate_keys, list)
        or not duplicate_keys
        or not all(isinstance(item, str) and item for item in duplicate_keys)
    ):
        errors.append("duplicate_keys must be a non-empty list of strings")

    for field in ("reserved_at", "expires_at"):
        value = entry.get(field)
        if isinstance(value, str) and value:
            try:
                datetime.fromisoformat(value.replace("Z", "+00:00"))
            except ValueError:
                errors.append(f"{field} must be an RFC3339 timestamp")

    owner = entry.get("owner")
    if owner is not None:
        if not isinstance(owner, dict):
            errors.append("owner must be an object when present")
        else:
            for field in ("automation_id", "branch", "pr_url", "run_id"):
                value = owner.get(field)
                if value is not None and (not isinstance(value, str) or not value):
                    errors.append(f"owner.{field} must be non-empty when present")
            pr_url = owner.get("pr_url")
            if pr_url is not None and (not isinstance(pr_url, str) or not pr_url.startswith("https://")):
                errors.append("owner.pr_url must be an https URL when present")

    evidence_notes = entry.get("evidence_notes", [])
    if evidence_notes is not None and (
        not isinstance(evidence_notes, list)
        or not all(isinstance(item, str) and item for item in evidence_notes)
    ):
        errors.append("evidence_notes must be a list of non-empty strings when present")

    status = entry.get("status")
    if status == "consumed" and (
        not isinstance(entry.get("consumed_by_social_post"), str) or not entry["consumed_by_social_post"]
    ):
        errors.append("consumed_by_social_post is required when status is consumed")
    if status in {"canceled", "expired"} and (
        not isinstance(entry.get("release_reason"), str) or not entry["release_reason"]
    ):
        errors.append("release_reason is required when status is canceled or expired")

    return ValidationResult(ok=not errors, errors=errors)


def validate_social_post(entry: dict[str, Any]) -> ValidationResult:
    errors: list[str] = []

    if entry.get("schema") != SOCIAL_POST_SCHEMA:
        errors.append(f"schema must be {SOCIAL_POST_SCHEMA}")

    for field in ("slug", "audience"):
        if not isinstance(entry.get(field), str) or not entry[field]:
            errors.append(f"{field} must be a non-empty string")

    if entry.get("channel") != "x":
        errors.append("channel must be x")
    if entry.get("target_account") != "decodexspace":
        errors.append("target_account must be decodexspace")
    if entry.get("controller_account") != "hackink":
        errors.append("controller_account must be hackink")
    if entry.get("mode") not in SOCIAL_POST_MODES:
        errors.append(f"mode must be one of {sorted(SOCIAL_POST_MODES)}")
    if entry.get("status") not in SOCIAL_POST_STATUSES:
        errors.append(f"status must be one of {sorted(SOCIAL_POST_STATUSES)}")

    text = entry.get("text")
    validate_social_text_list(text, "text", errors)

    refs = entry.get("source_refs")
    if not isinstance(refs, dict):
        errors.append("source_refs must be an object")
    else:
        present = [
            name
            for name in (
                "reservations",
                "signals",
                "social_candidates",
                "upstream_impacts",
                "upstream_reviews",
                "urls",
            )
            if isinstance(refs.get(name), list) and refs[name]
        ]
        if not present:
            errors.append(
                "source_refs must include reservations, signals, social_candidates, "
                "upstream_impacts, upstream_reviews, or urls"
            )
        urls = refs.get("urls", [])
        if urls and (
            not isinstance(urls, list)
            or not all(isinstance(url, str) and url.startswith("https://") for url in urls)
        ):
            errors.append("source_refs.urls must be a list of https URLs")
        for field in (
            "reservations",
            "signals",
            "social_candidates",
            "upstream_impacts",
            "upstream_reviews",
        ):
            values = refs.get(field, [])
            if values and (
                not isinstance(values, list)
                or not all(isinstance(item, str) and item for item in values)
            ):
                errors.append(f"source_refs.{field} must be a list of non-empty strings")

    for list_field in ("evidence_notes", "claims"):
        values = entry.get(list_field)
        if not isinstance(values, list) or not values:
            errors.append(f"{list_field} must be a non-empty list")

    claims = entry.get("claims")
    if isinstance(claims, list):
        for index, claim in enumerate(claims):
            if not isinstance(claim, dict):
                errors.append(f"claims[{index}] must be an object")
                continue
            for field in ("text", "evidence"):
                if not isinstance(claim.get(field), str) or not claim[field]:
                    errors.append(f"claims[{index}].{field} must be a non-empty string")
            if claim.get("confidence") not in SIGNAL_CONFIDENCE:
                errors.append(f"claims[{index}].confidence must be one of {sorted(SIGNAL_CONFIDENCE)}")

    decision = entry.get("decision")
    if not isinstance(decision, dict):
        errors.append("decision must be an object")
    else:
        if decision.get("worthiness") not in SOCIAL_POST_WORTHINESS:
            errors.append(f"decision.worthiness must be one of {sorted(SOCIAL_POST_WORTHINESS)}")
        if decision.get("priority") not in SOCIAL_POST_PRIORITIES:
            errors.append(f"decision.priority must be one of {sorted(SOCIAL_POST_PRIORITIES)}")
        for field in ("idempotency_key", "reason", "day", "timezone"):
            if not isinstance(decision.get(field), str) or not decision[field]:
                errors.append(f"decision.{field} must be a non-empty string")
        if decision.get("daily_limit") != 8:
            errors.append("decision.daily_limit must be 8")
        for field in ("daily_count_before", "daily_count_after"):
            value = decision.get(field)
            if not isinstance(value, int) or value < 0:
                errors.append(f"decision.{field} must be a non-negative integer")
        before = decision.get("daily_count_before")
        after = decision.get("daily_count_after")
        status = entry.get("status")
        post_count = len(text) if isinstance(text, list) else 0
        if isinstance(before, int) and isinstance(after, int):
            if status == "published" and after != before + post_count:
                errors.append("decision.daily_count_after must add the published post count")
            if status != "published" and after != before:
                errors.append("decision.daily_count_after must remain unchanged unless published")

    status = entry.get("status")
    if status == "published":
        publication = entry.get("publication")
        if not isinstance(publication, dict):
            errors.append("publication is required when status is published")
        else:
            if publication.get("publisher") not in {"chrome", "x_api"}:
                errors.append("publication.publisher must be chrome or x_api")
            if publication.get("account_verified") is not True:
                errors.append("publication.account_verified must be true")
            if not isinstance(publication.get("made_with_ai"), bool):
                errors.append("publication.made_with_ai must be boolean")
            if "image_template" in publication and publication.get("image_template") != "decodex_signal_card":
                errors.append("publication.image_template must be decodex_signal_card when present")
            urls = publication.get("published_urls")
            if (
                not isinstance(urls, list)
                or not urls
                or not all(isinstance(url, str) and url.startswith("https://") for url in urls)
            ):
                errors.append("publication.published_urls must be a non-empty list of https URLs")
            if not isinstance(publication.get("posted_at"), str) or not publication["posted_at"]:
                errors.append("publication.posted_at must be a non-empty string")
    elif status == "blocked":
        block = entry.get("block")
        if not isinstance(block, dict):
            errors.append("block is required when status is blocked")
        else:
            if block.get("reason") not in SOCIAL_BLOCK_REASONS:
                errors.append(f"block.reason must be one of {sorted(SOCIAL_BLOCK_REASONS)}")
            count_before = decision.get("daily_count_before") if isinstance(decision, dict) else None
            if block.get("reason") == "daily_cap_exceeded" and (
                not isinstance(count_before, int) or count_before < 8
            ):
                errors.append("daily_cap_exceeded requires decision.daily_count_before >= 8")
            if not isinstance(block.get("operator_notice"), str) or not block["operator_notice"]:
                errors.append("block.operator_notice must be a non-empty string")
    elif status == "failed":
        failure = entry.get("failure")
        if not isinstance(failure, dict):
            errors.append("failure is required when status is failed")
    elif status == "skipped" and not isinstance(entry.get("skip"), dict):
        errors.append("skip is required when status is skipped")

    lifecycle = entry.get("post_lifecycle")
    if lifecycle is not None:
        if not isinstance(lifecycle, dict):
            errors.append("post_lifecycle must be an object when present")
        else:
            current_state = lifecycle.get("current_state")
            quote_eligible = lifecycle.get("quote_eligible")
            if current_state not in SOCIAL_POST_LIFECYCLE_STATES:
                errors.append(
                    "post_lifecycle.current_state must be one of "
                    f"{sorted(SOCIAL_POST_LIFECYCLE_STATES)}"
                )
            if not isinstance(quote_eligible, bool):
                errors.append("post_lifecycle.quote_eligible must be boolean")
            if not isinstance(lifecycle.get("reason"), str) or not lifecycle["reason"]:
                errors.append("post_lifecycle.reason must be a non-empty string")
            superseded_by = lifecycle.get("superseded_by_candidate")
            if superseded_by is not None and (not isinstance(superseded_by, str) or not superseded_by):
                errors.append("post_lifecycle.superseded_by_candidate must be non-empty when present")
            if quote_eligible is True and (status != "published" or current_state != "live"):
                errors.append("post_lifecycle.quote_eligible can be true only for live published posts")
            if isinstance(current_state, str) and current_state.startswith("superseded") and superseded_by is None:
                errors.append(
                    "post_lifecycle.superseded_by_candidate is required for superseded states"
                )

    for list_field in ("caveats", "media_refs"):
        values = entry.get(list_field, [])
        if values is not None and (
            not isinstance(values, list)
            or not all(isinstance(item, str) and item for item in values)
        ):
            errors.append(f"{list_field} must be a list of non-empty strings when present")

    return ValidationResult(ok=not errors, errors=errors)
