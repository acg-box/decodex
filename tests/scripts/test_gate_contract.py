"""Regression tests for the repository validation task graph."""

from pathlib import Path
import importlib.util
import json
import tomllib
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
FORMATTER_TOOLCHAIN = "nightly-2026-07-16"


class GateContractTests(unittest.TestCase):
    """Keep the blocking validation graph small and explicit."""

    @classmethod
    def setUpClass(cls) -> None:
        with (REPO_ROOT / "Makefile.toml").open("rb") as source:
            document = tomllib.load(source)
        cls.config = document["config"]
        cls.tasks = document["tasks"]
        spec = importlib.util.spec_from_file_location(
            "decodex_local_database_gate",
            REPO_ROOT / "scripts/vnext/local_database_gate.py",
        )
        if spec is None or spec.loader is None:
            raise RuntimeError("local database gate module is unavailable")
        cls.database_gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.database_gate)

    def test_rust_format_tasks_use_the_pinned_formatter(self) -> None:
        self.assertEqual(self.tasks["fmt-rust"]["command"], "rustup")
        self.assertEqual(
            self.tasks["fmt-rust"]["args"],
            ["run", FORMATTER_TOOLCHAIN, "cargo", "fmt", "--all"],
        )
        self.assertEqual(self.tasks["fmt-rust-check"]["extend"], "fmt-rust")

    def test_active_rust_toolchain_remains_stable(self) -> None:
        with (REPO_ROOT / "rust-toolchain.toml").open("rb") as source:
            toolchain = tomllib.load(source)["toolchain"]
        self.assertEqual(
            toolchain,
            {
                "channel": "stable",
                "components": ["clippy"],
                "profile": "minimal",
            },
        )

    def test_cargo_make_uses_global_workspace_defaults(self) -> None:
        self.assertEqual(
            self.config,
            {"default_to_workspace": False, "skip_core_tasks": True},
        )
        self.assertTrue(all("workspace" not in task for task in self.tasks.values()))

    def test_direct_cargo_validation_tasks_use_the_lockfile(self) -> None:
        for task_name in (
            "check-rust",
            "check-rust-headless",
            "lint-rust-fix",
            "test-rust",
            "test-rust-headless",
        ):
            with self.subTest(task=task_name):
                task = self.tasks[task_name]
                self.assertEqual(task["command"], "cargo")
                self.assertIn("--locked", task["args"])

    def test_cargo_uses_the_standard_thin_release_profile(self) -> None:
        with (REPO_ROOT / "Cargo.toml").open("rb") as source:
            cargo_manifest = tomllib.load(source)
        self.assertEqual(cargo_manifest["profile"], {"release": {"lto": "thin"}})

    def test_blocking_test_aggregates_include_the_local_database_gate(self) -> None:
        for task_name in ("test", "test-sandboxed", "test-headless", "test-headless-sandboxed"):
            with self.subTest(task=task_name):
                dependencies = self.tasks[task_name]["dependencies"]
                self.assertIn("test-local-database", dependencies)
                self.assertIn("test-vnext-architecture", dependencies)

    def test_local_database_task_is_the_canonical_schema_gate(self) -> None:
        self.assertNotIn("test-vnext-latest-schema", self.tasks)
        self.assertEqual(
            self.tasks["test-local-database"],
            {
                "command": "python3",
                "args": ["scripts/vnext/local_database_gate.py"],
            },
        )
        self.assertFalse((REPO_ROOT / "scripts/vnext/latest_schema_gate.py").exists())

    def test_database_gate_contract_is_current(self) -> None:
        self.database_gate.validate_repository_contract()
        self.assertEqual(self.database_gate.APPLICATION_ID, 0x4443_5831)
        self.assertIn("provider_attempts", self.database_gate.REQUIRED_TABLES)
        self.assertIn("account_credentials", self.database_gate.REQUIRED_TABLES)

    def test_migration_digest_has_a_domain_separator(self) -> None:
        source = b"CREATE TABLE example(value INTEGER);\n"
        digest = self.database_gate.migration_digest(source)
        self.assertEqual(len(digest), 64)
        self.assertNotEqual(digest, __import__("hashlib").sha256(source).hexdigest())

    def test_task_graph_has_no_retired_schema_gate(self) -> None:
        serialized = json.dumps(self.tasks, sort_keys=True).lower()
        self.assertNotIn("latest_schema_gate", serialized)


if __name__ == "__main__":
    unittest.main()
