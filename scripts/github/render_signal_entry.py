#!/usr/bin/env python3
"""Render a final Decodex signal entry from a GitHub bundle plus local analysis."""

from __future__ import annotations

import argparse
import sys
import json
import re
from pathlib import Path
from typing import Any

SCRIPT_HOME = Path(__file__).resolve().parent
REPO_HOME = SCRIPT_HOME.parent.parent
CONFIG_FEATURE_CATALOG_PATH = REPO_HOME / "site/src/generated/codex-config-features.json"
if str(SCRIPT_HOME) not in sys.path:
    sys.path.insert(0, str(SCRIPT_HOME))

from contracts import (  # noqa: E402
    GENERIC_COMMIT_TITLES,
    SIGNAL_SCHEMA,
    dump_json,
    first_line,
    load_json,
    slugify,
    utc_now_iso,
    validate_analysis_draft,
    validate_bundle,
    validate_signal,
)

ENABLE_FEATURE_RE = re.compile(r"^--enable\s+([a-z0-9_]+)$", re.IGNORECASE)
FEATURE_PATH_RE = re.compile(r"^(?:features\.)?([a-z0-9_]+)(?:\s*=\s*true)?$", re.IGNORECASE)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", required=True, help="Path to github_change_bundle/v1 JSON.")
    parser.add_argument("--analysis", required=True, help="Path to local editorial analysis JSON.")
    parser.add_argument("--out", required=True, help="Path to write the rendered signal entry.")
    parser.add_argument("--published-at", help="Override publication timestamp.")
    return parser.parse_args()


def short_sha(value: str) -> str:
    return value[:7]


def normalized_config_flags(raw_flags: Any) -> list[str]:
    if not isinstance(raw_flags, list):
        return []

    known_features = load_known_feature_names()
    normalized: list[str] = []
    seen: set[str] = set()
    for flag in raw_flags:
        if not isinstance(flag, str):
            continue
        value = flag.strip()
        if not value or value in seen:
            continue

        feature_name = normalize_feature_flag(value, known_features)
        if feature_name:
            value = f"features.{feature_name} = true"

        actionable = (
            value.startswith("--")
            or "=" in value
            or value.endswith(".json")
            or value.endswith(".toml")
        )
        if not actionable:
            continue

        seen.add(value)
        normalized.append(value)

    return normalized


def load_known_feature_names() -> set[str]:
    if not CONFIG_FEATURE_CATALOG_PATH.exists():
        return set()
    try:
        payload = json.loads(CONFIG_FEATURE_CATALOG_PATH.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return set()

    items = payload.get("features")
    if not isinstance(items, list):
        return set()

    names: set[str] = set()
    for item in items:
        if not isinstance(item, dict):
            continue
        name = item.get("name")
        if isinstance(name, str) and name:
            names.add(name)
    return names


def normalize_feature_flag(value: str, known_features: set[str]) -> str | None:
    enable_match = ENABLE_FEATURE_RE.fullmatch(value)
    if enable_match:
        candidate = enable_match.group(1).lower()
        return candidate if candidate in known_features else None

    feature_match = FEATURE_PATH_RE.fullmatch(value)
    if feature_match:
        candidate = feature_match.group(1).lower()
        return candidate if candidate in known_features else None

    return None


def rendered_source_items(bundle: dict[str, Any]) -> list[dict[str, str]]:
    items: list[dict[str, str]] = []
    primary_pr = bundle.get("primary_pr")
    if isinstance(primary_pr, dict) and primary_pr.get("url") and primary_pr.get("title"):
        meta = primary_pr.get("number")
        item: dict[str, str] = {
            "kind": "pull_request",
            "title": first_line(primary_pr["title"]),
            "url": primary_pr["url"],
        }
        if isinstance(meta, int):
            item["meta"] = f"#{meta}"
        items.append(item)

    fallback_items: list[dict[str, str]] = []
    picked_items: list[dict[str, str]] = []
    seen_titles: set[str] = set()

    for commit in bundle["commits"]:
        title = first_line(commit.get("message", ""))
        if not title or title in seen_titles:
            continue
        seen_titles.add(title)
        entry = {
            "kind": "commit",
            "title": title,
            "url": commit["url"],
            "meta": short_sha(commit["sha"]),
        }
        if title.startswith("Merge branch "):
            continue
        fallback_items.append(entry)
        if title.lower() in GENERIC_COMMIT_TITLES:
            continue
        picked_items.append(entry)

    items.extend(picked_items or fallback_items)
    return items


def rendered_source_refs(bundle: dict[str, Any]) -> dict[str, Any]:
    refs: dict[str, Any] = {
        "repo": bundle["repo"],
        "commit_urls": [commit["url"] for commit in bundle["commits"]],
        "items": rendered_source_items(bundle),
    }
    primary_pr = bundle.get("primary_pr")
    if isinstance(primary_pr, dict) and primary_pr.get("url"):
        refs["pr_url"] = primary_pr["url"]
    return refs


def pick_published_at(bundle: dict[str, Any], analysis: dict[str, Any], override: str | None) -> str:
    if override:
        return override
    if isinstance(analysis.get("published_at"), str) and analysis["published_at"]:
        return analysis["published_at"]
    primary_pr = bundle.get("primary_pr")
    if isinstance(primary_pr, dict) and primary_pr.get("merged_at"):
        return primary_pr["merged_at"]
    first_commit = bundle["commits"][0]
    return first_commit.get("committed_at") or utc_now_iso()


def main() -> None:
    args = parse_args()
    bundle = load_json(args.bundle)
    analysis = load_json(args.analysis)

    bundle_result = validate_bundle(bundle)
    if not bundle_result.ok:
        raise SystemExit("Bundle validation failed:\n- " + "\n- ".join(bundle_result.errors))

    analysis_result = validate_analysis_draft(analysis)
    if not analysis_result.ok:
        raise SystemExit("Analysis draft validation failed:\n- " + "\n- ".join(analysis_result.errors))

    config_flags = analysis.get("config_flags")
    if config_flags is None:
        config_flags = bundle.get("extracted_flags", [])
    config_flags = normalized_config_flags(config_flags)

    signal = {
        "schema": SIGNAL_SCHEMA,
        "slug": analysis.get("slug") or slugify(analysis["title"]),
        "lane": "github",
        "kind": analysis["kind"],
        "title": analysis["title"],
        "published_at": pick_published_at(bundle, analysis, args.published_at),
        "summary": analysis["summary"],
        "why_it_matters": analysis["why_it_matters"],
        "confidence": analysis["confidence"],
        "impact": analysis["impact"],
        "config_flags": config_flags,
        "proof_points": analysis["proof_points"],
        "source_refs": rendered_source_refs(bundle),
    }

    if analysis.get("how_to_try"):
        signal["how_to_try"] = analysis["how_to_try"]
    if analysis.get("expected_effect"):
        signal["expected_effect"] = analysis["expected_effect"]
    if analysis.get("caveats"):
        signal["caveats"] = analysis["caveats"]
    if analysis.get("watch_state"):
        signal["watch_state"] = analysis["watch_state"]

    validation = validate_signal(signal)
    if not validation.ok:
        raise SystemExit("Signal validation failed:\n- " + "\n- ".join(validation.errors))

    dump_json(args.out, signal)
    print(args.out)


if __name__ == "__main__":
    main()
