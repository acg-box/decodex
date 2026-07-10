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


if __name__ == "__main__":
	unittest.main()
