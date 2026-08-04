from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path


CONFIG_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(CONFIG_DIR))

import portfolio  # noqa: E402


class PortfolioTests(unittest.TestCase):
    def test_manifest_renders_exact_five_automations(self) -> None:
        manifest = portfolio.load_manifest()
        self.assertEqual(portfolio.validate_manifest(manifest), [])
        rendered = portfolio.rendered_automations(manifest)

        self.assertEqual(len(rendered), 5)
        self.assertEqual({item["id"] for item in rendered}, portfolio.EXPECTED_IDS)
        self.assertEqual(
            {item["id"]: (item["model"], item["reasoning_effort"]) for item in rendered},
            {
                "codex-upstream-maintainer": ("gpt-5.6-sol", "max"),
                "codex-upstream-reviewer": ("gpt-5.6-sol", "max"),
                "codex-upstream-health": ("gpt-5.6-luna", "max"),
                "decodex-content-manager": ("gpt-5.6-luna", "max"),
                "decodex-xurl-publisher": ("gpt-5.6-luna", "max"),
            },
        )
        expected_cwd = str(portfolio.primary_worktree())
        for item in rendered:
            if item["model"] == "gpt-5.6-sol":
                self.assertIn(
                    item["id"],
                    {"codex-upstream-maintainer", "codex-upstream-reviewer"},
                )
                self.assertEqual(item["reasoning_effort"], "max")
            else:
                self.assertEqual(
                    (item["model"], item["reasoning_effort"]),
                    ("gpt-5.6-luna", "max"),
                )
            self.assertEqual(item["cwds"], [expected_cwd])
            self.assertNotIn(".worktrees", item["cwds"][0])
            self.assertNotIn("xhigh", item["reasoning_effort"].casefold())

    def test_status_and_allowed_promotion_contract(self) -> None:
        manifest = portfolio.load_manifest()
        self.assertIn(manifest["status"], {"ACTIVE", "PAUSED"})
        self.assertEqual(
            {item["status"] for item in portfolio.rendered_automations(manifest)},
            {manifest["status"]},
        )
        for status in ("PAUSED", "ACTIVE"):
            candidate_manifest = {**manifest, "status": status}
            self.assertEqual(portfolio.validate_manifest(candidate_manifest), [])
            self.assertEqual(
                {item["status"] for item in portfolio.rendered_automations(candidate_manifest)},
                {status},
            )
        self.assertIn(
            "portfolio status must be one of 'ACTIVE', 'PAUSED'",
            portfolio.validate_manifest({**manifest, "status": "DISABLED"}),
        )

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

            extra = codex_home / "automations/decodex-unexpected/automation.toml"
            extra.parent.mkdir(parents=True)
            extra.write_text('id = "decodex-unexpected"\n', encoding="utf-8")
            report = portfolio.evaluate_runtime(codex_home)
            self.assertEqual(report["status"], "fail")
            self.assertEqual(report["extra_managed_ids"], ["decodex-unexpected"])

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
                if item["id"] == "codex-upstream-health":
                    values["status"] = "PAUSED" if item["status"] == "ACTIVE" else "ACTIVE"
                path.write_text(
                    "\n".join(f"{key} = {json.dumps(value)}" for key, value in values.items()) + "\n",
                    encoding="utf-8",
                )
            report = portfolio.evaluate_runtime(codex_home)
            errors = next(item["errors"] for item in report["results"] if item["id"] == "codex-upstream-health")
            self.assertIn("native status differs from portfolio", errors)


if __name__ == "__main__":
    unittest.main()
