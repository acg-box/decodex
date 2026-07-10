from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "summarize_automation_effectiveness.py"
SPEC = importlib.util.spec_from_file_location("effectiveness", SCRIPT)
assert SPEC and SPEC.loader
effectiveness = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(effectiveness)


class EffectivenessScorecardTests(unittest.TestCase):
	def test_social_finds_open_candidate_and_stale_reservation(self) -> None:
		with tempfile.TemporaryDirectory() as value:
			root = Path(value)
			social = root / ".agent/automations/decodex/cache/social/x"
			(social / "candidates").mkdir(parents=True)
			(social / "posts").mkdir()
			(social / "reservations").mkdir()
			(social / "candidates/candidate.json").write_text(
				json.dumps(
					{
						"decision": {
							"worthiness": "publish",
							"idempotency_key": "candidate-1",
						}
					}
				),
				encoding="utf-8",
			)
			(social / "reservations/reservation.json").write_text(
				json.dumps(
					{
						"status": "active",
						"expires_at": "2026-07-01T00:00:00Z",
					}
				),
				encoding="utf-8",
			)
			end = datetime(2026, 7, 10, tzinfo=timezone.utc)
			with patch.object(effectiveness, "RUNTIME_ROOT", root):
				result = effectiveness.inspect_social(end - timedelta(days=7), end)
			self.assertEqual(result["open_publishable_candidates"], ["candidate.json"])
			self.assertEqual(result["stale_reservations"], ["reservation.json"])

	def test_live_config_reports_worktree_binding(self) -> None:
		with tempfile.TemporaryDirectory() as value:
			codex_home = Path(value)
			path = codex_home / "automations/manager/automation.toml"
			path.parent.mkdir(parents=True)
			path.write_text(
				'status = "ACTIVE"\ncwds = ["/repo/.worktrees/task"]\n',
				encoding="utf-8",
			)
			result = effectiveness.inspect_live_configs(codex_home, ["manager"])
			self.assertEqual(result["worktree_bound"], ["manager"])

	def test_active_experiment_reports_active_and_expired(self) -> None:
		with tempfile.TemporaryDirectory() as value:
			root = Path(value)
			path = root / ".agent/automations/decodex/cache/manager/experiments/active.json"
			path.parent.mkdir(parents=True)
			path.write_text(
				json.dumps(
					{
						"effective_window": {
							"start": "2026-07-10T00:00:00Z",
							"end": "2026-07-17T00:00:00Z",
						},
						"experiments": [{"status": "active"}],
					}
				),
				encoding="utf-8",
			)
			with (
				patch.object(effectiveness, "RUNTIME_ROOT", root),
				patch.object(effectiveness, "MANAGER_ROOT", path.parents[1]),
			):
				active = effectiveness.inspect_active_experiment(
					datetime(2026, 7, 12, tzinfo=timezone.utc)
				)
				expired = effectiveness.inspect_active_experiment(
					datetime(2026, 7, 18, tzinfo=timezone.utc)
				)
			self.assertEqual(active["status"], "active")
			self.assertEqual(expired["status"], "expired")

	def test_management_uses_live_update_as_coverage_baseline(self) -> None:
		with tempfile.TemporaryDirectory() as value:
			root = Path(value)
			manager_root = root / ".agent/automations/decodex/cache/manager"
			(manager_root / "reports/2026-07-10").mkdir(parents=True)
			(manager_root / "reports/2026-07-10/report.md").write_text("report\n", encoding="utf-8")
			with (
				patch.object(effectiveness, "RUNTIME_ROOT", root),
				patch.object(effectiveness, "MANAGER_ROOT", manager_root),
			):
				result = effectiveness.inspect_management(
					datetime(2026, 7, 3, tzinfo=timezone.utc),
					datetime(2026, 7, 10, 12, tzinfo=timezone.utc),
					"2026-07-10T00:00:00Z",
				)
			self.assertEqual(result["expected_daily_coverage_days"], 0)
			self.assertEqual(result["daily_reports"], 1)

	def test_scorecard_blocks_missing_experiment_and_coverage_gap(self) -> None:
		live = {
			"managed": 1,
			"statuses": {"ACTIVE": 1},
			"missing": [],
			"worktree_bound": [],
			"updated_at": {"decodex-automation-manager": "2026-07-01T00:00:00Z"},
		}
		social = {
			"stale_reservations": [],
			"published_records": 1,
			"open_publishable_candidates": [],
		}
		management = {
			"daily_reports": 1,
			"daily_coverage_days": ["2026-07-09"],
			"expected_daily_coverage_days": 7,
			"active_experiment": {"status": "missing"},
		}
		with (
			patch.object(effectiveness, "managed_automation_ids", return_value=["manager"]),
			patch.object(effectiveness, "inspect_live_configs", return_value=live),
			patch.object(effectiveness, "inspect_social", return_value=social),
			patch.object(effectiveness, "inspect_radar", return_value={"impacts": 0}),
			patch.object(effectiveness, "inspect_management", return_value=management),
		):
			result = effectiveness.build_scorecard(
				Path("/tmp/codex-home"),
				datetime(2026, 7, 3, tzinfo=timezone.utc),
				datetime(2026, 7, 10, tzinfo=timezone.utc),
			)
		self.assertEqual(result["status"], "needs_action")
		self.assertEqual(
			[item["code"] for item in result["blockers"]],
			["daily_manager_coverage_gap", "active_experiment_unavailable"],
		)


if __name__ == "__main__":
	unittest.main()
