#!/usr/bin/env python3
"""Build the latest stable-vs-prerelease release-delta artifact for Decodex."""

from __future__ import annotations

import argparse
import json
import os
import re
import socket
import ssl
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

SCRIPT_HOME = Path(__file__).resolve().parent
if str(SCRIPT_HOME) not in sys.path:
    sys.path.insert(0, str(SCRIPT_HOME))

from contracts import (  # noqa: E402
    RELEASE_DELTA_SCHEMA,
    dump_json,
    load_json,
    utc_now_iso,
    validate_release_delta,
    validate_signal,
)

COMMIT_URL_RE = re.compile(r"/commit/([0-9a-f]{7,40})$")
PR_URL_RE = re.compile(r"/pull/(\d+)$")
PR_IN_MESSAGE_RE = re.compile(r"\(#(\d+)\)")
RETRYABLE_HTTP_STATUS_CODES = {429, 500, 502, 503, 504}
GITHUB_REQUEST_ATTEMPTS = 4
GITHUB_REQUEST_BACKOFF_SECONDS = 1.0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True, help="GitHub repository in owner/name format.")
    parser.add_argument("--signals-dir", required=True, help="Directory containing published signal-entry JSON files.")
    parser.add_argument("--out", required=True, help="Path to write the release-delta JSON artifact.")
    parser.add_argument("--tag-prefix", default="rust-v", help="Release tag prefix to scope the tracked channel.")
    parser.add_argument("--token-env", help="Environment variable name holding a GitHub token.")
    parser.add_argument(
        "--stable-limit",
        type=int,
        default=0,
        help="Maximum number of recent stable releases to include. Use 0 for all releases at or above the floor.",
    )
    parser.add_argument(
        "--preview-limit",
        type=int,
        default=0,
        help="Maximum number of recent prereleases to include. Use 0 for all supported prereleases.",
    )
    parser.add_argument(
        "--pair-limit",
        type=int,
        default=0,
        help="Maximum number of precomputed stable->preview compare entries. Use 0 for all valid pairs.",
    )
    parser.add_argument(
        "--min-stable-tag",
        default="rust-v0.116.0",
        help="Minimum stable tag to include in the comparator option set.",
    )
    return parser.parse_args()


def routed_token_env() -> str | None:
    try:
        identity = (
            subprocess.run(
                ["git", "config", "--get", "codex.github-identity"],
                check=True,
                capture_output=True,
                text=True,
            )
            .stdout.strip()
        )
    except subprocess.CalledProcessError:
        return None
    return {"x": "GITHUB_PAT_X", "y": "GITHUB_PAT_Y"}.get(identity, "GITHUB_TOKEN")


def is_retryable_github_error(exc: urllib.error.HTTPError | urllib.error.URLError) -> bool:
    if isinstance(exc, urllib.error.HTTPError):
        return exc.code in RETRYABLE_HTTP_STATUS_CODES

    reason = exc.reason
    return isinstance(reason, (ConnectionResetError, TimeoutError, socket.timeout, ssl.SSLError))


def github_request(url: str, token: str | None) -> Any:
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}" if token else "",
            "User-Agent": "decodex-release-delta-builder",
        },
    )
    if not token:
        request.headers.pop("Authorization")

    for attempt in range(1, GITHUB_REQUEST_ATTEMPTS + 1):
        try:
            with urllib.request.urlopen(request) as response:
                return json.load(response)
        except urllib.error.HTTPError as exc:
            details = exc.read().decode("utf-8", errors="replace")
            if not is_retryable_github_error(exc) or attempt == GITHUB_REQUEST_ATTEMPTS:
                raise SystemExit(f"GitHub API request failed for {url}: {exc.code} {details}") from exc
        except urllib.error.URLError as exc:
            if not is_retryable_github_error(exc) or attempt == GITHUB_REQUEST_ATTEMPTS:
                raise SystemExit(f"GitHub API request failed for {url}: {exc.reason}") from exc
        time.sleep(GITHUB_REQUEST_BACKOFF_SECONDS * attempt)

    raise SystemExit(f"GitHub API request failed for {url}: exhausted retry loop")


