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
ANALYSIS_MODES = {"pr_first", "commit_only"}
SIGNAL_KINDS = {"capability", "behavior_change", "try_now"}
SIGNAL_CONFIDENCE = {"confirmed", "likely", "weak"}
SIGNAL_IMPACT = {"low", "medium", "high"}
SOURCE_ITEM_KINDS = {"pull_request", "commit"}
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

    return ValidationResult(ok=not errors, errors=errors)
