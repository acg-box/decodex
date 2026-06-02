#!/usr/bin/env python3
"""Sync the deterministic Codex upstream review queue for Decodex Radar."""

from __future__ import annotations

import argparse
import json
import os
import re
import urllib.parse
from pathlib import Path
from typing import Any

from build_change_bundle import (
    build_commit_bundle,
    build_pr_bundle,
    github_request,
    maybe_promote_commit_to_pr,
    repo_default_branch,
    routed_token_env,
)
from contracts import (
    dump_json,
    load_json,
    utc_now_iso,
    validate_signal,
    validate_upstream_review_queue,
)
from radar_ledger import DEFAULT_LEDGER_PATH, connect as connect_ledger, record_commit, record_review

SCRIPT_HOME = Path(__file__).resolve().parent
COMMIT_URL_RE = re.compile(r"/commit/([0-9a-f]{7,40})$")
PR_URL_RE = re.compile(r"/pull/(\d+)$")

SURFACE_RULES: tuple[tuple[str, tuple[str, ...]], ...] = (
    ("app_server_protocol", ("app-server", "app_server", "protocol", "jsonrpc", "json-rpc")),
    ("mcp_plugins", ("mcp", "plugin", "tool-search", "tool_search")),
    ("browser_chrome", ("browser", "chrome", "webview")),
    ("sandbox_permissions", ("sandbox", "permission", "approval", "policy", "denylist", "allowlist")),
    ("config_hooks", ("config", "hook", "settings", "toml")),
    ("auth_accounts", ("auth", "account", "login", "token")),
    ("model_provider", ("model", "provider", "rate-limit", "ratelimit", "quota")),
    ("cli_tui", ("cli", "tui", "terminal", "chatwidget")),
    ("release_packaging", ("release", "appcast", "sparkle", "version", "install", "package")),
    ("docs_examples", ("docs/", "readme", "example")),
    ("tests_ci", ("test", "tests", ".github", "ci", "fixture")),
)

ATTENTION_RULES: tuple[tuple[str, tuple[str, ...]], ...] = (
    ("new_feature", ("feat", "feature", "add ", "adds ", "support", "enable", "implement", "introduce")),
    ("deprecated_removed", ("deprecat", "remove", "removed", "delete", "disable", "no longer")),
    ("protocol_change", ("protocol", "schema", "api", "json-rpc", "jsonrpc", "notification", "request", "response")),
    ("breaking_change", ("breaking", "break ", "rename", "migration", "incompat", "no longer")),
    ("security_policy", ("sandbox", "permission", "approval", "full access", "network", "denylist", "allowlist")),
    ("rate_limit", ("rate limit", "ratelimit", "quota", "usage limit", "message cap")),
    ("auth_account", ("auth", "account", "login", "token")),
    ("release_packaging", ("release", "appcast", "sparkle", "beta", "version")),
)

HIGH_VALUE_SURFACES = {
    "app_server_protocol",
    "mcp_plugins",
    "browser_chrome",
    "sandbox_permissions",
    "config_hooks",
    "auth_accounts",
    "model_provider",
}
HIGH_VALUE_FLAGS = {
    "deprecated_removed",
    "protocol_change",
    "breaking_change",
    "security_policy",
    "rate_limit",
    "auth_account",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="openai/codex", help="GitHub repository in owner/name format.")
    parser.add_argument("--search-limit", type=int, default=40, help="How many recent upstream commits to inspect.")
    parser.add_argument("--signals-dir", default="site/src/content/signals", help="Published signal directory.")
    parser.add_argument(
        "--queue-out",
        default="artifacts/github/review-queue/openai-codex-latest.json",
        help="Path to write the deterministic upstream_review_queue/v1 artifact.",
    )
    parser.add_argument("--token-env", help="Environment variable containing a GitHub token.")
    parser.add_argument(
        "--ledger",
        default=DEFAULT_LEDGER_PATH,
        help="Local SQLite Radar ledger path. Defaults to .decodex/radar.sqlite3.",
    )
    parser.add_argument("--no-ledger", action="store_true", help="Disable local Radar ledger writes.")
    parser.add_argument("--dry-run", action="store_true", help="Print the queue without writing queue-out.")
    return parser.parse_args()


