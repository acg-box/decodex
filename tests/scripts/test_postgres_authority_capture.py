import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/vnext/postgres_store_test.py"
SPEC = importlib.util.spec_from_file_location("postgres_store_test", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
POSTGRES_STORE_TEST = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = POSTGRES_STORE_TEST
SPEC.loader.exec_module(POSTGRES_STORE_TEST)

SOURCE_BINDING = {"head": "a" * 40, "tree": "b" * 40}
DATABASE = "decodex_xy1300_authority_capture"


def component(*, available, complete, error=None, manifest=None):
    return {
        "available": available,
        "complete": complete,
        "error": error,
        "manifest": manifest,
    }


def envelope(schema, authority=None, binding=None):
    if binding is None:
        binding = {
            "requested": DATABASE,
            "migration_url": DATABASE,
            "runtime_url": DATABASE,
            "observed_migration": DATABASE,
            "observed_runtime": DATABASE,
        }
    return {
        "schema": schema,
        "authority": authority or component(
            available=True, complete=True, manifest="[]"
        ),
        "binding": binding,
        "semantic_authority": {
            "predicates": [
                {"name": name, "passed": True}
                for name in POSTGRES_STORE_TEST.SEMANTIC_AUTHORITY_PREDICATES
            ],
            "schema": POSTGRES_STORE_TEST.SEMANTIC_AUTHORITY_SCHEMA,
        },
        "sequence_state": [],
    }


def diagnostic(*, document=None, components=(), **artifact):
    return json.loads(POSTGRES_STORE_TEST.capture_manifest_diagnostic(
        "source",
        DATABASE,
        source_binding=SOURCE_BINDING,
        secret_markers=("xy1300-secret",),
        document=document,
        component_names=components,
        **artifact,
    ))


def dependency_row(
    kind, source_identity, dependency_type, reference_class, reference_key, resolved,
    *, source_kind="constraint",
):
    identity = [source_identity, dependency_type, reference_class, reference_key]
    if kind == "dependency":
        identity.insert(0, source_kind)
    return [kind, identity, json.dumps([resolved], separators=(",", ":"))]


def constraint_contract(definition, *, validated=True):
    return json.dumps([
        "f", definition, False, False, validated, True, "a", "a", "s", True,
        0, False, ["managed_run_id"], ["managed_run_id"], "decodex",
        "managed_runs",
    ], separators=(",", ":"))


def runtime_authority(database):
    return {
        "database": database,
        "migration_role": "decodex_migration",
        "runtime_role": "decodex_runtime_xy1300",
        "non_default_runtime_role": True,
        "runtime_login": True,
        "anchor_execute": True,
        "direct_non_grantable_execute_count": 15,
        "direct_non_grantable_type_usage_count": 5,
    }


class PostgresAuthorityCaptureDiagnosticTests(unittest.TestCase):
    def artifact_failure(self, document):
        artifact = json.dumps(document, separators=(",", ":")).encode()
        with self.assertRaises(POSTGRES_STORE_TEST.TestFailure) as failure:
            POSTGRES_STORE_TEST.parse_capture_manifest(
                artifact,
                "source",
                DATABASE,
                source_binding=SOURCE_BINDING,
                secret_markers=("xy1300-secret",),
            )
        message = str(failure.exception)
        return artifact, message, json.loads(message[message.index("{") :])

    def restore_parity_failure(
        self, source_rows, restored_rows, *, secret_markers=("xy1300-secret",)
    ):
        source = json.dumps(source_rows, separators=(",", ":"))
        restored = json.dumps(restored_rows, separators=(",", ":"))
        with self.assertRaises(POSTGRES_STORE_TEST.TestFailure) as failure:
            POSTGRES_STORE_TEST.require_restore_parity(
                "schema",
                source,
                restored,
                secret_markers=secret_markers,
            )
        message = str(failure.exception)
        return source, restored, message, json.loads(message[message.index("{") :])

    def test_database_query_error_is_structured_bounded_and_redacted(self):
        message_prefix = "permission denied xy1300-secret "
        query_error = json.dumps({
            "classification": "database_error",
            "message": message_prefix + "x" * (
                POSTGRES_STORE_TEST.MANIFEST_DIAGNOSTIC_ERROR_LIMIT
                - len(message_prefix)
            ),
            "message_truncated": True,
            "schema": POSTGRES_STORE_TEST.MANIFEST_QUERY_ERROR_SCHEMA,
            "sqlstate": "42501",
        })
        result = diagnostic(
            document=envelope(component(
                available=False, complete=False, error=query_error
            )),
            components=("schema",),
        )
        serialized = json.dumps(result, sort_keys=True)
        failure = result["components"][0]["error"]

        self.assertNotIn("xy1300-secret", serialized)
        self.assertEqual(failure["classification"], "database_error")
        self.assertEqual(failure["sqlstate"], "42501")
        self.assertTrue(failure["truncated"])
        self.assertLessEqual(
            len(failure["text"]),
            POSTGRES_STORE_TEST.MANIFEST_DIAGNOSTIC_ERROR_LIMIT,
        )

    def test_incomplete_dependencies_include_only_bounded_reference_evidence(self):
        rows = [
            dependency_row(
                "dependency",
                ["source", index, "xy1300-secret"],
                "normal",
                ["function"],
                None if index == 0 else [
                    "function", index, "xy1300-secret", "x" * 400
                ],
                False,
            )
            for index in range(10)
        ]
        manifest = json.dumps(rows, separators=(",", ":"))
        result = diagnostic(
            document=envelope(component(
                available=True, complete=False, manifest=manifest
            )),
            components=("schema",),
        )
        serialized = json.dumps(result, sort_keys=True)
        schema = result["components"][0]
        unresolved = schema["unresolved_dependencies"]

        self.assertNotIn("xy1300-secret", serialized)
        self.assertEqual(schema["manifest"]["row_count"], 10)
        self.assertEqual(
            schema["manifest"]["sha256"], hashlib.sha256(manifest.encode()).hexdigest()
        )
        self.assertEqual(unresolved["count"], 10)
        self.assertEqual(len(unresolved["evidence"]), 8)
        self.assertTrue(unresolved["evidence_truncated"])
        evidence = unresolved["evidence"][0]
        self.assertEqual(evidence["kind"], "constraint")
        self.assertEqual(evidence["dependency_type"], "normal")
        self.assertEqual(evidence["reference_class"], '["function"]')
        self.assertEqual(evidence["reference_key"], "null")
        self.assertNotIn("contract", evidence)

    def test_semantic_dependency_identity_preserves_endpoints_and_deptypes(self):
        source = ["relation", "decodex", "managed_runs", "constraint", "t"]
        rows = [
            dependency_row(
                "dependency",
                source,
                "i",
                ["trigger"],
                [
                    "trigger",
                    ["user_trigger", ["decodex", "managed_runs", "r"], name],
                ],
                True,
            )
            for name in ("first_trigger", "second_trigger")
        ]
        rows.append(dependency_row(
            "dependency", source, "n", ["trigger"], rows[0][1][-1], True
        ))
        evidence = POSTGRES_STORE_TEST.authority_manifest_evidence(json.dumps(rows))

        self.assertEqual(evidence["row_count"], 3)
        self.assertEqual(evidence["duplicate_key_multiplicities"], [])
        self.assertTrue(evidence["complete"])
        self.assertTrue(evidence["resolved"])
        self.assertTrue(evidence["unique"])
        self.assertNotIn("rows", evidence)
        self.assertEqual(len({json.dumps(row[:2], sort_keys=True) for row in rows}), 3)

    def test_duplicate_semantic_dependency_edge_remains_rejected(self):
        row = dependency_row(
            "function_dependency",
            ["decodex", "function", []],
            "n",
            ["namespace"],
            ["namespace", "decodex"],
            True,
        )
        manifest = json.dumps([row, row])
        evidence = POSTGRES_STORE_TEST.authority_manifest_evidence(manifest)

        self.assertEqual(
            evidence["duplicate_key_multiplicities"][0]["multiplicity"], 2
        )
        self.assertFalse(evidence["unique"])
        self.assertNotIn("rows", evidence)
        with self.assertRaisesRegex(
            POSTGRES_STORE_TEST.TestFailure, "duplicate kind/identity key"
        ):
            POSTGRES_STORE_TEST.semantic_row_diff(manifest, "[]")

    def test_dependency_row_shape_validation_is_exact(self):
        rows = [
            dependency_row(
                "dependency", ["constraint"], "i", ["trigger"], None, False
            ),
            dependency_row(
                "function_dependency", ["decodex", "function", []], "n",
                ["namespace"], ["namespace", "decodex"], True,
            ),
            dependency_row(
                "type_dependency", ["decodex", "type"], "n", ["namespace"],
                ["namespace", "decodex"], True,
            ),
        ]
        for row in rows:
            with self.subTest(kind=row[0]):
                self.assertIsNotNone(
                    POSTGRES_STORE_TEST.decode_dependency_manifest_row(row)
                )

        malformed = [
            ["dependency", rows[0][1][1:], json.dumps([False])],
            ["function_dependency", rows[1][1], json.dumps([True, False])],
            ["type_dependency", rows[2][1][:-1] + ["namespace"], json.dumps([True])],
        ]
        for row in malformed:
            with self.subTest(row=row), self.assertRaisesRegex(
                POSTGRES_STORE_TEST.TestFailure,
                "malformed schema dependency contract",
            ):
                POSTGRES_STORE_TEST.decode_dependency_manifest_row(row)

    def test_restore_parity_diagnostic_is_bounded_deterministic_and_fail_closed(self):
        source_only = [
            [
                "relation",
                ["source_only", index, "xy1300-secret", "i" * 400],
                json.dumps(["source", "xy1300-secret", "s" * 400]),
            ]
            for index in range(10)
        ]
        restored_only = [
            [
                "relation",
                ["restored_only", index, "xy1300-secret", "i" * 400],
                json.dumps(["restored", "xy1300-secret", "r" * 400]),
            ]
            for index in range(10)
        ]
        source_shared = [
            ["column", ["shared", index], json.dumps(["before", index])]
            for index in range(10)
        ]
        restored_shared = [
            ["column", ["shared", index], json.dumps(["after", index])]
            for index in range(10)
        ]

        source, restored, first_message, diagnostic = self.restore_parity_failure(
            source_only + source_shared,
            restored_only + restored_shared,
        )
        _, _, second_message, _ = self.restore_parity_failure(
            source_only + source_shared,
            restored_only + restored_shared,
        )
        serialized = json.dumps(diagnostic, sort_keys=True)

        self.assertEqual(first_message, second_message)
        self.assertTrue(first_message.startswith(
            "authority candidate restore parity diagnostic: {"
        ))
        self.assertNotIn("xy1300-secret", serialized)
        self.assertEqual(
            set(diagnostic), {"changes", "component", "restored", "schema", "source"}
        )
        self.assertEqual(
            diagnostic["schema"],
            POSTGRES_STORE_TEST.RESTORE_PARITY_DIAGNOSTIC_SCHEMA,
        )
        self.assertEqual(diagnostic["component"], "schema")
        self.assertEqual(
            set(diagnostic["changes"]),
            {"before_only", "after_only", "contract_mismatches"},
        )
        self.assertEqual(diagnostic["source"]["row_count"], 20)
        self.assertEqual(diagnostic["restored"]["row_count"], 20)
        self.assertEqual(
            diagnostic["source"]["sha256"], hashlib.sha256(source.encode()).hexdigest()
        )
        self.assertEqual(
            diagnostic["restored"]["sha256"],
            hashlib.sha256(restored.encode()).hexdigest(),
        )
        self.assertEqual(
            diagnostic["source"]["grouped_kind_counts"],
            [{"count": 10, "kind": "column"}, {"count": 10, "kind": "relation"}],
        )
        self.assertEqual(
            diagnostic["restored"]["grouped_kind_counts"],
            [{"count": 10, "kind": "column"}, {"count": 10, "kind": "relation"}],
        )
        for category in ("before_only", "after_only", "contract_mismatches"):
            with self.subTest(category=category):
                change = diagnostic["changes"][category]
                self.assertEqual(change["count"], 10)
                self.assertEqual(len(change["samples"]), 8)
                self.assertTrue(change["truncated"])
        before_sample = diagnostic["changes"]["before_only"]["samples"][0]
        self.assertLessEqual(
            len(before_sample["identity"]),
            POSTGRES_STORE_TEST.MANIFEST_DIAGNOSTIC_IDENTITY_LIMIT,
        )
        self.assertEqual(set(before_sample), {"identity", "kind"})
        mismatch_sample = diagnostic["changes"]["contract_mismatches"]["samples"][0]
        self.assertNotEqual(
            mismatch_sample["before_redacted_sha256"],
            mismatch_sample["after_redacted_sha256"],
        )
        self.assertEqual(set(mismatch_sample), {
            "after_redacted_sha256",
            "before_redacted_sha256",
            "identity",
            "kind",
        })

    def test_restore_parity_samples_sort_only_after_redaction(self):
        markers = ("zzz-secret", "aaa-secret")
        changes = []
        for marker in markers:
            source_only = [
                ["relation", [marker], json.dumps(["constant"])]
            ] + [
                ["relation", [f"stable-{index}"], json.dumps(["constant"])]
                for index in range(8)
            ]
            _, _, _, result = self.restore_parity_failure(
                source_only + [["column", ["shared"], json.dumps([marker])]],
                [["column", ["shared"], json.dumps(["restored"])]],
                secret_markers=markers,
            )
            changes.append(result["changes"])

        self.assertEqual(changes[0], changes[1])
        serialized = json.dumps(changes, sort_keys=True)
        for marker in markers:
            self.assertNotIn(marker, serialized)

    def test_authority_capture_requires_both_complete_restore_edges(self):
        self.assertEqual(POSTGRES_STORE_TEST.AUTHORITY_CAPTURE_RESTORE_EDGES, (
            ("source_to_restored_once", "source", "restored_once"),
            ("restored_once_to_restored_twice", "restored_once", "restored_twice"),
        ))
        manifests = {"schema": "[]", "authority": "[]"}
        expected = {
            "schema_manifest": True,
            "configured_authority_manifest": True,
            "migration_ledger": True,
            "semantic_state": True,
            "runtime_authority_shape": True,
            "populated_fixture": True,
        }
        for checkpoint, before, after in (
            POSTGRES_STORE_TEST.AUTHORITY_CAPTURE_RESTORE_EDGES
        ):
            with self.subTest(checkpoint=checkpoint):
                evidence = POSTGRES_STORE_TEST.restore_edge_evidence(
                    checkpoint,
                    manifests,
                    manifests,
                    before_ledger=[{"version": 20}],
                    after_ledger=[{"version": 20}],
                    before_semantic_state=[],
                    after_semantic_state=[],
                    before_runtime_authority=runtime_authority(before),
                    after_runtime_authority=runtime_authority(after),
                    before_population={"account_id": "stable"},
                    after_population={"account_id": "stable"},
                    secret_markers=("xy1300-secret",),
                )
                self.assertEqual(evidence, expected)

    def test_semantic_authority_failure_reports_only_bounded_predicate_names(self):
        document = envelope(component(available=True, complete=True, manifest="[]"))
        for predicate in document["semantic_authority"]["predicates"]:
            if predicate["name"] in {"schema_usage", "exact_table_authority"}:
                predicate["passed"] = False

        with self.assertRaises(POSTGRES_STORE_TEST.TestFailure) as failure:
            POSTGRES_STORE_TEST.require_capture_semantic_authority(
                document, "source", secret_markers=("xy1300-secret",)
            )

        message = str(failure.exception)
        diagnostic = json.loads(message[message.index("{") :])
        self.assertEqual(diagnostic, {
            "checkpoint": "source",
            "failed_predicates": ["exact_table_authority", "schema_usage"],
            "predicate_count": len(POSTGRES_STORE_TEST.SEMANTIC_AUTHORITY_PREDICATES),
            "schema": POSTGRES_STORE_TEST.SEMANTIC_AUTHORITY_DIAGNOSTIC_SCHEMA,
        })
        self.assertNotIn(DATABASE, message)

    def test_semantic_authority_evidence_requires_the_canonical_predicate_order(self):
        document = envelope(component(available=True, complete=True, manifest="[]"))
        predicates = document["semantic_authority"]["predicates"]
        predicates[0], predicates[1] = predicates[1], predicates[0]

        with self.assertRaisesRegex(
            POSTGRES_STORE_TEST.TestFailure, "predicate contract differs"
        ):
            POSTGRES_STORE_TEST.validate_semantic_authority_evidence(
                document["semantic_authority"]
            )

    def test_private_receipt_read_is_descriptor_pinned_and_bounded(self):
        with tempfile.TemporaryDirectory(
            dir=Path(tempfile.gettempdir()).resolve()
        ) as directory:
            parent = Path(directory)
            parent.chmod(0o700)
            receipt = parent / "candidate.json"
            payload = b'{"schema":"candidate"}\n'
            receipt.write_bytes(payload)
            receipt.chmod(0o600)

            observed, digest = POSTGRES_STORE_TEST.read_private_authority_receipt(
                receipt
            )
            self.assertEqual(observed, payload)
            self.assertEqual(digest, hashlib.sha256(payload).hexdigest())

            parent.chmod(0o755)
            with self.assertRaisesRegex(
                POSTGRES_STORE_TEST.TestFailure, "parent is not operator-private"
            ):
                POSTGRES_STORE_TEST.read_private_authority_receipt(receipt)
            parent.chmod(0o700)

            receipt.write_bytes(
                b"x" * (POSTGRES_STORE_TEST.AUTHORITY_CANDIDATE_RECEIPT_MAX_BYTES + 1)
            )
            with self.assertRaisesRegex(
                POSTGRES_STORE_TEST.TestFailure, "file metadata is invalid"
            ):
                POSTGRES_STORE_TEST.read_private_authority_receipt(receipt)

            if hasattr(os, "O_NOFOLLOW"):
                receipt.unlink()
                target = parent / "target.json"
                target.write_bytes(payload)
                target.chmod(0o600)
                receipt.symlink_to(target.name)
                with self.assertRaisesRegex(
                    POSTGRES_STORE_TEST.TestFailure, "could not be read safely"
                ):
                    POSTGRES_STORE_TEST.read_private_authority_receipt(receipt)

    def test_phase_a_receipt_rejects_duplicate_json_without_echoing_input(self):
        with tempfile.TemporaryDirectory(
            dir=Path(tempfile.gettempdir()).resolve()
        ) as directory:
            parent = Path(directory)
            parent.chmod(0o700)
            receipt = parent / "candidate.json"
            receipt.write_bytes(b'{"private-payload":1,"private-payload":2}\n')
            receipt.chmod(0o600)

            with self.assertRaises(POSTGRES_STORE_TEST.TestFailure) as failure:
                POSTGRES_STORE_TEST.load_phase_a_authority_receipt(receipt)

            self.assertEqual(str(failure.exception), "Phase A receipt is malformed")
            self.assertNotIn("private-payload", str(failure.exception))

    def test_phase_a_binding_requires_a_real_commit_with_the_exact_tree(self):
        binding = POSTGRES_STORE_TEST.frozen_source_binding()
        POSTGRES_STORE_TEST.require_commit_tree_binding(binding)

        with self.assertRaisesRegex(
            POSTGRES_STORE_TEST.TestFailure, "commit binding is invalid"
        ):
            POSTGRES_STORE_TEST.require_commit_tree_binding({
                "head": binding["tree"],
                "tree": binding["tree"],
            })
        with self.assertRaisesRegex(
            POSTGRES_STORE_TEST.TestFailure, "commit tree binding differs"
        ):
            POSTGRES_STORE_TEST.require_commit_tree_binding({
                "head": binding["head"],
                "tree": "0" * 40,
            })

    def test_git_lineage_reads_are_bounded_timed_and_strictly_decoded(self):
        with self.assertRaisesRegex(
            POSTGRES_STORE_TEST.TestFailure, "Git source lineage is unavailable"
        ):
            POSTGRES_STORE_TEST.git_read_bytes(
                "rev-parse", "HEAD", byte_limit=1
            )
        class PendingGitProcess:
            def __init__(self):
                self.stdout = tempfile.TemporaryFile()
                self.returncode = None
                self.killed = False
                self.reaped = False

            def poll(self):
                return self.returncode

            def kill(self):
                self.killed = True

            def wait(self, timeout=None):
                self.assert_no_timeout(timeout)
                self.reaped = True
                self.returncode = -9
                return self.returncode

            @staticmethod
            def assert_no_timeout(timeout):
                if timeout is not None:
                    raise AssertionError("reap must be definitive")

        pending = PendingGitProcess()
        with mock.patch.object(
            POSTGRES_STORE_TEST.subprocess, "Popen", return_value=pending
        ), mock.patch.object(
            POSTGRES_STORE_TEST.select, "select", return_value=([], [], [])
        ):
            with self.assertRaisesRegex(
                POSTGRES_STORE_TEST.TestFailure, "Git source lineage is unavailable"
            ):
                POSTGRES_STORE_TEST.git_read_bytes(
                    "rev-parse", "HEAD", byte_limit=1024
                )
        self.assertTrue(pending.killed)
        self.assertTrue(pending.reaped)
        self.assertEqual(pending.returncode, -9)
        with mock.patch.object(
            POSTGRES_STORE_TEST, "git_read_bytes", return_value=b"\xff"
        ):
            with self.assertRaisesRegex(
                POSTGRES_STORE_TEST.TestFailure, "Git source lineage is unavailable"
            ):
                POSTGRES_STORE_TEST.git_read_text(
                    "rev-parse", "HEAD", byte_limit=1024
                )

    def test_phase_b_requires_the_direct_single_parent_transition(self):
        phase_a = "a" * 40
        phase_b = "b" * 40
        POSTGRES_STORE_TEST.require_direct_parent_lineage(
            phase_a, (phase_a,)
        )
        for invalid in (
            ("c" * 40,),
            (phase_a, "c" * 40),
            (phase_b,),
        ):
            with self.assertRaisesRegex(
                POSTGRES_STORE_TEST.TestFailure,
                "Phase B source commit lineage is invalid",
            ):
                POSTGRES_STORE_TEST.require_direct_parent_lineage(
                    phase_a, invalid
                )

    def test_phase_b_source_delta_allows_only_both_authorized_digest_arrays(self):
        def source(schema_byte, authority_byte, suffix=""):
            schema = ", ".join([f"0x{schema_byte:02x}"] * 32)
            authority = ", ".join([f"0x{authority_byte:02x}"] * 32)
            return (
                f"const SCHEMA_CONTRACT_SHA256: [u8; 32] = [{schema}];\n"
                "middle\n"
                f"const CONFIGURED_AUTHORITY_SHA256: [u8; 32] = [{authority}];\n"
                + suffix
            )

        mismatches = [
            {
                "component": "schema",
                "expected_sha256": "00" * 32,
                "actual_sha256": "aa" * 32,
            },
            {
                "component": "configured_authority",
                "expected_sha256": "11" * 32,
                "actual_sha256": "bb" * 32,
            },
        ]
        before = source(0x00, 0x11)
        after = source(0xAA, 0xBB)
        self.assertEqual(POSTGRES_STORE_TEST.digest_constants_from_source(after), {
            "schema": "aa" * 32,
            "configured_authority": "bb" * 32,
        })
        POSTGRES_STORE_TEST.require_digest_only_authority_source(
            before, after, mismatches
        )
        POSTGRES_STORE_TEST.require_phase_b_changed_paths([
            "crates/decodex-postgres/src/authority.rs"
        ])

        with self.assertRaises(POSTGRES_STORE_TEST.TestFailure):
            POSTGRES_STORE_TEST.require_digest_only_authority_source(
                before, after + "unauthorized source delta\n", mismatches
            )
        with self.assertRaises(POSTGRES_STORE_TEST.TestFailure):
            POSTGRES_STORE_TEST.require_phase_b_changed_paths([
                "crates/decodex-postgres/src/authority.rs",
                "scripts/vnext/postgres_store_test.py",
            ])

        hex_comment = ", ".join(["0xaa"] * 32)
        counterexample = after.replace(
            "const SCHEMA_CONTRACT_SHA256: [u8; 32] = ["
            + ", ".join(["0xaa"] * 32)
            + "];",
            "const SCHEMA_CONTRACT_SHA256: [u8; 32] = "
            f"[0; 32 /* {hex_comment} */];",
        )
        with self.assertRaises(POSTGRES_STORE_TEST.TestFailure):
            POSTGRES_STORE_TEST.digest_constants_from_source(counterexample)
        with self.assertRaises(POSTGRES_STORE_TEST.TestFailure):
            POSTGRES_STORE_TEST.normalized_digest_source(counterexample)

    def test_phase_fields_distinguish_derivation_from_acceptance(self):
        phase_b_binding = {"head": "c" * 40, "tree": "d" * 40}
        derivation = POSTGRES_STORE_TEST.authority_candidate_phase_fields(
            None, SOURCE_BINDING, SOURCE_BINDING
        )
        self.assertEqual(derivation, {
            "acceptance": False,
            "acceptance_lineage": {
                "phase_a_receipt_sha256": None,
                "phase_a_source_binding": SOURCE_BINDING,
                "phase_b_source_binding": None,
            },
        })

        phase_a = POSTGRES_STORE_TEST.PhaseAAuthorityReceipt(
            {"source_binding": {"end": SOURCE_BINDING}}, "e" * 64
        )
        acceptance = POSTGRES_STORE_TEST.authority_candidate_phase_fields(
            phase_a, phase_b_binding, phase_b_binding
        )
        self.assertTrue(acceptance["acceptance"])
        self.assertEqual(acceptance["acceptance_lineage"], {
            "phase_a_receipt_sha256": "e" * 64,
            "phase_a_source_binding": SOURCE_BINDING,
            "phase_b_source_binding": {
                "start": phase_b_binding,
                "end": phase_b_binding,
            },
        })

    def test_second_restore_mismatch_names_only_the_bounded_checkpoint(self):
        before = {
            "schema": json.dumps([
                ["relation", ["xy1300-secret"], json.dumps(["before"])]
            ], separators=(",", ":")),
            "authority": "[]",
        }
        after = {
            "schema": json.dumps([
                ["relation", ["xy1300-secret"], json.dumps(["after"])]
            ], separators=(",", ":")),
            "authority": "[]",
        }

        with self.assertRaises(POSTGRES_STORE_TEST.TestFailure) as failure:
            POSTGRES_STORE_TEST.restore_edge_evidence(
                "restored_once_to_restored_twice",
                before,
                after,
                before_ledger=[{"version": 20}],
                after_ledger=[{"version": 20}],
                before_semantic_state=[],
                after_semantic_state=[],
                before_runtime_authority=runtime_authority("before"),
                after_runtime_authority=runtime_authority("after"),
                before_population={"account_id": "stable"},
                after_population={"account_id": "stable"},
                secret_markers=("xy1300-secret",),
            )

        message = str(failure.exception)
        self.assertIn("restored_once_to_restored_twice", message)
        self.assertNotIn("xy1300-secret", message)

    def test_constraint_mismatches_label_all_definition_changes_before_sampling(self):
        expected_fields = (
            "constraint_type", "definition", "deferrable", "deferred",
            "validated", "enforced", "update_action", "delete_action",
            "match_type", "is_local", "inheritance_count", "no_inherit",
            "source_columns", "referenced_columns", "referenced_namespace",
            "referenced_relation",
        )
        self.assertEqual(
            POSTGRES_STORE_TEST.CONSTRAINT_CONTRACT_FIELDS, expected_fields
        )
        markers = ("first-definition-secret", "second-definition-secret")
        changes = []
        for marker in markers:
            source = [
                [
                    "constraint", ["decodex", "runs", f"constraint-{index}"],
                    constraint_contract(f"before definition {index} {marker}"),
                ]
                for index in range(9)
            ]
            restored = [
                [
                    "constraint", ["decodex", "runs", f"constraint-{index}"],
                    constraint_contract(f"after definition {index} {marker}"),
                ]
                for index in range(9)
            ]
            _, _, _, result = self.restore_parity_failure(
                source, restored, secret_markers=markers
            )
            changes.append(result["changes"])

        self.assertEqual(changes[0], changes[1])
        mismatch = changes[0]["contract_mismatches"]
        self.assertEqual(mismatch["count"], 9)
        self.assertEqual(len(mismatch["samples"]), 8)
        self.assertTrue(mismatch["truncated"])
        self.assertEqual(mismatch["constraint_field_change_counts"], [
            {"count": 9, "field": "definition"}
        ])
        for sample in mismatch["samples"]:
            self.assertEqual(set(sample), {"changed_fields", "identity", "kind"})
            self.assertEqual(len(sample["changed_fields"]), 1)
            definition = sample["changed_fields"][0]
            self.assertEqual(set(definition), {
                "after_sha256",
                "after_utf8_byte_length",
                "before_sha256",
                "before_utf8_byte_length",
                "common_prefix_utf8_byte_length",
                "field",
            })
            self.assertEqual(definition["field"], "definition")
        serialized = json.dumps(changes, sort_keys=True)
        self.assertNotIn("before definition", serialized)
        self.assertNotIn("after definition", serialized)
        for marker in markers:
            self.assertNotIn(marker, serialized)

    def test_constraint_nondefinition_change_surfaces_bounded_semantic_values(self):
        source = [[
            "constraint", ["decodex", "runs", "runs_fk"],
            constraint_contract("FOREIGN KEY (...) REFERENCES ...", validated=True),
        ]]
        restored = [[
            "constraint", ["decodex", "runs", "runs_fk"],
            constraint_contract("FOREIGN KEY (...) REFERENCES ...", validated=False),
        ]]

        _, _, _, diagnostic = self.restore_parity_failure(source, restored)
        mismatch = diagnostic["changes"]["contract_mismatches"]

        self.assertEqual(mismatch["constraint_field_change_counts"], [
            {"count": 1, "field": "validated"}
        ])
        self.assertEqual(mismatch["samples"][0]["changed_fields"], [{
            "after": "false",
            "before": "true",
            "field": "validated",
        }])
        self.assertNotIn("FOREIGN KEY", json.dumps(mismatch, sort_keys=True))

    def test_restore_parity_dependency_only_samples_include_null_reference(self):
        source = dependency_row(
            "dependency", ["source"], "i", ["trigger"], None, False
        )
        restored = dependency_row(
            "dependency", ["restored"], "n", ["namespace"],
            ["namespace", "decodex"], True,
        )

        _, _, _, diagnostic = self.restore_parity_failure([source], [restored])
        before = diagnostic["changes"]["before_only"]["samples"][0]
        after = diagnostic["changes"]["after_only"]["samples"][0]

        self.assertEqual(before["reference_key"], "null")
        self.assertFalse(before["resolved"])
        self.assertEqual(after["reference_key"], '["namespace","decodex"]')
        self.assertTrue(after["resolved"])

    def test_malformed_constraint_contract_keeps_parity_classification(self):
        malformed_contract = json.dumps([None] * 15, separators=(",", ":"))
        malformed = [["constraint", ["invalid"], malformed_contract]]
        restored = [[
            "constraint", ["invalid"], constraint_contract("valid definition")
        ]]

        _, _, message, diagnostic = self.restore_parity_failure(malformed, restored)

        self.assertTrue(message.startswith(
            "authority candidate restore parity diagnostic: {"
        ))
        self.assertEqual(diagnostic, {
            "classification": "diagnostic_unavailable",
            "schema": POSTGRES_STORE_TEST.RESTORE_PARITY_DIAGNOSTIC_SCHEMA,
        })

    def test_restore_parity_dependency_mismatch_decodes_only_semantic_fields(self):
        source = dependency_row(
            "dependency",
            ["constraint", "xy1300-secret"],
            "i",
            ["trigger"],
            ["trigger", ["user_trigger", ["decodex", "runs", "r"], "stable"]],
            True,
        )
        restored = [source[0], source[1], json.dumps([False])]

        _, _, _, diagnostic = self.restore_parity_failure([source], [restored])
        serialized = json.dumps(diagnostic, sort_keys=True)
        change = diagnostic["changes"]["contract_mismatches"]
        sample = change["samples"][0]

        self.assertNotIn("xy1300-secret", serialized)
        self.assertEqual(change["count"], 1)
        self.assertFalse(change["truncated"])
        self.assertEqual(set(sample), {
            "after_resolved",
            "before_resolved",
            "dependency_type",
            "reference_class",
            "reference_key",
            "source_identity",
            "source_kind",
        })
        self.assertEqual(sample["source_kind"], "constraint")
        self.assertEqual(sample["source_identity"], '["constraint","[REDACTED]"]')
        self.assertEqual(sample["dependency_type"], "i")
        self.assertEqual(sample["reference_class"], '["trigger"]')
        self.assertEqual(
            sample["reference_key"],
            '["trigger",["user_trigger",["decodex","runs","r"],"stable"]]',
        )
        self.assertTrue(sample["before_resolved"])
        self.assertFalse(sample["after_resolved"])
        self.assertNotIn("contract", sample)

    def test_malformed_artifact_diagnostic_is_bounded_and_does_not_echo_bytes(self):
        artifact = b'{"secret":"xy1300-secret"}'
        result = diagnostic(
            artifact_classification="artifact_malformed",
            artifact_bytes=artifact,
            artifact_error="invalid JSON xy1300-secret " + "parser text " * 100,
        )
        serialized = json.dumps(result, sort_keys=True)
        evidence = result["artifact"]

        self.assertNotIn("xy1300-secret", serialized)
        self.assertNotIn("secret", serialized)
        self.assertEqual(evidence["classification"], "artifact_malformed")
        self.assertEqual(evidence["byte_length"], len(artifact))
        self.assertEqual(evidence["sha256"], hashlib.sha256(artifact).hexdigest())
        self.assertTrue(evidence["error"]["truncated"])

    def test_incorrect_or_empty_binding_is_a_redacted_malformed_artifact(self):
        for binding in ({}, {"requested": "xy1300-secret-wrong-database"}):
            with self.subTest(binding=binding):
                artifact, message, result = self.artifact_failure(envelope(
                    component(available=True, complete=True, manifest="[]"),
                    binding=binding,
                ))
                self.assertNotIn("xy1300-secret", message)
                self.assertEqual(
                    result["artifact"]["classification"], "artifact_malformed"
                )
                self.assertEqual(result["artifact"]["byte_length"], len(artifact))
                self.assertEqual(
                    result["artifact"]["sha256"], hashlib.sha256(artifact).hexdigest()
                )

    def test_unrecognized_or_extra_field_error_is_a_redacted_malformed_artifact(self):
        valid_error = {
            "classification": "database_error",
            "message": "permission denied",
            "message_truncated": False,
            "schema": POSTGRES_STORE_TEST.MANIFEST_QUERY_ERROR_SCHEMA,
            "sqlstate": "42501",
        }
        errors = [
            "xy1300-secret-legacy-error",
            json.dumps({**valid_error, "extra": "xy1300-secret-extra"}),
            json.dumps({**valid_error, "classification": "xy1300-secret-class"}),
            json.dumps({**valid_error, "message": ""}),
            json.dumps({**valid_error, "message_truncated": "xy1300-secret"}),
            json.dumps({**valid_error, "sqlstate": "xy1300-secret"}),
        ]
        for error in errors:
            with self.subTest(error=error):
                _, message, result = self.artifact_failure(envelope(component(
                    available=False, complete=False, error=error
                )))
                self.assertNotIn("xy1300-secret", message)
                self.assertEqual(
                    result["artifact"]["classification"], "artifact_malformed"
                )

    def test_component_failure_shape_contains_only_the_source_checkpoint(self):
        result = diagnostic(
            document=envelope(component(
                available=True, complete=False, manifest="[]"
            )),
            components=("schema",),
        )

        self.assertEqual(result["checkpoint"]["name"], "source")
        self.assertEqual(result["checkpoint"]["expected_database"], DATABASE)
        self.assertEqual(result["source_binding"], SOURCE_BINDING)
        self.assertEqual([item["component"] for item in result["components"]], ["schema"])
        self.assertNotIn("restored", json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    unittest.main()
