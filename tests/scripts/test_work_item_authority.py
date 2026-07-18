"""Static regressions for the XY-1343 V11 exact WorkItem boundary."""

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
MIGRATION = ROOT / "crates/decodex-postgres/migrations/V11__work_item_authority.sql"
RUST_API = ROOT / "crates/decodex-postgres/src/work_items.rs"
AUTHORITY = ROOT / "crates/decodex-postgres/src/authority.rs"
MIGRATIONS = ROOT / "crates/decodex-postgres/src/migrations.rs"
HARNESS = ROOT / "scripts/vnext/postgres_store_test.py"


class WorkItemAuthorityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.migration = MIGRATION.read_text(encoding="utf-8")
        cls.rust_api = RUST_API.read_text(encoding="utf-8")
        cls.authority = AUTHORITY.read_text(encoding="utf-8")
        cls.migrations = MIGRATIONS.read_text(encoding="utf-8")
        cls.harness = HARNESS.read_text(encoding="utf-8")

    def test_v11_remains_the_exact_predecessor_to_v12(self) -> None:
        versions = sorted(
            int(path.name.split("__", 1)[0][1:])
            for path in MIGRATION.parent.glob("V*.sql")
        )
        self.assertEqual(versions, list(range(1, 13)))
        self.assertIn("EXPECTED_LATEST_MIGRATION_VERSION: i32 = 12", self.migrations)
        self.assertIn("WORK_ITEM_MIGRATION", self.authority)

    def test_v11_schema_is_a_singleton_pg18_restore_fixed_point(self) -> None:
        canonical_constraints = {
            "account_snapshots_facts": (
                "pg_catalog.octet_length(display_label) >= 1",
                "pg_catalog.octet_length(display_label) <= 128",
            ),
            "exact_command_receipts_identity_bounded": (
                "pg_catalog.octet_length(protocol_version) >= 1",
                "pg_catalog.octet_length(protocol_version) <= 64",
                "pg_catalog.octet_length(idempotency_key) >= 1",
                "pg_catalog.octet_length(idempotency_key) <= 256",
            ),
            "role_profile_revisions_configuration": (
                "pg_catalog.octet_length(model) >= 1",
                "pg_catalog.octet_length(model) <= 128",
                "pg_catalog.octet_length(reasoning_effort) >= 1",
                "pg_catalog.octet_length(reasoning_effort) <= 32",
                "pg_catalog.octet_length(service_tier) >= 1",
                "pg_catalog.octet_length(service_tier) <= 32",
                "pg_catalog.octet_length(instructions) >= 1",
                "pg_catalog.octet_length(instructions) <= 65536",
                "pg_catalog.octet_length(provenance) >= 1",
                "pg_catalog.octet_length(provenance) <= 4096",
            ),
            "work_item_readiness_blockers_state_bounded": (
                "pg_catalog.octet_length(observed_state) >= 1",
                "pg_catalog.octet_length(observed_state) <= 64",
            ),
        }
        for name, predicates in canonical_constraints.items():
            self.assertEqual(self.migration.count(f"CONSTRAINT {name} CHECK"), 1)
            definition = self.migration.split(f"CONSTRAINT {name} CHECK", 1)[1]
            end = min(
                definition.index(marker)
                for marker in ("\n\t),", "\n\t);")
                if marker in definition
            )
            definition = definition[:end]
            self.assertNotIn("BETWEEN", definition)
            for predicate in predicates:
                self.assertIn(predicate, definition)

        for table, constraint in (
            ("account_snapshots", "account_snapshots_facts"),
            ("exact_command_receipts", "exact_command_receipts_identity_bounded"),
            ("role_profile_revisions", "role_profile_revisions_configuration"),
        ):
            statement = self.migration.split(f"ALTER TABLE decodex.{table}", 1)[1]
            statement = statement.split(";", 1)[0]
            self.assertIn(f"DROP CONSTRAINT {constraint}", statement)
            self.assertIn(f"ADD CONSTRAINT {constraint} CHECK", statement)
            self.assertNotIn("NOT VALID", statement)
            self.assertNotIn("VALIDATE CONSTRAINT", statement)

        self.assertIn(
            "0x99, 0xb6, 0x41, 0xfb, 0xd0, 0xee, 0x07, 0xc1",
            self.authority,
        )
        self.assertNotIn("expected_manifest_digest", self.authority)
        for override in (
            "DECODEX_TEST_GENERATED_MANIFEST_DIGESTS",
            "DECODEX_TEST_EXPECTED_SCHEMA_SHA256",
            "DECODEX_TEST_EXPECTED_CONFIGURED_AUTHORITY_SHA256",
        ):
            self.assertNotIn(override, self.authority)
            self.assertNotIn(override, self.harness)

    def test_focused_harness_finalizes_evidence_before_propagating_failure(self) -> None:
        focused = self.harness.split("def run_work_item_focused_contracts", 1)[1]
        focused = focused.split("\ndef run_runtime_session_crash_recovery", 1)[0]
        baseline = 'checkpoints["baseline"] = load_semantic_manifest'
        behavior = 'run_work_item_test("postgres_exact_work_item_commands", env)'
        post_attempt = 'checkpoints["post_attempt"] = load_semantic_manifest'
        diagnostics = 'diagnostics, manifest_failures = manifest_diagnostics(checkpoints)'
        aggregate = 'raise TestFailure(\n\t\t\t"XY-1343 focused evidence finalized with failures:'
        self.assertLess(focused.index(baseline), focused.index(behavior))
        self.assertLess(focused.index(behavior), focused.index(post_attempt))
        self.assertLess(focused.index(post_attempt), focused.index(diagnostics))
        self.assertLess(focused.index(diagnostics), focused.index(aggregate))
        self.assertEqual(focused.count('run(["pg_dump"'), 1)
        self.assertEqual(focused.count('"pg_restore", "--exit-on-error"'), 1)
        self.assertIn("source_behavior_error = error", focused)
        self.assertIn("failures.extend(manifest_failures)", focused)
        self.assertIn("if not failures:", focused)
        self.assertIn('run_work_item_test("postgres_exact_work_item_restore", env)', focused)
        self.assertIn('for component in ("schema", "authority")', self.harness)
        self.assertIn('"checkpoints_equal": checkpoints_equal', self.harness)

    def test_normalized_relations_are_same_project_and_bounded(self) -> None:
        for table in (
            "work_items", "work_item_objectives", "work_item_edges",
            "work_item_readiness_blockers", "work_item_acceptances",
        ):
            self.assertIn(f"CREATE TABLE decodex.{table}", self.migration)
        self.assertGreaterEqual(self.migration.count("FOREIGN KEY (work_item_id, project_id)"), 3)
        self.assertIn("pg_catalog.cardinality(p_depends_on_ids) +", self.migration)
        self.assertIn("> 16384", self.migration)
        self.assertIn(">= 4096", self.migration)

    def test_exact_commands_own_receipts_activity_and_outbox(self) -> None:
        for function in (
            "create_work_item_exact", "update_work_item_exact",
            "assess_work_item_readiness_exact", "accept_work_item_exact",
        ):
            self.assertIn(f"CREATE FUNCTION decodex.{function}", self.migration)
            self.assertIn(function, self.authority)
        self.assertIn("decodex.exact_command_receipts", self.migration)
        self.assertIn("INSERT INTO decodex.activity", self.migration)
        self.assertIn("INSERT INTO decodex.outbox", self.migration)
        self.assertIn("exact idempotency conflict", self.migration)

    def test_readiness_is_current_state_transactional_and_nontransferable(self) -> None:
        readiness = self.migration.split(
            "CREATE FUNCTION decodex.assess_work_item_readiness_exact", 1
        )[1].split("CREATE FUNCTION decodex.accept_work_item_exact", 1)[0]
        self.assertIn("FOR UPDATE OF work", readiness)
        self.assertIn("FOR SHARE OF project,lead", readiness)
        self.assertIn("FOR SHARE OF objective", readiness)
        self.assertIn("FOR SHARE OF related", readiness)
        self.assertIn("work_item_readiness_blockers", readiness)
        self.assertIn("target_state='ready'", readiness)
        self.assertNotIn("RETURNS TABLE", readiness)
        self.assertNotIn("permit", readiness.lower())

    def test_cycle_rejection_rolls_back_candidate_relations(self) -> None:
        update = self.migration.split(
            "CREATE FUNCTION decodex.update_work_item_exact", 1
        )[1].split("CREATE FUNCTION decodex.assess_work_item_readiness_exact", 1)[0]
        self.assertIn("BEGIN\n\t\tDELETE FROM decodex.work_item_readiness_blockers", update)
        self.assertIn("RAISE EXCEPTION 'candidate WorkItem graph contains a cycle'", update)
        self.assertIn("EXCEPTION WHEN SQLSTATE 'P1343'", update)
        self.assertIn("'dependency_cycle'", update)

    def test_acceptance_is_exact_revision_immutable_and_never_completes(self) -> None:
        acceptance = self.migration.split(
            "CREATE FUNCTION decodex.accept_work_item_exact", 1
        )[1].split("CREATE FUNCTION decodex.guard_work_item_running_resume", 1)[0]
        self.assertIn("item.state<>'review'", acceptance)
        self.assertIn("p_actor_id<>item.lead_agent_id", acceptance)
        self.assertIn("'provenance',p_provenance", acceptance)
        self.assertIn("work_item_revision", acceptance)
        self.assertIn("item.acceptance_criteria", acceptance)
        self.assertNotIn("UPDATE decodex.work_items", acceptance)
        self.assertIn("WorkItem acceptances are immutable", self.migration)

    def test_future_guard_is_inert_and_returns_no_authority(self) -> None:
        guard = self.migration.split(
            "CREATE FUNCTION decodex.guard_work_item_running_resume", 1
        )[1].split("REVOKE ALL ON TABLE", 1)[0]
        self.assertIn(") RETURNS void", guard)
        self.assertIn("FOR UPDATE", guard)
        self.assertIn("FOR SHARE OF project,lead", guard)
        self.assertIn("work_item_readiness", guard)
        for forbidden in (
            "INSERT INTO", "UPDATE decodex", "DELETE FROM",
            "decodex.exact_command_receipts", "decodex.outbox",
        ):
            self.assertNotIn(forbidden, guard)
        self.assertIn("Result<(), StoreError>", self.rust_api)


if __name__ == "__main__":
    unittest.main()
