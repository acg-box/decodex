import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import time
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
FIXTURE_SEMANTIC_AUTHORITY_DEFINITION = {
    "schema": POSTGRES_STORE_TEST.SEMANTIC_AUTHORITY_DEFINITION_SCHEMA,
    "predicates": [
        {"name": "fixture_unsafe", "classification": "unsafe"},
        {"name": "fixture_incompatible", "classification": "incompatible"},
    ],
}
FIXTURE_SEMANTIC_AUTHORITY_FINGERPRINT = (
    POSTGRES_STORE_TEST.semantic_authority_definition_fingerprint(
        FIXTURE_SEMANTIC_AUTHORITY_DEFINITION
    )
)


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
            "definition": json.loads(json.dumps(
                FIXTURE_SEMANTIC_AUTHORITY_DEFINITION
            )),
            "fingerprint": FIXTURE_SEMANTIC_AUTHORITY_FINGERPRINT,
            "observations": [
                {"name": predicate["name"], "passed": True}
                for predicate in FIXTURE_SEMANTIC_AUTHORITY_DEFINITION["predicates"]
            ],
            "schema": POSTGRES_STORE_TEST.SEMANTIC_AUTHORITY_SCHEMA,
        },
        "sequence_state": [],
    }


def semantic_authority_evidence(predicates, passed):
    if len(predicates) != len(passed):
        raise ValueError("semantic authority fixture cardinality differs")
    definition = {
        "predicates": json.loads(json.dumps(predicates)),
        "schema": POSTGRES_STORE_TEST.SEMANTIC_AUTHORITY_DEFINITION_SCHEMA,
    }
    return {
        "definition": definition,
        "fingerprint": (
            POSTGRES_STORE_TEST.semantic_authority_definition_fingerprint(definition)
        ),
        "observations": [
            {"name": predicate["name"], "passed": observation}
            for predicate, observation in zip(predicates, passed)
        ],
        "schema": POSTGRES_STORE_TEST.SEMANTIC_AUTHORITY_SCHEMA,
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
    def setUp(self):
        self.supported_fingerprint = mock.patch.object(
            POSTGRES_STORE_TEST,
            "SUPPORTED_SEMANTIC_AUTHORITY_FINGERPRINT",
            FIXTURE_SEMANTIC_AUTHORITY_FINGERPRINT,
        )

        self.supported_fingerprint.start()
        self.addCleanup(self.supported_fingerprint.stop)

    def test_bootstrap_config_rejects_overlong_local_transport_path(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / ("x" * 104)

            with self.assertRaisesRegex(
                POSTGRES_STORE_TEST.TestFailure,
                "Decodex local transport Unix socket path is too long",
            ):
                POSTGRES_STORE_TEST.write_bootstrap_config(
                    root,
                    Path(temporary) / "socket",
                    54_321,
                    "decodex_test",
                    "decodex_migration",
                    "decodex_runtime",
                )

            self.assertFalse(root.exists())

    def test_secret_logging_reader_consumes_coalesced_pipe_frames(self):
        read_descriptor, write_descriptor = os.pipe()
        try:
            expected = (
                "panic|panic|none|off|-1|-1|0|0|0|0|off|off|off|off|off|off|off|off|stderr",
                "XY1272_SECRET_LOGGING_READY",
            )
            payload = ("\n".join(expected) + "\n").encode("ascii")
            self.assertEqual(os.write(write_descriptor, payload), len(payload))
            os.close(write_descriptor)
            write_descriptor = -1

            frames = POSTGRES_STORE_TEST._read_bounded_secret_logging_frames(
                read_descriptor, deadline=time.monotonic() + 1.0
            )
        finally:
            os.close(read_descriptor)
            if write_descriptor >= 0:
                os.close(write_descriptor)

        self.assertEqual(frames, expected)

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

    def candidate_failure(
        self,
        document,
        *,
        checkpoint="source",
        database=DATABASE,
        source_binding=SOURCE_BINDING,
        secret_markers=("xy1300-secret",),
    ):
        artifact = json.dumps(document, separators=(",", ":")).encode()
        with self.assertRaises(POSTGRES_STORE_TEST.TestFailure) as failure:
            POSTGRES_STORE_TEST.parse_candidate_capture_manifest(
                artifact,
                checkpoint,
                database,
                source_binding=source_binding,
                secret_markers=secret_markers,
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

    def test_candidate_semantic_failure_is_complete_ordered_canonical_and_redacted(self):
        predicates = [
            {"name": "zeta_unsafe", "classification": "unsafe"},
            {
                "name": "alpha_conditional",
                "classification": "unsafe_if_excess_otherwise_incompatible",
            },
            {"name": "middle_incompatible", "classification": "incompatible"},
        ]
        private_database = "decodex_private_database"
        private_error = "private-error-text"
        private_sql = "SELECT private_catalog_value"
        private_path = "/private/authority-candidate.json"
        document = envelope(
            component(
                available=True,
                complete=True,
                manifest=json.dumps([
                    [
                        "relation",
                        ["private_role", private_path],
                        json.dumps([private_sql]),
                    ],
                ]),
            ),
            authority=component(
                available=False,
                complete=False,
                error=json.dumps({
                    "classification": "database_error",
                    "message": private_error,
                    "message_truncated": False,
                    "schema": POSTGRES_STORE_TEST.MANIFEST_QUERY_ERROR_SCHEMA,
                    "sqlstate": "42501",
                }),
            ),
            binding={
                key: private_database
                for key in (
                    "requested",
                    "migration_url",
                    "runtime_url",
                    "observed_migration",
                    "observed_runtime",
                )
            },
        )
        document["sequence_state"] = [{
            "actual_count": 41,
            "credential": "private-credential",
            "expected_count": 40,
        }]
        document["semantic_authority"] = semantic_authority_evidence(
            predicates, [False, False, True]
        )
        fingerprint = document["semantic_authority"]["fingerprint"]

        with mock.patch.object(
            POSTGRES_STORE_TEST,
            "SUPPORTED_SEMANTIC_AUTHORITY_FINGERPRINT",
            fingerprint,
        ):
            _, message, result = self.candidate_failure(
                document,
                database=private_database,
                secret_markers=(
                    private_database,
                    private_error,
                    private_sql,
                    private_path,
                    "private-credential",
                ),
            )

        expected = {
            "checkpoint": "source",
            "definition_fingerprint": fingerprint,
            "failures": [
                {"failure_policy": "unsafe", "predicate": "zeta_unsafe"},
                {
                    "failure_policy": (
                        "unsafe_if_excess_otherwise_incompatible"
                    ),
                    "predicate": "alpha_conditional",
                },
            ],
            "schema": "decodex/postgres-semantic-authority-diagnostic/2",
            "source_binding": SOURCE_BINDING,
        }
        serialized = json.dumps(
            expected, sort_keys=True, separators=(",", ":"), ensure_ascii=True
        )
        self.assertEqual(
            message,
            "authority candidate semantic diagnostic: " + serialized,
        )
        self.assertEqual(result, expected)
        self.assertEqual(set(result), {
            "checkpoint",
            "definition_fingerprint",
            "failures",
            "schema",
            "source_binding",
        })
        self.assertTrue(all(
            set(failure) == {"failure_policy", "predicate"}
            for failure in result["failures"]
        ))
        for forbidden in (
            "middle_incompatible",
            private_database,
            private_error,
            private_sql,
            private_path,
            "private_role",
            "private-credential",
            "actual_count",
            "expected_count",
            "concrete_class",
            "all_passed",
            "evidence_sha256",
            "predicate_count",
            "row_count",
            "observations",
            "manifest",
            "actual_sha256",
            "expected_sha256",
            "candidate_digest_mismatch",
        ):
            self.assertNotIn(forbidden, message)

    def test_candidate_semantic_malformed_inputs_never_emit_failure_identities(self):
        def false_document():
            document = envelope(
                component(available=True, complete=True, manifest="[]")
            )
            document["semantic_authority"]["observations"][0]["passed"] = False
            return document

        def unsupported_definition(document):
            predicates = [
                *document["semantic_authority"]["definition"]["predicates"],
                {"name": "attempted_extra", "classification": "unsafe"},
            ]
            document["semantic_authority"] = semantic_authority_evidence(
                predicates, [False, True, True]
            )

        mutations = {
            "shape": lambda document: document.__setitem__("extra", True),
            "wrong_schema": lambda document: document["semantic_authority"].__setitem__(
                "schema", "decodex/attempted-semantic-authority/99"
            ),
            "wrong_definition_schema": (
                lambda document: document["semantic_authority"]["definition"].__setitem__(
                    "schema", "decodex/attempted-definition/99"
                )
            ),
            "wrong_emitted_fingerprint": (
                lambda document: document["semantic_authority"].__setitem__(
                    "fingerprint", "0" * 64
                )
            ),
            "unsupported_recomputed_fingerprint": unsupported_definition,
            "invalid_name": (
                lambda document: document["semantic_authority"]["definition"][
                    "predicates"
                ][0].__setitem__("name", "attempted-name")
            ),
            "unknown_policy": (
                lambda document: document["semantic_authority"]["definition"][
                    "predicates"
                ][0].__setitem__("classification", "attempted_policy")
            ),
            "missing_observation": (
                lambda document: document["semantic_authority"][
                    "observations"
                ].pop()
            ),
            "extra_observation": (
                lambda document: document["semantic_authority"][
                    "observations"
                ].append({"name": "attempted_extra", "passed": True})
            ),
            "duplicate_observation": (
                lambda document: document["semantic_authority"][
                    "observations"
                ].__setitem__(
                    1,
                    {
                        "name": document["semantic_authority"]["observations"][0][
                            "name"
                        ],
                        "passed": True,
                    },
                )
            ),
            "reordered_observation": (
                lambda document: document["semantic_authority"][
                    "observations"
                ].reverse()
            ),
            "non_boolean": (
                lambda document: document["semantic_authority"]["observations"][
                    0
                ].__setitem__("passed", 1)
            ),
            "database_binding": (
                lambda document: document["binding"].__setitem__(
                    "requested", "attempted_database"
                )
            ),
        }
        for case, mutate in mutations.items():
            with self.subTest(case=case):
                document = false_document()
                mutate(document)
                _, message, result = self.candidate_failure(document)
                self.assertEqual(
                    result["artifact"]["classification"], "artifact_malformed"
                )
                self.assertNotIn("failures", message)
                self.assertNotIn("fixture_unsafe", message)
                self.assertNotIn("attempted", message)

        for case, checkpoint, source_binding in (
            ("invalid_checkpoint", "restored", SOURCE_BINDING),
            (
                "invalid_source_binding",
                "source",
                {"head": "A" * 40, "tree": "b" * 40},
            ),
        ):
            with self.subTest(case=case):
                _, message, result = self.candidate_failure(
                    false_document(),
                    checkpoint=checkpoint,
                    source_binding=source_binding,
                )
                self.assertEqual(
                    result["artifact"]["classification"], "artifact_malformed"
                )
                self.assertNotIn("failures", message)
                self.assertNotIn("fixture_unsafe", message)
                self.assertNotIn(checkpoint, message)

    def test_candidate_semantic_serialization_and_redaction_fail_closed(self):
        document = envelope(component(available=True, complete=True, manifest="[]"))
        failed_predicate = document["semantic_authority"]["observations"][0]["name"]
        document["semantic_authority"]["observations"][0]["passed"] = False

        with mock.patch.object(
            POSTGRES_STORE_TEST,
            "_serialize_semantic_authority_diagnostic",
            side_effect=TypeError("attempted serialization detail"),
        ):
            _, serialization_message, serialization_result = self.candidate_failure(
                document
            )
        _, redaction_message, redaction_result = self.candidate_failure(
            document, secret_markers=(failed_predicate,)
        )

        for message, result in (
            (serialization_message, serialization_result),
            (redaction_message, redaction_result),
        ):
            self.assertEqual(
                result["artifact"]["classification"], "artifact_malformed"
            )
            self.assertNotIn("failures", message)
            self.assertNotIn(failed_predicate, message)
            self.assertNotIn("attempted serialization detail", message)

    def test_candidate_semantic_all_true_path_preserves_summary(self):
        document = envelope(component(available=True, complete=True, manifest="[]"))
        artifact = json.dumps(document, separators=(",", ":")).encode()

        parsed, summary = POSTGRES_STORE_TEST.parse_candidate_capture_manifest(
            artifact,
            "source",
            DATABASE,
            source_binding=SOURCE_BINDING,
            secret_markers=("xy1300-secret",),
        )

        evidence = document["semantic_authority"]
        self.assertEqual(parsed, document)
        self.assertEqual(summary, {
            "all_passed": True,
            "definition_schema": (
                POSTGRES_STORE_TEST.SEMANTIC_AUTHORITY_DEFINITION_SCHEMA
            ),
            "evidence_sha256": hashlib.sha256(json.dumps(
                evidence,
                sort_keys=True,
                separators=(",", ":"),
                ensure_ascii=True,
            ).encode()).hexdigest(),
            "fingerprint": FIXTURE_SEMANTIC_AUTHORITY_FINGERPRINT,
            "observation_count": 2,
            "schema": POSTGRES_STORE_TEST.SEMANTIC_AUTHORITY_SCHEMA,
        })

    def test_shared_retained_title_loader_still_rejects_false_evidence(self):
        document = envelope(component(available=True, complete=True, manifest="[]"))
        document["semantic_authority"]["observations"][0]["passed"] = False
        with tempfile.TemporaryDirectory() as directory:
            artifact_path = Path(directory) / "retained-title.json"
            artifact_path.write_text(json.dumps(document), encoding="utf-8")
            with self.assertRaises(POSTGRES_STORE_TEST.TestFailure) as failure:
                POSTGRES_STORE_TEST.load_capture_manifest(
                    artifact_path,
                    "retained_title",
                    DATABASE,
                    source_binding=SOURCE_BINDING,
                    secret_markers=("xy1300-secret",),
                )

        message = str(failure.exception)
        result = json.loads(message[message.index("{") :])
        self.assertEqual(result["artifact"]["classification"], "artifact_malformed")
        self.assertNotIn("authority candidate semantic diagnostic", message)
        self.assertNotIn("fixture_unsafe", message)

    def test_phase_a_false_semantic_path_stops_before_digest_or_receipt_work(self):
        document = envelope(component(available=True, complete=True, manifest="[]"))
        document["semantic_authority"]["observations"][0]["passed"] = False
        artifact = json.dumps(document, separators=(",", ":")).encode()

        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)

            def write_manifest(path, *_args, **_kwargs):
                path.write_bytes(artifact)

            with mock.patch.multiple(
                POSTGRES_STORE_TEST,
                authority_candidate_phase_fields=mock.DEFAULT,
                authority_manifest_evidence=mock.DEFAULT,
                capture_migration_ledger=mock.DEFAULT,
                capture_runtime_authority=mock.DEFAULT,
                capture_upgrade_anchor_binding=mock.DEFAULT,
                capture_upgrade_runtime_authority=mock.DEFAULT,
                capture_upgrade_type_bindings=mock.DEFAULT,
                capture_zero_grantee_migration_authority=mock.DEFAULT,
                create_database=mock.DEFAULT,
                dump_schema_manifest=mock.DEFAULT,
                frozen_source_binding=mock.DEFAULT,
                provision_runtime=mock.DEFAULT,
                psql=mock.DEFAULT,
                psql_as=mock.DEFAULT,
                run=mock.DEFAULT,
                run_migration=mock.DEFAULT,
                run_migration_through_v24=mock.DEFAULT,
                rust_digest_constant=mock.DEFAULT,
                set_contract_urls=mock.DEFAULT,
            ) as patched:
                patched["frozen_source_binding"].return_value = SOURCE_BINDING
                patched["psql"].return_value = json.dumps({
                    "major": 18,
                    "version": "PostgreSQL 18",
                    "version_num": 180000,
                })
                patched["capture_migration_ledger"].return_value = []
                patched["dump_schema_manifest"].side_effect = write_manifest

                with self.assertRaisesRegex(
                    POSTGRES_STORE_TEST.TestFailure,
                    "^authority candidate semantic diagnostic: ",
                ):
                    POSTGRES_STORE_TEST.run_authority_candidate_capture(
                        Path(directory),
                        5432,
                        work,
                        work / "postgres.log",
                        {},
                        (),
                    )

                self.assertEqual(
                    patched["frozen_source_binding"].call_count, 1
                )
                patched["rust_digest_constant"].assert_not_called()
                patched["authority_manifest_evidence"].assert_not_called()
                patched["authority_candidate_phase_fields"].assert_not_called()
                patched["run"].assert_not_called()

    def test_semantic_authority_definition_fingerprint_is_independently_recomputed(self):
        document = envelope(component(available=True, complete=True, manifest="[]"))
        observations = POSTGRES_STORE_TEST.validate_semantic_authority_evidence(
            document["semantic_authority"]
        )
        self.assertEqual(
            [observation["name"] for observation in observations],
            ["fixture_unsafe", "fixture_incompatible"],
        )

        document["semantic_authority"]["fingerprint"] = "0" * 64
        with self.assertRaisesRegex(
            POSTGRES_STORE_TEST.TestFailure, "emitted fingerprint differs"
        ):
            POSTGRES_STORE_TEST.validate_semantic_authority_evidence(
                document["semantic_authority"]
            )

    def test_semantic_authority_definition_must_match_the_supported_fingerprint(self):
        document = envelope(component(available=True, complete=True, manifest="[]"))
        with mock.patch.object(
            POSTGRES_STORE_TEST,
            "SUPPORTED_SEMANTIC_AUTHORITY_FINGERPRINT",
            "f" * 64,
        ):
            with self.assertRaisesRegex(
                POSTGRES_STORE_TEST.TestFailure, "definition is not supported"
            ):
                POSTGRES_STORE_TEST.validate_semantic_authority_evidence(
                    document["semantic_authority"]
                )

    def test_semantic_authority_evidence_rejects_reordered_observations(self):
        document = envelope(component(available=True, complete=True, manifest="[]"))
        observations = document["semantic_authority"]["observations"]
        observations[0], observations[1] = observations[1], observations[0]

        with self.assertRaisesRegex(
            POSTGRES_STORE_TEST.TestFailure, "observation order differs"
        ):
            POSTGRES_STORE_TEST.validate_semantic_authority_evidence(
                document["semantic_authority"]
            )

    def test_semantic_authority_evidence_rejects_missing_duplicate_and_extra_observations(self):
        mutations = {
            "missing": lambda observations: observations.pop(),
            "duplicate": lambda observations: observations.__setitem__(
                1, {"name": observations[0]["name"], "passed": True}
            ),
            "extra": lambda observations: observations.append(
                {"name": "fixture_extra", "passed": True}
            ),
        }
        for case, mutate in mutations.items():
            with self.subTest(case=case):
                document = envelope(
                    component(available=True, complete=True, manifest="[]")
                )
                mutate(document["semantic_authority"]["observations"])
                with self.assertRaises(POSTGRES_STORE_TEST.TestFailure):
                    POSTGRES_STORE_TEST.validate_semantic_authority_evidence(
                        document["semantic_authority"]
                    )

    def test_semantic_authority_evidence_rejects_non_boolean_and_false_observations(self):
        for case, value, pattern in (
            ("non_boolean", 1, "invalid semantic authority observation"),
            ("false", False, "failed observation"),
        ):
            with self.subTest(case=case):
                document = envelope(
                    component(available=True, complete=True, manifest="[]")
                )
                document["semantic_authority"]["observations"][0]["passed"] = value
                with self.assertRaisesRegex(
                    POSTGRES_STORE_TEST.TestFailure, pattern
                ):
                    POSTGRES_STORE_TEST.validate_semantic_authority_evidence(
                        document["semantic_authority"]
                    )

    def test_semantic_authority_evidence_rejects_malformed_definition(self):
        document = envelope(component(available=True, complete=True, manifest="[]"))
        document["semantic_authority"]["definition"]["predicates"][0]["extra"] = True
        with self.assertRaisesRegex(
            POSTGRES_STORE_TEST.TestFailure, "definition predicate"
        ):
            POSTGRES_STORE_TEST.validate_semantic_authority_evidence(
                document["semantic_authority"]
            )

    def test_semantic_authority_checkpoint_evidence_must_not_diverge(self):
        document = envelope(component(available=True, complete=True, manifest="[]"))
        _, summary = POSTGRES_STORE_TEST.parse_candidate_capture_manifest(
            json.dumps(document, separators=(",", ":")).encode(),
            "source",
            DATABASE,
            source_binding=SOURCE_BINDING,
            secret_markers=("xy1300-secret",),
        )
        checkpoints = {
            "source": summary,
            "restored_once": dict(summary),
            "restored_twice": dict(summary),
        }
        POSTGRES_STORE_TEST.require_semantic_authority_checkpoint_parity(
            checkpoints
        )
        checkpoints["restored_twice"]["evidence_sha256"] = "f" * 64
        with self.assertRaisesRegex(
            POSTGRES_STORE_TEST.TestFailure, "differs across checkpoints"
        ):
            POSTGRES_STORE_TEST.require_semantic_authority_checkpoint_parity(
                checkpoints
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
        expected_digests = {"schema": "00" * 32, "authority": "11" * 32}
        self.assertEqual(POSTGRES_STORE_TEST.digest_constants_from_source(after), {
            "schema": "aa" * 32,
            "configured_authority": "bb" * 32,
        })
        POSTGRES_STORE_TEST.require_digest_only_authority_source(
            before, after, mismatches, expected_digests
        )
        POSTGRES_STORE_TEST.require_phase_b_changed_paths([
            "crates/decodex-postgres/src/authority.rs"
        ], mismatches)

        with self.assertRaises(POSTGRES_STORE_TEST.TestFailure):
            POSTGRES_STORE_TEST.require_digest_only_authority_source(
                before, after + "unauthorized source delta\n", mismatches,
                expected_digests,
            )
        with self.assertRaises(POSTGRES_STORE_TEST.TestFailure):
            POSTGRES_STORE_TEST.require_phase_b_changed_paths([
                "crates/decodex-postgres/src/authority.rs",
                "scripts/vnext/postgres_store_test.py",
            ], mismatches)

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
            POSTGRES_STORE_TEST.normalized_digest_source(
                counterexample, {"schema", "configured_authority"}
            )

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


class RestorePrerequisiteGateStateTests(unittest.TestCase):
    SECRET = "xy1421_private_secret_marker"

    def complete_until(self, state, target=None):
        for checkpoint in POSTGRES_STORE_TEST.RESTORE_PREREQUISITE_EXECUTION_CHECKPOINTS:
            if checkpoint == target:
                return
            def complete(checkpoint=checkpoint):
                if checkpoint == "output_contract":
                    state.bind_output_path(Path("/private/tmp/xy1421-receipt.json"))
                elif checkpoint == "source_binding_preflight":
                    state.bind_source(SOURCE_BINDING)
                elif checkpoint == "toolchain_preflight":
                    state.bind_toolchain("c" * 64)
                elif checkpoint == "private_work":
                    state.bind_secret_markers((self.SECRET,))
                elif checkpoint == "invocation_policy":
                    state.bind_invocation_policy({
                        name: True
                        for name in (
                            POSTGRES_STORE_TEST
                            .RESTORE_PREREQUISITE_INVOCATION_POLICIES
                        )
                    })
            state.run(checkpoint, complete)
        if target is not None:
            raise AssertionError(f"unknown checkpoint: {target}")

    def execution_failure(self, checkpoint, error):
        state = POSTGRES_STORE_TEST.RestorePrerequisiteGateState()
        self.complete_until(state, checkpoint)
        with self.assertRaises(POSTGRES_STORE_TEST.RestorePrerequisiteGateAbort):
            state.run(checkpoint, lambda: (_ for _ in ()).throw(error))
        state.ensure_cleanup_finalized_without_work()
        return state, state.failure_document()

    def completed_state(self, *, cleanup_required=False):
        state = POSTGRES_STORE_TEST.RestorePrerequisiteGateState()
        self.complete_until(state)
        state.begin_cleanup(cleanup_required, cleanup_required)
        if cleanup_required:
            for owner in POSTGRES_STORE_TEST.RESTORE_PREREQUISITE_CLEANUP_OWNERS:
                state.run_cleanup(owner, lambda: None)
                state.complete_cleanup_owner(owner)
        state.begin_cleanup_finalization()
        state.finish_cleanup()
        return state

    def semantic_failure(self, checkpoint):
        semantic_checkpoint = {
            "source_semantic_authority": "source",
            "restored_once_semantic_authority": "restored_once",
        }[checkpoint]
        return POSTGRES_STORE_TEST.SemanticAuthorityDiagnostic(json.dumps({
            "checkpoint": semantic_checkpoint,
            "definition_fingerprint": (
                POSTGRES_STORE_TEST.SUPPORTED_SEMANTIC_AUTHORITY_FINGERPRINT
            ),
            "failures": [{
                "failure_policy": "unsafe",
                "predicate": "fixture_unsafe",
            }],
            "schema": POSTGRES_STORE_TEST.SEMANTIC_AUTHORITY_DIAGNOSTIC_SCHEMA,
            "source_binding": SOURCE_BINDING,
        }, sort_keys=True, separators=(",", ":")))

    def test_postgres_toolchain_fingerprint_uses_fixed_order_and_binds_state(self):
        expected_order = ("initdb", "pg_ctl", "psql", "pg_dump", "pg_restore")
        self.assertEqual(POSTGRES_STORE_TEST.POSTGRES_TOOL_NAMES, expected_order)
        payloads = {
            name: b"inert PostgreSQL fixture\0" + name.encode("ascii")
            for name in expected_order
        }
        versions = {
            name: f"{name} (PostgreSQL) 18.0 fixture\n".encode("ascii")
            for name in expected_order
        }
        environment = {"LC_ALL": "C"}

        with tempfile.TemporaryDirectory() as directory:
            fixture_directory = Path(directory)
            fixture_directory.chmod(0o700)
            tools = {}
            for name in expected_order:
                path = fixture_directory / name
                path.write_bytes(payloads[name])
                path.chmod(0o700)
                tools[name] = path.resolve(strict=True)

            with mock.patch.object(
                POSTGRES_STORE_TEST,
                "postgres_tool_version",
                side_effect=[versions[name] for name in expected_order],
            ) as version_reader:
                fingerprint = POSTGRES_STORE_TEST.postgres_toolchain_fingerprint(
                    tools, environment
                )

        self.assertEqual(
            version_reader.call_args_list,
            [mock.call(tools[name], name, environment) for name in expected_order],
        )
        expected = hashlib.sha256(b"decodex/postgres-toolchain-authority/1\0")
        for name in expected_order:
            for value in (
                name.encode("ascii"),
                hashlib.sha256(payloads[name]).digest(),
                hashlib.sha256(versions[name]).digest(),
            ):
                expected.update(len(value).to_bytes(4, "big"))
                expected.update(value)
        self.assertEqual(fingerprint, expected.hexdigest())
        self.assertRegex(fingerprint, r"\A[0-9a-f]{64}\Z")

        state = POSTGRES_STORE_TEST.RestorePrerequisiteGateState()
        self.complete_until(state, "toolchain_preflight")
        bound = state.run(
            "toolchain_preflight", lambda: state.bind_toolchain(fingerprint)
        )
        self.assertEqual(bound, fingerprint)
        self.assertEqual(state.toolchain_fingerprint, fingerprint)
        self.assertEqual(state.completed_checkpoints[-1], "toolchain_preflight")

    def test_representative_execution_failures_have_distinct_fixed_owners(self):
        cases = (
            (
                "outer",
                "output_contract",
                POSTGRES_STORE_TEST.TestFailure(self.SECRET),
                "contract_invalid",
            ),
            (
                "cluster",
                "cluster_start",
                POSTGRES_STORE_TEST.TestFailure(self.SECRET),
                "operation_failed",
            ),
            (
                "preflight",
                "toolchain_preflight",
                POSTGRES_STORE_TEST.TestFailure(self.SECRET),
                "authority_unavailable",
            ),
            (
                "s0",
                "source_migrated",
                POSTGRES_STORE_TEST.TestFailure(self.SECRET),
                "operation_failed",
            ),
            (
                "archive",
                "archive_declaration_guarded",
                POSTGRES_STORE_TEST.AuthorityRestoreTargetFailure(
                    "archive_declaration_guarded", "archive_declaration_invalid"
                ),
                "archive_declaration_invalid",
            ),
            (
                "helper",
                "restore_pgcrypto_absent",
                POSTGRES_STORE_TEST.AuthorityRestoreTargetFailure(
                    "restore_pgcrypto_absent", "target_not_fresh"
                ),
                "target_not_fresh",
            ),
            (
                "semantic",
                "restored_once_semantic_authority",
                self.semantic_failure("restored_once_semantic_authority"),
                "operation_failed",
            ),
        )
        owners = []
        for case, checkpoint, error, reason in cases:
            with self.subTest(case=case):
                _, diagnostic = self.execution_failure(checkpoint, error)
                self.assertEqual(diagnostic["primary_checkpoint"], checkpoint)
                self.assertEqual(diagnostic["primary_reason"], reason)
                owners.append(diagnostic["primary_checkpoint"])
                if case == "semantic":
                    self.assertIsNotNone(
                        diagnostic["semantic_authority_diagnostic"]
                    )
                else:
                    self.assertIsNone(
                        diagnostic["semantic_authority_diagnostic"]
                    )
        self.assertEqual(len(owners), len(set(owners)))

    def cleanup_fault_state(self, point, *, prior_primary=False, system_exit=False):
        state = POSTGRES_STORE_TEST.RestorePrerequisiteGateState()
        if prior_primary:
            self.complete_until(state, "source_migrated")
            with self.assertRaises(POSTGRES_STORE_TEST.RestorePrerequisiteGateAbort):
                state.run(
                    "source_migrated",
                    lambda: (_ for _ in ()).throw(
                        POSTGRES_STORE_TEST.TestFailure(self.SECRET)
                    ),
                )
        else:
            self.complete_until(state)

        def inject(current):
            if current == point:
                error = (
                    SystemExit(self.SECRET)
                    if system_exit else KeyboardInterrupt(self.SECRET)
                )
                raise error

        with mock.patch.object(POSTGRES_STORE_TEST.shutil, "rmtree"):
            POSTGRES_STORE_TEST.cleanup_restore_prerequisite_gate(
                state,
                Path("/private/tmp/xy1421-private-work"),
                Path("/private/tmp/xy1421-private-work/postgres"),
                {},
                True,
                fault_injector=inject,
            )
        return state, state.failure_document()

    def test_cleanup_faults_keep_the_exact_pending_owner_and_never_pass(self):
        cases = (
            ("before_first_cleanup_owner", "cluster_stop", ()),
            (
                "after_cluster_stop_action_before_transition",
                "cluster_stop",
                (),
            ),
            (
                "between_cluster_stop_and_private_work_cleanup",
                "private_work_cleanup",
                ("cluster_stop",),
            ),
            (
                "after_private_work_cleanup_action_before_transition",
                "private_work_cleanup",
                ("cluster_stop",),
            ),
            (
                "during_cleanup_finalization",
                "cleanup_finalization",
                ("cluster_stop", "private_work_cleanup"),
            ),
        )
        for point, owner, completed in cases:
            with self.subTest(point=point):
                state, diagnostic = self.cleanup_fault_state(
                    point,
                    system_exit=point == "during_cleanup_finalization",
                )
                self.assertEqual(diagnostic["primary_checkpoint"], owner)
                self.assertEqual(diagnostic["primary_reason"], "interrupted")
                self.assertNotEqual(
                    diagnostic["primary_checkpoint"], "receipt_validation"
                )
                self.assertEqual(diagnostic["cleanup_status"], "failed")
                self.assertTrue(diagnostic["cleanup_finalized"])
                self.assertEqual(
                    diagnostic["required_cleanup_owners"],
                    ["cluster_stop", "private_work_cleanup"],
                )
                self.assertEqual(
                    diagnostic["completed_cleanup_owners"], list(completed)
                )
                self.assertIsNone(diagnostic["secondary_cleanup_reason"])
                self.assertNotEqual(state.cleanup_status, "passed")

        for point in (
            "between_cluster_stop_and_private_work_cleanup",
            "during_cleanup_finalization",
        ):
            with self.subTest(point=point, prior_primary=True):
                _, diagnostic = self.cleanup_fault_state(
                    point, prior_primary=True
                )
                self.assertEqual(
                    (diagnostic["primary_checkpoint"], diagnostic["primary_reason"]),
                    ("source_migrated", "operation_failed"),
                )
                self.assertEqual(diagnostic["cleanup_status"], "failed")
                self.assertEqual(
                    diagnostic["secondary_cleanup_reason"], "cleanup_failed"
                )

        operation_state = POSTGRES_STORE_TEST.RestorePrerequisiteGateState()
        self.complete_until(operation_state)
        with mock.patch.object(
            POSTGRES_STORE_TEST.shutil,
            "rmtree",
            side_effect=OSError(self.SECRET),
        ):
            POSTGRES_STORE_TEST.cleanup_restore_prerequisite_gate(
                operation_state,
                Path("/private/tmp/xy1421-private-work"),
                Path("/private/tmp/xy1421-private-work/postgres"),
                {},
                True,
            )
        operation_diagnostic = operation_state.failure_document()
        self.assertEqual(
            (
                operation_diagnostic["primary_checkpoint"],
                operation_diagnostic["primary_reason"],
            ),
            ("private_work_cleanup", "cleanup_failed"),
        )
        self.assertNotIn(
            self.SECRET,
            POSTGRES_STORE_TEST.canonical_restore_prerequisite_gate_diagnostic(
                operation_diagnostic
            ),
        )

    def test_cleanup_pass_requires_owner_completion_and_finalization(self):
        state = POSTGRES_STORE_TEST.RestorePrerequisiteGateState()
        state.begin_cleanup(True, True)
        with self.assertRaises(POSTGRES_STORE_TEST.HarnessCorruption):
            _ = state.cleanup_status
        for owner in POSTGRES_STORE_TEST.RESTORE_PREREQUISITE_CLEANUP_OWNERS:
            state.run_cleanup(owner, lambda: None)
            with self.assertRaises(POSTGRES_STORE_TEST.HarnessCorruption):
                _ = state.cleanup_status
            state.complete_cleanup_owner(owner)
        state.begin_cleanup_finalization()
        with self.assertRaises(POSTGRES_STORE_TEST.HarnessCorruption):
            _ = state.cleanup_status
        state.finish_cleanup()
        self.assertEqual(state.cleanup_status, "passed")
        self.assertEqual(
            state.required_cleanup_owners,
            POSTGRES_STORE_TEST.RESTORE_PREREQUISITE_CLEANUP_OWNERS,
        )
        self.assertEqual(
            state.completed_cleanup_owners,
            POSTGRES_STORE_TEST.RESTORE_PREREQUISITE_CLEANUP_OWNERS,
        )
        self.assertTrue(state.cleanup_finalization_completed)

    def test_receipt_validation_and_publication_have_fixed_owners(self):
        validation_state = self.completed_state()
        with self.assertRaises(POSTGRES_STORE_TEST.RestorePrerequisiteGateAbort):
            validation_state.run_receipt_lifecycle(
                "receipt_validation",
                lambda: (_ for _ in ()).throw(
                    POSTGRES_STORE_TEST.TestFailure(self.SECRET)
                ),
            )
        validation_diagnostic = validation_state.failure_document()
        self.assertEqual(
            (
                validation_diagnostic["primary_checkpoint"],
                validation_diagnostic["primary_reason"],
            ),
            ("receipt_validation", "receipt_invalid"),
        )

        source_state = self.completed_state()
        source_state.run_receipt_lifecycle("receipt_validation", lambda: None)
        with self.assertRaises(POSTGRES_STORE_TEST.RestorePrerequisiteGateAbort):
            source_state.run_receipt_lifecycle(
                "receipt_source_binding",
                lambda: (_ for _ in ()).throw(
                    POSTGRES_STORE_TEST.RestorePrerequisiteExpectedFailure("changed")
                ),
            )
        source_diagnostic = source_state.failure_document()
        self.assertEqual(
            (
                source_diagnostic["primary_checkpoint"],
                source_diagnostic["primary_reason"],
            ),
            ("receipt_source_binding", "changed"),
        )

        publication_state = self.completed_state()
        publication_state.run_receipt_lifecycle(
            "receipt_validation", lambda: None
        )
        publication_state.run_receipt_lifecycle(
            "receipt_source_binding", lambda: None
        )
        with self.assertRaises(POSTGRES_STORE_TEST.RestorePrerequisiteGateAbort):
            publication_state.run_receipt_lifecycle(
                "receipt_publication",
                lambda: (_ for _ in ()).throw(OSError(self.SECRET)),
            )
        publication_diagnostic = publication_state.failure_document()
        self.assertEqual(
            (
                publication_diagnostic["primary_checkpoint"],
                publication_diagnostic["primary_reason"],
            ),
            ("receipt_publication", "publication_failed"),
        )

    def test_progress_and_diagnostic_serialization_are_closed(self):
        _, diagnostic = self.execution_failure(
            "cluster_start", RuntimeError(self.SECRET + " raw payload")
        )
        serialized = (
            POSTGRES_STORE_TEST.canonical_restore_prerequisite_gate_diagnostic(
                diagnostic
            )
        )
        self.assertEqual(diagnostic["primary_reason"], "harness_corruption")
        self.assertNotIn(self.SECRET, serialized)
        self.assertNotIn("raw payload", serialized)

        malformed = dict(diagnostic)
        malformed["extra"] = self.SECRET
        with self.assertRaises(POSTGRES_STORE_TEST.TestFailure):
            POSTGRES_STORE_TEST.canonical_restore_prerequisite_gate_diagnostic(
                malformed
            )
        malformed = dict(diagnostic)
        malformed["primary_reason"] = "archive_declaration_invalid"
        with self.assertRaises(POSTGRES_STORE_TEST.TestFailure):
            POSTGRES_STORE_TEST.canonical_restore_prerequisite_gate_diagnostic(
                malformed
            )
        malformed = dict(diagnostic)
        malformed["completed_checkpoints"] = [
            *diagnostic["completed_checkpoints"],
            "cluster_start",
        ]
        with self.assertRaises(POSTGRES_STORE_TEST.TestFailure):
            POSTGRES_STORE_TEST.canonical_restore_prerequisite_gate_diagnostic(
                malformed
            )

        state = POSTGRES_STORE_TEST.RestorePrerequisiteGateState()
        self.complete_until(state, "source_semantic_authority")
        raw_semantic = POSTGRES_STORE_TEST.SemanticAuthorityDiagnostic(json.dumps({
            "checkpoint": "source",
            "definition_fingerprint": (
                POSTGRES_STORE_TEST.SUPPORTED_SEMANTIC_AUTHORITY_FINGERPRINT
            ),
            "failures": [{
                "failure_policy": "unsafe",
                "predicate": self.SECRET,
            }],
            "schema": POSTGRES_STORE_TEST.SEMANTIC_AUTHORITY_DIAGNOSTIC_SCHEMA,
            "source_binding": SOURCE_BINDING,
        }, sort_keys=True, separators=(",", ":")))
        with self.assertRaises(POSTGRES_STORE_TEST.RestorePrerequisiteGateAbort):
            state.run(
                "source_semantic_authority",
                lambda: (_ for _ in ()).throw(raw_semantic),
            )
        state.ensure_cleanup_finalized_without_work()
        semantic_serialized = (
            POSTGRES_STORE_TEST.canonical_restore_prerequisite_gate_diagnostic(
                state.failure_document()
            )
        )
        self.assertNotIn(self.SECRET, semantic_serialized)
        self.assertEqual(state.primary_reason, "harness_corruption")

    def test_failure_document_cleanup_corruption_uses_the_fixed_owned_fallback(self):
        prior_state, _ = self.execution_failure(
            "source_migrated", POSTGRES_STORE_TEST.TestFailure(self.SECRET)
        )
        prior_state._cleanup_status = self.SECRET
        repaired = prior_state.run_receipt_lifecycle(
            "receipt_validation",
            prior_state.failure_document_with_fixed_fallback,
            recovery=True,
        )
        serialized = (
            POSTGRES_STORE_TEST.canonical_restore_prerequisite_gate_diagnostic(
                repaired
            )
        )
        self.assertEqual(
            (repaired["primary_checkpoint"], repaired["primary_reason"]),
            ("source_migrated", "operation_failed"),
        )
        self.assertEqual(repaired["cleanup_status"], "failed")
        self.assertEqual(
            repaired["secondary_cleanup_reason"], "cleanup_failed"
        )
        self.assertTrue(repaired["failure_document_repaired"])
        self.assertNotIn(self.SECRET, serialized)

        no_primary = self.completed_state(cleanup_required=True)
        no_primary._cleanup_finalization_status = self.SECRET
        repaired_no_primary = no_primary.run_receipt_lifecycle(
            "receipt_validation",
            no_primary.failure_document_with_fixed_fallback,
            recovery=True,
        )
        self.assertEqual(
            (
                repaired_no_primary["primary_checkpoint"],
                repaired_no_primary["primary_reason"],
            ),
            ("cleanup_finalization", "harness_corruption"),
        )
        self.assertEqual(repaired_no_primary["cleanup_status"], "failed")
        self.assertIsNone(repaired_no_primary["secondary_cleanup_reason"])
        self.assertNotIn(
            self.SECRET,
            POSTGRES_STORE_TEST.canonical_restore_prerequisite_gate_diagnostic(
                repaired_no_primary
            ),
        )

    def test_v2_identity_and_mechanical_definition_fingerprint_are_exact(self):
        self.assertEqual(
            POSTGRES_STORE_TEST.RESTORE_PREREQUISITE_CLI,
            "--capture-authority-restore-prerequisite-v2",
        )
        self.assertTrue(
            POSTGRES_STORE_TEST.RESTORE_PREREQUISITE_GATE_SCHEMA.endswith("/2")
        )
        self.assertTrue(
            POSTGRES_STORE_TEST.RESTORE_PREREQUISITE_DIAGNOSTIC_SCHEMA.endswith("/2")
        )
        self.assertTrue(
            POSTGRES_STORE_TEST.RESTORE_PREREQUISITE_DEFINITION_SCHEMA.endswith("/2")
        )
        source = SCRIPT.read_text(encoding="utf-8")
        for retired in (
            '"--capture-authority-restore-prerequisite' + '"',
            "decodex/postgres-restore-prerequisite-r1-gate/" + "1",
            "decodex/postgres-restore-prerequisite-r1-diagnostic/" + "1",
            "decodex/postgres-restore-prerequisite-r1-definition/" + "1",
        ):
            self.assertNotIn(retired, source)
        self.assertNotIn(
            "gate", POSTGRES_STORE_TEST.RESTORE_PREREQUISITE_ALLOWED_REASONS
        )
        self.assertNotIn(
            "stage_failed",
            POSTGRES_STORE_TEST.RESTORE_PREREQUISITE_DIAGNOSTIC_REASONS,
        )
        definition = POSTGRES_STORE_TEST.restore_prerequisite_definition()
        fingerprint = hashlib.sha256(json.dumps(
            definition,
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=True,
        ).encode("utf-8")).hexdigest()
        self.assertEqual(
            fingerprint,
            POSTGRES_STORE_TEST.RESTORE_PREREQUISITE_DEFINITION_FINGERPRINT,
        )
        self.assertEqual(
            fingerprint,
            "53bb20b8e43a6199c3aa578269cee8b941ed549fd8f10db0dce361a03016524a",
        )
        self.assertEqual(
            fingerprint,
            POSTGRES_STORE_TEST.restore_prerequisite_definition_fingerprint(),
        )

        pass_state = self.completed_state(cleanup_required=True)
        receipt = POSTGRES_STORE_TEST.restore_prerequisite_gate_receipt(pass_state)
        self.assertEqual(
            POSTGRES_STORE_TEST.validate_restore_prerequisite_gate_receipt(receipt),
            receipt,
        )
        self.assertFalse(receipt["acceptance"])
        self.assertTrue(receipt["passed"])


if __name__ == "__main__":
    unittest.main()
