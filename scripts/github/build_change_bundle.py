#!/usr/bin/env python3
"""Build a PR-first or commit-only GitHub change bundle for Decodex."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

SCRIPT_HOME = Path(__file__).resolve().parent
if str(SCRIPT_HOME) not in sys.path:
    sys.path.insert(0, str(SCRIPT_HOME))

from contracts import (  # noqa: E402
    BUNDLE_SCHEMA,
    collect_flags,
    collect_issue_refs,
    dump_json,
    first_line,
    truncate_patch,
    validate_bundle,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True, help="GitHub repository in owner/name format.")
    parser.add_argument("--pr", type=int, help="Pull request number to fetch.")
    parser.add_argument("--commit", help="Commit SHA to fetch when PR is unavailable.")
    parser.add_argument("--force-commit-only", action="store_true", help="Skip PR lookup for commit input.")
    parser.add_argument("--token-env", help="Environment variable name holding a GitHub token.")
    parser.add_argument("--out", required=True, help="Path to write the bundle JSON.")
    parser.add_argument("--note", action="append", default=[], help="Additional note strings to store in the bundle.")
    args = parser.parse_args()
    if not args.pr and not args.commit:
        parser.error("one of --pr or --commit is required")
    return args


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


def github_request(url: str, token: str | None) -> tuple[Any, dict[str, str]]:
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}" if token else "",
            "User-Agent": "decodex-github-bundle-builder",
        },
    )
    if not token:
        request.headers.pop("Authorization")
    try:
        with urllib.request.urlopen(request) as response:
            return json.load(response), dict(response.headers)
    except urllib.error.HTTPError as exc:
        details = exc.read().decode("utf-8", errors="replace")
        raise SystemExit(f"GitHub API request failed for {url}: {exc.code} {details}") from exc


def github_paginated(url: str, token: str | None) -> list[Any]:
    items: list[Any] = []
    next_url: str | None = url
    while next_url:
        payload, headers = github_request(next_url, token)
        if not isinstance(payload, list):
            raise SystemExit(f"Expected list payload from {next_url}")
        items.extend(payload)
        next_url = parse_next_link(headers.get("Link"))
    return items


def parse_next_link(header: str | None) -> str | None:
    if not header:
        return None
    for part in header.split(","):
        section = part.strip().split(";")
        if len(section) < 2:
            continue
        url_part, *meta = section
        if any(item.strip() == 'rel="next"' for item in meta):
            return url_part.strip()[1:-1]
    return None


def repo_default_branch(repo: str, token: str | None) -> str:
    payload, _ = github_request(f"https://api.github.com/repos/{repo}", token)
    default_branch = payload.get("default_branch")
    if not isinstance(default_branch, str) or not default_branch:
        raise SystemExit(f"Unable to resolve default branch for {repo}")
    return default_branch


def build_pr_bundle(repo: str, pr_number: int, token: str | None, notes: list[str]) -> dict[str, Any]:
    pr, _ = github_request(f"https://api.github.com/repos/{repo}/pulls/{pr_number}", token)
    commits = github_paginated(
        f"https://api.github.com/repos/{repo}/pulls/{pr_number}/commits?per_page=100", token
    )
    files = github_paginated(
        f"https://api.github.com/repos/{repo}/pulls/{pr_number}/files?per_page=100", token
    )
    default_branch = repo_default_branch(repo, token)

    commit_items = [
        {
            "sha": item["sha"],
            "message": first_line(item["commit"]["message"]),
            "url": item["html_url"],
            "author": (item.get("author") or {}).get("login")
            or (item["commit"].get("author") or {}).get("name"),
            "committed_at": (item["commit"].get("author") or {}).get("date"),
        }
        for item in commits
    ]

    file_items = [
        {
            "path": item["filename"],
            "status": item["status"],
            "additions": item["additions"],
            "deletions": item["deletions"],
            "patch_excerpt": truncate_patch(item.get("patch")),
        }
        for item in files
    ]

    docs_refs = [
        item["filename"]
        for item in files
        if item["filename"].startswith("docs/") or item["filename"].endswith("README.md")
    ]
    examples_refs = [
        item["filename"]
        for item in files
        if "example" in item["filename"].lower() or "examples/" in item["filename"]
    ]
    all_patch_text = "\n".join(item.get("patch", "") for item in files)
    all_commit_text = "\n".join(item["commit"]["message"] for item in commits)
    linked_issues = collect_issue_refs(pr.get("body", ""), all_commit_text)
    extracted_flags = collect_flags(pr.get("body", ""), all_commit_text, all_patch_text)

    bundle = {
        "schema": BUNDLE_SCHEMA,
        "repo": repo,
        "analysis_mode": "pr_first",
        "default_branch": default_branch,
        "primary_pr": {
            "number": pr["number"],
            "title": pr["title"],
            "body": pr.get("body") or "",
            "state": "merged" if pr.get("merged_at") else pr["state"],
            "merged_at": pr.get("merged_at"),
            "labels": [label["name"] for label in pr.get("labels", [])],
            "url": pr["html_url"],
        },
        "commits": commit_items,
        "files": file_items,
        "linked_issues": linked_issues,
        "extracted_flags": extracted_flags,
        "docs_refs": docs_refs,
        "examples_refs": examples_refs,
        "notes": [
            "Built from GitHub pull-request, commits, files, and repo endpoints.",
            *notes,
        ],
    }
    result = validate_bundle(bundle)
    if not result.ok:
        raise SystemExit("Bundle validation failed:\n- " + "\n- ".join(result.errors))
    return bundle


def build_commit_bundle(repo: str, commit_sha: str, token: str | None, notes: list[str]) -> dict[str, Any]:
    commit, _ = github_request(f"https://api.github.com/repos/{repo}/commits/{commit_sha}", token)
    default_branch = repo_default_branch(repo, token)
    files = commit.get("files") or []
    bundle = {
        "schema": BUNDLE_SCHEMA,
        "repo": repo,
        "analysis_mode": "commit_only",
        "default_branch": default_branch,
        "commits": [
            {
                "sha": commit["sha"],
                "message": first_line(commit["commit"]["message"]),
                "url": commit["html_url"],
                "author": (commit.get("author") or {}).get("login")
                or (commit["commit"].get("author") or {}).get("name"),
                "committed_at": (commit["commit"].get("author") or {}).get("date"),
            }
        ],
        "files": [
            {
                "path": item["filename"],
                "status": item["status"],
                "additions": item["additions"],
                "deletions": item["deletions"],
                "patch_excerpt": truncate_patch(item.get("patch")),
            }
            for item in files
        ],
        "linked_issues": collect_issue_refs(commit["commit"]["message"]),
        "extracted_flags": collect_flags(
            commit["commit"]["message"], "\n".join(item.get("patch", "") for item in files)
        ),
        "docs_refs": [
            item["filename"]
            for item in files
            if item["filename"].startswith("docs/") or item["filename"].endswith("README.md")
        ],
        "examples_refs": [
            item["filename"]
            for item in files
            if "example" in item["filename"].lower() or "examples/" in item["filename"]
        ],
        "notes": [
            "Built from GitHub commit endpoint without PR context.",
            *notes,
        ],
    }
    result = validate_bundle(bundle)
    if not result.ok:
        raise SystemExit("Bundle validation failed:\n- " + "\n- ".join(result.errors))
    return bundle


def maybe_promote_commit_to_pr(repo: str, commit_sha: str, token: str | None) -> int | None:
    url = f"https://api.github.com/repos/{repo}/commits/{commit_sha}/pulls"
    try:
        pulls = github_paginated(url, token)
    except SystemExit:
        return None
    if not pulls:
        return None
    first = pulls[0]
    if not isinstance(first, dict) or "number" not in first:
        return None
    return int(first["number"])


def main() -> None:
    args = parse_args()
    token_env = args.token_env or routed_token_env() or "GITHUB_TOKEN"
    token = os.environ.get(token_env)

    if args.pr is not None:
        bundle = build_pr_bundle(args.repo, args.pr, token, args.note)
    else:
        assert args.commit
        promoted_pr = None if args.force_commit_only else maybe_promote_commit_to_pr(args.repo, args.commit, token)
        bundle = (
            build_pr_bundle(args.repo, promoted_pr, token, args.note)
            if promoted_pr is not None
            else build_commit_bundle(args.repo, args.commit, token, args.note)
        )

    dump_json(args.out, bundle)
    print(args.out)


if __name__ == "__main__":
    main()
