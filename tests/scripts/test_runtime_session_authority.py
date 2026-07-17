"""Static regressions for the XY-1337 V10 exact RuntimeSession boundary."""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
MIGRATION = ROOT / "crates/decodex-postgres/migrations/V10__runtime_session_snapshots.sql"
RUST_API = ROOT / "crates/decodex-postgres/src/runtime_sessions.rs"
CONVERSATIONS = ROOT / "crates/decodex-postgres/src/conversations.rs"
AUTHORITY = ROOT / "crates/decodex-postgres/src/authority.rs"
MIGRATIONS = ROOT / "crates/decodex-postgres/src/migrations.rs"


class RuntimeSessionAuthorityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.migration = MIGRATION.read_text(encoding="utf-8")
        cls.rust_api = RUST_API.read_text(encoding="utf-8")
        cls.conversations = CONVERSATIONS.read_text(encoding="utf-8")
        cls.authority = AUTHORITY.read_text(encoding="utf-8")
        cls.migrations = MIGRATIONS.read_text(encoding="utf-8")

    def test_v10_is_the_one_next_forward_zero_state_cutover(self) -> None:
        versions = sorted(
            int(path.name.split("__", 1)[0][1:])
            for path in MIGRATION.parent.glob("V*.sql")
        )
        self.assertEqual(versions, list(range(1, 11)))
        self.assertIn("EXPECTED_LATEST_MIGRATION_VERSION: i32 = 10", self.migrations)
        self.assertIn("IN ACCESS EXCLUSIVE MODE", self.migration)
        self.assertIn("runtime_session_v10_zero_state", self.migration)
        self.assertIn("operation IN ('create_runtime_session', 'transition_runtime_session')", self.migration)

    def test_creation_request_contains_only_complete_typed_consumed_inputs(self) -> None:
        match = re.search(
            r"CREATE FUNCTION decodex\.build_runtime_session_create_request\((.*?)\) RETURNS jsonb",
            self.migration,
            re.DOTALL,
        )
        self.assertIsNotNone(match)
        signature = match.group(1)
        for required in (
            "p_session_id uuid", "p_conversation_id uuid",
            "p_role decodex.role_profile_role", "p_account_snapshot_id uuid",
            "p_source_account_id uuid", "p_display_label text",
            "p_observed_state decodex.account_state",
            "p_account_source_revision bigint", "p_codex_thread_id uuid",
            "p_initial_state decodex.runtime_session_state",
        ):
            self.assertIn(required, signature)
        for forbidden in (
            "profile_snapshot_id", "profile_revision", "request_digest",
            "claim_token", "lease", "takeover", "pending",
        ):
            self.assertNotIn(forbidden, signature)

    def test_transition_request_is_exactly_session_revision_and_target(self) -> None:
        match = re.search(
            r"CREATE FUNCTION decodex\.build_runtime_session_transition_request\((.*?)\) RETURNS jsonb",
            self.migration,
            re.DOTALL,
        )
        self.assertIsNotNone(match)
        signature = match.group(1)
        self.assertIn("p_session_id uuid", signature)
        self.assertIn("p_expected_revision bigint", signature)
        self.assertIn("p_target_state decodex.runtime_session_state", signature)
        self.assertNotIn("note", signature)

    def test_commands_are_command_complete_and_replay_stored_bytes(self) -> None:
        for name in ("create_runtime_session_exact", "transition_runtime_session_exact"):
            body = self.migration.split(f"CREATE FUNCTION decodex.{name}", 1)[1]
            body = body.split("END\n$$;", 1)[0]
            self.assertIn("SECURITY DEFINER", body)
            self.assertIn("existing_request <> request_value", body)
            self.assertIn("RETURN existing_response", body)
            self.assertIn("response_bytes = response_value", body)
            self.assertIn("activity_sequence", body)
            self.assertIn("outbox_id", body)
        self.assertIn("FOR SHARE OF profile", self.migration)
        self.assertIn("ON CONFLICT (account_snapshot_id) DO NOTHING", self.migration)
        self.assertIn("account_snapshot_conflict", self.migration)

    def test_commands_acquire_hierarchy_coordinator_before_tuple_selection(self) -> None:
        create = self.migration.split(
            "CREATE FUNCTION decodex.create_runtime_session_exact", 1
        )[1].split("CREATE FUNCTION decodex.transition_runtime_session_exact", 1)[0]
        transition = self.migration.split(
            "CREATE FUNCTION decodex.transition_runtime_session_exact", 1
        )[1]
        self.assertLess(
            create.index("pg_advisory_xact_lock(1271)"),
            create.index("SELECT status INTO conversation_status"),
        )
        self.assertLess(
            transition.index("pg_advisory_xact_lock(1271)"),
            transition.index("FROM decodex.runtime_sessions AS session"),
        )

    def test_audit_namespace_closes_nested_and_legacy_representations(self) -> None:
        for marker in (
            "runtime_session_recorded",
            "@.aggregate_kind == \"runtime_session\"",
            "@.event_kind == \"runtime_session_transitioned\"",
            "exists(@.runtime_session)",
            "exists(@.runtime_session_id)",
            "exists(@.profile_snapshot)",
            "exists(@.account_snapshot)",
            "jsonb_path_query(",
            "$.**.activity_sequence",
        ):
            self.assertIn(marker, self.migration)

    def test_stable_rejections_and_immutable_full_snapshots_are_closed(self) -> None:
        for code in (
            "missing_target", "duplicate_target", "stale_revision",
            "illegal_transition", "invalid_account_snapshot",
            "account_snapshot_conflict",
        ):
            self.assertIn(code, self.migration)
            self.assertIn(self._rust_variant(code), self.rust_api)
        for field in ("instructions", "provenance", "instructions_digest", "source_revision"):
            self.assertIn(field, self.migration)
            self.assertIn(field, self.rust_api)
        self.assertIn("RuntimeSession snapshots are immutable", self.migration)

    def test_runtime_dml_helpers_and_event_namespaces_are_closed(self) -> None:
        for relation in ("profile_snapshots", "account_snapshots", "runtime_sessions"):
            self.assertIn(f"('{relation}', true, false, false, false)", self.authority)
            self.assertIn(f"{relation}_command_owner", self.migration)
        runtime = self.authority.split("const RUNTIME_EXECUTE_FUNCTIONS", 1)[1].split("];", 1)[0]
        self.assertIn("create_runtime_session_exact", runtime)
        self.assertIn("transition_runtime_session_exact", runtime)
        self.assertNotIn("complete_exact_runtime_session_rejection", runtime)
        self.assertIn("activity_runtime_session_namespace", self.migration)
        self.assertIn("outbox_runtime_session_namespace", self.migration)

    def test_rust_input_excludes_postgresql_authored_facts_and_parses_full_effect(self) -> None:
        create_input = self.rust_api.split(
            "pub struct CreateRuntimeSessionAccountSnapshot", 1
        )[1].split("}", 1)[0]
        self.assertNotIn("created_at", create_input)
        for required in (
            "RuntimeSessionCommandEffect", "prior_state", "new_state",
            "prior_revision", "new_revision", "activity_sequence",
            "activity_payload", "outbox_id", "outbox_payload",
        ):
            self.assertIn(required, self.rust_api)
        self.assertIn("exact RuntimeSession response effect is inconsistent", self.rust_api)
        self.assertIn("exact RuntimeSession audit effect is inconsistent", self.rust_api)
        self.assertIn("validate_request_context", self.rust_api)
        self.assertIn("parse_create_response", self.rust_api)
        self.assertIn("parse_transition_response", self.rust_api)
        self.assertIn("stored RuntimeSession UUID is invalid", self.rust_api)

    def test_focused_final_gate_fixtures_are_named_without_running_them(self) -> None:
        fixture = (
            ROOT
            / "crates/decodex-postgres/tests/postgres_store/runtime_sessions.rs"
        ).read_text(encoding="utf-8")
        for fixture_name in (
            "postgres_exact_runtime_session_commands",
            "postgres_exact_runtime_session_atomic_rollback",
            "postgres_exact_runtime_session_retry_convergence",
            "postgres_v9_to_v10_runtime_session_upgrade",
            "postgres_v10_rejects_classified_runtime_state",
            "postgres_v10_fences_blocked_old_runtime_writer",
            "postgres_exact_runtime_session_crash_recovery",
            "postgres_exact_runtime_session_restore",
        ):
            self.assertIn(fixture_name, fixture)
        for boundary in ("receipt", "domain", "activity", "outbox", "response"):
            self.assertIn(f'"{boundary}"', fixture)
        for hostile in (
            "IdempotencyConflict", "DuplicateTarget", "MissingTarget",
            "StaleRevision", "IllegalTransition", "AccountSnapshotConflict",
            "profile_race", "duplicate_race",
        ):
            self.assertIn(hostile, fixture)

    def test_rust_uses_only_exact_entrypoints_and_one_retry_owner(self) -> None:
        self.assertIn("create_runtime_session_exact", self.rust_api)
        self.assertIn("transition_runtime_session_exact", self.rust_api)
        self.assertIn("execute_exact_with_retry", self.rust_api)
        self.assertNotIn("command_receipts", self.rust_api)
        self.assertNotRegex(self.rust_api, r"(?i)\b(?:insert|update|delete)\s+(?:into|from)?\s*decodex\.")
        self.assertNotIn("pub async fn create_runtime_session", self.conversations)
        self.assertNotIn("pub async fn transition_runtime_session", self.conversations)

    @staticmethod
    def _rust_variant(code: str) -> str:
        return "".join(part.title() for part in code.split("_"))


if __name__ == "__main__":
    unittest.main()