def repo_root() -> Path:
    return SCRIPT_HOME.parents[1]


def published_subjects(signals_dir: Path) -> tuple[set[int], set[str]]:
    published_prs: set[int] = set()
    published_shas: set[str] = set()
    for path in sorted(signals_dir.glob("*.json")):
        payload = load_json(path)
        validation = validate_signal(payload)
        if not validation.ok:
            raise SystemExit(f"Signal validation failed for {path}:\n- " + "\n- ".join(validation.errors))
        pr_url = payload.get("source_refs", {}).get("pr_url")
        if isinstance(pr_url, str):
            match = PR_URL_RE.search(pr_url)
            if match:
                published_prs.add(int(match.group(1)))
        for url in payload.get("source_refs", {}).get("commit_urls", []):
            if not isinstance(url, str):
                continue
            match = COMMIT_URL_RE.search(url)
            if match:
                published_shas.add(match.group(1))
    return published_prs, published_shas


def recent_commits(repo: str, token: str | None, search_limit: int) -> tuple[str, list[dict[str, Any]]]:
    default_branch = repo_default_branch(repo, token)
    payload, _ = github_request(
        f"https://api.github.com/repos/{repo}/commits?sha={urllib.parse.quote(default_branch)}&per_page={search_limit}",
        token,
    )
    if not isinstance(payload, list):
        raise SystemExit("Expected commits list payload from GitHub API")
    results: list[dict[str, Any]] = []
    for item in payload:
        sha = item.get("sha")
        commit = item.get("commit")
        url = item.get("html_url")
        if not isinstance(sha, str) or not isinstance(commit, dict) or not isinstance(url, str):
            continue
        message = commit.get("message")
        if not isinstance(message, str) or not message:
            continue
        results.append(
            {
                "sha": sha,
                "title": message.strip().splitlines()[0],
                "url": url,
                "committed_at": (commit.get("committer") or {}).get("date"),
            }
        )
    return default_branch, results


def text_blob(bundle: dict[str, Any]) -> str:
    parts: list[str] = []
    primary_pr = bundle.get("primary_pr")
    if isinstance(primary_pr, dict):
        parts.extend([str(primary_pr.get("title") or ""), str(primary_pr.get("body") or "")])
    for commit in bundle.get("commits", []):
        if isinstance(commit, dict):
            parts.append(str(commit.get("message") or ""))
    for item in bundle.get("files", []):
        if isinstance(item, dict):
            parts.extend([str(item.get("path") or ""), str(item.get("patch_excerpt") or "")])
    return "\n".join(parts).lower()


def detect_surface_hints(bundle: dict[str, Any]) -> list[str]:
    paths = [str(item.get("path") or "").lower() for item in bundle.get("files", []) if isinstance(item, dict)]
    haystack = "\n".join(paths)
    hints = {
        surface
        for surface, terms in SURFACE_RULES
        if any(term in haystack for term in terms)
    }
    if not hints:
        hints.add("internal_churn")
    return sorted(hints)


def detect_attention_flags(bundle: dict[str, Any]) -> list[str]:
    haystack = text_blob(bundle)
    return sorted(
        flag
        for flag, terms in ATTENTION_RULES
        if any(term in haystack for term in terms)
    )


def priority_for(surface_hints: list[str], attention_flags: list[str]) -> str:
    surfaces = set(surface_hints)
    flags = set(attention_flags)
    if (flags & {"breaking_change", "deprecated_removed"}) and (surfaces & HIGH_VALUE_SURFACES):
        return "critical"
    if (surfaces & HIGH_VALUE_SURFACES) and (flags & HIGH_VALUE_FLAGS):
        return "high"
    if surfaces & HIGH_VALUE_SURFACES:
        return "high"
    if flags & {"new_feature", "protocol_change", "release_packaging"}:
        return "normal"
    return "low"


