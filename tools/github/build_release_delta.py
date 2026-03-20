#!/usr/bin/env python3
"""Build the latest stable-vs-prerelease release-delta artifact for Decodex."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
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


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True, help="GitHub repository in owner/name format.")
    parser.add_argument("--signals-dir", required=True, help="Directory containing published signal-entry JSON files.")
    parser.add_argument("--out", required=True, help="Path to write the release-delta JSON artifact.")
    parser.add_argument("--tag-prefix", default="rust-v", help="Release tag prefix to scope the tracked channel.")
    parser.add_argument("--token-env", help="Environment variable name holding a GitHub token.")
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
    try:
        with urllib.request.urlopen(request) as response:
            return json.load(response)
    except urllib.error.HTTPError as exc:
        details = exc.read().decode("utf-8", errors="replace")
        raise SystemExit(f"GitHub API request failed for {url}: {exc.code} {details}") from exc


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


def compact_release(release: dict[str, Any]) -> dict[str, Any]:
    return {
        "tag_name": release["tag_name"],
        "name": release.get("name") or release["tag_name"],
        "prerelease": bool(release.get("prerelease")),
        "published_at": release["published_at"],
        "url": release["html_url"],
    }


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

    compare = github_request(
        f"https://api.github.com/repos/{args.repo}/compare/{stable_release['tag_name']}...{prerelease['tag_name']}",
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

    signal_entries = load_signals(args.signals_dir, args.repo)
    tracked_signal_slugs: list[str] = []
    for signal in sorted(signal_entries, key=lambda item: item["published_at"], reverse=True):
        signal_shas = extract_signal_commit_shas(signal)
        signal_pr_number = extract_signal_pr_number(signal)
        if signal_shas.intersection(compare_commit_set) or (
            signal_pr_number is not None and signal_pr_number in compare_pr_number_set
        ):
            tracked_signal_slugs.append(signal["slug"])

    payload = {
        "schema": RELEASE_DELTA_SCHEMA,
        "repo": args.repo,
        "tag_prefix": args.tag_prefix,
        "generated_at": utc_now_iso(),
        "stable_release": compact_release(stable_release),
        "prerelease": compact_release(prerelease),
        "compare": {
            "status": compare["status"],
            "ahead_by": compare["ahead_by"],
            "total_commits": compare["total_commits"],
            "url": compare["html_url"],
            "commit_shas": compare_commit_shas,
            "pr_numbers": compare_pr_numbers,
        },
        "tracked_signal_slugs": tracked_signal_slugs,
    }

    validation = validate_release_delta(payload)
    if not validation.ok:
        raise SystemExit("Release-delta validation failed:\n- " + "\n- ".join(validation.errors))

    dump_json(args.out, payload)
    print(args.out)


if __name__ == "__main__":
    main()
