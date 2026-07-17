"""Static regressions for the XY-1346 V9 exact RoleProfile boundary."""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
MIGRATION = ROOT / "crates/decodex-postgres/migrations/V9__exact_role_profiles.sql"
RUST_API = ROOT / "crates/decodex-postgres/src/role_profiles.rs"
AUTHORITY = ROOT / "crates/decodex-postgres/src/authority.rs"


class ExactRoleProfileAuthorityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.migration = MIGRATION.read_text(encoding="utf-8")
        cls.rust_api = RUST_API.read_text(encoding="utf-8")
        cls.authority = AUTHORITY.read_text(encoding="utf-8")

    def test_v9_owns_separate_receipts_and_exact_four_role_set(self) -> None:
        self.assertIn("CREATE TABLE decodex.exact_command_receipts", self.migration)
        self.assertIn("PRIMARY KEY (protocol_version, idempotency_key)", self.migration)
        self.assertIn("DEFERRABLE INITIALLY DEFERRED", self.migration)
        self.assertIn("exact_command_receipts_no_credentials", self.migration)
        self.assertNotIn("ALTER TABLE decodex.command_receipts", self.migration)
        enum = re.search(
            r"CREATE TYPE decodex\.role_profile_role AS ENUM \((.*?)\);",
            self.migration,
            re.DOTALL,
        )
        self.assertIsNotNone(enum)
        self.assertEqual(
            re.findall(r"'([^']+)'", enum.group(1)),
            ["advisor", "lead", "task", "reviewer"],
        )

    def test_command_signatures_have_complete_typed_inputs_only(self) -> None:
        for name in ("bootstrap_role_profiles_exact", "update_role_profile_exact"):
            match = re.search(
                rf"CREATE FUNCTION decodex\.{name}\((.*?)\) RETURNS bytea",
                self.migration,
                re.DOTALL,
            )
            self.assertIsNotNone(match)
            signature = match.group(1)
            self.assertIn("p_protocol text", signature)
            self.assertIn("p_idempotency_key text", signature)
            for forbidden in ("request_digest", "claim_token", "lease", "pending"):
                with self.subTest(name=name, forbidden=forbidden):
                    self.assertNotIn(forbidden, signature)

    def test_runtime_api_has_no_receipt_table_access(self) -> None:
        self.assertNotIn("exact_command_receipts", self.rust_api)
        self.assertNotRegex(
            self.rust_api,
            r"(?i)\b(?:insert|update|delete)\s+(?:into|from)?\s*decodex\.",
        )
        self.assertIn("bootstrap_role_profiles_exact", self.rust_api)
        self.assertIn("update_role_profile_exact", self.rust_api)

    def test_stored_response_and_outcomes_remain_distinct(self) -> None:
        self.assertIn("existing_request <> request_value", self.migration)
        self.assertIn("RETURN existing_response", self.migration)
        self.assertIn("completed_rejected", self.migration)
        self.assertIn("ERRCODE = 'DX001'", self.migration)
        self.assertIn("is_retryable_exact_database_error", self.rust_api)
        self.assertIn("serde_json::from_slice(response)", self.rust_api)

    def test_authority_closes_new_relations_and_only_command_entrypoints(self) -> None:
        for relation in (
            "exact_command_receipts",
            "role_profiles",
            "role_profile_revisions",
        ):
            with self.subTest(relation=relation):
                self.assertIn(
                    f"('{relation}', false, false, false, false)", self.authority
                )
        runtime = self.authority.split("const RUNTIME_EXECUTE_FUNCTIONS", 1)[1].split(
            "];", 1
        )[0]
        self.assertIn("bootstrap_role_profiles_exact", runtime)
        self.assertIn("update_role_profile_exact", runtime)
        self.assertNotIn("complete_exact_role_profile_rejection", runtime)
        self.assertIn("ROLE_PROFILE_MIGRATION", self.authority)


if __name__ == "__main__":
    unittest.main()
