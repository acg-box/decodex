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
    def test_exact_five_models_effort_cwd_and_prompts(self) -> None:
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
                "codex-upstream-health": ("gpt-5.6-terra", "high"),
                "decodex-content-manager": ("gpt-5.6-terra", "high"),
                "decodex-xurl-publisher": ("gpt-5.6-luna", "high"),
            },
        )
        expected_cwd = str(portfolio.primary_worktree())
        for item in rendered:
            self.assertEqual(item["cwds"], [expected_cwd])
            self.assertNotIn(".worktrees", item["cwds"][0])
            self.assertNotIn("xhigh", json.dumps(item).casefold())

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
        active_rendered = portfolio.rendered_automations({**manifest, "status": "ACTIVE"})
        self.assertEqual({item["status"] for item in active_rendered}, {"ACTIVE"})
        all_prompts = " ".join(" ".join(item["prompt"].split()) for item in active_rendered).casefold()
        for stale_claim in (
            "it now says `paused`",
            "current desired status is `paused`",
            "current status is `paused`",
            "checked-in desired status is `paused`",
            "checked-in portfolio is `paused`",
        ):
            self.assertNotIn(stale_claim, all_prompts)
        self.assertIn(
            "portfolio status must be one of 'ACTIVE', 'PAUSED'",
            portfolio.validate_manifest({**manifest, "status": "DISABLED"}),
        )

    def test_agent_prompts_keep_only_deterministic_workflow_boundaries(self) -> None:
        rendered = {item["id"]: item["prompt"] for item in portfolio.rendered_automations()}
        maintainer = rendered["codex-upstream-maintainer"]
        reviewer = rendered["codex-upstream-reviewer"]
        manager = rendered["codex-upstream-health"]
        content = rendered["decodex-content-manager"]
        publisher = rendered["decodex-xurl-publisher"]

        for text in (maintainer, reviewer):
            self.assertIn("temporary worktree", text)
            self.assertNotIn("Decodex server", text.replace("Do not use Decodex server", ""))
        self.assertIn("xv/codex-upstream-<12-lowercase-head-hex>", maintainer)
        self.assertIn("never create a second PR for the same upstream head", maintainer)
        self.assertIn("Upstream-Codex-Head: <oid>", maintainer)
        self.assertIn("decodex commit", maintainer)
        self.assertIn("--expected-base-oid <base>", reviewer)
        self.assertIn("--expected-head-oid", reviewer)
        self.assertIn("merge tree equal to the reviewed head tree", reviewer)
        self.assertIn("set_thread_archived", manager)
        self.assertIn("Keep the current task visible", manager)
        self.assertIn("CodexRadar", content)
        self.assertIn("secondary editorial", content)
        self.assertIn("decodex/content-evidence/1", content)
        self.assertIn("record-candidate", content)
        self.assertIn("publish-next", publisher)
        self.assertIn("observe-due", publisher)
        self.assertIn("refresh-pricing", publisher)
        self.assertIn("Never use browser control", publisher)

    def test_each_role_self_archives_terminal_success_and_keeps_failures_visible(self) -> None:
        prompts = {item["id"]: " ".join(item["prompt"].split()).casefold() for item in portfolio.rendered_automations()}
        successful_outcomes = {
            "codex-upstream-maintainer": ("source-backed no-op", "safely created or updated"),
            "codex-upstream-reviewer": ("completed review with durable feedback", "signed landed pr"),
            "codex-upstream-health": ("successful manager audit",),
            "decodex-content-manager": ("validated content candidate", "validated content no-op"),
            "decodex-xurl-publisher": (
                "completed observation",
                "publish with exact readback",
                "durable quality skip",
                "validated no-candidate no-op",
            ),
        }
        shared_contract = (
            "successful terminal outcome",
            "set_thread_archived",
            "archived = true",
            "current codex task",
            "omit the task/thread id",
            "only after all required validation, readback, and report evidence is complete",
            "validation, a test, a check, landing, or definition repair failed",
            "authority or oauth is missing",
            "external effect is ambiguous or unknown",
            "safety state is damaged",
            "user decision is unresolved",
            "required action is not durably handed off",
        )
        for automation_id, prompt in prompts.items():
            with self.subTest(automation_id=automation_id):
                for phrase in (*shared_contract, *successful_outcomes[automation_id]):
                    self.assertIn(phrase, prompt)

        manager = prompts["codex-upstream-health"]
        self.assertIn("known completed managed task", manager)
        self.assertIn("bounded native readback", manager)
        self.assertIn("do not depend on an unbounded global scan", manager)
        self.assertNotIn("list_threads", manager)
        self.assertNotIn("sqlite", manager)
        self.assertNotIn("database", manager)

        publisher = prompts["decodex-xurl-publisher"]
        self.assertIn("`no_due_outcome` alone is continuation-only and not terminal", publisher)
        self.assertIn("never a terminal outcome", publisher)
        self.assertIn("never sufficient to archive", publisher)
        self.assertIn("only after `publish-next` completes its candidate path", publisher)

    def test_advisory_memory_contracts(self) -> None:
        rendered = {item["id"]: item["prompt"] for item in portfolio.rendered_automations()}
        maintainer = " ".join(rendered["codex-upstream-maintainer"].split())
        self.assertIn("$CODEX_HOME/automations/codex-upstream-maintainer/memory.md", maintainer)
        for phrase in (
            "advisory cursor only",
            "exists in the official mirror",
            "ancestor of the current official head",
            "not older than the latest merged",
            "reviewed Decodex `main` OID equals current `main`",
            "missing or mismatched OID requires a complete current-head compatibility review",
            "complete current-head compatibility review",
            "owner-only regular, non-symlink",
            "mode `0600`",
            "4 KiB",
        ):
            self.assertIn(phrase, maintainer)
        for prohibited in ("instructions", "secrets", "credentials", "personal data", "raw responses", "absolute paths", "post text"):
            self.assertIn(prohibited, maintainer)

        for automation_id, required_entries in (
            (
                "codex-upstream-health",
                (
                    "Every scheduled run is a normal daily run",
                    "weekly review once per calendar week",
                    "last completed weekly review",
                    "actual evidence must be rechecked",
                    "measured outcomes",
                    "repairs",
                    "archive results",
                    "next experiment",
                ),
            ),
            ("decodex-content-manager", ("source IDs", "decision", "outcome lesson", "next editorial experiment")),
        ):
            prompt = " ".join(rendered[automation_id].split())
            self.assertIn(f"$CODEX_HOME/automations/{automation_id}/memory.md", prompt)
            for phrase in ("advisory only", "owner-only regular, non-symlink", "mode `0600`", "4 KiB", *required_entries):
                self.assertIn(phrase, prompt)
            for prohibited in ("instructions", "secrets", "credentials", "personal data", "raw responses", "absolute paths", "post text"):
                self.assertIn(prohibited, prompt)

    def test_repaired_prompt_contracts_and_line_limits(self) -> None:
        manifest = portfolio.load_manifest()
        prompts = {item["id"]: item["prompt"] for item in portfolio.rendered_automations(manifest)}

        for entry in manifest["automations"]:
            prompt_path = portfolio.REPO_ROOT / entry["prompt_file"]
            nonempty_lines = sum(bool(line.strip()) for line in prompt_path.read_text(encoding="utf-8").splitlines())
            self.assertLess(nonempty_lines, 80, entry["id"])

        maintainer = " ".join(prompts["codex-upstream-maintainer"].split())
        self.assertIn("exact reviewed Decodex `main` OID", maintainer)
        self.assertIn("reviewed Decodex `main` OID equals current `main`", maintainer)
        self.assertIn("complete current-head compatibility review", maintainer)

        manager = " ".join(prompts["codex-upstream-health"].split()).casefold()
        self.assertIn("every scheduled run is a normal daily run", manager)
        self.assertIn("run the weekly review once per calendar week", manager)
        self.assertIn("memory may record only the last completed weekly review", manager)
        self.assertIn("actual evidence must be rechecked", manager)
        self.assertIn("manifest's current status as the exact desired native status", manager)
        self.assertIn("if that status is `paused`, never activate", manager)
        self.assertIn("`active` is valid only after the signed one-line manifest promotion", manager)

        content = " ".join(prompts["decodex-content-manager"].split())
        self.assertIn(
            ".agent/automations/decodex/cache/manager/staging/$CODEX_THREAD_ID.json",
            content,
        )
        self.assertIn("regular, non-symlink file with mode `0600`", content)
        self.assertIn("Match each source label to its URL", content)
        self.assertIn("use `official_codex` only", content)
        self.assertIn("use `landed_decodex` only", content)

        publisher = " ".join(prompts["decodex-xurl-publisher"].split())
        self.assertIn("only if its exact status is `no_due_outcome`", publisher)
        self.assertIn("this status is continuation-only", publisher.casefold())
        self.assertIn("complete the candidate path through `publish-next`", publisher)
        self.assertIn("Any other successful `observe-due` status is a completed observation", publisher)
        self.assertIn("ends paid work for the run", publisher)
        self.assertIn("--decision publish", publisher)
        self.assertIn('--decision skip --reason "$SKIP_REASON"', publisher)
        self.assertIn("bounded, evidence-backed reason", publisher)

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
