from __future__ import annotations

import sys
import unittest
from pathlib import Path
from unittest.mock import patch


CONFIG_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(CONFIG_ROOT))

import automation_checkout  # noqa: E402
from automation_eval.model import AutomationResult  # noqa: E402
from automation_eval.validators import validate_active_config  # noqa: E402


class AutomationCheckoutTests(unittest.TestCase):
	def test_selects_checkout_owning_main(self) -> None:
		worktrees = """worktree /repo
HEAD abc
branch refs/heads/main

worktree /repo/.worktrees/task
HEAD def
branch refs/heads/xy/task
"""
		with patch.object(automation_checkout, "_git", return_value=worktrees):
			self.assertEqual(
				automation_checkout.primary_checkout_for_branch(Path("/repo/.worktrees/task")),
				Path("/repo"),
			)

	def test_rejects_linked_worktree_runtime_root(self) -> None:
		with patch.object(automation_checkout, "is_linked_worktree", return_value=True):
			with self.assertRaisesRegex(ValueError, "must not be a linked worktree"):
				automation_checkout.validate_runtime_checkout(Path("/repo/.worktrees/task"))

	def test_requires_main_branch(self) -> None:
		with (
			patch.object(automation_checkout, "is_linked_worktree", return_value=False),
			patch.object(automation_checkout, "_git", return_value="xy/task"),
		):
			with self.assertRaisesRegex(ValueError, "must use branch 'main'"):
				automation_checkout.validate_runtime_checkout(Path("/repo"))

	def test_active_config_rejects_worktree_cwd_explicitly(self) -> None:
		result = AutomationResult("manager")
		defaults = {
			"kind": "cron",
			"status": "ACTIVE",
			"model": "gpt-5.6-sol",
			"reasoning_effort": "high",
			"execution_environment": "local",
			"cwd": "{repo_root}",
		}
		automation = {"name": "Manager", "rrule": "FREQ=DAILY"}
		active = {
			"kind": "cron",
			"name": "Manager",
			"status": "ACTIVE",
			"rrule": "FREQ=DAILY",
			"model": "gpt-5.6-sol",
			"reasoning_effort": "high",
			"execution_environment": "local",
			"cwds": ["/repo/.worktrees/task"],
			"prompt": "prompt",
			"created_at": 123,
			"updated_at": 456,
		}
		with patch("automation_eval.validators.expected_cwd", return_value="/repo"):
			validate_active_config(automation, defaults, "prompt", active, result)
		self.assertIn(
			"active cwds must not bind automations to a worktree",
			result.errors,
		)

	def test_active_config_requires_codex_app_list_metadata(self) -> None:
		result = AutomationResult("manager")
		defaults = {
			"kind": "cron",
			"status": "ACTIVE",
			"model": "gpt-5.6-sol",
			"reasoning_effort": "high",
			"execution_environment": "local",
			"cwd": "{repo_root}",
		}
		automation = {"name": "Manager", "rrule": "FREQ=DAILY"}
		active = {
			"kind": "cron",
			"name": "Manager",
			"status": "ACTIVE",
			"rrule": "FREQ=DAILY",
			"model": "gpt-5.6-sol",
			"reasoning_effort": "high",
			"execution_environment": "local",
			"cwds": ["/repo"],
			"prompt": "prompt",
		}
		with patch("automation_eval.validators.expected_cwd", return_value="/repo"):
			validate_active_config(automation, defaults, "prompt", active, result)
		self.assertIn("active created_at must be a positive integer", result.errors)
		self.assertIn("active updated_at must be a positive integer", result.errors)

		result = AutomationResult("manager")
		active["created_at"] = 456
		active["updated_at"] = 123
		with patch("automation_eval.validators.expected_cwd", return_value="/repo"):
			validate_active_config(automation, defaults, "prompt", active, result)
		self.assertEqual(
			result.errors,
			["active updated_at must not be earlier than created_at"],
		)


if __name__ == "__main__":
	unittest.main()
