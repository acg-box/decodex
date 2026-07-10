#!/usr/bin/env python3
"""Build a deterministic Decodex automation effectiveness scorecard."""

from __future__ import annotations

import argparse
import json
import os
import sys
import tomllib
from collections import Counter
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Iterable


SOURCE_ROOT = Path(__file__).resolve().parents[4]
CONFIG_ROOT = SOURCE_ROOT / "automations/decodex/scripts/config"
sys.path.insert(0, str(CONFIG_ROOT))

from automation_checkout import primary_checkout_for_branch  # noqa: E402


RUNTIME_ROOT = primary_checkout_for_branch(SOURCE_ROOT)
MANIFESTS = (
	SOURCE_ROOT / "automations/decodex/automations.toml",
	SOURCE_ROOT / "automations/radar/automations.toml",
)
MANAGER_ROOT = RUNTIME_ROOT / ".agent/automations/decodex/cache/manager"
TERMINAL_AUTOMATION_HANDOFF_STATUSES = {"closed", "resolved", "superseded"}


def parse_time(value: str) -> datetime:
	return datetime.fromisoformat(value.replace("Z", "+00:00")).astimezone(timezone.utc)


def load_json(path: Path) -> dict[str, Any] | None:
	try:
		value = json.loads(path.read_text(encoding="utf-8"))
	except (OSError, json.JSONDecodeError):
		return None
	return value if isinstance(value, dict) else None


def files_in_window(root: Path, pattern: str, start: datetime, end: datetime) -> list[Path]:
	if not root.exists():
		return []
	result = []
	for path in root.glob(pattern):
		if not path.is_file():
			continue
		modified = datetime.fromtimestamp(path.stat().st_mtime, timezone.utc)
		if start <= modified < end:
			result.append(path)
	return sorted(result)


def managed_automation_ids() -> list[str]:
	ids: list[str] = []
	for path in MANIFESTS:
		with path.open("rb") as handle:
			manifest = tomllib.load(handle)
		ids.extend(item["id"] for item in manifest.get("automations", []))
	return ids


def inspect_live_configs(codex_home: Path, managed_ids: list[str]) -> dict[str, Any]:
	statuses: Counter[str] = Counter()
	missing: list[str] = []
	worktree_bound: list[str] = []
	updated_at: dict[str, str] = {}
	for automation_id in managed_ids:
		path = codex_home / "automations" / automation_id / "automation.toml"
		if not path.exists():
			missing.append(automation_id)
			continue
		with path.open("rb") as handle:
			config = tomllib.load(handle)
		statuses[str(config.get("status", "UNKNOWN"))] += 1
		cwds = config.get("cwds", [])
		if any(".worktrees" in Path(str(cwd)).parts for cwd in cwds):
			worktree_bound.append(automation_id)
		if isinstance(config.get("updated_at"), int):
			updated_at[automation_id] = datetime.fromtimestamp(
				config["updated_at"] / 1000,
				timezone.utc,
			).isoformat().replace("+00:00", "Z")
	return {
		"managed": len(managed_ids),
		"statuses": dict(sorted(statuses.items())),
		"missing": missing,
		"worktree_bound": worktree_bound,
		"updated_at": updated_at,
	}


def all_json(root: Path, pattern: str) -> Iterable[tuple[Path, dict[str, Any]]]:
	if not root.exists():
		return
	for path in sorted(root.glob(pattern)):
		if value := load_json(path):
			yield path, value


