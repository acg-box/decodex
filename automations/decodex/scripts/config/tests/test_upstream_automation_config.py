from __future__ import annotations

import copy
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path


CONFIG_ROOT = Path(__file__).resolve().parents[1]
REPO_ROOT = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(CONFIG_ROOT))

from automation_eval.io import expected_cwd, load_toml  # noqa: E402
from automation_eval.model import AutomationResult  # noqa: E402
from automation_eval.validators import (  # noqa: E402
	validate_active_config,
	validate_manifest_shape,
	validate_prompt_text,
	validate_xurl_runtime,
)
from automation_plan.cli import render_plan  # noqa: E402
from automation_plan.manifest import automation_specs  # noqa: E402
from automation_plan.paths import DEFAULT_MANIFESTS  # noqa: E402


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
				"decodex-xurl-publisher",
			],
		)
		self.assertEqual(
			{
				item["id"]: item["name"]
				for item in self.content_manifest["automations"]
			},
			{
				"decodex-content-manager": "Decodex Content Manager",
				"decodex-xurl-publisher": "Decodex Xurl Publisher",
			},
		)
		help_result = subprocess.run(
			[
				sys.executable,
				str(
					REPO_ROOT
					/ "automations/decodex/scripts/config/render_automation_plan.py"
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
		self.assertEqual(
			self.content_manifest["retired_automation_ids"],
			["decodex-x-browser-publisher"],
		)
		defaults = self.content_manifest["defaults"]
		self.assertEqual(defaults["status"], "ACTIVE")
		self.assertEqual(defaults["model"], "gpt-5.6-sol")
		self.assertEqual(defaults["reasoning_effort"], "high")
		self.assertEqual(defaults["execution_environment"], "local")
		self.assertEqual(defaults["cwd"], "{repo_root}")
		self.assertEqual(defaults["source_root"], "automations/decodex")

	def test_manifest_rejects_active_retirement_overlap(self) -> None:
		manifest = copy.deepcopy(self.content_manifest)
		manifest["retired_automation_ids"] = ["decodex-content-manager"]

		self.assertIn(
			"manifest active and retired automation ids must not overlap",
			validate_manifest_shape(manifest),
		)

	def test_xurl_artifact_schemas_pin_the_exact_supported_version(self) -> None:
		for relative_path in (
			"automations/decodex/scripts/social/social_outcome.schema.json",
			"automations/decodex/scripts/social/social_post.schema.json",
		):
			with self.subTest(schema=relative_path):
				schema = json.loads((REPO_ROOT / relative_path).read_text(encoding="utf-8"))
				if relative_path.endswith("social_outcome.schema.json"):
					version = schema["properties"]["observation"]["properties"]["xurl_version"]
				else:
					version = schema["properties"]["publication"]["properties"]["xurl_version"]
				self.assertEqual(version, {"const": "1.3.1"})

	def test_active_config_requires_local_destination_projection(self) -> None:
		automation = self.content_manifest["automations"][0]
		defaults = self.content_manifest["defaults"]
		prompt = (
			REPO_ROOT / automation["prompt_file"]
		).read_text(encoding="utf-8").strip()
		active = {
			"kind": "cron",
			"name": automation["name"],
			"status": "ACTIVE",
			"rrule": automation["rrule"],
			"model": "gpt-5.6-sol",
			"reasoning_effort": "high",
			"execution_environment": "local",
			"target": {"type": "project", "project_id": "local-test"},
			"cwds": [expected_cwd(defaults["cwd"])],
			"prompt": prompt,
			"created_at": 1,
			"updated_at": 1,
		}
		result = AutomationResult(automation_id=automation["id"])
		validate_active_config(automation, defaults, prompt, active, result)
		self.assertEqual(result.status, "pass")

		active["destination"] = "worktree"
		mismatch = AutomationResult(automation_id=automation["id"])
		validate_active_config(automation, defaults, prompt, active, mismatch)
		self.assertIn(
			"active destination must be a local project",
			mismatch.errors,
		)

	def test_prompt_fail_closed_requirement_is_case_insensitive(self) -> None:
		result = AutomationResult(automation_id="test")
		validate_prompt_text(
			"\n".join(
				[
					"Codex app automation",
					"Keep state under .agent/automations/cache.",
					"Do not use GitHub Actions.",
					"Run pwd.",
					"Run git status --short --branch.",
					"Run git rev-parse HEAD.",
					"Fail closed on mismatch.",
				]
			),
			".agent/automations/cache",
			[],
			result,
		)
		self.assertEqual(result.status, "pass")

	def test_xurl_runtime_consumes_only_publisher_probe_report(self) -> None:
		with tempfile.TemporaryDirectory() as directory:
			publisher = Path(directory) / "decodex-publisher"
			report = {
				"status": "ready",
				"ready": True,
				"xurl_version": "1.3.1",
				"xurl_app": "default",
				"account_label": "decodexspace",
				"authorization_contract": {
					"policy_id": "xurl-oauth-least-privilege/3",
					"status": "current",
					"target_account": "decodexspace",
					"xurl_app": "default",
					"required_operator_authorized_scopes": [
						"tweet.read",
						"users.read",
						"tweet.write",
						"offline.access",
					],
					"xurl_version": "1.3.1",
					"xurl_binary_sha256": (
						"7b85a210009db7a3f2d6183684674441f"
						"bf81276f1101f73d36d0266ec9aa01e"
					),
					"sealed_at": "2026-07-27T12:00:00Z",
				},
				"pricing_policy": {
					"policy_id": "x-api-pay-per-usage/2026-07-27",
					"official_source": (
						"https://docs.x.com/x-api/getting-started/pricing.md"
					),
					"reviewed_at": "2026-07-27T00:00:00Z",
					"effective_at": "2026-07-27T00:00:00Z",
					"expires_at": "2026-07-28T12:00:00Z",
					"status": "current",
					"user_read_cost_microusd": 10_000,
					"url_free_content_create_cost_microusd": 15_000,
					"post_read_cost_ceiling_microusd": 5_000,
					"monthly_reservation_cap_microusd": 1_250_000,
				},
			}

			def write_probe(payload: object) -> None:
				publisher.write_text(
					(
						"#!/bin/sh\n"
						"[ \"$#\" -eq 2 ] || exit 64\n"
						"[ \"$1 $2\" = 'social probe-xurl' ] || exit 64\n"
						f"printf '%s\\n' '{json.dumps(payload)}'\n"
					),
					encoding="utf-8",
				)
				publisher.chmod(0o700)

			write_probe(report)
			result = AutomationResult(
				automation_id="decodex-xurl-publisher"
			)
			validate_xurl_runtime(
				result,
				repo_only=False,
				publisher=publisher,
			)
			self.assertEqual(result.status, "pass")

			mismatched = copy.deepcopy(report)
			mismatched["account_label"] = "other"
			write_probe(mismatched)
			mismatch = AutomationResult(
				automation_id="decodex-xurl-publisher"
			)
			validate_xurl_runtime(
				mismatch,
				repo_only=False,
				publisher=publisher,
			)
			self.assertEqual(mismatch.status, "fail")
			self.assertEqual(
				mismatch.errors,
				["Publisher xurl readiness report is not ready"],
			)

			for name, mutate in (
				(
					"wrong authorization policy",
					lambda value: value["authorization_contract"].__setitem__(
						"policy_id",
						"xurl-oauth-least-privilege/1",
					),
				),
				(
					"missing binary digest",
					lambda value: value["authorization_contract"].pop(
						"xurl_binary_sha256"
					),
				),
				(
					"wrong exact version",
					lambda value: value["authorization_contract"].__setitem__(
						"xurl_version",
						"1.3.2",
					),
				),
				(
					"wrong operator-authorized scopes",
					lambda value: value["authorization_contract"].__setitem__(
						"required_operator_authorized_scopes",
						["tweet.read", "users.read"],
					),
				),
				(
					"wrong binary digest",
					lambda value: value["authorization_contract"].__setitem__(
						"xurl_binary_sha256",
						"0" * 64,
					),
				),
				(
					"non-Markdown pricing URL",
					lambda value: value["pricing_policy"].__setitem__(
						"official_source",
						"https://docs.x.com/x-api/getting-started/pricing",
					),
				),
			):
				with self.subTest(name=name):
					invalid = copy.deepcopy(report)
					mutate(invalid)
					write_probe(invalid)
					rejected = AutomationResult(
						automation_id="decodex-xurl-publisher"
					)
					validate_xurl_runtime(
						rejected,
						repo_only=False,
						publisher=publisher,
					)
					self.assertEqual(
						rejected.errors,
						[
							"Publisher xurl readiness report is not ready"
						],
					)

			oversized = copy.deepcopy(report)
			oversized["unexpected"] = "x" * (64 * 1024)
			write_probe(oversized)
			bounded = AutomationResult(
				automation_id="decodex-xurl-publisher"
			)
			validate_xurl_runtime(
				bounded,
				repo_only=False,
				publisher=publisher,
			)
			self.assertEqual(
				bounded.errors,
				["Publisher xurl readiness probe failed"],
			)

	def test_xurl_repo_only_evaluation_executes_no_probe(self) -> None:
		with tempfile.TemporaryDirectory() as directory:
			root = Path(directory)
			publisher = root / "decodex-publisher"
			marker = root / "executed"
			publisher.write_text(
				"#!/bin/sh\n"
				f"touch '{marker}'\n"
				"exit 70\n",
				encoding="utf-8",
			)
			publisher.chmod(0o700)
			result = AutomationResult(
				automation_id="decodex-xurl-publisher"
			)
			validate_xurl_runtime(
				result,
				repo_only=True,
				publisher=publisher,
			)
			self.assertEqual(result.status, "pass")
			self.assertFalse(marker.exists())

	def test_xurl_runtime_rejects_insecure_publisher_entrypoint(self) -> None:
		with tempfile.TemporaryDirectory() as directory:
			root = Path(directory)
			publisher = root / "decodex-publisher"
			publisher.write_text(
				"#!/bin/sh\nexit 0\n",
				encoding="utf-8",
			)
			publisher.chmod(0o720)
			insecure = AutomationResult(
				automation_id="decodex-xurl-publisher"
			)

			validate_xurl_runtime(
				insecure,
				repo_only=False,
				publisher=publisher,
			)

			self.assertEqual(
				insecure.errors,
				["Publisher probe executable is not trusted"],
			)

			target = root / "target"
			target.write_text("#!/bin/sh\n", encoding="utf-8")
			target.chmod(0o700)
			publisher.unlink()
			publisher.symlink_to(target)
			symlink = AutomationResult(
				automation_id="decodex-xurl-publisher"
			)

			validate_xurl_runtime(
				symlink,
				repo_only=False,
				publisher=publisher,
			)

			self.assertEqual(
				symlink.errors,
				["Publisher probe executable is not trusted"],
			)

	def test_python_and_prompts_never_invoke_xurl_directly(self) -> None:
		validator = (
			REPO_ROOT
			/ "automations/decodex/scripts/config/automation_eval/validators.py"
		).read_text(encoding="utf-8")
		self.assertIn('"social", "probe-xurl"', validator)
		self.assertNotIn('".local/bin/xurl"', validator)
		self.assertNotIn('"--app"', validator)
		self.assertNotIn('"--version"', validator)

		for prompt_path in (
			REPO_ROOT / "automations/upstream/prompts"
		).glob("*.md"):
			with self.subTest(prompt=prompt_path.name):
				prompt = prompt_path.read_text(encoding="utf-8")
				self.assertNotRegex(
					prompt,
					re.compile(r"`(?:[^` ]+/)?xurl\s+[^`]+`"),
				)
		for prompt_path in (
			REPO_ROOT / "automations/decodex/prompts"
		).glob("*.md"):
			with self.subTest(prompt=prompt_path.name):
				prompt = prompt_path.read_text(encoding="utf-8")
				self.assertNotRegex(
					prompt,
					re.compile(r"`(?:[^` ]+/)?xurl\s+[^`]+`"),
				)

	def test_managed_automation_start_minutes_do_not_collide(self) -> None:
		starts = {}
		rules = {}
		for manifest in (self.manifest, self.content_manifest):
			for automation in manifest["automations"]:
				parts = dict(
					part.split("=", 1)
					for part in automation["rrule"].split(";")
				)
				starts[automation["id"]] = int(parts["BYMINUTE"])
				rules[automation["id"]] = parts

		self.assertEqual(
			starts,
			{
				"codex-upstream-maintainer": 5,
				"codex-upstream-reviewer": 35,
				"codex-upstream-health": 0,
				"decodex-content-manager": 50,
				"decodex-xurl-publisher": 20,
			},
		)
		self.assertEqual(len(starts.values()), len(set(starts.values())))
		self.assertEqual(
			rules,
			{
				"codex-upstream-maintainer": {
					"FREQ": "HOURLY",
					"INTERVAL": "6",
					"BYMINUTE": "5",
					"BYSECOND": "0",
				},
				"codex-upstream-reviewer": {
					"FREQ": "HOURLY",
					"INTERVAL": "12",
					"BYMINUTE": "35",
					"BYSECOND": "0",
				},
				"codex-upstream-health": {
					"FREQ": "DAILY",
					"BYHOUR": "6,18",
					"BYMINUTE": "0",
					"BYSECOND": "0",
				},
				"decodex-content-manager": {
					"FREQ": "DAILY",
					"BYHOUR": "9",
					"BYMINUTE": "50",
					"BYSECOND": "0",
				},
				"decodex-xurl-publisher": {
					"FREQ": "DAILY",
					"BYHOUR": "10,16,22",
					"BYMINUTE": "20",
					"BYSECOND": "0",
				},
			},
		)
		daily_wakes = {
			automation_id: (
				24 // int(rule["INTERVAL"])
				if rule["FREQ"] == "HOURLY"
				else len(rule["BYHOUR"].split(","))
			)
			for automation_id, rule in rules.items()
		}
		self.assertEqual(
			daily_wakes,
			{
				"codex-upstream-maintainer": 4,
				"codex-upstream-reviewer": 2,
				"codex-upstream-health": 2,
				"decodex-content-manager": 1,
				"decodex-xurl-publisher": 3,
			},
		)
		self.assertEqual(sum(daily_wakes.values()), 12)
		self.assertEqual(sum(daily_wakes.values()) * 30, 360)
		self.assertEqual(sum(daily_wakes.values()) * 31, 372)
		upstream_operations = (
			REPO_ROOT
			/ "openwiki/operations/codex-upstream-autopilot.md"
		).read_text(encoding="utf-8")
		content_operations = (
			REPO_ROOT
			/ "openwiki/operations/decodex-content-automation.md"
		).read_text(encoding="utf-8")
		self.assertIn("12 `high` task wakes per day", upstream_operations)
		self.assertIn("360 in 30 days", upstream_operations)
		self.assertIn("372 in 31 days", upstream_operations)
		self.assertIn(
			"one publication, one due 24-hour outcome, or one due seven-day",
			content_operations,
		)
		self.assertIn("$1.20 in 30 days", content_operations)
		self.assertIn("$1.24 in 31", content_operations)
		self.assertIn("hard $1.25 calendar-month cap", content_operations)

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
				"test-headless-sandboxed",
			],
		)
		self.assertEqual(
			tasks["check-sandboxed"]["dependencies"][-1],
			"test-sandboxed",
		)
		for ordinary, sandboxed in (
			("test", "test-sandboxed"),
			("test-headless", "test-headless-sandboxed"),
		):
			self.assertEqual(
				tasks[sandboxed]["dependencies"],
				[
					dependency
					for dependency in tasks[ordinary]["dependencies"]
					if dependency != "test-vnext-postgres-store"
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

	def test_native_plan_leaves_codex_app_metadata_to_native_lifecycle(self) -> None:
		for manifest_path in (self.manifest_path, self.content_manifest_path):
			for spec in automation_specs(manifest_path):
				with self.subTest(automation=spec["id"]):
					item = render_plan([spec], Path("/portable/main"))[0]
					config = item["native_fields"]

					self.assertNotIn("created_at", config)
					self.assertNotIn("updated_at", config)
					self.assertEqual(config["reasoningEffort"], "high")
					self.assertNotEqual(config["reasoningEffort"], "xhigh")
					self.assertEqual(config["executionEnvironment"], "local")
					self.assertEqual(config["destination"], "local")
					self.assertEqual(config["cwds"], ["/portable/main"])

	def test_native_plan_cannot_write_scheduler_runtime(self) -> None:
		legacy_script = (
			REPO_ROOT
			/ "automations/decodex/scripts/config/sync_automations.py"
		)
		plan_script = (
			REPO_ROOT
			/ "automations/decodex/scripts/config/render_automation_plan.py"
		)
		self.assertFalse(legacy_script.exists())

		help_result = subprocess.run(
			[sys.executable, str(plan_script), "--help"],
			check=True,
			capture_output=True,
			text=True,
		)
		self.assertNotIn("--apply", help_result.stdout)
		self.assertNotIn("--codex-home", help_result.stdout)

		with tempfile.TemporaryDirectory() as directory:
			temp_root = Path(directory)
			codex_home = temp_root / "codex-home"
			repo_root = temp_root / "primary"
			subprocess.run(
				["git", "init", "--initial-branch=main", str(repo_root)],
				check=True,
				capture_output=True,
				text=True,
			)
			runtime = (
				codex_home
				/ "automations"
				/ "sentinel"
				/ "automation.toml"
			)
			runtime.parent.mkdir(parents=True)
			runtime.write_text(
				"created_at = 123\nupdated_at = 456\n",
				encoding="utf-8",
			)
			before = runtime.read_bytes()
			environment = os.environ.copy()
			environment["CODEX_HOME"] = str(codex_home)

			result = subprocess.run(
				[
					sys.executable,
					str(plan_script),
					"--repo-root",
					str(repo_root),
					"--json",
				],
				check=True,
				capture_output=True,
				text=True,
				env=environment,
			)

			payload = json.loads(result.stdout)
			self.assertEqual(payload["status"], "pass")
			self.assertEqual(payload["mode"], "native-lifecycle-plan")
			self.assertEqual(len(payload["definitions"]), 5)
			self.assertEqual(
				{
					tuple(item["native_fields"]["cwds"])
					for item in payload["definitions"]
				},
				{(str(repo_root.resolve()),)},
			)
			self.assertEqual(
				payload["retirements"],
				["decodex-x-browser-publisher"],
			)
			self.assertEqual(runtime.read_bytes(), before)
			self.assertEqual(
				[path.relative_to(codex_home) for path in codex_home.rglob("*")],
				[
					Path("automations"),
					Path("automations/sentinel"),
					Path("automations/sentinel/automation.toml"),
				],
			)

	def test_content_prompts_enforce_bounded_xurl_only_publication(self) -> None:
		manager = (
			REPO_ROOT / "automations/decodex/prompts/content-manager.md"
		).read_text(encoding="utf-8")
		publisher = (
			REPO_ROOT / "automations/decodex/prompts/xurl-publisher.md"
		).read_text(encoding="utf-8")
		health = (
			REPO_ROOT / "automations/upstream/prompts/health.md"
		).read_text(encoding="utf-8")
		quality = (
			REPO_ROOT
			/ "automations/decodex/skills/x-post-quality-system/SKILL.md"
		).read_text(encoding="utf-8")

		self.assertIn("`decodex-xurl-publisher` is the only X writer", manager)
		self.assertIn("Do not publish to X", manager)
		self.assertIn("Do not use `xurl`, X MCP, browser control", manager)
		self.assertIn("https://codexradar.com/", manager)
		self.assertIn("social_strategy/v1", manager)
		self.assertIn("Never create more than one social artifact", manager)
		self.assertIn("run exactly one `<radar> review-next", " ".join(manager.split()))
		self.assertIn("Any unconsumed candidate is backpressure", manager)
		self.assertIn("`no_eligible_item` is a proven no-op", manager)
		self.assertIn("contain no URL", manager)
		self.assertIn("one concrete change and why it matters", manager)
		self.assertIn("ordinary web research", manager)
		self.assertIn("with no path arguments", manager)
		self.assertIn("Never commit or upload them", manager)

		self.assertIn("only process that may invoke `xurl`", publisher)
		self.assertIn("Do not call `xurl`, X MCP, browser control", publisher)
		self.assertIn("one post per day", publisher)
		self.assertIn("no URL in public text", publisher)
		self.assertIn("1,250,000 micro-USD ($1.25)", publisher)
		self.assertIn("oauth2: decodexspace", publisher)
		self.assertIn("social publish-xurl", publisher)
		self.assertIn("social observe-xurl", publisher)
		self.assertIn("social terminalize-skip", publisher)
		self.assertIn("23 to 48 hours", publisher)
		self.assertIn("167 to 192 hours", publisher)
		self.assertIn(
			"When an outcome is processed, do not process a candidate", publisher
		)
		self.assertIn("Only when no outcome was processed", publisher)
		self.assertIn("Never retry an uncertain create", publisher)
		self.assertIn("canonical", publisher)
		self.assertIn("30,000 micro-USD", publisher)
		self.assertIn("($0.030)", publisher)
		self.assertIn("5,000 micro-USD ($0.005)", publisher)
		self.assertIn("with no path arguments", publisher)
		for removed_route in (
			"acquire-browser-lease",
			"verify-browser-lease",
			"release-browser-lease",
			"browser_touched",
			'publisher = "chrome"',
			"agent.browsers",
			"chrome.tabs",
			"browser-runtime-trust.json",
		):
			self.assertNotIn(removed_route, publisher)

		for name, prompt in (
			("manager", manager),
			("publisher", publisher),
			("health", health),
		):
			with self.subTest(publisher_bootstrap=name):
				normalized = " ".join(prompt.split())
				if name == "manager":
					self.assertIn(
						"cargo build --locked -p radar -p decodex-publisher",
						normalized,
					)
				else:
					self.assertIn(
						"cargo build --locked -p decodex-publisher",
						normalized,
					)
				self.assertIn("$PWD/target/debug/decodex-publisher", normalized)
				if name == "manager":
					self.assertIn("as `<radar>` and `<publisher>`", normalized)
				else:
					self.assertIn("as `<publisher>`", normalized)
				self.assertNotIn("`decodex-publisher validate-social", prompt)

		manager_normalized = " ".join(manager.split())
		self.assertIn("$PWD/target/debug/radar", manager_normalized)
		self.assertIn("as `<radar>` and `<publisher>`", manager_normalized)
		self.assertIn("<radar> refresh-upstream-queue", manager)
		self.assertIn("<radar> refresh-release-delta", manager)
		self.assertIn("<radar> validate", manager)
		self.assertIn("<radar> review-next --cache-root", manager_normalized)
		self.assertIn("needs_source_review", manager)
		self.assertIn("handled-state digest", manager)
		self.assertIn("<radar> bundle build --repo openai/codex", manager_normalized)
		self.assertIn("codex-code-analysis/SKILL.md", manager)
		self.assertIn("Titles, filenames, surface hints", manager)
		self.assertIn("concrete implementation, test, documentation, or schema anchor", manager)
		self.assertIn("radar_content_review_pair_staging/v1", manager)
		self.assertIn("<radar> content-pair-commit --cache-root", manager_normalized)
		self.assertIn("radar_content_review_pair_commit/v1", manager)
		self.assertIn("atomically commits the pair", manager_normalized)
		self.assertIn("exactly 64 zeroes", manager)
		self.assertNotIn("cache/github/reviews/", manager)
		self.assertNotIn("cache/github/impact/", manager)
		self.assertIn('public_signal_decision = "publish"', manager)
		self.assertIn('publisher_angle = "none"', manager)
		self.assertIn("bounded result in memory", manager)
		self.assertIn("daily review is not a social artifact", manager)
		self.assertIn("only for the weekly checkpoint", manager)
		self.assertIn("evidence-backed strategy change", manager)
		self.assertIn("$CODEX_THREAD_ID.json", manager)
		self.assertIn("content-eligibility --queue <exact-queue>", manager_normalized)
		self.assertIn("exactly once", manager)
		self.assertIn("social record-manager --staging", manager_normalized)
		self.assertIn("Never write directly to the candidate or strategy", manager_normalized)
		self.assertIn("canonical ordered claim composition", quality)
		self.assertIn("at most 12 public URLs", manager_normalized)
		self.assertIn("must not incur X API cost", manager)
		self.assertIn("API calls (`0`)", manager)
		self.assertIn("X spend (`$0.000`)", manager)

		self.assertIn("Do not invoke `xurl` directly", health)
		self.assertIn("nonbillable fixed-entrypoint", health)
		self.assertIn("must never run paid `whoami`", " ".join(health.split()))
		self.assertIn("<publisher> social cost-report", health)
		self.assertIn("Publisher is the sole v4 ledger parser", health)
		self.assertIn("Never parse the ledger outside Publisher", publisher)
		self.assertIn("recorded X cost ceilings", health)
		self.assertIn("Never describe a recorded ceiling", health)
		self.assertIn("decodex-x-browser-publisher", health)
		self.assertIn("retirements", health)
		self.assertIn("delete", health)
		self.assertIn("local destination and execution", health)
		self.assertIn("The xurl route is text-only", quality)
		self.assertNotIn("## Media Gate", quality)
		self.assertNotIn("Before upload", quality)

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
		for radar_path in (
			"apps/radar/Cargo.toml",
			"apps/radar/README.md",
			"apps/radar/src/content_pair.rs",
			"apps/radar/src/lib.rs",
			"apps/radar/src/main.rs",
			"automations/radar/scripts/github/content_review_pair_commit_report.schema.json",
			"automations/radar/scripts/github/content_review_pair_staging.schema.json",
		):
			self.assertIn(radar_path, by_id["decodex-content-manager"])
		self.assertIn(
			"automations/decodex/skills/x-post-publisher/SKILL.md",
			by_id["decodex-content-manager"],
		)
		for required_xurl_source in (
			"apps/decodex-publisher/src/social_xurl.rs",
		):
			self.assertIn(
				required_xurl_source,
				by_id["decodex-xurl-publisher"],
			)
		for paths in by_id.values():
			self.assertNotIn(
				"automations/decodex/browser-runtime-trust.json",
				paths,
			)
		self.assertTrue(
			required
			- {"automations/decodex/scripts/social/social_strategy.schema.json"}
			<= by_id["decodex-xurl-publisher"]
		)
		health = next(
			automation
			for automation in self.manifest["automations"]
			if automation["id"] == "codex-upstream-health"
		)
		self.assertTrue(required <= set(health["required_paths"]))
		publisher_sources = {
			"apps/decodex-publisher/src/cli/social.rs",
			"apps/decodex-publisher/src/cli/validation.rs",
			"apps/decodex-publisher/src/filesystem.rs",
			"apps/decodex-publisher/src/lib.rs",
			"apps/decodex-publisher/src/social_clock.rs",
			"apps/decodex-publisher/src/social_contracts.rs",
			"apps/decodex-publisher/src/social_evidence.rs",
			"apps/decodex-publisher/src/social_gc.rs",
			"apps/decodex-publisher/src/social_gc/inventory.rs",
			"apps/decodex-publisher/src/social_gc/plan.rs",
			"apps/decodex-publisher/src/social_gc/tests.rs",
			"apps/decodex-publisher/src/social_publish.rs",
			"apps/decodex-publisher/src/social_publish/scan.rs",
			"apps/decodex-publisher/src/social_record.rs",
			"apps/decodex-publisher/src/social_skip.rs",
			"apps/decodex-publisher/src/social_validation.rs",
			"apps/decodex-publisher/src/social_validation/candidate.rs",
			"apps/decodex-publisher/src/social_validation/cross_file.rs",
			"apps/decodex-publisher/src/social_xurl.rs",
			"apps/decodex-publisher/src/social_xurl/auth_contract.rs",
			"apps/decodex-publisher/src/social_xurl/ledger.rs",
			"apps/decodex-publisher/src/social_xurl/model.rs",
			"apps/decodex-publisher/src/social_xurl/observe.rs",
			"apps/decodex-publisher/src/social_xurl/pricing.rs",
			"apps/decodex-publisher/src/social_xurl/publish.rs",
			"apps/decodex-publisher/src/social_xurl/reconcile.rs",
			"apps/decodex-publisher/src/social_xurl/runtime.rs",
			"apps/decodex-publisher/src/tests.rs",
		}
		for automation_id in (
			"decodex-content-manager",
			"decodex-xurl-publisher",
		):
			self.assertTrue(publisher_sources <= by_id[automation_id])
			self.assertTrue(required <= by_id[automation_id])
		self.assertTrue(publisher_sources <= set(health["required_paths"]))
		self.assertIn(
			"apps/decodex-publisher/src/social_xurl.rs",
			by_id["decodex-xurl-publisher"],
		)
		self.assertIn(
			"apps/decodex-publisher/src/social_xurl.rs",
			health["required_paths"],
		)

	def test_upstream_manifest_tracks_the_complete_pricing_contract(self) -> None:
		pricing_paths = {
			"apps/decodex-publisher/README.md",
			"apps/decodex-publisher/src/social_xurl/pricing.rs",
			"apps/decodex-publisher/src/social_xurl/pricing/tests.rs",
			"automations/upstream/scripts/upstream_autopilot_lib/pricing.py",
			"automations/upstream/tests/fixtures/x-pricing-current.md",
			"automations/upstream/tests/test_upstream_autopilot.py",
			"openwiki/operations/codex-upstream-autopilot.md",
		}
		for automation in self.manifest["automations"]:
			with self.subTest(automation=automation["id"]):
				self.assertTrue(
					pricing_paths <= set(automation["required_paths"])
				)

	def test_all_managed_runs_use_manager_owned_task_retention(self) -> None:
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
		normalized = " ".join(retention.split())

		self.assertIn("set_thread_archived", retention)
		self.assertIn("Health Manager is the only cross-task archive owner", normalized)
		self.assertIn("keep_visible", retention)
		self.assertIn("task-retention-seal", retention)
		self.assertIn("atomically creates one mode-`0600` receipt", normalized)
		self.assertIn("app-provided `CODEX_THREAD_ID`", normalized)
		self.assertIn(
			"No evidence path, absolute path, evidence content, task text, prompt",
			normalized,
		)
		self.assertIn("decodex/codex-task-retention-receipt/2", retention)
		self.assertIn("at most 50 `pending_tasks` records", normalized)
		self.assertIn("There is no `pending_thread_ids` compatibility field", normalized)
		self.assertIn("task-retention-plan", retention)
		self.assertIn(
			"task-retention-settle --thread-id <id> --result archived",
			normalized,
		)
		self.assertIn("--result keep-visible", normalized)
		self.assertIn("Native exact readback is required", normalized)
		self.assertIn("No `list_threads` query is required", normalized)
		for removed_command in (
			"task-retention-prepare",
			"task-retention-probe",
			"task-retention-discover",
			"task-retention-attest",
		):
			self.assertNotIn(removed_command, retention)

		for manifest in (self.manifest, self.content_manifest):
			for automation in manifest["automations"]:
				with self.subTest(automation=automation["id"]):
					self.assertIn(retention_ref, automation["required_paths"])
					prompt = (
						REPO_ROOT / automation["prompt_file"]
					).read_text(encoding="utf-8")
					normalized_prompt = " ".join(prompt.split())
					self.assertIn("scheduled-run-thread-retention.md", prompt)
					self.assertIn("Task retention: manager_archive", prompt)
					self.assertIn("Task retention: keep_visible", prompt)
					self.assertIn("task-retention-seal", prompt)
					self.assertIn("--automation-id " + automation["id"], prompt)
					self.assertIn("--terminal-result-code", prompt)
					self.assertIn("task_retention_sealed", prompt)
					self.assertIn("Do not archive the active", normalized_prompt)
					self.assertIn("visible", prompt)
					self.assertNotIn("Archive the task.", prompt)
					self.assertNotIn("THREAD_RETENTION", prompt)
					if automation["id"] != "codex-upstream-health":
						self.assertNotIn("set_thread_archived", prompt)

		health_prompt = (
			REPO_ROOT / "automations/upstream/prompts/health.md"
		).read_text(encoding="utf-8")
		normalized_health = " ".join(health_prompt.split())
		self.assertIn("`set_thread_archived`", health_prompt)
		self.assertNotIn("`list_threads`", health_prompt)
		self.assertIn("`read_thread`", health_prompt)
		self.assertIn("task-retention-plan --json", normalized_health)
		self.assertIn(
			"returns at most 50 bound `pending_tasks` records",
			normalized_health,
		)
		self.assertIn("retention_projection_mismatch", health_prompt)
		self.assertIn("app-provided `CODEX_THREAD_ID`", normalized_health)
		self.assertIn("task_retention_contract_drift", health_prompt)
		self.assertIn("--result archived", health_prompt)
		self.assertIn("--result keep-visible", health_prompt)
		self.assertIn("`archived = false`", health_prompt)
		self.assertIn("Never archive the active Health task", normalized_health)
		self.assertIn("failed, cancelled", normalized_health)
		self.assertIn("needs-attention", normalized_health)
		self.assertIn("Archiving cleans the Codex task list only", normalized_health)
		self.assertIn("must not disable recurring definitions", normalized_health)
		self.assertIn(
			"Never export task content, rollout data, or Codex database rows",
			normalized,
		)
		self.assertIn("changes only the native archived flag", normalized)
		self.assertNotIn("`hostId = \"local\"`", health_prompt)
		self.assertLess(
			health_prompt.index("task-retention-plan --json"),
			health_prompt.index("health --repair-expired"),
		)
		self.assertLess(
			health_prompt.index("health --repair-expired"),
			health_prompt.index("audit-automations --manifest upstream"),
		)
		self.assertLess(
			health_prompt.index("audit-automations --manifest upstream"),
			health_prompt.index("observe --json"),
		)

		for readme_path in (
			REPO_ROOT / "automations/upstream/README.md",
			REPO_ROOT / "automations/decodex/README.md",
		):
			readme = readme_path.read_text(encoding="utf-8")
			normalized_readme = " ".join(readme.split())
			self.assertIn("Health", normalized_readme)
			self.assertIn("task-retention-seal", normalized_readme)
			self.assertIn("at most 50", normalized_readme)
			self.assertIn("evidence", normalized_readme)
			self.assertNotIn("task-retention-discover", normalized_readme)
			self.assertNotIn("task-retention-attest", normalized_readme)
			self.assertNotIn("THREAD_RETENTION", normalized_readme)

		for openwiki_path in (
			REPO_ROOT / "openwiki/operations/codex-upstream-autopilot.md",
			REPO_ROOT / "openwiki/operations/decodex-content-automation.md",
		):
			openwiki = openwiki_path.read_text(encoding="utf-8")
			normalized_openwiki = " ".join(openwiki.split())
			self.assertIn("task-retention-seal", openwiki)
			self.assertIn("at most 50", normalized_openwiki)
			self.assertIn("evidence", normalized_openwiki)
			self.assertNotIn("task-retention-discover", openwiki)
			self.assertNotIn("task-retention-attest", openwiki)
			self.assertNotIn(
				"calls native\n`set_thread_archived` with `archived = true`",
				openwiki,
			)

		for manifest in (self.manifest, self.content_manifest):
			for automation in manifest["automations"]:
				self.assertIn(
					"automations/upstream/scripts/upstream_autopilot_lib/retention.py",
					automation["required_paths"],
				)

	def test_health_prompt_owns_bounded_native_reconciliation(self) -> None:
		prompt = (
			REPO_ROOT / "automations/upstream/prompts/health.md"
		).read_text(encoding="utf-8")
		self.assertIn("`automation_update`", prompt)
		self.assertIn("Never write `$CODEX_HOME`", prompt)
		self.assertIn("render_automation_plan.py --json", prompt)
		self.assertIn("The renderer is read-only", prompt)
		self.assertIn("Codex App\n   alone owns app metadata", prompt)
		for automation_id in (
			"codex-upstream-maintainer",
			"codex-upstream-reviewer",
			"codex-upstream-health",
			"decodex-content-manager",
			"decodex-xurl-publisher",
		):
			self.assertIn(automation_id, prompt)
		self.assertIn("Read back every created or updated definition", prompt)
		self.assertIn("five exact automation definitions", prompt)
		self.assertIn("Do not list, mutate,", prompt)
		self.assertIn("unrelated scheduler definitions", prompt)
		self.assertIn("all five managed", prompt)
		self.assertIn("content_loop_degraded", prompt)
		self.assertIn("weekly_benchmark_missing", prompt)
		self.assertIn("audit-automations --manifest upstream --scope repo", prompt)
		self.assertIn("--manifest content", prompt)
		self.assertIn("--scope live", prompt)
		self.assertLess(
			prompt.index("task-retention-plan --json"),
			prompt.index("health --repair-expired"),
		)
		self.assertLess(
			prompt.index("health --repair-expired"),
			prompt.index("audit-automations --manifest upstream"),
		)
		self.assertLess(
			prompt.index("audit-automations --manifest upstream"),
			prompt.index("observe --json"),
		)

		health = next(
			automation
			for automation in self.manifest["automations"]
			if automation["id"] == "codex-upstream-health"
		)
		for required_path in (
			"automations/decodex/scripts/config/automation_checkout.py",
			"automations/decodex/scripts/config/automation_plan/__init__.py",
			"automations/decodex/scripts/config/automation_plan/cli.py",
			"automations/decodex/scripts/config/automation_plan/manifest.py",
			"automations/decodex/scripts/config/automation_plan/paths.py",
			"automations/decodex/scripts/config/render_automation_plan.py",
		):
			self.assertIn(required_path, health["required_paths"])

	def test_health_prompt_uses_canonical_task_retention_settle_order(self) -> None:
		prompt = (
			REPO_ROOT / "automations/upstream/prompts/health.md"
		).read_text(encoding="utf-8")
		normalized = " ".join(prompt.split())
		launcher = "automations/upstream/scripts/run_upstream_autopilot"

		self.assertIn(
			f'{launcher} task-retention-settle '
			"--thread-id <id> --result archived --json",
			normalized,
		)
		self.assertIn(
			f'{launcher} task-retention-settle '
			"--thread-id <id> --result keep-visible "
			"--reason <bounded-reason-code> --json",
			normalized,
		)
		self.assertNotIn("--manager-thread-id", normalized)
		self.assertNotIn("task-retention-settle --result", normalized)

	def test_health_prompt_validates_all_origin_urls_before_fetch_and_build(
		self,
	) -> None:
		prompt = (
			REPO_ROOT / "automations/upstream/prompts/health.md"
		).read_text(encoding="utf-8")
		normalized = " ".join(prompt.split())
		fetch_url = normalized.index("git remote get-url origin")
		push_urls = normalized.index("git remote get-url --push --all origin")
		fetch = normalized.index("git fetch --quiet origin main")
		build = normalized.index("cargo build --locked -p decodex-publisher")

		self.assertLess(fetch_url, fetch)
		self.assertLess(push_urls, fetch)
		self.assertLess(fetch_url, build)
		self.assertLess(push_urls, build)
		self.assertIn("fetch URL and every non-empty push URL", normalized)
		self.assertIn("require at least one push URL", normalized)
		self.assertIn("Fail closed before fetch or build", normalized)

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
		self.assertIn("native worker subagent", prompt)
		self.assertIn("parent automation must not edit or stage", prompt)
		self.assertIn("Reuse the same\n   worker for no correction", prompt)
		self.assertIn("Do not correct it in this parent task", prompt)

	def test_pricing_prompts_bind_failure_evidence_and_official_tables(self) -> None:
		prompts = {
			name: (
				REPO_ROOT / f"automations/upstream/prompts/{name}.md"
			).read_text(encoding="utf-8")
			for name in ("maintainer", "reviewer", "health")
		}
		for name in ("maintainer", "reviewer"):
			with self.subTest(prompt=name):
				self.assertIn(
					"decodex/x-pricing-audit-failure/2",
					prompts[name],
				)
				self.assertIn(
					"decodex/x-pricing-parser-diagnostic/1",
					prompts[name],
				)
				self.assertIn("receipt_sha256", prompts[name])
				self.assertIn("at most 16 KiB", prompts[name])
		health = " ".join(prompts["health"].split())
		for fragment in (
			"monotonic 10-second total deadline",
			"Resource | Unit cost",
			"Action | Unit cost",
			"Post: Create (with URL)",
			"per-1,000 tables must fail parsing",
			"dynamic 36 hours",
			"first `parse_failed` result",
			"latest failure marker blocks the Publisher immediately",
		):
			self.assertIn(fragment, health)

	def test_upstream_prompts_pin_trusted_python_launcher_and_native_subagents(self) -> None:
		prompts = {
			name: (
				REPO_ROOT / f"automations/upstream/prompts/{name}.md"
			).read_text(encoding="utf-8")
			for name in ("maintainer", "reviewer", "health")
		}
		for name, prompt in prompts.items():
			with self.subTest(prompt=name):
				normalized = " ".join(prompt.split())
				self.assertIn("`automations/upstream/scripts/run_upstream_autopilot`", prompt)
				self.assertIn("root-owned, read-only", prompt)
				self.assertIn("Python 3.11 or later", prompt)
				self.assertIn("state tool", normalized)
				self.assertIn("bare `python3`", normalized)
				self.assertNotIn(
					"`python3 automations/upstream/scripts/upstream_autopilot.py",
					prompt,
				)
		self.assertIn("Spawn exactly one native worker subagent", prompts["maintainer"])
		self.assertIn("must not edit or stage tracked candidate", prompts["maintainer"])
		self.assertIn("Spawn exactly one native read-only review subagent", prompts["reviewer"])
		self.assertIn("must not\n   edit or stage files", prompts["reviewer"])
		for name in ("maintainer", "reviewer"):
			with self.subTest(parent=name):
				normalized = " ".join(prompts[name].split())
				self.assertIn(
					"at most one read-only `tool_search` for the exact native "
					"multi-agent tools, one `spawn_agent`, and one `wait_agent`",
					normalized,
				)
				self.assertIn(
					"use `tool_search` exactly once to discover those exact tools",
					normalized,
				)
				self.assertIn("must not call `apply_patch`", normalized)
				self.assertIn("`write_stdin`, `send_input`", normalized)
				self.assertIn("shell redirection, substitution, pipelines", normalized)
				self.assertIn(
					"state-tool and managed-worktree lifecycle are the only parent mutations",
					normalized,
				)
				self.assertNotIn(
					"Stop successfully for `no_candidate`",
					prompts[name],
				)
				self.assertIn(
					"do not return from the task",
					normalized,
				)
				self.assertIn(
					"are terminal results and are not exceptions to this seal requirement",
					normalized,
				)

	def test_upstream_launcher_selects_a_trusted_modern_python(self) -> None:
		launcher = REPO_ROOT / "automations/upstream/scripts/run_upstream_autopilot"
		self.assertTrue(launcher.is_file())
		self.assertTrue(launcher.stat().st_mode & 0o111)
		content = launcher.read_text(encoding="utf-8")
		self.assertIn("/nix/store/*-python3-3.<->*/bin/python3", content)
		self.assertIn("zmodload -F zsh/stat b:zstat", content)
		self.assertIn("zstat -H metadata -L", content)
		self.assertIn("metadata[uid] == 0", content)
		self.assertIn("metadata[mode] & 8#022", content)
		self.assertLess(
			content.index("trusted_python \"${candidate}\""),
			content.index("\"${candidate}\" -I -S -c"),
		)
		self.assertIn(
			'exec "${candidate}" -I -S "${autopilot}" "$@"',
			content,
		)
		self.assertNotIn("<<'PY'", content)
		self.assertNotIn("codex-primary-runtime", content)
		result = subprocess.run(
			[str(launcher), "--help"],
			cwd=REPO_ROOT,
			check=False,
			capture_output=True,
			text=True,
		)
		self.assertEqual(result.returncode, 0, result.stderr)
		self.assertIn("upstream_autopilot.py", result.stdout)

	def test_upstream_launcher_ignores_python_startup_injection(self) -> None:
		launcher = REPO_ROOT / "automations/upstream/scripts/run_upstream_autopilot"
		with tempfile.TemporaryDirectory() as temporary_directory:
			startup = Path(temporary_directory) / "startup"
			startup.mkdir()
			marker = Path(temporary_directory) / "sitecustomize-executed"
			(startup / "sitecustomize.py").write_text(
				"import os\n"
				"from pathlib import Path\n"
				"Path(os.environ['DECODEX_UPSTREAM_TEST_MARKER']).write_text("
				"'executed', encoding='utf-8')\n",
				encoding="utf-8",
			)
			environment = os.environ.copy()
			environment.update(
				{
					"DECODEX_UPSTREAM_TEST_MARKER": str(marker),
					"PYTHONHOME": str(startup),
					"PYTHONPATH": str(startup),
					"PYTHONUSERBASE": str(startup),
				}
			)
			result = subprocess.run(
				[str(launcher), "--help"],
				cwd=REPO_ROOT,
				check=False,
				capture_output=True,
				text=True,
				env=environment,
			)

			self.assertEqual(result.returncode, 0, result.stderr)
			self.assertIn("upstream_autopilot.py", result.stdout)
			self.assertFalse(marker.exists())

	def test_upstream_subagent_handoffs_are_state_bound(self) -> None:
		maintainer = (
			REPO_ROOT / "automations/upstream/prompts/maintainer.md"
		).read_text(encoding="utf-8")
		reviewer = (
			REPO_ROOT / "automations/upstream/prompts/reviewer.md"
		).read_text(encoding="utf-8")
		for prompt in (maintainer, reviewer):
			normalized = " ".join(prompt.split())
			self.assertIn("`handoff_challenge`", normalized)
			self.assertIn("`handoff_receipt_path`", normalized)
			self.assertIn("Never pass the lease token", normalized)
			self.assertIn("mode `0600` JSON receipt", normalized)
			self.assertIn("decodex/codex-upstream-agent-context/1", normalized)
			self.assertIn(
				"decodex/codex-upstream-agent-handoff-projection/1",
				normalized,
			)
			for field in (
				'"candidate_id"',
				'"role"',
				'"claim_generation"',
				'"worktree"',
				'"base_head"',
				'"worktree_sha256"',
				'"repository_head"',
				'"repository_tree"',
				'"staged_paths_sha256"',
				'"receipt_sha256"',
			):
				self.assertIn(field, prompt)
			self.assertIn("exactly one compact JSON agent-context", normalized)
			self.assertIn("exactly one compact JSON handoff projection", normalized)
			self.assertIn("wait_agent` exactly once", normalized)
			self.assertIn("Do not call `send_input`", normalized)
			self.assertIn(
				"non-replayable state-bound handoff receipt",
				normalized,
			)
			self.assertIn("not a cryptographic identity signature", normalized)
		self.assertIn("--worker-receipt <exact-receipt-path>", maintainer)
		self.assertIn("--reviewer-receipt <exact-receipt-path>", reviewer)
		self.assertLess(
			reviewer.index("Spawn exactly one native read-only review subagent"),
			reviewer.index("For a pending decision, only after"),
		)

	def test_health_executes_gc_recovery_and_publisher_probe_contracts(self) -> None:
		health = (
			REPO_ROOT / "automations/upstream/prompts/health.md"
		).read_text(encoding="utf-8")
		normalized = " ".join(health.split())
		gc = health.index("<publisher> social gc")
		first_validation = health.index("<publisher> validate-social")

		self.assertLess(gc, first_validation)
		self.assertIn("first mandatory phase recovers", normalized)
		self.assertIn("durable deletion journal", normalized)
		self.assertIn("<publisher> social probe-xurl", health)
		self.assertIn("Consume only its bounded JSON report", normalized)
		self.assertIn("Python evaluator must never parse raw xurl output", normalized)
		self.assertIn("decodex/automation-memory/1", health)
		self.assertIn("Do not add another field", normalized)
		self.assertIn("relative or absolute local paths", normalized)

	def test_upstream_launcher_never_executes_an_untrusted_candidate(self) -> None:
		launcher = REPO_ROOT / "automations/upstream/scripts/run_upstream_autopilot"
		with tempfile.TemporaryDirectory() as temporary_directory:
			bin_directory = Path(temporary_directory) / "bin"
			bin_directory.mkdir()
			candidate = bin_directory / "python3"
			marker = Path(temporary_directory) / "executed"
			candidate.write_text(
				"#!/bin/sh\n: > \"$DECODEX_UPSTREAM_TEST_MARKER\"\nexit 0\n",
				encoding="utf-8",
			)
			candidate.chmod(0o755)
			environment = os.environ.copy()
			environment.update(
				{
					"DECODEX_UPSTREAM_PYTHON_CANDIDATE": str(candidate),
					"DECODEX_UPSTREAM_TEST_MARKER": str(marker),
				}
			)
			result = subprocess.run(
				[str(launcher), "--help"],
				cwd=REPO_ROOT,
				check=False,
				capture_output=True,
				text=True,
				env=environment,
			)

			self.assertEqual(result.returncode, 78, result.stderr)
			self.assertIn("trusted Python 3.11+ runtime unavailable", result.stderr)
			self.assertFalse(marker.exists())

	def test_only_successful_runs_become_archive_eligible_after_readback(self) -> None:
		retention = (
			REPO_ROOT
			/ "automations/decodex/skills/references/"
			"scheduled-run-thread-retention.md"
		).read_text(encoding="utf-8")
		normalized = " ".join(retention.split())
		self.assertIn("caller cannot select", normalized)
		self.assertIn(
			"failed, blocked, cancelled, needs-attention, user-continued",
			normalized,
		)
		self.assertIn("unknown external effect always stays visible", normalized)
		self.assertIn(
			"completed and independently read-back result uses status "
			"`pending_archive`",
			normalized,
		)
		self.assertIn(
			"Health Manager is the only cross-task archive owner",
			normalized,
		)
		self.assertIn("Native exact readback is required", normalized)
		self.assertIn("Pending receipts", normalized)
		self.assertIn("never removed by age", normalized)
		self.assertIn("does not inspect Codex SQLite", normalized)
		self.assertIn("needs-attention", retention)
		self.assertNotIn("task_retention_window_saturated", retention)
		self.assertNotIn(
			"task-retention-seal --automation-id <exact-id> --outcome",
			normalized,
		)

	def test_validation_diagnostics_remain_bounded_in_prompts(self) -> None:
		maintainer = (
			REPO_ROOT / "automations/upstream/prompts/maintainer.md"
		).read_text(encoding="utf-8")
		health = (
			REPO_ROOT / "automations/upstream/prompts/health.md"
		).read_text(encoding="utf-8")
		normalized_maintainer = " ".join(maintainer.split())
		normalized_health = " ".join(health.split())
		self.assertIn(
			"validation-diagnostic --error-digest <exact-digest> --json",
			normalized_maintainer,
		)
		self.assertIn("exact `error_digest` returned by the wrapper", normalized_maintainer)
		self.assertIn("Pass the worker only that returned bounded structure", normalized_maintainer)
		self.assertIn("artifact digests", normalized_maintainer)
		self.assertIn("Never pass raw output", normalized_maintainer)
		self.assertIn(
			"validation-diagnostic --error-digest <exact-digest> --json",
			normalized_health,
		)
		self.assertIn("Report only the digest, failure code", normalized_health)
		self.assertIn("Never read or report raw validation output", normalized_health)
		self.assertNotIn("diagnostics/<digest>.json", maintainer)

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
