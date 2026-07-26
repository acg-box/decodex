from __future__ import annotations

import json
import subprocess
import sys
import tomllib
import unittest
from pathlib import Path


CONFIG_ROOT = Path(__file__).resolve().parents[1]
REPO_ROOT = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(CONFIG_ROOT))

from automation_eval.io import load_toml  # noqa: E402
from automation_eval.validators import validate_manifest_shape  # noqa: E402
from automation_sync.manifest import automation_specs  # noqa: E402
from automation_sync.paths import DEFAULT_MANIFESTS  # noqa: E402
from automation_sync.render import render_live_config  # noqa: E402


class UpstreamAutomationConfigTests(unittest.TestCase):
	def setUp(self) -> None:
		self.manifest_path = REPO_ROOT / "automations/upstream/automations.toml"
		self.manifest = load_toml(self.manifest_path)
		self.content_manifest_path = REPO_ROOT / "automations/decodex/automations.toml"
		self.content_manifest = load_toml(self.content_manifest_path)

	def test_default_install_contains_upstream_and_content_loops(self) -> None:
		self.assertEqual(
			DEFAULT_MANIFESTS,
			[self.manifest_path, self.content_manifest_path],
		)
		self.assertEqual(
			[item["id"] for item in self.manifest["automations"]],
			[
				"codex-upstream-maintainer",
				"codex-upstream-reviewer",
				"codex-upstream-health",
			],
		)
		self.assertEqual(
			[item["id"] for item in self.content_manifest["automations"]],
			[
				"decodex-content-manager",
				"decodex-x-browser-publisher",
			],
		)
		help_result = subprocess.run(
			[
				sys.executable,
				str(
					REPO_ROOT
					/ "automations/decodex/scripts/config/sync_automations.py"
				),
				"--help",
			],
			check=True,
			capture_output=True,
			text=True,
		)
		normalized_help = " ".join(help_result.stdout.split())
		self.assertIn(
			"Defaults to the current upstream and content manifests.",
			normalized_help,
		)
		self.assertNotIn("Decodex and Radar manifests", normalized_help)

	def test_manifest_is_active_high_local_and_primary_checkout_portable(self) -> None:
		self.assertEqual(validate_manifest_shape(self.manifest), [])
		defaults = self.manifest["defaults"]
		self.assertEqual(defaults["status"], "ACTIVE")
		self.assertEqual(defaults["model"], "gpt-5.6-sol")
		self.assertEqual(defaults["reasoning_effort"], "high")
		self.assertEqual(defaults["execution_environment"], "local")
		self.assertEqual(defaults["cwd"], "{repo_root}")
		self.assertEqual(defaults["source_root"], "automations/upstream")

	def test_content_manifest_is_active_high_local_and_primary_checkout_portable(self) -> None:
		self.assertEqual(validate_manifest_shape(self.content_manifest), [])
		defaults = self.content_manifest["defaults"]
		self.assertEqual(defaults["status"], "ACTIVE")
		self.assertEqual(defaults["model"], "gpt-5.6-sol")
		self.assertEqual(defaults["reasoning_effort"], "high")
		self.assertEqual(defaults["execution_environment"], "local")
		self.assertEqual(defaults["cwd"], "{repo_root}")
		self.assertEqual(defaults["source_root"], "automations/decodex")

	def test_radar_has_no_scheduled_manifest(self) -> None:
		self.assertFalse((REPO_ROOT / "automations/radar/automations.toml").exists())

	def test_headless_gate_preserves_all_non_gpui_checks(self) -> None:
		with (REPO_ROOT / "Makefile.toml").open("rb") as makefile:
			tasks = tomllib.load(makefile)["tasks"]

		self.assertEqual(
			tasks["check-upstream-automation"]["dependencies"],
			[
				"audit-node",
				"build",
				"check-node",
				"check-rust-headless",
				"fmt-check",
				"lint-rust-headless",
				"test-headless",
			],
		)
		self.assertEqual(
			tasks["check-upstream-automation-sandboxed"]["dependencies"],
			[
				"build",
				"check-node",
				"check-rust-headless",
				"fmt-check-sandboxed",
				"lint-rust-headless",
				"test-headless",
			],
		)
		self.assertEqual(
			tasks["fmt-rust-check-sandboxed"],
			{
				"workspace": False,
				"command": "${DECODEX_TRUSTED_NIGHTLY_CARGO_FMT}",
				"args": ["--all", "--", "--check"],
			},
		)
		for task_name in (
			"check-rust-headless",
			"lint-rust-headless",
			"test-rust-headless",
		):
			self.assertIn("--exclude", tasks[task_name]["args"])
			self.assertIn("decodex-gpui", tasks[task_name]["args"])
		self.assertEqual(
			tasks["audit-node"]["dependencies"],
			[
				"audit-node-lock",
				"prepare-node",
				"audit-node-advisories",
				"audit-node-provenance",
				"audit-node-signatures",
			],
		)
		for task_name in (
			"prepare-node",
			"audit-node-advisories",
			"audit-node-signatures",
			"build-node",
			"check-node-types",
		):
			self.assertEqual(
				tasks[task_name]["dependencies"],
				["audit-node-lock"],
			)
		self.assertEqual(
			tasks["audit-node-provenance"]["dependencies"],
			["prepare-node"],
		)

	def test_astro_runtime_and_package_manager_are_explicit(self) -> None:
		package = json.loads(
			(REPO_ROOT / "site/package.json").read_text(encoding="utf-8")
		)
		self.assertEqual(package["packageManager"], "npm@11.17.0")
		self.assertEqual(package["engines"], {"node": ">=22.12.0"})
		self.assertEqual(
			(REPO_ROOT / "site/.nvmrc").read_text(encoding="utf-8"),
			"22.12.0\n",
		)
		self.assertTrue((REPO_ROOT / "scripts/audit_node_lock.py").is_file())

	def test_live_renderer_keeps_codex_app_metadata(self) -> None:
		for manifest_path in (self.manifest_path, self.content_manifest_path):
			for spec in automation_specs(manifest_path):
				with self.subTest(automation=spec["id"]):
					rendered = render_live_config(
						spec,
						Path("/portable/main"),
						created_at=123,
						updated_at=456,
					)
					config = tomllib.loads(rendered)

					self.assertEqual(config["created_at"], 123)
					self.assertEqual(config["updated_at"], 456)
					self.assertEqual(config["reasoning_effort"], "high")
					self.assertNotEqual(config["reasoning_effort"], "xhigh")
					self.assertEqual(config["execution_environment"], "local")
					self.assertEqual(config["cwds"], ["/portable/main"])

	def test_content_prompts_enforce_browser_only_x_and_account_restore(self) -> None:
		manager = (
			REPO_ROOT / "automations/decodex/prompts/content-manager.md"
		).read_text(encoding="utf-8")
		publisher = (
			REPO_ROOT / "automations/decodex/prompts/x-browser-publisher.md"
		).read_text(encoding="utf-8")

		self.assertIn("Publisher is the only X operator", manager)
		self.assertIn("Do not open X, use X MCP or X API", manager)
		self.assertIn("use `https://codexradar.com/` only for secondary", manager)
		self.assertIn("social_strategy/v1", manager)
		self.assertIn("most 16 decisions", manager)
		self.assertIn('decision.worthiness = "skip"', manager)
		self.assertIn("with no path arguments", manager)
		self.assertIn("Never commit, upload, publish, or archive them to GitHub", manager)
		self.assertIn("Use browser control for every X read and write", publisher)
		self.assertIn("Do not use X MCP, X API", publisher)
		self.assertIn("acquire-browser-lease", publisher)
		self.assertIn("verify-browser-lease", publisher)
		self.assertIn("release-browser-lease", publisher)
		self.assertIn("restore the initial account", publisher)
		self.assertIn("browser_touched", publisher)
		self.assertIn("publication.publisher = \"chrome\"", publisher)
		self.assertIn("social_outcome/v1", publisher)
		self.assertIn("23 to 48 hours", publisher)
		self.assertIn("167 to 192 hours", publisher)
		self.assertIn("with no path arguments", publisher)
		self.assertIn("Never commit, upload, publish, or archive them to", publisher)

	def test_content_manifest_tracks_all_social_contracts(self) -> None:
		required = {
			"automations/decodex/scripts/social/social_candidate.schema.json",
			"automations/decodex/scripts/social/social_outcome.schema.json",
			"automations/decodex/scripts/social/social_post.schema.json",
			"automations/decodex/scripts/social/social_publish_reservation.schema.json",
			"automations/decodex/scripts/social/social_strategy.schema.json",
		}
		by_id = {
			automation["id"]: set(automation["required_paths"])
			for automation in self.content_manifest["automations"]
		}
		self.assertIn(
			"automations/decodex/scripts/social/social_strategy.schema.json",
			by_id["decodex-content-manager"],
		)
		self.assertTrue(
			required
			- {"automations/decodex/scripts/social/social_strategy.schema.json"}
			<= by_id["decodex-x-browser-publisher"]
		)
		health = next(
			automation
			for automation in self.manifest["automations"]
			if automation["id"] == "codex-upstream-health"
		)
		self.assertTrue(required <= set(health["required_paths"]))

	def test_all_managed_runs_use_conditional_native_self_archive(self) -> None:
		retention_path = (
			REPO_ROOT
			/ "automations/decodex/skills/references/"
			"scheduled-run-thread-retention.md"
		)
		retention_ref = (
			"automations/decodex/skills/references/"
			"scheduled-run-thread-retention.md"
		)
		retention = retention_path.read_text(encoding="utf-8")

		self.assertIn("set_thread_archived", retention)
		self.assertIn("omit `threadId`", retention)
		self.assertIn("auto_archive", retention)
		self.assertIn("keep_visible", retention)
		self.assertIn("account restoration failure", retention)
		self.assertIn("unknown push/merge/publication result", retention)

		for manifest in (self.manifest, self.content_manifest):
			for automation in manifest["automations"]:
				with self.subTest(automation=automation["id"]):
					self.assertIn(retention_ref, automation["required_paths"])
					prompt = (
						REPO_ROOT / automation["prompt_file"]
					).read_text(encoding="utf-8")
					self.assertIn("scheduled-run-thread-retention.md", prompt)
					self.assertIn("set_thread_archived", prompt)
					self.assertIn("visible", prompt)
					self.assertNotIn("Archive the task.", prompt)

	def test_health_prompt_owns_bounded_native_reconciliation(self) -> None:
		prompt = (
			REPO_ROOT / "automations/upstream/prompts/health.md"
		).read_text(encoding="utf-8")
		self.assertIn("`automation_update`", prompt)
		self.assertIn("Never write `$CODEX_HOME`", prompt)
		for automation_id in (
			"codex-upstream-maintainer",
			"codex-upstream-reviewer",
			"codex-upstream-health",
			"decodex-content-manager",
			"decodex-x-browser-publisher",
		):
			self.assertIn(automation_id, prompt)
		self.assertIn("Read back every created or updated definition", prompt)
		self.assertIn("five exact automation definitions", prompt)
		self.assertIn("Do not list, mutate,", prompt)
		self.assertIn("unrelated scheduler definitions", prompt)
		self.assertIn("all five managed", prompt)
		self.assertIn("content_loop_degraded", prompt)
		self.assertLess(
			prompt.index("Recover before new observation"),
			prompt.index("Discover `automation_update`"),
		)
		self.assertLess(
			prompt.index("Discover `automation_update`"),
			prompt.index("After recovery and reconciliation"),
		)

	def test_reviewer_delegates_all_landing_writes_to_decodex(self) -> None:
		prompt = (
			REPO_ROOT / "automations/upstream/prompts/reviewer.md"
		).read_text(encoding="utf-8")
		self.assertIn(
			"verifies the exact clean pull-request branch worktree",
			prompt,
		)
		self.assertIn(
			"Only `decodex land` creates and\n   pushes the signed merge",
			prompt,
		)
		self.assertIn("pushes the signed merge", prompt)
		self.assertIn(
			"`--force-with-lease` expected old object ID",
			prompt,
		)
		self.assertIn("`--expected-base-oid`", prompt)
		self.assertIn("`--expected-head-oid`", prompt)
		self.assertIn("21,000-second land budget", prompt)
		self.assertIn("After a `land_started` crash", prompt)
		self.assertIn("clean primary `main`", prompt)
		self.assertIn("force-with-lease", prompt)
		self.assertIn(
			"returned to\n   Maintainer with `base_stale`",
			prompt,
		)
		self.assertIn("exact intent-bound JSON", prompt)
		self.assertIn("Only the state tool `land` command", prompt)
		self.assertIn("it never creates the merge or cleans\n   the lane", prompt)
		self.assertNotIn("already-merged recovery", prompt)

	def test_maintainer_uses_only_transactional_effect_wrappers(self) -> None:
		prompt = (
			REPO_ROOT / "automations/upstream/prompts/maintainer.md"
		).read_text(encoding="utf-8")
		for command in ("commit-candidate", "publish", "retire-pr"):
			self.assertIn(command, prompt)
		self.assertNotIn("upstream_autopilot.py check-lease", prompt)
		self.assertIn(
			"Do not invoke `decodex`, `git commit`, `git push`, `gh pr create`",
			prompt,
		)
		self.assertIn("automatically renews only when needed", prompt)
		self.assertIn("Do not execute candidate code, tests", prompt)
		self.assertIn("external-network-denied macOS sandbox", prompt)

	def test_state_wrapper_has_no_landing_write_implementation(self) -> None:
		effects = (
			REPO_ROOT
			/ "automations/upstream/scripts/upstream_autopilot_lib/effects.py"
		).read_text(encoding="utf-8")
		cli = (
			REPO_ROOT
			/ "automations/upstream/scripts/upstream_autopilot_lib/cli.py"
		).read_text(encoding="utf-8")
		for removed_authority in (
			"create_signed_land_merge_commit",
			"push_land_merge_commit_cas",
			"recover_merged_land_lane",
			'"commit-tree"',
		):
			self.assertNotIn(removed_authority, effects)
			self.assertNotIn(removed_authority, cli)
		self.assertIn("run_decodex_land(", cli)
		self.assertIn('"--expected-base-oid"', effects)
		self.assertIn('"--expected-head-oid"', effects)

	def test_lease_policy_fences_validation_and_external_effect_timeouts(self) -> None:
		policy = json.loads(
			(REPO_ROOT / "automations/upstream/policy.json").read_text(
				encoding="utf-8"
			)
		)
		self.assertGreaterEqual(policy["lease_seconds"], 21_000)
		self.assertGreaterEqual(policy["lease_write_guard_seconds"], 9_000)
		self.assertLess(
			policy["lease_write_guard_seconds"],
			policy["lease_seconds"],
		)

	def test_manifest_tracks_effect_and_validation_owners(self) -> None:
		for automation in self.manifest["automations"]:
			with self.subTest(automation=automation["id"]):
				self.assertIn(
					"automations/upstream/scripts/upstream_autopilot_lib/effects.py",
					automation["required_paths"],
				)
				self.assertIn(
					"automations/upstream/scripts/upstream_autopilot_lib/validation.py",
					automation["required_paths"],
				)


if __name__ == "__main__":
	unittest.main()