def inspect_social(start: datetime, end: datetime) -> dict[str, Any]:
	root = RUNTIME_ROOT / ".agent/automations/decodex/cache/social/x"
	posts = list(all_json(root / "posts", "**/*.json"))
	reservations = list(all_json(root / "reservations", "**/*.json"))
	candidates = list(all_json(root / "candidates", "*.json"))
	terminal_keys = {
		str(value.get("decision", {}).get("idempotency_key"))
		for _, value in posts
		if value.get("decision", {}).get("idempotency_key")
	}
	open_candidates = []
	overdue_candidates = []
	for path, value in candidates:
		decision = value.get("decision", {})
		key = decision.get("idempotency_key")
		if decision.get("worthiness") == "publish" and key not in terminal_keys:
			open_candidates.append(path.name)
			modified = datetime.fromtimestamp(path.stat().st_mtime, timezone.utc)
			if modified + timedelta(hours=24) <= end:
				overdue_candidates.append(path.name)

	window_posts = [
		value
		for path, value in posts
		if start <= datetime.fromtimestamp(path.stat().st_mtime, timezone.utc) < end
	]
	status_counts = Counter(str(value.get("status", "unknown")) for value in window_posts)
	published = [value for value in window_posts if value.get("status") == "published"]
	published_units = sum(
		max(1, len(value.get("text", []))) if isinstance(value.get("text"), list) else 1
		for value in published
	)
	latest_urls = []
	for value in published:
		for url in value.get("source_refs", {}).get("urls", []):
			if isinstance(url, str) and url.startswith("https://x.com/"):
				latest_urls.append(url)

	stale_reservations = []
	active_reservations = []
	for path, value in reservations:
		if value.get("status") != "active":
			continue
		active_reservations.append(path.name)
		expires_at = value.get("expires_at")
		if isinstance(expires_at, str) and parse_time(expires_at) < end:
			stale_reservations.append(path.name)

	return {
		"candidate_files": len(candidates),
		"open_publishable_candidates": open_candidates,
		"overdue_publishable_candidates": overdue_candidates,
		"post_outcomes": dict(sorted(status_counts.items())),
		"published_records": len(published),
		"published_post_units": published_units,
		"latest_published_url": latest_urls[-1] if latest_urls else None,
		"active_reservations": active_reservations,
		"stale_reservations": stale_reservations,
	}


def inspect_radar(start: datetime, end: datetime) -> dict[str, int]:
	root = RUNTIME_ROOT / ".agent/automations/radar/cache"
	patterns = {
		"reviews": "github/reviews/*.json",
		"impacts": "github/impact/*.json",
		"signals": "site-content/signals/*.json",
		"control_plane_candidates": "github/control-plane-upgrades/*.json",
		"release_checkpoints": "generated/release-checkpoints/*",
	}
	return {
		name: len(files_in_window(root, pattern, start, end))
		for name, pattern in patterns.items()
	}


def inspect_active_experiment(end: datetime) -> dict[str, Any]:
	path = MANAGER_ROOT / "experiments/active.json"
	value = load_json(path)
	if value is None:
		return {"status": "missing", "path": str(path.relative_to(RUNTIME_ROOT))}
	try:
		window = value["effective_window"]
		window_start = parse_time(window["start"])
		window_end = parse_time(window["end"])
		experiments = value["experiments"]
	except (KeyError, TypeError, ValueError):
		return {"status": "invalid", "path": str(path.relative_to(RUNTIME_ROOT))}
	if not isinstance(experiments, list) or not experiments:
		status = "invalid"
	elif window_end <= end:
		status = "expired"
	elif window_start > end:
		status = "pending"
	elif not any(item.get("status") == "active" for item in experiments if isinstance(item, dict)):
		status = "invalid"
	else:
		status = "active"
	return {
		"status": status,
		"path": str(path.relative_to(RUNTIME_ROOT)),
		"effective_window": {
			"start": window_start.isoformat().replace("+00:00", "Z"),
			"end": window_end.isoformat().replace("+00:00", "Z"),
		},
		"experiment_count": len(experiments),
	}


def inspect_automation_handoffs() -> dict[str, Any]:
	handoffs = []
	for path, value in all_json(MANAGER_ROOT / "handoffs", "**/*.json"):
		if value.get("schema") != "decodex_automation_handoff/v1":
			continue
		status = str(value.get("status", "unknown"))
		if status in TERMINAL_AUTOMATION_HANDOFF_STATUSES:
			continue
		handoffs.append(
			{
				"id": str(value.get("id", path.stem)),
				"severity": str(value.get("severity", "p1")),
				"status": status,
				"path": str(path.relative_to(RUNTIME_ROOT)),
			}
		)
	return {"unresolved": handoffs, "unresolved_count": len(handoffs)}


