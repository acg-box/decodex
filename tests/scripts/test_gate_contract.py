"""Regression tests for the repository's canonical validation task graph."""

from pathlib import Path
import importlib.util
import json
import os
import subprocess
import tempfile
import tomllib
import unittest
from unittest import mock


REPO_ROOT = Path(__file__).resolve().parents[2]
FORMATTER_TOOLCHAIN = "nightly-2026-07-16"


class GateContractTests(unittest.TestCase):
    """Keep canonical checks explicit, deterministic, and non-overlapping."""

    @classmethod
    def setUpClass(cls) -> None:
        with (REPO_ROOT / "Makefile.toml").open("rb") as makefile:
            cls.tasks = tomllib.load(makefile)["tasks"]
        spec = importlib.util.spec_from_file_location(
            "decodex_latest_schema_gate_contract",
            REPO_ROOT / "scripts/vnext/latest_schema_gate.py",
        )
        if spec is None or spec.loader is None:
            raise RuntimeError("latest-schema gate module is unavailable")
        cls.latest_schema_gate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.latest_schema_gate)

    def bootstrap_report_fixture(self) -> dict[str, object]:
        gate = self.latest_schema_gate

        def observation(name: str, failure_class: str) -> dict[str, object]:
            return {"class": failure_class, "name": name, "pass": True}

        platform = [
            observation(
                name,
                "unsafe" if name.startswith("trusted_") else "incompatible",
            )
            for name in gate.BOOTSTRAP_PLATFORM_NAMES
        ]
        namespace = [
            observation("namespace_present", "incompatible"),
            observation("namespace_owner", "unsafe"),
        ]
        semantic = [
            observation(
                name,
                "incompatible"
                if name in gate.BOOTSTRAP_SEMANTIC_INCOMPATIBLE
                or name == "exact_function_inventory"
                else "unsafe",
            )
            for name in gate.BOOTSTRAP_SEMANTIC_NAMES
        ]
        return {
            "classification": "unsafe_authority",
            "complete": True,
            "configured_authority": {
                "actual_sha256": "b" * 64,
                "class": "unsafe",
                "complete": True,
                "expected_sha256": "a" * 64,
                "pass": False,
            },
            "namespace": namespace,
            "platform": platform,
            "query_failure": None,
            "schema": gate.BOOTSTRAP_AUTHORITY_REPORT_SCHEMA,
            "schema_contract": {
                "actual_sha256": "d" * 64,
                "class": "incompatible",
                "complete": True,
                "expected_sha256": "c" * 64,
                "pass": False,
            },
            "semantic": semantic,
        }

    def partial_bootstrap_report_fixture(self, operation: str) -> dict[str, object]:
        gate = self.latest_schema_gate
        phase, platform_complete, completed_authority = (
            gate.BOOTSTRAP_QUERY_FAILURE_OPERATIONS[operation]
        )
        report = self.bootstrap_report_fixture()
        report["classification"] = "incompatible"
        report["complete"] = False
        report["query_failure"] = {
            "category": "catalog",
            "operation": operation,
            "phase": phase,
        }
        report["schema_contract"] = None
        if completed_authority < 3:
            report["configured_authority"] = None
        if completed_authority < 2:
            report["semantic"] = []
        if completed_authority < 1:
            report["namespace"] = []
        if not platform_complete:
            report["platform"] = []
        return report

    def parse_bootstrap_report_fixture(
        self,
        report: dict[str, object],
    ) -> tuple[str, tuple[str, ...]]:
        gate = self.latest_schema_gate
        encoded = json.dumps(report, sort_keys=True, separators=(",", ":")).encode("ascii")
        with tempfile.TemporaryDirectory() as temporary:
            logs = gate.GateLogDirectory.create(Path(temporary) / "logs")
            try:
                logs.write_diagnostic(
                    "bootstrap.log",
                    gate.BOOTSTRAP_AUTHORITY_REPORT_PREFIX
                    + encoded
                    + b"\nError: Database(UnsafeAuthority)\n",
                )
                return gate.validate_bootstrap_authority_report(logs)
            finally:
                logs.close()

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
            ],
        )

    def test_all_test_aggregates_omit_the_retired_live_postgres_gate(self) -> None:
        self.assertNotIn("test-vnext-postgres-store", self.tasks)
        pairs = (
            ("test", "test-sandboxed"),
            ("test-headless", "test-headless-sandboxed"),
        )
        for ordinary, sandboxed in pairs:
            with self.subTest(ordinary=ordinary, sandboxed=sandboxed):
                ordinary_dependencies = self.tasks[ordinary]["dependencies"]
                sandboxed_dependencies = self.tasks[sandboxed]["dependencies"]
                self.assertEqual(sandboxed_dependencies, ordinary_dependencies)

        self.assertEqual(
            self.tasks["check-automations-sandboxed"]["dependencies"][-1],
            "test-headless-sandboxed",
        )
        self.assertEqual(
            self.tasks["check-sandboxed"]["dependencies"][-1],
            "test-sandboxed",
        )

    def test_retired_account_migration_gate_has_no_task_or_source(self) -> None:
        serialized_tasks = json.dumps(self.tasks, sort_keys=True)
        self.assertNotIn("account-migration-transition", serialized_tasks)
        self.assertNotIn("build-vnext-account-migration-transition", self.tasks)
        self.assertNotIn("test-vnext-account-migration-transition", self.tasks)
        self.assertFalse(
            (REPO_ROOT / "scripts/vnext/account_migration_transition_test.py").exists()
        )

    def test_latest_schema_and_current_authority_have_one_product_gate(self) -> None:
        self.assertEqual(
            self.tasks["test-vnext-latest-schema"],
            {
                "workspace": False,
                "command": "python3",
                "args": ["scripts/vnext/latest_schema_gate.py"],
            },
        )
        owners = {
            name
            for name, task in self.tasks.items()
            if "latest-schema" in name
            or "current-authority" in name
            or "latest_schema_gate.py" in json.dumps(task, sort_keys=True)
        }
        self.assertEqual(owners, {"test-vnext-latest-schema"})
        self.assertTrue((REPO_ROOT / "scripts/vnext/latest_schema_gate.py").is_file())

    def test_reverse_scan_rejects_schema_ddl_in_one_active_vnext_python_source(self) -> None:
        gate = self.latest_schema_gate
        gate.reverse_scan(os.environ.copy())

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            schema = root / "crates/decodex-postgres/schema.sql"
            script = root / "scripts/vnext/injected_schema_owner.py"
            schema.parent.mkdir(parents=True)
            script.parent.mkdir(parents=True)
            schema.write_text("-- canonical latest-schema fixture\n", encoding="utf-8")
            script.write_text(
                'PRODUCT_DDL = "CREATE TABLE decodex.duplicate_owner(id bigint);"\n',
                encoding="utf-8",
            )
            subprocess.run(
                ["git", "init", "--quiet"],
                cwd=root,
                check=True,
                capture_output=True,
            )
            subprocess.run(
                ["git", "add", "."],
                cwd=root,
                check=True,
                capture_output=True,
            )

            with mock.patch.object(gate, "ROOT", root), mock.patch.object(
                gate, "SCHEMA", schema
            ):
                with self.assertRaises(gate.GateFailure) as raised:
                    gate.reverse_scan(os.environ.copy())

            self.assertEqual(
                str(raised.exception),
                "prohibited schema DDL in "
                "scripts/vnext/injected_schema_owner.py: CREATE TABLE",
            )

    def test_bootstrap_authority_report_parser_requires_closed_ordered_evidence(self) -> None:
        encoded, failures = self.parse_bootstrap_report_fixture(
            self.bootstrap_report_fixture()
        )
        self.assertLessEqual(
            len(encoded.encode("ascii")),
            self.latest_schema_gate.BOOTSTRAP_AUTHORITY_REPORT_MAX_BYTES,
        )
        self.assertEqual(
            failures,
            (
                "configured_authority:unsafe",
                "schema_contract:incompatible",
            ),
        )

        duplicate = self.bootstrap_report_fixture()
        duplicate["semantic"][1]["name"] = duplicate["semantic"][0]["name"]
        with self.assertRaises(self.latest_schema_gate.GateFailure):
            self.parse_bootstrap_report_fixture(duplicate)

    def test_bootstrap_authority_report_rejects_unowned_fields_and_is_retained(self) -> None:
        self.assertIn(
            "bootstrap.log",
            self.latest_schema_gate.GATE_LOG_NAMES,
        )
        report = self.bootstrap_report_fixture()
        report["database_message"] = "not permitted"
        with self.assertRaises(self.latest_schema_gate.GateFailure):
            self.parse_bootstrap_report_fixture(report)

    def test_partial_bootstrap_report_requires_the_exact_operation_prefix(self) -> None:
        gate = self.latest_schema_gate
        expected_operations = {
            "platform": ("platform", False, 0),
            "initial_authorization": ("initial_authorization", True, 0),
            "namespace": ("authority", True, 0),
            "semantic": ("authority", True, 1),
            "configured_authority": ("authority", True, 2),
            "schema_contract": ("authority", True, 3),
        }
        self.assertEqual(gate.BOOTSTRAP_QUERY_FAILURE_OPERATIONS, expected_operations)
        for operation, (
            phase,
            platform_complete,
            completed_authority,
        ) in expected_operations.items():
            with self.subTest(operation=operation):
                encoded, failures = self.parse_bootstrap_report_fixture(
                    self.partial_bootstrap_report_fixture(operation)
                )
                report = json.loads(encoded)
                self.assertEqual(
                    failures,
                    (f"query:{phase}:{operation}:catalog",),
                )
                self.assertEqual(bool(report["platform"]), platform_complete)
                self.assertEqual(bool(report["namespace"]), completed_authority >= 1)
                self.assertEqual(bool(report["semantic"]), completed_authority >= 2)
                self.assertEqual(
                    report["configured_authority"] is not None,
                    completed_authority >= 3,
                )
                self.assertIsNone(report["schema_contract"])

        missing_completed = self.partial_bootstrap_report_fixture("schema_contract")
        missing_completed["semantic"] = []
        with self.assertRaises(gate.GateFailure):
            self.parse_bootstrap_report_fixture(missing_completed)

        present_current = self.partial_bootstrap_report_fixture("semantic")
        present_current["semantic"] = self.bootstrap_report_fixture()["semantic"]
        with self.assertRaises(gate.GateFailure):
            self.parse_bootstrap_report_fixture(present_current)

        changed_class = self.partial_bootstrap_report_fixture("schema_contract")
        changed_class["namespace"][0]["class"] = "unsafe"
        with self.assertRaises(gate.GateFailure):
            self.parse_bootstrap_report_fixture(changed_class)

        invalid_digest = self.partial_bootstrap_report_fixture("schema_contract")
        invalid_digest["configured_authority"]["actual_sha256"] = "not-a-digest"
        with self.assertRaises(gate.GateFailure):
            self.parse_bootstrap_report_fixture(invalid_digest)

    def test_partial_bootstrap_report_rejects_open_failure_diagnostics(self) -> None:
        gate = self.latest_schema_gate
        for field, value in (
            ("operation", "arbitrary_query"),
            ("category", "arbitrary_category"),
            ("phase", "arbitrary_phase"),
        ):
            with self.subTest(field=field):
                report = self.partial_bootstrap_report_fixture("semantic")
                report["query_failure"][field] = value
                with self.assertRaises(gate.GateFailure):
                    self.parse_bootstrap_report_fixture(report)

        missing_operation = self.partial_bootstrap_report_fixture("semantic")
        del missing_operation["query_failure"]["operation"]
        with self.assertRaises(gate.GateFailure):
            self.parse_bootstrap_report_fixture(missing_operation)

        mismatched_phase = self.partial_bootstrap_report_fixture("semantic")
        mismatched_phase["query_failure"]["phase"] = "platform"
        with self.assertRaises(gate.GateFailure):
            self.parse_bootstrap_report_fixture(mismatched_phase)

        mismatched_classification = self.partial_bootstrap_report_fixture("semantic")
        mismatched_classification["classification"] = "unsafe_authority"
        with self.assertRaises(gate.GateFailure):
            self.parse_bootstrap_report_fixture(mismatched_classification)

        report = self.partial_bootstrap_report_fixture("semantic")
        report["query_failure"]["detail"] = "private database detail"
        with self.assertRaises(gate.GateFailure):
            self.parse_bootstrap_report_fixture(report)

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
