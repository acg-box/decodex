from __future__ import annotations

import json
import sys
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace
from pathlib import Path


CONFIG_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(CONFIG_DIR))

import portfolio  # noqa: E402


class PortfolioTests(unittest.TestCase):
    def test_manifest_renders_the_configured_values_and_primary_worktree(self) -> None:
        manifest = portfolio.load_manifest()
        rendered = {item["id"]: item for item in portfolio.rendered_automations(manifest)}
        self.assertEqual(set(rendered), {entry["id"] for entry in manifest["automations"]})
        for entry in manifest["automations"]:
            with self.subTest(automation=entry["id"]):
                item = rendered[entry["id"]]
                for key in ("model", "reasoning_effort", "rrule"):
                    self.assertEqual(item[key], entry[key])
                for key in ("status", "execution_environment"):
                    self.assertEqual(item[key], entry.get(key, manifest[key]))
                self.assertEqual(item["cwds"], [str(portfolio.primary_worktree())])

    def test_primary_project_is_independent_of_branch_and_caller(self) -> None:
        listing = (
            "worktree /project/decodex\nHEAD abc\nbranch refs/heads/xv/prototype\n\n"
            "worktree /tmp/linked\nHEAD def\nbranch refs/heads/main\n\n"
        )
        with patch.object(portfolio.subprocess, "run", return_value=SimpleNamespace(stdout=listing)):
            self.assertEqual(portfolio.primary_worktree(Path("/tmp/linked")), Path("/project/decodex"))
        with patch.object(portfolio.subprocess, "run", return_value=SimpleNamespace(stdout="worktree /tmp/bare\nbare\n")):
            with self.assertRaises(portfolio.PortfolioError):
                portfolio.primary_worktree(Path("/tmp/bare"))

    def test_global_status_does_not_override_explicit_manual_pause(self) -> None:
        manifest = portfolio.load_manifest()
        manifest["automations"][0]["status"] = "PAUSED"
        for status in ("PAUSED", "ACTIVE"):
            candidate = {**manifest, "status": status}
            self.assertEqual(portfolio.validate_manifest(candidate), [])
            rendered = {item["id"]: item for item in portfolio.rendered_automations(candidate)}
            self.assertEqual(rendered["codex-upstream-maintainer"]["status"], "PAUSED")
            self.assertEqual(rendered["decodex-content-manager"]["status"], status)
        candidate = {**manifest, "automations": [dict(item) for item in manifest["automations"]]}
        candidate["automations"][0]["status"] = "DISABLED"
        self.assertTrue(any("invalid status" in error for error in portfolio.validate_manifest(candidate)))
        candidate["automations"][0]["status"] = "ACTIVE"
        self.assertEqual(portfolio.validate_manifest(candidate), [])
        self.assertEqual(portfolio.rendered_automations(candidate)[0]["status"], "ACTIVE")

    def test_runtime_evaluation_requires_metadata_and_rejects_extra_managed_ids(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            codex_home = Path(directory)
            for item in portfolio.rendered_automations():
                path = codex_home / "automations" / item["id"] / "automation.toml"
                path.parent.mkdir(parents=True)
                values = {**item, "created_at": 1, "updated_at": 2}
                path.write_text(
                    "\n".join(f"{key} = {json.dumps(value)}" for key, value in values.items())
                    + "\n",
                    encoding="utf-8",
                )
            self.assertEqual(portfolio.evaluate_runtime(codex_home)["status"], "pass")

            extra = codex_home / "automations/codex-upstream-reviewer/automation.toml"
            extra.parent.mkdir(parents=True)
            extra.write_text('id = "codex-upstream-reviewer"\n', encoding="utf-8")
            report = portfolio.evaluate_runtime(codex_home)
            self.assertEqual(report["status"], "fail")
            self.assertEqual(report["extra_managed_ids"], ["codex-upstream-reviewer"])

            first = codex_home / "automations/codex-upstream-maintainer/automation.toml"
            first.write_text(first.read_text(encoding="utf-8").replace("created_at = 1\n", ""), encoding="utf-8")
            report = portfolio.evaluate_runtime(codex_home)
            errors = next(item["errors"] for item in report["results"] if item["id"] == "codex-upstream-maintainer")
            self.assertIn("native created_at metadata is missing", errors)

    def test_runtime_status_must_match_the_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            codex_home = Path(directory)
            for item in portfolio.rendered_automations():
                path = codex_home / "automations" / item["id"] / "automation.toml"
                path.parent.mkdir(parents=True)
                values = {**item, "created_at": 1, "updated_at": 2}
                if item["id"] == "codex-upstream-maintainer":
                    values["status"] = "PAUSED" if item["status"] == "ACTIVE" else "ACTIVE"
                path.write_text(
                    "\n".join(f"{key} = {json.dumps(value)}" for key, value in values.items()) + "\n",
                    encoding="utf-8",
                )
            report = portfolio.evaluate_runtime(codex_home)
            errors = next(item["errors"] for item in report["results"] if item["id"] == "codex-upstream-maintainer")
            self.assertIn("native status differs from portfolio", errors)


if __name__ == "__main__":
    unittest.main()
