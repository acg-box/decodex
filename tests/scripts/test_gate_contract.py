"""Regression tests for the repository's canonical validation task graph."""

from pathlib import Path
import json
import tomllib
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
FORMATTER_TOOLCHAIN = "nightly-2026-07-16"


class GateContractTests(unittest.TestCase):
    """Keep canonical checks explicit, deterministic, and non-overlapping."""

    @classmethod
    def setUpClass(cls) -> None:
        with (REPO_ROOT / "Makefile.toml").open("rb") as makefile:
            cls.tasks = tomllib.load(makefile)["tasks"]

    def test_rust_format_tasks_use_the_same_pinned_nightly(self) -> None:
        expected_args = {
            "fmt-rust": ["run", FORMATTER_TOOLCHAIN, "cargo", "fmt", "--all"],
            "fmt-rust-check": [
                "run",
                FORMATTER_TOOLCHAIN,
                "cargo",
                "fmt",
                "--all",
                "--",
                "--check",
            ],
        }
        self.assertEqual(self.tasks["fmt-rust"]["command"], "rustup")
        self.assertEqual(self.tasks["fmt-rust-check"]["extend"], "fmt-rust")
        for task_name, args in expected_args.items():
            with self.subTest(task=task_name):
                self.assertEqual(self.tasks[task_name]["args"], args)

    def test_workspace_compiler_remains_separately_pinned(self) -> None:
        with (REPO_ROOT / "rust-toolchain.toml").open("rb") as toolchain_file:
            toolchain = tomllib.load(toolchain_file)["toolchain"]

        self.assertEqual(toolchain["channel"], "1.97.0")
        self.assertIn("rustfmt", toolchain["components"])
        self.assertNotEqual(toolchain["channel"], FORMATTER_TOOLCHAIN)

    def test_blocking_aggregates_retain_every_non_vstyle_gate(self) -> None:
        self.assertEqual(
            self.tasks["check"]["dependencies"],
            [
                "audit-node",
                "build",
                "check-node",
                "check-rust",
                "fmt-check",
                "lint",
                "test",
            ],
        )
        self.assertEqual(self.tasks["lint"]["dependencies"], ["lint-rust"])
        self.assertEqual(self.tasks["lint-fix"]["dependencies"], ["lint-rust-fix"])
        self.assertEqual(
            self.tasks["test"]["dependencies"],
            [
                "test-automations",
                "test-gate-contract",
                "test-rust",
                "test-vnext-architecture",
                "test-vnext-cli-diagnostics",
                "test-vnext-postgres-store",
            ],
        )
        self.assertEqual(
            self.tasks["test-headless"]["dependencies"],
            [
                "test-automations",
                "test-gate-contract",
                "test-rust-headless",
                "test-vnext-architecture",
                "test-vnext-cli-diagnostics",
                "test-vnext-postgres-store",
            ],
        )

    def test_sandbox_test_aggregates_omit_only_the_live_postgres_gate(self) -> None:
        omitted = {"test-vnext-postgres-store"}
        pairs = (
            ("test", "test-sandboxed"),
            ("test-headless", "test-headless-sandboxed"),
        )
        for ordinary, sandboxed in pairs:
            with self.subTest(ordinary=ordinary, sandboxed=sandboxed):
                ordinary_dependencies = self.tasks[ordinary]["dependencies"]
                sandboxed_dependencies = self.tasks[sandboxed]["dependencies"]
                self.assertEqual(
                    set(ordinary_dependencies) - set(sandboxed_dependencies),
                    omitted,
                )
                self.assertEqual(
                    set(sandboxed_dependencies) - set(ordinary_dependencies),
                    set(),
                )
                self.assertEqual(
                    sandboxed_dependencies,
                    [
                        dependency
                        for dependency in ordinary_dependencies
                        if dependency not in omitted
                    ],
                )

        self.assertEqual(
            self.tasks["check-upstream-automation-sandboxed"]["dependencies"][-1],
            "test-headless-sandboxed",
        )
        self.assertEqual(
            self.tasks["check-sandboxed"]["dependencies"][-1],
            "test-sandboxed",
        )

    def test_vstyle_is_a_read_only_explicit_audit(self) -> None:
        self.assertEqual(
            self.tasks["audit-vstyle-rust"],
            {
                "workspace": False,
                "command": "python3",
                "args": ["scripts/vstyle_audit.py"],
            },
        )
        serialized_tasks = json.dumps(self.tasks, sort_keys=True)
        self.assertNotIn("tune", serialized_tasks)
        self.assertNotIn("lint-vstyle-fix", self.tasks)
        self.assertNotIn("lint-vstyle-rust", self.tasks)

    def test_checked_in_vstyle_contract_pins_identity_and_baseline(self) -> None:
        with (REPO_ROOT / "config" / "vstyle-rust-audit.json").open(encoding="utf-8") as file:
            contract = json.load(file)

        self.assertEqual(contract["schema"], "decodex/vstyle-rust-audit/1")
        self.assertEqual(
            contract["tool"]["commit"],
            "3a0959eac5363c4c427382bae1d80d87ecadb702",
        )
        self.assertEqual(len(contract["rust_rules"]), 37)
        self.assertEqual(contract["accepted_baseline"], {
            "checked_files": 227,
            "findings": 184,
            "manual": 7,
        })
        self.assertEqual(sum(item["count"] for item in contract["baseline"]), 184)


if __name__ == "__main__":
    unittest.main()
