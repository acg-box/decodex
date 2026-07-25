"""Static regressions for the V26 ManagedRun execution boundary."""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
MIGRATION_DIR = ROOT / "crates/decodex-postgres/migrations"
CUTOVER = MIGRATION_DIR / "V26__execution_coordinator_cutover.sql"
RUST_API = ROOT / "crates/decodex-postgres/src/managed_runs.rs"
CORE = ROOT / "crates/decodex-core/src/managed_run.rs"
AUTHORITY = ROOT / "crates/decodex-postgres/src/authority.rs"
MIGRATIONS = ROOT / "crates/decodex-postgres/src/migrations.rs"
RUST_TEST = ROOT / "crates/decodex-postgres/tests/postgres_store/managed_runs.rs"
HARNESS = ROOT / "scripts/vnext/postgres_store_test.py"


def canonical_function_source(migrations: list[str], signature: str) -> str:
    declarations = (
        f"CREATE FUNCTION decodex.{signature}",
        f"CREATE OR REPLACE FUNCTION decodex.{signature}",
    )
    selected: tuple[int, str] | None = None
    source = ""
    for migration in migrations:
        selected_in_migration: tuple[int, str] | None = None
        for declaration in declarations:
            offset = migration.rfind(declaration)
            if (
                offset >= 0
                and (
                    selected_in_migration is None
                    or offset >= selected_in_migration[0]
                )
            ):
                selected_in_migration = (offset, declaration)
        if selected_in_migration is not None:
            selected = selected_in_migration
            source = migration
    if selected is None:
        raise AssertionError(f"canonical function {signature} is absent")
    offset, declaration = selected
    return source[offset + len(declaration) :].split("$$;", 1)[0]


class ManagedRunAuthorityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        paths = sorted(
            MIGRATION_DIR.glob("V*.sql"),
            key=lambda path: int(path.name.split("__", 1)[0][1:]),
        )
        cls.migration_sources = [
            path.read_text(encoding="utf-8") for path in paths
        ]
        cls.cutover = CUTOVER.read_text(encoding="utf-8")
        cls.rust_api = RUST_API.read_text(encoding="utf-8")
        cls.core = CORE.read_text(encoding="utf-8")
        cls.authority = AUTHORITY.read_text(encoding="utf-8")
        cls.migrations = MIGRATIONS.read_text(encoding="utf-8")
        cls.rust_test = RUST_TEST.read_text(encoding="utf-8")
        cls.harness = HARNESS.read_text(encoding="utf-8")

    def test_integrated_authority_ends_at_v26(self) -> None:
        versions = sorted(
            int(path.name.split("__", 1)[0][1:])
            for path in MIGRATION_DIR.glob("V*.sql")
        )
        self.assertEqual(versions, list(range(1, 27)))
        self.assertIn("EXPECTED_LATEST_MIGRATION_VERSION: i32 = 26", self.migrations)
        self.assertIn("EXECUTION_COORDINATOR_MIGRATION", self.authority)

    def test_execution_path_trigger_inventory_is_current(self) -> None:
        canonical_source = self.authority.split(
            'const TRIGGER_CONTRACT_SQL: &str = r#"', 1
        )[1].split('"#;', 1)[0]
        execution_source = self.authority.split(
            'const EXECUTION_PATH_CONTRACT_SQL: &str = r#"', 1
        )[1].split('"#;', 1)[0]
        canonical = set(
            re.findall(
                r"\('([^']+)', '([^']+)', '([^']+)', \d+\)",
                canonical_source,
            )
        )
        execution = set(
            re.findall(
                r"\('([^']+)', '([^']+)', 'decodex\.([^']+)\(\)'\)",
                execution_source,
            )
        )
        self.assertIn("const SAFETY_TRIGGER_COUNT: usize = 146;", self.authority)
        self.assertEqual(len(canonical), 146)
        self.assertEqual(execution, canonical)
        for function in (
            "enforce_managed_run_assignment_scope",
            "enforce_managed_run_event_namespace",
            "enforce_process_generation_transition",
            "enforce_provider_attempt_binding",
        ):
            self.assertIn(function, self.authority)

    def test_runtime_function_inventory_is_v26_native(self) -> None:
        runtime = self.authority.split(
            "const RUNTIME_EXECUTE_FUNCTIONS: [&str; 70] = [", 1
        )[1].split("];", 1)[0]
        self.assertEqual(len(re.findall(r'^\s*"decodex\.', runtime, re.MULTILINE)), 70)
        for function in (
            "resolve_routing_snapshot_exact",
            "route_account_exact",
            "read_execution_decision_exact",
            "read_managed_run_execution_exact",
            "prepare_process_generation_exact",
            "prepare_provider_attempt_exact",
        ):
            self.assertIn(function, runtime)

    def test_v26_migration_history_intentionally_retires_v12_authority(self) -> None:
        for relation in (
            "managed_run_effect_barriers",
            "managed_run_effects",
            "managed_run_submitted_turn_receipts",
            "managed_run_safety_inputs",
        ):
            self.assertIn(f"DROP TABLE decodex.{relation};", self.cutover)
        for type_name in (
            "effect_barrier_state",
            "managed_run_effect_kind",
            "managed_run_effect_state",
            "managed_run_safety_input_kind",
        ):
            self.assertIn(f"DROP TYPE decodex.{type_name};", self.cutover)
        self.assertIn(
            "DROP FUNCTION decodex.apply_managed_run_safety_input_exact(",
            self.cutover,
        )
        self.assertIn(
            "DROP FUNCTION decodex.enforce_effect_barrier_state();",
            self.cutover,
        )
        runtime = self.authority.split(
            "const RUNTIME_EXECUTE_FUNCTIONS: [&str; 70] = [", 1
        )[1].split("];", 1)[0]
        self.assertNotIn("apply_managed_run_safety_input_exact", runtime)
        for retired in (
            "apply_managed_run_safety_input",
            "ManagedRunSafetyInput",
            "EffectBarrier",
            "SubmittedTurnReceipt",
        ):
            self.assertNotIn(retired, self.rust_test)
        self.assertNotIn("postgres_managed_run_safety_contract", self.harness)
        self.assertNotIn("postgres_managed_run_safety_restore", self.harness)

    def test_assignment_scope_remains_role_profile_bound(self) -> None:
        body = canonical_function_source(
            self.migration_sources, "enforce_managed_run_assignment_scope()"
        )
        self.assertIn("snapshot_role::text <> NEW.role::text", body)
        self.assertIn("managed_run_assignment_scope", body)
        self.assertIn("execution_assignment_role", "\n".join(self.migration_sources))

    def test_event_namespace_remains_relation_aware_and_fail_closed(self) -> None:
        body = canonical_function_source(
            self.migration_sources, "enforce_managed_run_event_namespace()"
        )
        activity, outbox = body.split(
            "IF TG_TABLE_NAME = 'activity' THEN", 1
        )[1].split("\n\tELSE\n", 1)
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
        self.assertIn(
            "invalid_text_representation OR numeric_value_out_of_range", body
        )
        self.assertIn("NEW.id IS DISTINCT FROM OLD.id", body)

    def test_readback_consumes_provider_attempts_without_copying_authority(self) -> None:
        self.assertIn("read_managed_run_exact", self.rust_api)
        self.assertIn("read_managed_run_execution_exact", self.rust_api)
        self.assertIn(
            "pub provider_attempts: Vec<ManagedRunProviderAttempt>", self.rust_api
        )
        self.assertIn("'provider_attempts',COALESCE((", self.cutover)
        self.assertIn("FROM decodex.provider_attempts AS attempt", self.cutover)
        self.assertIn(
            "attempt.consumer_kind='managed_run_execution'", self.cutover
        )
        for forbidden in (
            "pub async fn create_managed_run",
            "pub async fn transition_managed_run",
            "pub async fn dispatch_managed_run",
        ):
            self.assertNotIn(forbidden, self.rust_api)

    def test_behavioral_surface_covers_current_negative_and_restore_contracts(self) -> None:
        for evidence in (
            "managed_run_assignment_scope",
            "managed_run_event_namespace",
            "malformed-link",
            "overflow-link",
            "read_managed_run_exact",
            "ProviderAttemptState::Succeeded",
            "postgres_managed_run_v26_contract",
            "postgres_managed_run_v26_restore",
        ):
            self.assertIn(evidence, self.rust_test)

    def test_focused_harness_targets_the_current_exact_tests(self) -> None:
        for test_name in (
            "postgres_managed_run_v26_contract",
            "postgres_managed_run_v26_restore",
        ):
            self.assertIn(f'"{test_name}"', self.harness)
        self.assertIn("--focus-managed-runs", self.harness)
        self.assertIn("XY-1416 V26 semantic manifest diagnostics", self.harness)

    def test_state_algebra_remains_pure_and_fixture_driven(self) -> None:
        self.assertIn("pub const fn from_parts", self.core)
        self.assertIn("InvalidState", self.core)
        self.assertIn("state_algebra_accepts_only_canonical", self.core)
        self.assertNotIn("Postgres", self.core)
        self.assertNotIn("tokio", self.core)


if __name__ == "__main__":
    unittest.main()
