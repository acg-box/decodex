"""Static regressions for the inert XY-1338 V12 ManagedRun boundary."""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
MIGRATION = ROOT / "crates/decodex-postgres/migrations/V12__managed_run_safety.sql"
CONVERSATION_MIGRATION = (
    ROOT / "crates/decodex-postgres/migrations/V3__conversation_history.sql"
)
RUNTIME_SESSION_MIGRATION = (
    ROOT / "crates/decodex-postgres/migrations/V10__runtime_session_snapshots.sql"
)
RUST_API = ROOT / "crates/decodex-postgres/src/managed_runs.rs"
RUNTIME_SESSION_RUST_API = ROOT / "crates/decodex-postgres/src/runtime_sessions.rs"
CORE = ROOT / "crates/decodex-core/src/managed_run.rs"
AUTHORITY = ROOT / "crates/decodex-postgres/src/authority.rs"
MIGRATIONS = ROOT / "crates/decodex-postgres/src/migrations.rs"


class ManagedRunAuthorityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.migration = MIGRATION.read_text(encoding="utf-8")
        cls.conversation_migration = CONVERSATION_MIGRATION.read_text(
            encoding="utf-8"
        )
        cls.runtime_session_migration = RUNTIME_SESSION_MIGRATION.read_text(
            encoding="utf-8"
        )
        cls.rust_api = RUST_API.read_text(encoding="utf-8")
        cls.runtime_session_rust_api = RUNTIME_SESSION_RUST_API.read_text(
            encoding="utf-8"
        )
        cls.core = CORE.read_text(encoding="utf-8")
        cls.authority = AUTHORITY.read_text(encoding="utf-8")
        cls.migrations = MIGRATIONS.read_text(encoding="utf-8")

    def test_v12_is_the_only_next_forward_migration(self) -> None:
        versions = sorted(
            int(path.name.split("__", 1)[0][1:])
            for path in MIGRATION.parent.glob("V*.sql")
        )
        self.assertEqual(versions, list(range(1, 13)))
        self.assertIn("EXPECTED_LATEST_MIGRATION_VERSION: i32 = 12", self.migrations)
        self.assertIn("MANAGED_RUN_MIGRATION", self.authority)

    def test_execution_path_trigger_inventory_matches_canonical_full_identities(self) -> None:
        canonical_source = self.authority.split(
            'const TRIGGER_CONTRACT_SQL: &str = r#"', 1
        )[1].split('"#;', 1)[0]
        execution_source = self.authority.split(
            'const EXECUTION_PATH_CONTRACT_SQL: &str = r#"', 1
        )[1].split('"#;', 1)[0]
        canonical = set(re.findall(
            r"\('([^']+)', '([^']+)', '([^']+)', \d+\)",
            canonical_source,
        ))
        execution = set(re.findall(
            r"\('([^']+)', '([^']+)', 'decodex\.([^']+)\(\)'\)",
            execution_source,
        ))
        self.assertEqual(len(canonical), 84)
        self.assertEqual(execution, canonical)

    def test_event_namespace_uses_relation_aware_row_shapes(self) -> None:
        body = self.migration.split(
            "CREATE FUNCTION decodex.enforce_managed_run_event_namespace()", 1
        )[1].split("CREATE TRIGGER activity_managed_run_namespace", 1)[0]
        activity, outbox = body.split("IF TG_TABLE_NAME = 'activity' THEN", 1)[1].split(
            "\n\tELSE\n", 1
        )
        outbox = outbox.split("\n\tEND IF;\n\tIF linked", 1)[0]
        for row in ("NEW", "OLD"):
            self.assertIn(f"{row}.aggregate_kind", activity)
            self.assertIn(f"{row}.event_kind", activity)
            self.assertIn(f"{row}.payload", activity)
            self.assertIn(f"{row}.aggregate_kind", outbox)
            self.assertIn(f"{row}.payload", outbox)
            self.assertNotIn(f"{row}.event_kind", outbox)
        self.assertIn("NEW.payload, '$.**.activity_sequence'", outbox)
        self.assertIn("OLD.payload, '$.**.activity_sequence'", outbox)
        self.assertIn("invalid_text_representation OR numeric_value_out_of_range", body)
        self.assertIn("NEW.id IS DISTINCT FROM OLD.id", body)

    def test_v12_replaces_the_ambiguous_runtime_session_snapshot_local(self) -> None:
        signature = "decodex.create_runtime_session_exact("
        replacement = f"CREATE OR REPLACE FUNCTION {signature}"
        self.assertEqual(self.migration.count(replacement), 1)
        replacement_body = self.migration.split(replacement, 1)[1].split("$$;", 1)[0]
        self.assertIn("DECLARE created_profile_snapshot_id uuid;", replacement_body)
        self.assertNotIn("DECLARE profile_snapshot_id uuid;", replacement_body)
        self.assertEqual(replacement_body.count("created_profile_snapshot_id"), 4)
        self.assertEqual(
            replacement_body.count("'profile_snapshot_id', profile_snapshot_id"), 2
        )

        transition_signature = "decodex.transition_runtime_session_exact("
        transition_replacement = f"CREATE OR REPLACE FUNCTION {transition_signature}"
        self.assertEqual(self.migration.count(transition_replacement), 1)
        transition_body = self.migration.split(transition_replacement, 1)[1].split(
            "$$;", 1
        )[0]
        for body in (replacement_body, transition_body):
            self.assertEqual(
                body.count("'runtime_session_snapshot', session_value"), 2
            )
            self.assertNotIn("'runtime_session', session_value", body)
        self.assertIn(
            'required_value(effect, "runtime_session_snapshot")',
            self.runtime_session_rust_api,
        )
        self.assertIn(
            'activity_payload.get("runtime_session_snapshot")',
            self.runtime_session_rust_api,
        )
        self.assertNotIn(
            'required_value(effect, "runtime_session")', self.runtime_session_rust_api
        )
        self.assertNotIn(
            'activity_payload.get("runtime_session")', self.runtime_session_rust_api
        )

        resolver = self.authority.split(
            "fn canonical_function_source", 1
        )[1].split("async fn verify_identity_cast_authority", 1)[0]
        self.assertIn('format!("CREATE FUNCTION decodex.{}"', resolver)
        self.assertIn('format!("CREATE OR REPLACE FUNCTION decodex.{}"', resolver)
        self.assertIn(".rev()", resolver)
        self.assertIn("migration.rfind(declaration.as_str())", resolver)

        canonical_body = None
        for migration in (self.runtime_session_migration, self.migration):
            definitions = []
            for prefix in ("CREATE FUNCTION ", "CREATE OR REPLACE FUNCTION "):
                declaration = f"{prefix}{signature}"
                index = migration.rfind(declaration)
                if index >= 0:
                    definitions.append((index, declaration))
            if definitions:
                index, declaration = max(definitions)
                canonical_body = migration[index + len(declaration):].split(
                    "$$;", 1
                )[0]
        self.assertEqual(canonical_body, replacement_body)

    def test_v12_repairs_invoker_hierarchy_locks_without_runtime_session_update(self) -> None:
        for signature, retained_lock, removed_lock in (
            (
                "decodex.enforce_turn_state()",
                "FOR UPDATE OF c;",
                "FOR UPDATE OF c, rs",
            ),
            (
                "decodex.enforce_history_item_state()",
                "FOR UPDATE OF c, t;",
                "FOR UPDATE OF c, rs",
            ),
        ):
            replacement = f"CREATE OR REPLACE FUNCTION {signature}"
            self.assertEqual(self.migration.count(replacement), 1)
            replacement_body = self.migration.split(replacement, 1)[1].split(
                "$$;", 1
            )[0]
            self.assertIn("SET search_path = pg_catalog, decodex", replacement_body)
            self.assertIn(
                "current_setting('transaction_isolation') <> 'read committed'",
                replacement_body,
            )
            self.assertIn("ERRCODE = '40001'", replacement_body)
            self.assertIn(retained_lock, replacement_body)
            self.assertNotIn(removed_lock, replacement_body)
            self.assertNotIn("SECURITY DEFINER", replacement_body)

            canonical_body = None
            for migration in (self.conversation_migration, self.migration):
                definitions = []
                for prefix in ("CREATE FUNCTION ", "CREATE OR REPLACE FUNCTION "):
                    declaration = f"{prefix}{signature}"
                    index = migration.rfind(declaration)
                    if index >= 0:
                        definitions.append((index, declaration))
                if definitions:
                    index, declaration = max(definitions)
                    canonical_body = migration[index + len(declaration):].split(
                        "$$;", 1
                    )[0]
            self.assertEqual(canonical_body, replacement_body)

    def test_persistence_is_exactly_project_work_item_and_session_scoped(self) -> None:
        for constraint in (
            "managed_runs_work_item_project_fk",
            "managed_runs_runtime_session_revision_fk",
            "managed_run_assignments_run_project_fk",
            "managed_run_effects_run_scope_fk",
            "managed_run_effects_barrier_fk",
        ):
            self.assertIn(constraint, self.migration)
        self.assertIn("DEFERRABLE INITIALLY DEFERRED", self.migration)
        self.assertIn("runtime_session_revision", self.rust_api)

    def test_turn_identity_preserves_the_accepted_canonical_uuid_domain(self) -> None:
        receipt_table = self.migration.split(
            "CREATE TABLE decodex.managed_run_submitted_turn_receipts", 1
        )[1].split("CREATE TABLE decodex.managed_run_safety_inputs", 1)[0]
        safety_table = self.migration.split(
            "CREATE TABLE decodex.managed_run_safety_inputs", 1
        )[1].split("CREATE FUNCTION decodex.enforce_managed_run_command_owner", 1)[0]
        self.assertNotIn("turn_id::text COLLATE", receipt_table)
        self.assertNotIn("turn_id::text COLLATE", safety_table)
        self.assertIn("receipt_id::text COLLATE", receipt_table)
        self.assertIn("input_id::text COLLATE", safety_table)

    def test_execution_assignments_cannot_encode_advisor_or_lead(self) -> None:
        assignment_type = self.migration.split(
            "CREATE TYPE decodex.execution_assignment_role", 1
        )[1].split(";", 1)[0]
        self.assertIn("'task', 'reviewer'", assignment_type)
        self.assertNotIn("advisor", assignment_type)
        self.assertNotIn("lead", assignment_type)
        self.assertNotIn("agent_id", self.migration.split(
            "CREATE TABLE decodex.managed_run_assignments", 1
        )[1].split(");", 1)[0])
        self.assertIn("snapshot_role::text <> NEW.role::text", self.migration)

    def test_only_waiting_blocked_runs_and_fail_closed_barriers_persist(self) -> None:
        self.assertIn("managed_runs_inert_waiting_only", self.migration)
        self.assertIn("lifecycle = 'waiting'", self.migration)
        self.assertIn("AND blocked", self.migration)
        barrier_type = self.migration.split(
            "CREATE TYPE decodex.effect_barrier_state", 1
        )[1].split(";", 1)[0]
        self.assertEqual(barrier_type.count("'guarded'"), 1)
        self.assertEqual(barrier_type.count("'closed'"), 1)
        self.assertNotIn("open", barrier_type.lower())
        self.assertIn("effect barrier may only close exactly once", self.migration)

    def test_safety_inputs_are_positive_or_explicitly_inconclusive(self) -> None:
        safety_type = self.migration.split(
            "CREATE TYPE decodex.managed_run_safety_input_kind", 1
        )[1].split(";", 1)[0]
        for supported in (
            "positively_observed_unknown_turn",
            "submitted_turn_receipt",
            "inconclusive_observation",
        ):
            self.assertIn(supported, safety_type)
        forbidden = (
            "present", "complete", "present_empty", "absent", "not_found",
            "scan_exhaust", "no_event", "empty", "missing_method_result",
        )
        for value in forbidden:
            self.assertNotIn(value, safety_type.lower())
        self.assertNotIn("timestamped_method", safety_type)

    def test_atomic_command_closes_before_return_and_replays_durable_bytes(self) -> None:
        command = self.migration.split(
            "CREATE FUNCTION decodex.apply_managed_run_safety_input_exact", 1
        )[1].split("REVOKE ALL ON TABLE", 1)[0]
        for operation in (
            "UPDATE decodex.runtime_sessions",
            "UPDATE decodex.managed_runs",
            "UPDATE decodex.managed_run_effect_barriers",
            "INSERT INTO decodex.managed_run_safety_inputs",
            "UPDATE decodex.exact_command_receipts",
        ):
            self.assertIn(operation, command)
        self.assertLess(command.index("UPDATE decodex.managed_run_effect_barriers"),
                        command.index("response_value :="))
        self.assertIn("RETURN prior_input.response_bytes", command)
        self.assertIn("RETURN existing_response", self.migration)
        self.assertIn("prior_input.request_envelope<>request_value", command)
        self.assertIn("request_envelope,effect_envelope,response_bytes", command)
        reservation = command.index(
            "replay := decodex.reserve_exact_managed_run_safety_command"
        )
        validation = command.index("IF p_managed_run_id IS NULL")
        hierarchy = command.index("pg_advisory_xact_lock(1271)")
        run_scope = command.index("pg_advisory_xact_lock(1338")
        first_hierarchy_read = command.index("SELECT * INTO prior_input")
        self.assertLess(reservation, validation)
        self.assertLess(validation, hierarchy)
        self.assertLess(hierarchy, run_scope)
        self.assertLess(run_scope, first_hierarchy_read)

    def test_no_positive_run_mutation_or_producer_surface_exists(self) -> None:
        for forbidden in (
            "create_managed_run", "acquire_managed_run", "activate_managed_run",
            "advance_managed_run", "resume_managed_run", "complete_managed_run",
            "validate_managed_run", "dispatch_managed_run", "scan_codex",
            "submit_turn", "create_thread", "pagination", "wake_registration",
        ):
            self.assertNotIn(forbidden, self.rust_api.lower())
            self.assertNotIn(f"decodex.{forbidden}", self.migration.lower())
        self.assertNotIn("pub async fn create_", self.rust_api)
        self.assertNotIn("pub async fn transition_", self.rust_api)
        self.assertIn("read_managed_run_exact", self.rust_api)
        self.assertIn("apply_managed_run_safety_input", self.rust_api)

    def test_state_algebra_is_pure_and_exhaustively_fixture_driven(self) -> None:
        self.assertIn("pub const fn from_parts", self.core)
        self.assertIn("InvalidState", self.core)
        self.assertIn("state_algebra_accepts_only_canonical", self.core)
        self.assertNotIn("Postgres", self.core)
        self.assertNotIn("tokio", self.core)


if __name__ == "__main__":
    unittest.main()
