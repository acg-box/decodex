"""Regression tests for the repository's canonical validation task graph."""

from pathlib import Path
import hashlib
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
            "failure": {
                "category": "authority",
                "operation": "authority_verification",
                "phase": "post_schema_verify",
                "sqlstate": None,
                "statement_byte_position": None,
            },
            "namespace": namespace,
            "platform": platform,
            "rollback_failure": None,
            "schema": gate.BOOTSTRAP_REPORT_SCHEMA,
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
        phase, platform_complete, completed_authority, _ = (
            gate.BOOTSTRAP_FAILURE_OPERATIONS[operation]
        )
        report = self.bootstrap_report_fixture()
        report["classification"] = "incompatible"
        report["complete"] = False
        report["failure"] = {
            "category": "catalog",
            "operation": operation,
            "phase": phase,
            "sqlstate": None,
            "statement_byte_position": None,
        }
        report["schema_contract"] = None
        if completed_authority < 3:
            report["configured_authority"] = None
        else:
            configured_authority = report["configured_authority"]
            if not isinstance(configured_authority, dict):
                raise AssertionError("configured-authority fixture is unavailable")
            configured_authority["actual_sha256"] = configured_authority["expected_sha256"]
            configured_authority["pass"] = True
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
                    gate.BOOTSTRAP_REPORT_PREFIX
                    + encoded
                    + b"\nError: Database(UnsafeAuthority)\n",
                )
                return gate.validate_bootstrap_report(logs)
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

    def test_bootstrap_report_parser_requires_closed_ordered_evidence(self) -> None:
        encoded, failures = self.parse_bootstrap_report_fixture(
            self.bootstrap_report_fixture()
        )
        self.assertLessEqual(
            len(encoded.encode("ascii")),
            self.latest_schema_gate.BOOTSTRAP_REPORT_MAX_BYTES,
        )
        self.assertEqual(
            failures,
            (
                "failure:post_schema_verify:authority_verification:authority",
                "configured_authority:unsafe",
                "schema_contract:incompatible",
            ),
        )

        duplicate = self.bootstrap_report_fixture()
        duplicate["semantic"][1]["name"] = duplicate["semantic"][0]["name"]
        with self.assertRaises(self.latest_schema_gate.GateFailure):
            self.parse_bootstrap_report_fixture(duplicate)

    def test_bootstrap_report_rejects_unowned_fields_and_is_retained(self) -> None:
        self.assertIn(
            "bootstrap.log",
            self.latest_schema_gate.GATE_LOG_NAMES,
        )
        report = self.bootstrap_report_fixture()
        report["database_message"] = "not permitted"
        with self.assertRaises(self.latest_schema_gate.GateFailure):
            self.parse_bootstrap_report_fixture(report)

    def test_only_database_bootstrap_failures_require_the_transaction_report(self) -> None:
        gate = self.latest_schema_gate
        for diagnostic, required in (
            (b"Error: Configuration\n", False),
            (b"Error: Authentication\n", False),
            (b"Error: Database(Incompatible)\n", True),
        ):
            with self.subTest(diagnostic=diagnostic):
                with tempfile.TemporaryDirectory() as temporary:
                    logs = gate.GateLogDirectory.create(Path(temporary) / "logs")
                    try:
                        logs.write_diagnostic("bootstrap.log", diagnostic)
                        self.assertIs(
                            gate.bootstrap_failure_requires_report(logs),
                            required,
                        )
                    finally:
                        logs.close()

    def test_second_bootstrap_requires_one_canonical_target_refusal_and_error(self) -> None:
        gate = self.latest_schema_gate
        report = self.partial_bootstrap_report_fixture("target_verification")
        report["failure"]["category"] = "evidence"

        def output(candidate: dict[str, object], suffix: bytes = b"") -> bytes:
            encoded = json.dumps(candidate, sort_keys=True, separators=(",", ":")).encode(
                "ascii"
            )
            return (
                gate.BOOTSTRAP_REPORT_PREFIX
                + encoded
                + b"\n"
                + gate.SECOND_BOOTSTRAP_REFUSAL_ERROR
                + b"\n"
                + suffix
            )

        exact_output = output(report)
        with tempfile.TemporaryDirectory() as temporary:
            logs = gate.GateLogDirectory.create(Path(temporary) / "logs")
            try:
                logs.write_diagnostic("second-bootstrap.log", exact_output)
                gate.validate_second_bootstrap_refusal(logs)
            finally:
                logs.close()

        wrong_operation = self.partial_bootstrap_report_fixture("bootstrap_admission")
        wrong_operation["failure"]["category"] = "evidence"
        rollback_failed = json.loads(json.dumps(report))
        rollback_failed["rollback_failure"] = {"category": "transaction", "failed": True}
        for label, invalid_output in (
            ("wrong operation", output(wrong_operation)),
            ("rollback failed", output(rollback_failed)),
            ("extra output", output(report, b"unexpected output\n")),
        ):
            with self.subTest(label=label), tempfile.TemporaryDirectory() as temporary:
                logs = gate.GateLogDirectory.create(Path(temporary) / "logs")
                try:
                    logs.write_diagnostic("second-bootstrap.log", invalid_output)
                    with self.assertRaises(gate.GateFailure):
                        gate.validate_second_bootstrap_refusal(logs)
                finally:
                    logs.close()

    def test_bootstrap_report_accepts_closed_unsafe_path_and_database_transport_pairs(
        self,
    ) -> None:
        unsafe_path = self.partial_bootstrap_report_fixture("bootstrap_admission")
        unsafe_path["classification"] = "unsafe_host_path"
        unsafe_path["failure"]["category"] = "host_path"
        self.parse_bootstrap_report_fixture(unsafe_path)

        database_transport = self.partial_bootstrap_report_fixture("schema_batch")
        database_transport["classification"] = "incompatible"
        database_transport["failure"]["category"] = "transport"
        database_transport["failure"]["sqlstate"] = "08006"
        self.parse_bootstrap_report_fixture(database_transport)

        invalid = self.partial_bootstrap_report_fixture("bootstrap_admission")
        invalid["classification"] = "unsafe_host_path"
        invalid["failure"]["category"] = "transport"
        with self.assertRaises(self.latest_schema_gate.GateFailure):
            self.parse_bootstrap_report_fixture(invalid)

    def test_partial_bootstrap_report_requires_the_exact_operation_prefix(self) -> None:
        gate = self.latest_schema_gate
        expected_operations = {
            "bootstrap_admission": ("pre_schema", False, 0, False),
            "target_verification": ("pre_schema", False, 0, False),
            "runtime_role_binding": ("pre_schema", False, 0, False),
            "schema_batch": ("schema_apply", False, 0, True),
            "trusted_session_reset": ("post_schema_verify", False, 0, False),
            "platform": ("post_schema_verify", False, 0, False),
            "initial_authorization": ("post_schema_verify", True, 0, False),
            "namespace": ("post_schema_verify", True, 0, False),
            "semantic": ("post_schema_verify", True, 1, False),
            "configured_authority": ("post_schema_verify", True, 2, False),
            "schema_contract": ("post_schema_verify", True, 3, False),
            "authority_verification": ("post_schema_verify", True, 4, False),
            "transaction_commit": ("finalize", True, 4, False),
        }
        self.assertEqual(gate.BOOTSTRAP_FAILURE_OPERATIONS, expected_operations)
        for operation, (
            phase,
            platform_complete,
            completed_authority,
            _,
        ) in {
            name: contract
            for name, contract in expected_operations.items()
            if contract[2] < 4
        }.items():
            with self.subTest(operation=operation):
                encoded, failures = self.parse_bootstrap_report_fixture(
                    self.partial_bootstrap_report_fixture(operation)
                )
                report = json.loads(encoded)
                self.assertEqual(
                    failures,
                    (f"failure:{phase}:{operation}:catalog",),
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
                report["failure"][field] = value
                with self.assertRaises(gate.GateFailure):
                    self.parse_bootstrap_report_fixture(report)

        missing_operation = self.partial_bootstrap_report_fixture("semantic")
        del missing_operation["failure"]["operation"]
        with self.assertRaises(gate.GateFailure):
            self.parse_bootstrap_report_fixture(missing_operation)

        mismatched_phase = self.partial_bootstrap_report_fixture("semantic")
        mismatched_phase["failure"]["phase"] = "pre_schema"
        with self.assertRaises(gate.GateFailure):
            self.parse_bootstrap_report_fixture(mismatched_phase)

        mismatched_classification = self.partial_bootstrap_report_fixture("semantic")
        mismatched_classification["classification"] = "unsafe_authority"
        with self.assertRaises(gate.GateFailure):
            self.parse_bootstrap_report_fixture(mismatched_classification)

        report = self.partial_bootstrap_report_fixture("semantic")
        report["failure"]["detail"] = "private database detail"
        with self.assertRaises(gate.GateFailure):
            self.parse_bootstrap_report_fixture(report)

    def test_schema_apply_report_accepts_only_sqlstate_and_original_byte_position(self) -> None:
        gate = self.latest_schema_gate
        report = self.partial_bootstrap_report_fixture("schema_batch")
        report["failure"]["sqlstate"] = "42601"
        report["failure"]["statement_byte_position"] = 137
        encoded, failures = self.parse_bootstrap_report_fixture(report)
        self.assertNotIn("sql_text", encoded)
        self.assertEqual(
            failures,
            ("failure:schema_apply:schema_batch:catalog",),
        )

        outside_schema = self.partial_bootstrap_report_fixture("schema_batch")
        outside_schema["failure"]["sqlstate"] = "42601"
        outside_schema["failure"]["statement_byte_position"] = (
            gate.SCHEMA.stat().st_size + 2
        )
        with self.assertRaises(gate.GateFailure):
            self.parse_bootstrap_report_fixture(outside_schema)

    def test_bootstrap_report_keeps_primary_failure_when_rollback_fails(self) -> None:
        gate = self.latest_schema_gate
        report = self.partial_bootstrap_report_fixture("schema_batch")
        report["rollback_failure"] = {"category": "transaction", "failed": True}
        _, failures = self.parse_bootstrap_report_fixture(report)
        self.assertEqual(
            failures,
            ("failure:schema_apply:schema_batch:catalog",),
        )

        invalid = self.partial_bootstrap_report_fixture("schema_batch")
        invalid["rollback_failure"] = {"category": "transaction", "failed": False}
        with self.assertRaises(gate.GateFailure):
            self.parse_bootstrap_report_fixture(invalid)

    def test_finalize_failure_requires_complete_passing_authority_evidence(self) -> None:
        report = self.bootstrap_report_fixture()
        report["classification"] = "incompatible"
        report["failure"] = {
            "category": "transaction",
            "operation": "transaction_commit",
            "phase": "finalize",
            "sqlstate": "40003",
            "statement_byte_position": None,
        }
        for name in ("configured_authority", "schema_contract"):
            report[name]["actual_sha256"] = report[name]["expected_sha256"]
            report[name]["pass"] = True
        _, failures = self.parse_bootstrap_report_fixture(report)
        self.assertEqual(
            failures,
            ("failure:finalize:transaction_commit:transaction",),
        )

    def test_failure_retention_preserves_bounded_head_and_tail(self) -> None:
        gate = self.latest_schema_gate
        head = b"ERROR:  syntax error at or near closed_identity\n"
        tail = b"LOG:  database system is shut down\n"
        payload = head + b"x" * (gate.FAILURE_LOG_FILE_BYTES + 257) + tail
        with tempfile.TemporaryDirectory() as temporary:
            parent = Path(temporary)
            logs = gate.GateLogDirectory.create(parent / "logs")
            try:
                logs.write_diagnostic("postgres.log", payload)
                with mock.patch.object(gate, "FAILURE_EVIDENCE_PARENT", parent):
                    evidence, records, warnings = gate.retain_failure_logs(logs)
                retained = (evidence / "postgres.log").read_bytes()
            finally:
                logs.close()

        self.assertEqual(warnings, [])
        self.assertEqual(len(records), 1)
        name, digest, size, source_size, head_bytes, tail_offset = records[0]
        self.assertEqual(name, "postgres.log")
        self.assertEqual(size, gate.FAILURE_LOG_FILE_BYTES)
        self.assertEqual(source_size, len(payload))
        self.assertEqual(head_bytes, gate.FAILURE_LOG_FILE_BYTES // 2)
        self.assertEqual(tail_offset, len(payload) - gate.FAILURE_LOG_FILE_BYTES // 2)
        self.assertEqual(digest, hashlib.sha256(retained).hexdigest())
        self.assertTrue(retained.startswith(head))
        self.assertTrue(retained.endswith(tail))

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


class RustWorkspaceLintOwnerTests(unittest.TestCase):
    @staticmethod
    def _root():
        from pathlib import Path

        return Path(__file__).resolve().parents[2]

    @classmethod
    def _load_owner(cls):
        import importlib.util

        path = cls._root() / "scripts" / "lint_rust_workspace.py"
        spec = importlib.util.spec_from_file_location("lint_rust_workspace_contract", path)
        if spec is None or spec.loader is None:
            raise AssertionError("could not load the Rust workspace lint owner")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    @staticmethod
    def _task_section(source, task_name):
        marker = f"[tasks.{task_name}]"
        start = source.index(marker)
        end = source.find("\n[tasks.", start + len(marker))
        return source[start:] if end == -1 else source[start:end]

    @staticmethod
    def _expected_clippy_command(package_name):
        return [
            "cargo",
            "clippy",
            "--package",
            package_name,
            "--all-features",
            "--all-targets",
            "--keep-going",
            "--",
            "--no-deps",
            "-D",
            "clippy::all",
            "-D",
            "clippy::too_many_lines",
            "-D",
            "clippy::unwrap_used",
            "-D",
            "clippy::use_self",
            "-D",
            "clippy::wildcard_imports",
            "-D",
            "missing-docs",
            "-D",
            "unused-crate-dependencies",
            "-D",
            "warnings",
        ]

    def _run_owner(
        self,
        package_names,
        argv,
        failures=None,
        spawn_failures=None,
        diagnostics=None,
        metadata_spawn_failure=None,
    ):
        import io
        import json
        from contextlib import redirect_stderr, redirect_stdout
        from unittest import mock

        owner = self._load_owner()
        package_ids = {name: f"path+file:///{name}#{name}@0.2.0" for name in package_names}
        metadata = {
            "packages": [
                {"id": package_ids[name], "name": name}
                for name in reversed(package_names)
            ],
            "workspace_members": [package_ids[name] for name in package_names],
        }
        commands = []
        subprocess_kwargs = []
        failures = failures or {}
        spawn_failures = spawn_failures or {}
        diagnostics = diagnostics or {}

        def fake_run(command, **kwargs):
            commands.append(list(command))
            subprocess_kwargs.append(kwargs)
            if command[:2] == ["cargo", "metadata"]:
                if metadata_spawn_failure is not None:
                    raise OSError(metadata_spawn_failure)
                return owner.subprocess.CompletedProcess(
                    command,
                    0,
                    stdout=json.dumps(metadata),
                )
            package_name = command[command.index("--package") + 1]
            if package_name in spawn_failures:
                raise OSError(spawn_failures[package_name])
            if package_name in diagnostics:
                print(diagnostics[package_name], flush=True)
            return owner.subprocess.CompletedProcess(
                command,
                failures.get(package_name, 0),
            )

        output = io.StringIO()
        with mock.patch.object(owner.subprocess, "run", side_effect=fake_run):
            with redirect_stdout(output), redirect_stderr(output):
                status = owner.main(argv)
        return status, commands, output.getvalue(), subprocess_kwargs

    def test_lint_rust_tasks_route_only_the_supported_argument_boundary(self):
        source = (self._root() / "Makefile.toml").read_text(encoding="utf-8")
        lint_rust = self._task_section(source, "lint-rust")
        lint_rust_headless = self._task_section(source, "lint-rust-headless")

        self.assertEqual(
            [
                line.strip()
                for line in lint_rust.splitlines()
                if line.strip().startswith(("command =", "args ="))
            ],
            ['command = "python3"', 'args = ["scripts/lint_rust_workspace.py"]'],
        )
        self.assertEqual(
            [
                line.strip()
                for line in lint_rust_headless.splitlines()
                if line.strip().startswith(("command =", "args ="))
            ],
            [
                'command = "python3"',
                'args = ["scripts/lint_rust_workspace.py", "--headless"]',
            ],
        )

    def test_all_metadata_members_are_selected_once_in_deterministic_order(self):
        package_names = ["zeta", "decodex-gpui", "alpha"]
        status, commands, _, _ = self._run_owner(package_names, [])

        self.assertEqual(status, 0)
        self.assertEqual(commands[0], ["cargo", "metadata", "--format-version", "1", "--no-deps"])
        self.assertEqual(
            commands[1:],
            [self._expected_clippy_command(name) for name in sorted(package_names)],
        )

    def test_headless_excludes_only_decodex_gpui(self):
        package_names = ["zeta", "decodex-gpui", "alpha"]
        status, commands, _, _ = self._run_owner(package_names, ["--headless"])

        self.assertEqual(status, 0)
        self.assertEqual(
            commands[1:],
            [self._expected_clippy_command(name) for name in ["alpha", "zeta"]],
        )

    def test_failures_do_not_stop_later_packages_and_are_aggregated(self):
        status, commands, output, _ = self._run_owner(
            ["alpha", "beta", "gamma"],
            [],
            failures={"alpha": 7, "gamma": 3},
        )

        self.assertEqual(
            commands[1:],
            [self._expected_clippy_command(name) for name in ["alpha", "beta", "gamma"]],
        )
        self.assertEqual(status, 1)
        self.assertIn("Rust lint summary:", output)
        self.assertIn("  alpha: FAIL (exit 7)", output)
        self.assertIn("  beta: PASS", output)
        self.assertIn("  gamma: FAIL (exit 3)", output)
        self.assertIn("Rust lint result: 1 passed; 2 failed; 3 total.", output)

    def test_spawn_failure_does_not_stop_later_packages_and_is_attributed(self):
        status, commands, output, _ = self._run_owner(
            ["alpha", "beta", "gamma"],
            [],
            spawn_failures={"beta": "cargo executable unavailable"},
        )

        self.assertEqual(
            commands[1:],
            [self._expected_clippy_command(name) for name in ["alpha", "beta", "gamma"]],
        )
        self.assertEqual(status, 1)
        self.assertIn(
            "<== Rust lint [2/3]: beta: SPAWN FAILURE (cargo executable unavailable)",
            output,
        )
        self.assertIn("  beta: SPAWN FAILURE (cargo executable unavailable)", output)
        self.assertIn("  gamma: PASS", output)
        self.assertIn("Rust lint result: 2 passed; 1 failed; 3 total.", output)

    def test_clippy_stderr_is_streamed_inside_the_package_envelope(self):
        import subprocess

        status, _, output, subprocess_kwargs = self._run_owner(
            ["alpha"],
            [],
            diagnostics={"alpha": "alpha clippy diagnostic"},
        )

        self.assertEqual(status, 0)
        self.assertEqual(subprocess_kwargs[1]["stderr"], subprocess.STDOUT)
        self.assertNotIn("stdout", subprocess_kwargs[1])
        start = output.index("==> Rust lint [1/1]: alpha")
        diagnostic = output.index("alpha clippy diagnostic")
        end = output.index("<== Rust lint [1/1]: alpha: PASS")
        self.assertLess(start, diagnostic)
        self.assertLess(diagnostic, end)

    def test_metadata_spawn_failure_is_a_typed_setup_failure(self):
        status, commands, output, _ = self._run_owner(
            ["alpha"],
            [],
            metadata_spawn_failure="cargo executable unavailable",
        )

        self.assertEqual(status, 2)
        self.assertEqual(commands, [["cargo", "metadata", "--format-version", "1", "--no-deps"]])
        self.assertIn(
            "Rust lint setup failed: cargo metadata spawn failure: cargo executable unavailable",
            output,
        )
        self.assertNotIn("Traceback", output)


if __name__ == "__main__":
    unittest.main()