def select_release_pair(releases: list[dict[str, Any]], tag_prefix: str) -> tuple[dict[str, Any], dict[str, Any]]:
    relevant = [
        release
        for release in releases
        if not release.get("draft") and isinstance(release.get("tag_name"), str) and release["tag_name"].startswith(tag_prefix)
    ]
    if not relevant:
        raise SystemExit(f"No releases found for tag prefix {tag_prefix!r}")

    stable = next((release for release in relevant if not release.get("prerelease")), None)
    prerelease = next((release for release in relevant if release.get("prerelease")), None)
    if stable is None:
        raise SystemExit(f"No stable release found for tag prefix {tag_prefix!r}")
    if prerelease is None:
        raise SystemExit(f"No prerelease found for tag prefix {tag_prefix!r}")
    return stable, prerelease


def relevant_releases(releases: list[dict[str, Any]], tag_prefix: str) -> list[dict[str, Any]]:
    return [
        release
        for release in releases
        if not release.get("draft") and isinstance(release.get("tag_name"), str) and release["tag_name"].startswith(tag_prefix)
    ]


def stable_version_key(tag_name: str, tag_prefix: str) -> tuple[int, ...]:
    raw = tag_name.removeprefix(tag_prefix)
    parts = raw.split(".")
    key: list[int] = []
    for part in parts:
        digits = "".join(ch for ch in part if ch.isdigit())
        key.append(int(digits or "0"))
    return tuple(key)


