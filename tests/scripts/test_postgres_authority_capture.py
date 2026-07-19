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
            [
                "dependency",
                ["source", index, "xy1300-secret"],
                json.dumps([
                    "normal",
                    "pg_catalog.pg_proc",
                    False,
                    ["function", index, "xy1300-secret", "x" * 400],
                ]),
            ]
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
        self.assertEqual(evidence["dependency_type"], "normal")
        self.assertEqual(evidence["reference_class"], "pg_catalog.pg_proc")
        self.assertIn("reference_key", evidence)
        self.assertNotIn("contract", evidence)

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