def inspect_management(
	start: datetime,
	end: datetime,
	manager_updated_at: str | None = None,
) -> dict[str, Any]:
	daily = files_in_window(MANAGER_ROOT / "reports", "**/*.md", start, end)
	weekly = files_in_window(MANAGER_ROOT / "weekly", "**/*.md", start, end)
	covered_days = sorted(
		{
			datetime.fromtimestamp(path.stat().st_mtime, timezone.utc).date().isoformat()
			for path in daily
		}
	)
	coverage_start = start
	if manager_updated_at:
		coverage_start = max(start, parse_time(manager_updated_at))
	expected_days = max(0, int((end - coverage_start).total_seconds() // 86400))
	return {
		"daily_reports": len(daily),
		"daily_coverage_days": covered_days,
		"weekly_reports": len(weekly),
		"latest_daily_report": str(daily[-1].relative_to(RUNTIME_ROOT)) if daily else None,
		"coverage_baseline": coverage_start.isoformat().replace("+00:00", "Z"),
		"expected_daily_coverage_days": expected_days,
		"active_experiment": inspect_active_experiment(end),
		"automation_handoffs": inspect_automation_handoffs(),
	}


def build_scorecard(codex_home: Path, start: datetime, end: datetime) -> dict[str, Any]:
	managed_ids = managed_automation_ids()
	live = inspect_live_configs(codex_home, managed_ids)
	social = inspect_social(start, end)
	radar = inspect_radar(start, end)
	management = inspect_management(
		start,
		end,
		live["updated_at"].get("decodex-automation-manager"),
	)
	blockers: list[dict[str, str]] = []

	if live["missing"]:
		blockers.append({"severity": "p0", "code": "missing_live_automations"})
	if live["worktree_bound"]:
		blockers.append({"severity": "p0", "code": "worktree_bound_automations"})
	if live["statuses"].get("ACTIVE", 0) != live["managed"]:
		blockers.append({"severity": "p0", "code": "managed_automations_not_active"})
	if social["stale_reservations"]:
		blockers.append({"severity": "p0", "code": "stale_social_reservations"})
	if social["overdue_publishable_candidates"]:
		blockers.append({"severity": "p1", "code": "publisher_terminal_outcome_overdue"})
	if management["daily_reports"] == 0:
		blockers.append({"severity": "p1", "code": "missing_daily_manager_evidence"})
	elif len(management["daily_coverage_days"]) < management["expected_daily_coverage_days"]:
		blockers.append({"severity": "p1", "code": "daily_manager_coverage_gap"})
	if management["active_experiment"]["status"] in {"missing", "invalid", "expired", "pending"}:
		blockers.append({"severity": "p1", "code": "active_experiment_unavailable"})
	if management["automation_handoffs"]["unresolved_count"]:
		blockers.append({"severity": "p1", "code": "unresolved_automation_handoffs"})
	if (
		radar["impacts"] > 0
		and social["published_records"] == 0
		and not social["open_publishable_candidates"]
	):
		blockers.append({"severity": "p1", "code": "radar_to_publisher_pipeline_starved"})

	status = "healthy"
	if any(item["severity"] == "p0" for item in blockers):
		status = "blocked"
	elif blockers:
		status = "needs_action"
	return {
		"schema": "automation_effectiveness_scorecard/v1",
		"generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
		"window": {
			"start": start.isoformat().replace("+00:00", "Z"),
			"end": end.isoformat().replace("+00:00", "Z"),
		},
		"status": status,
		"live": live,
		"social": social,
		"radar": radar,
		"management": management,
		"blockers": blockers,
	}


def parse_args() -> argparse.Namespace:
	parser = argparse.ArgumentParser(description=__doc__)
	parser.add_argument("--codex-home", default=os.environ.get("CODEX_HOME", str(Path.home() / ".codex")))
	parser.add_argument("--window-days", type=int, default=7)
	parser.add_argument("--as-of", help="Exclusive ISO-8601 window end. Defaults to now.")
	parser.add_argument("--output", help="Optional scorecard path under the manager scorecard cache.")
	return parser.parse_args()


def main() -> int:
	args = parse_args()
	if args.window_days < 1:
		raise SystemExit("--window-days must be positive")
	end = parse_time(args.as_of) if args.as_of else datetime.now(timezone.utc)
	start = end - timedelta(days=args.window_days)
	payload = build_scorecard(Path(args.codex_home).expanduser(), start, end)
	rendered = json.dumps(payload, indent=2, sort_keys=True) + "\n"
	if args.output:
		path = Path(args.output)
		path = path if path.is_absolute() else RUNTIME_ROOT / path
		path = path.resolve()
		allowed = (MANAGER_ROOT / "scorecards").resolve()
		if path.parent != allowed and allowed not in path.parents:
			raise SystemExit("--output must stay under the manager scorecard cache")
		path.parent.mkdir(parents=True, exist_ok=True)
		temporary = path.with_suffix(path.suffix + ".tmp")
		temporary.write_text(rendered, encoding="utf-8")
		temporary.replace(path)
	sys.stdout.write(rendered)
	return 0 if payload["status"] == "healthy" else 1


if __name__ == "__main__":
	raise SystemExit(main())
