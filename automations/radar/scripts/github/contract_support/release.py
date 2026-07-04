from __future__ import annotations

from typing import Any

from contract_support.constants import RELEASE_DELTA_SCHEMA
from contract_support.core import ValidationResult


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
