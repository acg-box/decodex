from __future__ import annotations

from typing import Any

from contract_support.constants import (
    ANALYSIS_MODES,
    BUNDLE_SCHEMA,
    SIGNAL_CONFIDENCE,
    SIGNAL_IMPACT,
    SIGNAL_KINDS,
)
from contract_support.core import ValidationResult


def validate_bundle(bundle: Any) -> ValidationResult:
    if not isinstance(bundle, dict):
        return ValidationResult(ok=False, errors=["artifact must be an object"])
    errors: list[str] = []

    if bundle.get("schema") != BUNDLE_SCHEMA:
        errors.append(f"schema must be {BUNDLE_SCHEMA}")

    if not isinstance(bundle.get("repo"), str) or "/" not in bundle["repo"]:
        errors.append("repo must be owner/name")

    if not isinstance(bundle.get("analysis_mode"), str) or bundle["analysis_mode"] not in ANALYSIS_MODES:
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
                    continue
                value = item[field]
                if field in ("path", "status"):
                    valid = isinstance(value, str) and bool(value)
                    expected = "a non-empty string"
                else:
                    valid = type(value) is int and 0 <= value <= 2**63 - 1
                    expected = "a non-negative integer"
                if not valid:
                    errors.append(f"files[{index}].{field} must be {expected}")

    if bundle.get("analysis_mode") == "pr_first":
        pr = bundle.get("primary_pr")
        if not isinstance(pr, dict):
            errors.append("primary_pr is required when analysis_mode is pr_first")
        else:
            for field in ("number", "title", "body", "state", "labels", "url"):
                if field not in pr:
                    errors.append(f"primary_pr.{field} is required")
                    continue
                value = pr[field]
                if field == "number":
                    valid = type(value) is int and 0 < value <= 2**63 - 1
                    expected = "a positive integer"
                elif field == "body":
                    valid = isinstance(value, str)
                    expected = "a string"
                elif field == "labels":
                    valid = isinstance(value, list) and all(isinstance(label, str) and label for label in value)
                    expected = "a list of non-empty strings"
                else:
                    valid = isinstance(value, str) and bool(value)
                    expected = "a non-empty string"
                if not valid:
                    errors.append(f"primary_pr.{field} must be {expected}")

    return ValidationResult(ok=not errors, errors=errors)


def validate_analysis_draft(draft: Any) -> ValidationResult:
    if not isinstance(draft, dict):
        return ValidationResult(ok=False, errors=["Analysis draft must be an object"])
    errors: list[str] = []
    for field in ("kind", "title", "summary", "why_it_matters", "confidence", "impact"):
        if not isinstance(draft.get(field), str) or not draft[field]:
            errors.append(f"{field} is required in analysis draft")

    if not isinstance(draft.get("kind"), str) or draft["kind"] not in SIGNAL_KINDS:
        errors.append(f"kind must be one of {sorted(SIGNAL_KINDS)}")
    if not isinstance(draft.get("confidence"), str) or draft["confidence"] not in SIGNAL_CONFIDENCE:
        errors.append(f"confidence must be one of {sorted(SIGNAL_CONFIDENCE)}")
    if not isinstance(draft.get("impact"), str) or draft["impact"] not in SIGNAL_IMPACT:
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