def select_release_options(
    releases: list[dict[str, Any]],
    tag_prefix: str,
    stable_limit: int,
    preview_limit: int,
    min_stable_tag: str,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    relevant = relevant_releases(releases, tag_prefix)
    min_stable_key = stable_version_key(min_stable_tag, tag_prefix)
    stable = [
        release
        for release in relevant
        if not release.get("prerelease")
        and stable_version_key(release["tag_name"], tag_prefix) >= min_stable_key
    ]
    preview = [release for release in relevant if release.get("prerelease")]
    if stable_limit > 0:
        stable = stable[:stable_limit]
    if preview_limit > 0:
        preview = preview[:preview_limit]
    if not stable:
        raise SystemExit(
            f"No stable releases found for tag prefix {tag_prefix!r} at or above {min_stable_tag!r}"
        )
    if not preview:
        raise SystemExit(f"No prereleases found for tag prefix {tag_prefix!r}")
    return stable, preview


def compact_release(release: dict[str, Any]) -> dict[str, Any]:
    return {
        "tag_name": release["tag_name"],
        "name": release.get("name") or release["tag_name"],
        "prerelease": bool(release.get("prerelease")),
        "published_at": release["published_at"],
        "url": release["html_url"],
    }


def release_sort_key(release: dict[str, Any]) -> str:
    published_at = release.get("published_at")
    return published_at if isinstance(published_at, str) else ""


def compare_candidates(
    stable_releases: list[dict[str, Any]],
    preview_releases: list[dict[str, Any]],
    pair_limit: int,
) -> list[tuple[dict[str, Any], dict[str, Any]]]:
    candidates: list[tuple[dict[str, Any], dict[str, Any]]] = []
    for stable in stable_releases:
        stable_key = release_sort_key(stable)
        for preview in preview_releases:
            preview_key = release_sort_key(preview)
            if preview_key <= stable_key:
                continue
            candidates.append((stable, preview))
    candidates.sort(
        key=lambda pair: (
            release_sort_key(pair[1]),
            release_sort_key(pair[0]),
        ),
        reverse=True,
    )
    return candidates[:pair_limit] if pair_limit > 0 else candidates


def extract_signal_commit_shas(signal: dict[str, Any]) -> set[str]:
    shas: set[str] = set()
    for url in signal.get("source_refs", {}).get("commit_urls", []):
        match = COMMIT_URL_RE.search(url)
        if match:
            shas.add(match.group(1))
    return shas


def extract_signal_pr_number(signal: dict[str, Any]) -> int | None:
    pr_url = signal.get("source_refs", {}).get("pr_url")
    if not isinstance(pr_url, str):
        return None
    match = PR_URL_RE.search(pr_url)
    if not match:
        return None
    return int(match.group(1))


def load_signals(signals_dir: str | Path, repo: str) -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    for path in sorted(Path(signals_dir).glob("*.json")):
        if path.name == "README.md":
            continue
        payload = load_json(path)
        result = validate_signal(payload)
        if not result.ok:
            raise SystemExit(f"Signal validation failed for {path}:\n- " + "\n- ".join(result.errors))
        if payload.get("source_refs", {}).get("repo") == repo:
            entries.append(payload)
    return entries


def main() -> None:
    args = parse_args()
    token_env = args.token_env or routed_token_env() or "GITHUB_TOKEN"
    token = os.environ.get(token_env)

    releases = github_request(f"https://api.github.com/repos/{args.repo}/releases?per_page=100", token)
    if not isinstance(releases, list):
        raise SystemExit("Expected releases list payload from GitHub API")
    stable_release, prerelease = select_release_pair(releases, args.tag_prefix)
    stable_releases, preview_releases = select_release_options(
        releases,
        args.tag_prefix,
        args.stable_limit,
        args.preview_limit,
        args.min_stable_tag,
    )
    release_pairs = compare_candidates(stable_releases, preview_releases, args.pair_limit)
    default_pair = (stable_release, prerelease)
    if not any(
        pair[0]["tag_name"] == default_pair[0]["tag_name"] and pair[1]["tag_name"] == default_pair[1]["tag_name"]
        for pair in release_pairs
    ):
        release_pairs = [default_pair, *release_pairs[: max(args.pair_limit - 1, 0)]]

    allowed_stable_tags = {stable["tag_name"] for stable, _ in release_pairs}
    allowed_preview_tags = {preview["tag_name"] for _, preview in release_pairs}
    stable_releases = [release for release in stable_releases if release["tag_name"] in allowed_stable_tags]
    preview_releases = [release for release in preview_releases if release["tag_name"] in allowed_preview_tags]

    signal_entries = load_signals(args.signals_dir, args.repo)
    comparison_entries: list[dict[str, Any]] = []
    default_tracked_signal_slugs: list[str] = []
    default_compare_payload: dict[str, Any] | None = None

    for stable_candidate, preview_candidate in release_pairs:
        compare = github_request(
            f"https://api.github.com/repos/{args.repo}/compare/{stable_candidate['tag_name']}...{preview_candidate['tag_name']}",
            token,
        )
        commits = compare.get("commits")
        if not isinstance(commits, list):
            raise SystemExit("Expected compare.commits from GitHub API")
        compare_commit_shas = [commit["sha"] for commit in commits if isinstance(commit.get("sha"), str)]
        compare_commit_set = set(compare_commit_shas)
        compare_pr_numbers = sorted(
            {
                int(match.group(1))
                for commit in commits
                for match in PR_IN_MESSAGE_RE.finditer((commit.get("commit") or {}).get("message", ""))
            }
        )
        compare_pr_number_set = set(compare_pr_numbers)

        tracked_signal_slugs: list[str] = []
        for signal in sorted(signal_entries, key=lambda item: item["published_at"], reverse=True):
            signal_shas = extract_signal_commit_shas(signal)
            signal_pr_number = extract_signal_pr_number(signal)
            if signal_shas.intersection(compare_commit_set) or (
                signal_pr_number is not None and signal_pr_number in compare_pr_number_set
            ):
                tracked_signal_slugs.append(signal["slug"])

        compare_payload = {
            "status": compare["status"],
            "ahead_by": compare["ahead_by"],
            "total_commits": compare["total_commits"],
            "url": compare["html_url"],
            "commit_shas": compare_commit_shas,
            "pr_numbers": compare_pr_numbers,
        }
        comparison_entries.append(
            {
                "stable_tag_name": stable_candidate["tag_name"],
                "prerelease_tag_name": preview_candidate["tag_name"],
                "compare": compare_payload,
                "tracked_signal_slugs": tracked_signal_slugs,
            }
        )

        if (
            stable_candidate["tag_name"] == stable_release["tag_name"]
            and preview_candidate["tag_name"] == prerelease["tag_name"]
        ):
            default_compare_payload = compare_payload
            default_tracked_signal_slugs = tracked_signal_slugs

    if default_compare_payload is None:
        raise SystemExit("Default stable/prerelease pair was not included in comparison entries")

    payload = {
        "schema": RELEASE_DELTA_SCHEMA,
        "repo": args.repo,
        "tag_prefix": args.tag_prefix,
        "generated_at": utc_now_iso(),
        "stable_release": compact_release(stable_release),
        "prerelease": compact_release(prerelease),
        "compare": default_compare_payload,
        "release_options": {
            "stable": [compact_release(release) for release in stable_releases],
            "preview": [compact_release(release) for release in preview_releases],
        },
        "comparisons": comparison_entries,
        "tracked_signal_slugs": default_tracked_signal_slugs,
    }

    validation = validate_release_delta(payload)
    if not validation.ok:
        raise SystemExit("Release-delta validation failed:\n- " + "\n- ".join(validation.errors))

    dump_json(args.out, payload)
    print(args.out)


if __name__ == "__main__":
    main()
