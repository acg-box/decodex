"""Static regressions for the accepted XY-1345 command-authority boundary."""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
HARNESS = ROOT / "scripts/vnext/exact_command_prototype.py"
NORMATIVE_FILES = (
    ROOT / "openwiki/decisions/vnext-authority.md",
    ROOT / "openwiki/specs/vnext-authority.md",
    ROOT / "openwiki/specs/vnext-gates.md",
    ROOT / "openwiki/architecture/runtime-architecture.md",
)


class ExactCommandAuthorityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.harness = HARNESS.read_text(encoding="utf-8")
        cls.authority = "\n".join(
            path.read_text(encoding="utf-8") for path in NORMATIVE_FILES
        )

    def test_prototype_uses_a_separate_deferred_exact_receipt_authority(self) -> None:
        self.assertIn("CREATE TABLE decodex.exact_command_receipts", self.harness)
        self.assertIn("DEFERRABLE INITIALLY DEFERRED", self.harness)
        self.assertIn("PRIMARY KEY (protocol_version, idempotency_key)", self.harness)
        self.assertNotIn("ALTER TABLE decodex.command_receipts", self.harness)

    def test_complete_command_signature_has_no_split_phase_authority(self) -> None:
        match = re.search(
            r"CREATE FUNCTION decodex\.transition_runtime_session_exact\((.*?)\) "
            r"RETURNS pg_catalog\.bytea",
            self.harness,
            re.DOTALL,
        )
        self.assertIsNotNone(match)
        signature = match.group(1)
        for forbidden in (
            "request_hash",
            "request_digest",
            "claim_token",
            "claim_owner",
            "lease",
        ):
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, signature)
        self.assertIn("p_idempotency_key", signature)
        self.assertIn("p_target_state", signature)

    def test_envelope_equality_and_explicit_optional_nulls_are_owned_by_postgres(self) -> None:
        self.assertIn("existing_request <> request_value", self.harness)
        self.assertNotIn("existing_request @>", self.harness)
        self.assertIn("'codex_thread_id', p_codex_thread_id", self.harness)
        self.assertIn("'target_state', p_target_state, 'note', p_note", self.harness)
        for builder in (
            "build_role_profile_bootstrap_request",
            "build_role_profile_update_request",
            "build_runtime_session_create_request",
            "build_runtime_session_transition_request",
        ):
            with self.subTest(builder=builder):
                self.assertIn(f"CREATE FUNCTION decodex.{builder}", self.harness)

    def test_restore_compares_receipt_acl_semantics_not_relacl_serialization(self) -> None:
        self.assertNotIn("relacl::pg_catalog.text", self.harness)
        self.assertIn("pg_catalog.aclexplode(COALESCE(c.relacl", self.harness)
        relation_acl = self.harness.split("'relation_effective'", 1)[1].split(
            "'default_privileges'", 1
        )[0]
        for role in ("decodex_exact_owner", "decodex_exact_runtime", "public"):
            with self.subTest(role=role):
                self.assertIn(role, relation_acl)
        for privilege in (
            "SELECT", "INSERT", "UPDATE", "DELETE", "TRUNCATE",
            "REFERENCES", "TRIGGER", "MAINTAIN",
        ):
            with self.subTest(privilege=privilege):
                self.assertIn("has_table_privilege", relation_acl)
                self.assertIn(privilege, relation_acl)
        self.assertIn("relation_semantic_acl", relation_acl)
        self.assertIn("unexpected grantee/grant option", self.harness)
        self.assertIn(
            "ALTER DEFAULT PRIVILEGES FOR ROLE decodex_exact_owner\n"
            "\tREVOKE EXECUTE ON FUNCTIONS FROM PUBLIC",
            self.harness,
        )
        self.assertNotIn(
            "ALTER DEFAULT PRIVILEGES FOR ROLE decodex_exact_owner IN SCHEMA",
            self.harness,
        )

    def test_catalog_closes_every_prototype_function_and_trigger(self) -> None:
        expected = {
            "enforce_exact_receipt_completion",
            "forbid_exact_receipt_rewrite",
            "forbid_exact_receipt_truncate",
            "build_role_profile_bootstrap_request",
            "build_role_profile_update_request",
            "build_runtime_session_create_request",
            "build_runtime_session_transition_request",
            "prototype_failpoint",
            "complete_prototype_rejection",
            "transition_runtime_session_exact",
            "prototype_leave_incomplete",
        }
        manifest = self.harness.split("EXPECTED_FUNCTIONS = {", 1)[1].split("}\n", 1)[0]
        for function in expected:
            with self.subTest(function=function):
                self.assertIn(f'"{function}"', manifest)
        self.assertEqual(manifest.count('"): (') + manifest.count('": ('), len(expected))
        for assertion in (
            "function identity set drifted",
            "effective execute closure failed",
            "semantic function ACL drifted",
            "source/dependency closure is incomplete",
            "receipt trigger identity set drifted",
        ):
            self.assertIn(assertion, self.harness)
        self.assertIn("pg_catalog.pg_get_functiondef(p.oid)", self.harness)
        self.assertIn("function_definition_manifest_sha256", self.harness)

    def test_effect_mismatches_decode_response_and_join_every_persisted_effect(self) -> None:
        effect_proof = self.harness.split("def prove_effect_binding_and_restore", 1)[1]
        self.assertIn("pg_catalog.convert_from(r.response_bytes,'UTF8')", effect_proof)
        self.assertIn("LEFT JOIN decodex.prototype_runtime_sessions", effect_proof)
        self.assertIn("r.response->'effect' IS DISTINCT FROM r.effect_envelope", effect_proof)
        self.assertIn('"mismatched_responses": mismatch', effect_proof)

    def test_cleanup_requires_authoritative_cluster_shutdown(self) -> None:
        stop = self.harness.split("\tdef stop(self)", 1)[1].split("\n\tdef psql", 1)[0]
        self.assertIn("self.require", stop)
        self.assertNotIn("self.command", stop)
        self.assertIn('"cluster_stopped": cluster_stopped', self.harness)
        self.assertIn("if not cleaned and error is None", self.harness)

    def test_normative_authority_rejects_candidate_era_protocol(self) -> None:
        for required in (
            "exact_command_receipts",
            "DEFERRABLE INITIALLY DEFERRED",
            "READ COMMITTED",
            "XY-1345",
            "XY-1346",
            "Candidate 3",
        ):
            with self.subTest(required=required):
                self.assertIn(required, self.authority)
        self.assertIn("caller-supplied request hash", self.authority)


if __name__ == "__main__":
    unittest.main()
