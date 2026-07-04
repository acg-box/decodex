from __future__ import annotations

from typing import Any

from contract_support.constants import (
    ANALYSIS_MODES,
    BUNDLE_SCHEMA,
    SIGNAL_CONFIDENCE,
    SIGNAL_IMPACT,
    SIGNAL_KINDS,
    SIGNAL_SCHEMA,
    SOURCE_ITEM_KINDS,
)
from contract_support.core import ValidationResult


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
