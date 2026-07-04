from __future__ import annotations

from typing import Any

from contract_support.constants import (
    CODEX_COMPATIBILITY_STATUSES,
    CODEX_TARGET_CHANNELS,
    CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA,
    CONTROL_PLANE_UPGRADE_IMPACTS,
    CONTROL_PLANE_UPGRADE_PATHS,
    CONTROL_PLANE_UPGRADE_STATUSES,
)
from contract_support.core import ValidationResult


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