def review_reason(surface_hints: list[str], attention_flags: list[str]) -> str:
    if "internal_churn" in surface_hints and not attention_flags:
        return "Needs AI review because every recent upstream commit is tracked, but deterministic hints found only internal churn."
    if attention_flags:
        return "Needs AI review for " + ", ".join(attention_flags) + "."
    return "Needs AI review for surface hints: " + ", ".join(surface_hints) + "."


def subject_from_bundle(
    *,
    bundle: dict[str, Any],
    subject_kind: str,
    subject_id: str,
    seed_commit: dict[str, Any],
) -> dict[str, Any]:
    primary_pr = bundle.get("primary_pr")
    commits = [item for item in bundle.get("commits", []) if isinstance(item, dict)]
    files = [item for item in bundle.get("files", []) if isinstance(item, dict)]
    commit_shas = [str(item["sha"]) for item in commits if isinstance(item.get("sha"), str)]
    surface_hints = detect_surface_hints(bundle)
    attention_flags = detect_attention_flags(bundle)
    title = seed_commit["title"]
    url = seed_commit["url"]
    source_state = "commit_only"
    subject: dict[str, Any] = {
        "subject_kind": subject_kind,
        "subject_id": subject_id,
        "title": title,
        "url": url,
        "source_state": source_state,
        "commit_shas": commit_shas or [seed_commit["sha"]],
        "committed_at": seed_commit.get("committed_at"),
        "changed_file_count": len(files),
        "sample_paths": [str(item.get("path")) for item in files[:12] if item.get("path")],
        "surface_hints": surface_hints,
        "attention_flags": attention_flags,
        "review_priority": priority_for(surface_hints, attention_flags),
        "review_reason": review_reason(surface_hints, attention_flags),
        "next_step": "ai_review_required",
    }
    if isinstance(primary_pr, dict):
        title = str(primary_pr.get("title") or title)
        url = str(primary_pr.get("url") or url)
        subject.update(
            {
                "title": title,
                "url": url,
                "source_state": str(primary_pr.get("state") or "pr_first"),
                "pr_number": primary_pr.get("number"),
                "pr_url": primary_pr.get("url"),
            }
        )
    return subject


def sort_subjects(subjects: list[dict[str, Any]]) -> list[dict[str, Any]]:
    priority_rank = {"critical": 0, "high": 1, "normal": 2, "low": 3}
    return sorted(
        subjects,
        key=lambda item: (
            priority_rank.get(str(item.get("review_priority")), 9),
            str(item.get("committed_at") or ""),
            str(item.get("subject_kind") or ""),
            str(item.get("subject_id") or ""),
        ),
    )


