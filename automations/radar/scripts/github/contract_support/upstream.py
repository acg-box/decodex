from __future__ import annotations

import re
from typing import Any

from contract_support.constants import (
    SIGNAL_CONFIDENCE,
    UPSTREAM_REVIEW_ACTION_TYPES,
    UPSTREAM_REVIEW_NEXT_STEPS,
    UPSTREAM_REVIEW_PRIORITIES,
    UPSTREAM_REVIEW_QUEUE_SCHEMA,
    UPSTREAM_REVIEW_SCHEMA,
    UPSTREAM_SOURCE_STATES,
    UPSTREAM_SUBJECT_KINDS,
)
from contract_support.core import ValidationResult

GIT_OBJECT_ID_RE = re.compile(r"^[0-9a-f]{40}(?:[0-9a-f]{24})?$")


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
        if not isinstance(source.get("upstream_head"), str) or not GIT_OBJECT_ID_RE.fullmatch(
            source["upstream_head"]
        ):
            errors.append("source.upstream_head must be a lowercase 40- or 64-character Git object id")
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
            or not all(isinstance(item, str) and GIT_OBJECT_ID_RE.fullmatch(item) for item in commit_shas)
            or len(commit_shas) != len(set(commit_shas))
        ):
            errors.append(f"subjects[{index}].commit_shas must be unique Git object ids")

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
    if not isinstance(entry.get("upstream_head"), str) or not GIT_OBJECT_ID_RE.fullmatch(
        entry["upstream_head"]
    ):
        errors.append("upstream_head must be a lowercase 40- or 64-character Git object id")

    subject = entry.get("subject")
    if not isinstance(subject, dict):
        errors.append("subject must be an object")
    else:
        if subject.get("subject_kind") not in UPSTREAM_SUBJECT_KINDS:
            errors.append(f"subject.subject_kind must be one of {sorted(UPSTREAM_SUBJECT_KINDS)}")
        if not isinstance(subject.get("subject_id"), str) or not subject["subject_id"]:
            errors.append("subject.subject_id must be a non-empty string")
        commit_shas = subject.get("commit_shas")
        if (
            not isinstance(commit_shas, list)
            or not commit_shas
            or not all(isinstance(item, str) and GIT_OBJECT_ID_RE.fullmatch(item) for item in commit_shas)
            or len(commit_shas) != len(set(commit_shas))
        ):
            errors.append("subject.commit_shas must be a non-empty list of unique Git object ids")

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
