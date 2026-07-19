import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import unittest


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
