"""Static architecture checks for the local SQLite product slice."""

from pathlib import Path
import plistlib
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def toml(path: str) -> dict[str, object]:
    with (ROOT / path).open("rb") as source:
        return tomllib.load(source)


class LocalSqliteArchitectureTests(unittest.TestCase):
    """Protect one owner, one normal store, and one bounded transfer path."""

    def test_database_is_one_discoverable_workspace_owner(self) -> None:
        workspace = toml("Cargo.toml")["workspace"]
        members = set(workspace["members"])
        dependencies = workspace["dependencies"]
        self.assertIn("database", members)
        self.assertIn("database/transfer", members)
        self.assertIn("bundled", dependencies["rusqlite"]["features"])

    def test_normal_runtime_has_no_transfer_store_dependency(self) -> None:
        runtime = toml("crates/decodex-runtime/Cargo.toml")
        dependencies = runtime["dependencies"]
        self.assertIn("decodex-database", dependencies)
        self.assertNotIn("redb", dependencies)
        transfer = toml("database/transfer/Cargo.toml")["dependencies"]
        self.assertIn("redb", transfer)
        self.assertIn("decodex-database", transfer)

    def test_clients_remain_protocol_only(self) -> None:
        for manifest_path in ("apps/decodex-gpui/Cargo.toml",):
            dependencies = toml(manifest_path)["dependencies"]
            with self.subTest(manifest=manifest_path):
                self.assertIn("decodex-protocol", dependencies)
                self.assertNotIn("decodex-database", dependencies)
                self.assertNotIn("rusqlite", dependencies)
                self.assertNotIn("redb", dependencies)
        cli_dependencies = toml("apps/decodex-cli/Cargo.toml")["dependencies"]
        self.assertIn("decodex-protocol", cli_dependencies)
        self.assertIn("decodex-runtime", cli_dependencies)
        self.assertNotIn("decodex-database", cli_dependencies)
        self.assertNotIn("rusqlite", cli_dependencies)
        self.assertNotIn("redb", cli_dependencies)

    def test_desktop_has_one_foreground_app_bundle(self) -> None:
        manifests = sorted(ROOT.glob("apps/*/packaging/Info.plist"))
        self.assertEqual(len(manifests), 1)
        with manifests[0].open("rb") as source:
            info = plistlib.load(source)
        self.assertEqual(info["CFBundleIdentifier"], "box.acg.decodex")
        self.assertFalse(info.get("LSBackgroundOnly", False))

    def test_shared_auth_observation_must_not_terminate_other_apps(self) -> None:
        coordinator = read("crates/decodex-runtime/src/shared_auth_coordinator.rs")
        for forbidden in ("std::process::Command", "libc::kill", "SIGTERM", "SIGKILL"):
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, coordinator)

    def test_credentials_are_narrow_and_daemon_private(self) -> None:
        credentials = read("database/src/credentials.rs")
        adapter = read("crates/decodex-runtime/src/host_credentials/sqlite_store.rs")
        self.assertIn("Zeroizing<Vec<u8>>", credentials)
        self.assertIn("Debug for CredentialRecord", credentials)
        self.assertIn("SqliteCredentialStore", adapter)
        self.assertNotIn("security_framework::passwords", adapter)
        self.assertNotIn("redb", adapter)

    def test_account_transfer_is_one_shot_read_only_and_source_retaining(self) -> None:
        transfer = read("database/transfer/src/main.rs")
        installer = read("scripts/macos/install_decodex_local_service.py")
        staging = read("scripts/macos/stage_decodex_local_service.sh")
        self.assertIn("ReadOnlyDatabase::open", transfer)
        self.assertIn("account_credentials_v1", transfer)
        self.assertIn("source_vault_retained", transfer)
        self.assertNotRegex(transfer, r"arg\([^\n]*source")
        self.assertIn('"decodex-database-transfer"', installer)
        self.assertIn('"serve"', installer)
        self.assertIn("-p decodex-database-transfer", staging)
        self.assertIn("box.acg.decodex.database-transfer", staging)
        self.assertIn("--profile \"$PROFILE\"", staging)
        self.assertIn("cargo +stable build --locked", staging)
        for retired in ("pg_ctl", "initdb", "createuser", "createdb"):
            self.assertNotIn(retired, installer)

    def test_process_acceptance_ports_are_explicit_and_release_closed(self) -> None:
        account_service = read("crates/decodex-runtime/src/account_service.rs")
        bootstrap = read("crates/decodex-runtime/src/bootstrap.rs")
        shared_auth = read("crates/decodex-runtime/src/shared_auth_coordinator.rs")
        process_test = read("apps/decodex-cli/tests/account_route_process.rs")
        supervisor_test = read("apps/decodex-gpui/src/bundled_daemon.rs")
        self.assertIn(
            '#[cfg(all(feature = "process-acceptance-fixture", debug_assertions))]',
            account_service,
        )
        self.assertIn('Ok(REFRESH_ENDPOINT.to_owned())', account_service)
        self.assertIn('endpoint.host_str() == Some("127.0.0.1")', account_service)
        self.assertIn(
            ".filter(|endpoint| process_test_refresh_endpoint_is_safe(endpoint))",
            account_service,
        )
        self.assertIn("process_acceptance_fixture_endpoint().is_some()", bootstrap)
        self.assertIn("AccountApiRuntime::new", bootstrap)
        self.assertIn("process_acceptance_fixture_endpoint().is_some()", shared_auth)
        self.assertIn('CARGO_BIN_EXE_decodex', process_test)
        self.assertIn('actual_service_routes_a_b_a', process_test)
        self.assertIn("reconcile_projected_fixed_route_on_startup", account_service)
        self.assertIn("startup must repair the post-auth/pre-routing crash window", process_test)
        self.assertIn("startup must not infer a fixed target while routing is balanced", process_test)
        self.assertIn("startup must not rotate or rewrite exact credential bytes", process_test)
        self.assertIn('assert_no_credentials', process_test)
        self.assertIn('process_listener_loss_restarts_exact_owned_daemon', supervisor_test)
        self.assertIn('process_recovery_never_terminates_independently_managed_daemon', supervisor_test)

    def test_provider_thread_identity_has_one_bound_and_one_url_projector(self) -> None:
        core = read("crates/decodex-core/src/conversation.rs")
        codex = read("crates/decodex-codex/src/protocol.rs")
        protocol = read("crates/decodex-protocol/src/conversation.rs")
        resume = read("crates/decodex-runtime/src/provider_attempt_service.rs")
        application = read("crates/decodex-runtime/src/application.rs")
        packs = read("crates/decodex-runtime/src/domain_packs.rs")
        shell = read("apps/decodex-gpui/src/shell.rs")
        self.assertIn("MAX_PROVIDER_THREAD_ID_BYTES: usize = 512", core)
        self.assertIn("decodex_core::MAX_PROVIDER_THREAD_ID_BYTES", codex)
        self.assertIn("pub use decodex_core::MAX_PROVIDER_THREAD_ID_BYTES", protocol)
        self.assertIn("ExactThreadId::new(response.codex_thread_id.clone())", resume)
        for consumer in (application, packs, shell):
            self.assertIn(".codex_url()", consumer)
            self.assertNotIn('format!("codex://threads/', consumer)

    def test_retired_board_and_execution_decision_surfaces_stay_absent(self) -> None:
        application = read("crates/decodex-runtime/src/application.rs")
        protocol = read("crates/decodex-protocol/src/wire.rs")
        protocol_exports = read("crates/decodex-protocol/src/lib.rs")
        for retired in (
            "WorkItemBoard",
            "ListProjects",
            "GetWorkItemBoardPage",
            "RegisterProject",
            "CreateWorkItem",
            "StartWorkItem",
            "AcceptWorkItem",
        ):
            self.assertNotIn(retired, application)
            self.assertNotIn(retired, protocol)
        for retired in (
            "GetExecutionDecision",
            "ExecutionDecisionResult",
            "ExecutionDecisionDto",
            "ExecutionConsumerDto",
            "ExecutionRouteDto",
            "ExecutionRouteCauseDto",
            "ExecutionRouteBlockerDto",
            "ExecutionQuotaExclusionDto",
            "ExecutionQuotaWindowDto",
            "execution_decision_dto",
            "quota_exclusion_dto",
            "blocker_dto",
        ):
            with self.subTest(retired=retired):
                self.assertNotIn(retired, application)
                self.assertNotIn(retired, protocol)
                self.assertNotIn(retired, protocol_exports)
        self.assertNotIn("#[cfg(any())]", application)
        self.assertFalse((ROOT / "apps/decodex-gpui/src/work_items.rs").exists())
        self.assertFalse((ROOT / "crates/decodex-core/src/managed_repository.rs").exists())
        self.assertNotIn("ManagedRepository", protocol_exports)
        self.assertNotIn("ManagedRepository", read("crates/decodex-protocol/src/doctor.rs"))
        for retired in (
            "managed_repository_disabled.rs",
            "managed_repository_runtime.rs",
            "managed_repository_saga.rs",
            "managed_repository_executor.rs",
            "work_item_board.rs",
            "local_account_authority.rs",
        ):
            self.assertFalse((ROOT / "crates/decodex-runtime/src" / retired).exists())

    def test_agent_replaces_active_factory_but_preserves_historical_storage(self) -> None:
        wire = read("crates/decodex-protocol/src/wire.rs")
        commands = wire[wire.index("pub enum CommandPayload"):wire.index("pub enum ResultPayload")]
        for retired in ("CreateProgramCycle", "BindProgramDomainPack", "ContinueProgram", "RecordProgramReview"):
            self.assertNotIn(retired, commands)
        shell = read("apps/decodex-gpui/src/shell.rs")
        self.assertNotIn("Destination::Factory", shell)
        for retired in ("programs.rs", "program_graph.rs", "factory_surface.rs"):
            self.assertFalse((ROOT / "apps/decodex-gpui/src" / retired).exists())
        self.assertIn("GetAgentSnapshot", wire)
        self.assertIn("GetProgramCycle", wire)
        self.assertIn("ListPrograms", wire)
        self.assertIn("program_cycles", read("database/src/lib.rs"))
        self.assertIn("agent_work_items", read("database/migrations/0048_agent_baseline.sql"))


if __name__ == "__main__":
    unittest.main()