def build_review_queue(args: argparse.Namespace) -> tuple[dict[str, Any], dict[str, int]]:
    token_env = args.token_env or routed_token_env() or "GITHUB_TOKEN"
    token = os.environ.get(token_env)
    root = repo_root()
    default_branch, commits = recent_commits(args.repo, token, args.search_limit)
    published_prs, published_shas = published_subjects((root / args.signals_dir).resolve())
    ledger_path = None if args.no_ledger else Path(args.ledger)
    if ledger_path is not None and not ledger_path.is_absolute():
        ledger_path = root / ledger_path
    ledger = connect_ledger(ledger_path) if ledger_path is not None else None
    subjects: dict[tuple[str, str], dict[str, Any]] = {}
    published_seen = 0
    try:
        for commit in commits:
            pr_number = maybe_promote_commit_to_pr(args.repo, commit["sha"], token)
            subject_kind = "pr" if pr_number is not None else "commit"
            subject_id = str(pr_number) if pr_number is not None else commit["sha"]
            if ledger is not None:
                record_commit(
                    ledger,
                    repo=args.repo,
                    sha=commit["sha"],
                    title=commit["title"],
                    url=commit["url"],
                    committed_at=commit.get("committed_at"),
                    pr_number=pr_number,
                )
            if commit["sha"] in published_shas or (pr_number is not None and pr_number in published_prs):
                published_seen += 1
                if ledger is not None:
                    record_review(
                        ledger,
                        repo=args.repo,
                        subject_kind=subject_kind,
                        subject_id=subject_id,
                        status="signal",
                        reason="Already present in published signal collection.",
                        confidence="confirmed",
                    )
                continue
            key = (subject_kind, subject_id)
            if key in subjects:
                current = subjects[key]
                if commit["sha"] not in current["commit_shas"]:
                    current["commit_shas"].append(commit["sha"])
                continue
            bundle = (
                build_pr_bundle(args.repo, pr_number, token, ["Built in-memory for deterministic Radar queue hints."])
                if isinstance(pr_number, int)
                else build_commit_bundle(args.repo, commit["sha"], token, ["Built in-memory for deterministic Radar queue hints."])
            )
            subjects[key] = subject_from_bundle(
                bundle=bundle,
                subject_kind=subject_kind,
                subject_id=subject_id,
                seed_commit=commit,
            )
            if ledger is not None:
                record_review(
                    ledger,
                    repo=args.repo,
                    subject_kind=subject_kind,
                    subject_id=subject_id,
                    status="watch",
                    reason="Queued for AI upstream review by deterministic Radar sync.",
                    confidence="likely",
                )
        if ledger is not None:
            ledger.commit()
    finally:
        if ledger is not None:
            ledger.close()

    ordered_subjects = sort_subjects(list(subjects.values()))
    queue = {
        "schema": "upstream_review_queue/v1",
        "repo": args.repo,
        "generated_at": utc_now_iso(),
        "source": {
            "default_branch": default_branch,
            "search_limit": args.search_limit,
            "signals_dir": args.signals_dir,
        },
        "subjects": ordered_subjects,
        "counts": {
            "recent_commits_scanned": len(commits),
            "published_subjects_seen": published_seen,
            "subjects_queued": len(ordered_subjects),
            "critical": sum(1 for item in ordered_subjects if item["review_priority"] == "critical"),
            "high": sum(1 for item in ordered_subjects if item["review_priority"] == "high"),
            "normal": sum(1 for item in ordered_subjects if item["review_priority"] == "normal"),
            "low": sum(1 for item in ordered_subjects if item["review_priority"] == "low"),
        },
    }
    return queue, {"ledger_enabled": 0 if args.no_ledger else 1}


def material_queue(value: dict[str, Any]) -> dict[str, Any]:
    normalized = json.loads(json.dumps(value, sort_keys=True))
    if isinstance(normalized, dict):
        normalized["generated_at"] = ""
    return normalized


def write_queue_if_changed(path: Path, queue: dict[str, Any]) -> bool:
    if path.exists():
        try:
            existing = load_json(path)
        except json.JSONDecodeError:
            existing = None
        if isinstance(existing, dict) and material_queue(existing) == material_queue(queue):
            return False
    dump_json(path, queue)
    return True


def main() -> None:
    args = parse_args()
    root = repo_root()
    queue, extra_counts = build_review_queue(args)
    validation = validate_upstream_review_queue(queue)
    if not validation.ok:
        raise SystemExit("Upstream review queue validation failed:\n- " + "\n- ".join(validation.errors))
    if args.dry_run:
        print(json.dumps(queue, indent=2, sort_keys=True))
        return
    out = root / args.queue_out
    changed = write_queue_if_changed(out, queue)
    print(
        json.dumps(
            {
                "repo": queue["repo"],
                **queue["counts"],
                **extra_counts,
                "changed": changed,
                "queue_out": str(out),
            },
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
