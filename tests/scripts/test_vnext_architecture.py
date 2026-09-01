"""Static architecture checks for the local SQLite product slice."""

from pathlib import Path
import re
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

    def test_schema_is_versioned_and_owns_the_vertical_slice(self) -> None:
        migration = read("database/migrations/0001_local_product.sql")
        for table in (
            "schema_migrations",
            "accounts",
            "account_credentials",
            "routing_decisions",
            "continuation_plans",
            "conversations",
            "turns",
            "history_items",
            "runtime_sessions",
            "process_generations",
            "provider_attempts",
            "provider_attempt_positive_evidence",
        ):
            with self.subTest(table=table):
                self.assertRegex(migration, rf"CREATE TABLE {re.escape(table)}\s*\(")
        migrations = read("database/src/migrations.rs")
        repair = read("database/migrations/0002_nonempty_task_instructions.sql")
        execution_controls = read(
            "database/migrations/0003_quick_task_execution_controls.sql"
        )
        desktop_settings = read("database/migrations/0011_desktop_settings.sql")
        self.assertIn("schema_migrations", migrations)
        self.assertIn("0002_nonempty_task_instructions.sql", migrations)
        self.assertIn("0003_quick_task_execution_controls.sql", migrations)
        self.assertIn("0011_desktop_settings.sql", migrations)
        self.assertIn("BETWEEN 1 AND 65536", repair)
        self.assertIn("Follow the user request for this task.", repair)
        for column in ("model", "reasoning_effort", "fast"):
            self.assertIn(f"ADD COLUMN {column}", execution_controls)
        self.assertIn("CREATE TABLE desktop_settings", desktop_settings)
        self.assertIn("show_in_menu_bar", desktop_settings)
        self.assertIn("TransactionBehavior::Immediate", migrations)
        self.assertIn("PRAGMA foreign_keys = ON", migrations)
        self.assertIn("PRAGMA synchronous = FULL", migrations)
        self.assertIn("PRAGMA journal_mode = WAL", migrations)

    def test_daemon_composes_sqlite_directly(self) -> None:
        bootstrap = read("crates/decodex-runtime/src/bootstrap.rs")
        application = read("crates/decodex-runtime/src/application.rs")
        daemon = read("apps/decodexd/src/main.rs")
        self.assertIn("SqliteStore::open", bootstrap)
        self.assertIn("Available(SqliteStore)", application)
        self.assertIn("ProductStore::Available(store)", application)
        self.assertIn("InitializeLocalDatabase", daemon)
        self.assertIn("ValidateLocalDatabase", daemon)
        self.assertNotIn("SuperviseLocal", daemon)
        self.assertNotIn("BootstrapLatestSchema", daemon)

    def test_clients_remain_protocol_only(self) -> None:
        for manifest_path in ("apps/decodex-cli/Cargo.toml", "apps/decodex-gpui/Cargo.toml"):
            dependencies = toml(manifest_path)["dependencies"]
            with self.subTest(manifest=manifest_path):
                self.assertIn("decodex-protocol", dependencies)
                self.assertNotIn("decodex-database", dependencies)
                self.assertNotIn("rusqlite", dependencies)
                self.assertNotIn("redb", dependencies)

    def test_exact_current_protocol_and_artifact_cohort_cross_every_bundle_boundary(self) -> None:
        protocol = read("crates/decodex-protocol/src/lib.rs")
        gpui = read("apps/decodex-gpui/src/client_lifecycle/tests.rs")
        native_client = read("crates/decodex-app-client-ffi/src/lib.rs")
        staging = read("scripts/macos/stage_decodex_app.sh")
        bundle_verifier = read("scripts/macos/verify_decodex_bundle_contracts.py")
        self.assertIn("ProtocolVersion { major: 2, minor: 13 }", protocol)
        self.assertIn("CURRENT_ARTIFACT_COHORT: u32 = 9", protocol)
        self.assertIn("assert_eq!(CURRENT_VERSION.minor, 13)", gpui)
        self.assertIn("decodex_protocol::CURRENT_ARTIFACT_COHORT", native_client)
        self.assertIn("verify_decodex_bundle_contracts.py", staging)
        self.assertIn("decodex_app_native_client_artifact_cohort", bundle_verifier)

    def test_gpui_is_the_only_macos_gui_and_loads_the_original_swift_menu_bar(self) -> None:
        for retired in (
            "apps/decodex",
            "apps/decodex-app",
            "spikes/gpui",
        ):
            with self.subTest(retired=retired):
                self.assertFalse((ROOT / retired).exists())
        workspace = toml("Cargo.toml")["workspace"]
        self.assertNotIn("exclude", workspace)
        self.assertEqual(
            [member for member in workspace["members"] if member.endswith("-gpui")],
            ["apps/decodex-gpui"],
        )
        settings = read("apps/decodex-gpui/src/settings_surface.rs")
        main = read("apps/decodex-gpui/src/main.rs")
        native_menu_bar = read("apps/decodex-gpui/src/native_menu_bar.rs")
        launch_at_login = read(
            "apps/decodex-gpui/menubar/Sources/DecodexApp/LaunchAtLoginController.swift"
        )
        swift_menu_bar = read(
            "apps/decodex-gpui/menubar/Sources/DecodexApp/StatusPanelController.swift"
        )
        staging = read("scripts/macos/stage_decodex_app.sh")
        plist = read("apps/decodex-gpui/packaging/Info.plist")
        self.assertIn("NSStatusBar", swift_menu_bar)
        self.assertIn("libDecodexMenuBar.dylib", native_menu_bar)
        self.assertIn("decodex_menu_bar_set_visible", native_menu_bar)
        self.assertIn("SetDesktopSettings", read("apps/decodex-gpui/src/desktop_settings.rs"))
        self.assertIn("Show Decodex in the menu bar", settings)
        self.assertIn("SMAppService", launch_at_login)
        self.assertIn("service: SMAppService = .mainApp", launch_at_login)
        self.assertIn("keyAELaunchedAsLogInItem", launch_at_login)
        self.assertIn("on_window_should_close", main)
        self.assertIn("on_reopen", main)
        self.assertIn("order_out_native_windows();", main)
        self.assertIn("window.orderOut(None);", main)
        for daemon_owned_path in (
            "database/migrations/0011_desktop_settings.sql",
            "database/src/desktop_settings.rs",
            "crates/decodex-protocol/src/wire.rs",
            "crates/decodex-runtime/src/application.rs",
        ):
            with self.subTest(daemon_owned_path=daemon_owned_path):
                daemon_owned = read(daemon_owned_path)
                self.assertNotIn("launch_at_login", daemon_owned)
                self.assertNotIn("LaunchAtLogin", daemon_owned)
        self.assertIn("NSApplication::sharedApplication(main_thread).activate();", main)
        self.assertLess(
            main.index("window.activate_window()"),
            main.index("activate_native_application();"),
        )
        for retired in (
            "NSWorkspace",
            "NSUserDefaults",
            "Library/LoginItems/DecodexMenuBar.app",
        ):
            with self.subTest(retired=retired):
                self.assertNotIn(retired, settings + staging + native_menu_bar)
        self.assertIn('APP="$STAGE_ROOT/Decodex.app"', staging)
        self.assertIn('HELPERS="$CONTENTS/Helpers"', staging)
        self.assertIn('cp "$ROOT/target/release/decodexd" "$HELPERS/decodexd"', staging)
        self.assertIn("--product DecodexMenuBar", staging)
        self.assertIn(
            'DEFAULT_SIGN_IDENTITY="4EBCADF6B4D513E45CE33EC6934C08DBB0F03D7F"',
            staging,
        )
        self.assertIn('DEFAULT_SIGN_TEAM_IDENTIFIER="4N949UKQ55"', staging)
        self.assertIn('verify_signing_team "$APP"', staging)
        self.assertIn("ad-hoc signing is unsupported", staging)
        self.assertNotIn("DecodexMenuBar.app", staging)
        self.assertTrue((ROOT / "crates/decodex-app-client-ffi").is_dir())
        self.assertIn("<string>Decodex</string>", plist)
        self.assertIn("<string>box.acg.decodex</string>", plist)
        bundle_plists = sorted(ROOT.glob("apps/*/packaging/Info.plist"))
        self.assertEqual(
            [path.relative_to(ROOT).as_posix() for path in bundle_plists],
            ["apps/decodex-gpui/packaging/Info.plist"],
        )
        self.assertNotIn("LSBackgroundOnly", plist)
        service_stage = read("scripts/macos/stage_decodex_local_service.sh")
        self.assertIn(
            'install -m 755 "$ROOT/target/$PROFILE/decodexd" "$STAGE_ROOT/decodexd"',
            service_stage,
        )
        self.assertNotIn(".app", service_stage)

    def test_shared_auth_coordinator_is_read_only_until_quiescent_cutover(self) -> None:
        coordinator = read(
            "crates/decodex-runtime/src/shared_auth_coordinator.rs"
        )
        application = read("crates/decodex-runtime/src/application.rs")
        account_service = read("crates/decodex-runtime/src/account_service.rs")
        wire = read("crates/decodex-protocol/src/wire.rs")
        menu = read(
            "apps/decodex-gpui/menubar/Sources/DecodexApp/AccountControlViews.swift"
        )
        self.assertIn("proc_listpids", coordinator)
        self.assertIn("proc_pidpath", coordinator)
        self.assertIn("KERN_PROCARGS2", coordinator)
        self.assertIn("Zeroizing::new", coordinator)
        self.assertIn("CodexLiveness::MayBeRunning", coordinator)
        self.assertIn("MacosCodexHomeRelation::Isolated", coordinator)
        self.assertIn("CodexLivenessObservation::Blocked", coordinator)
        self.assertIn("project_shared_codex_auth_cas", coordinator)
        self.assertIn("AccountRouteWaitReasonDto", wire)
        self.assertIn("AccountRoutePendingStatusView", menu)
        self.assertIn("Waiting for Codex to close or restart.", menu)
        self.assertNotIn("PID \\(blocker.pid)", menu)
        self.assertNotIn("auth.json", menu)
        self.assertNotIn("atomic shared-auth", menu)
        for forbidden in (
            "std::process::Command",
            "libc::kill",
            "SIGTERM",
            "SIGKILL",
        ):
            self.assertNotIn(forbidden, coordinator)
        self.assertLess(
            application.index("shared_auth_may_be_running"),
            application.index("reclaim_account_route_command"),
        )
        self.assertNotIn("reproject_shared", account_service)

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
        process_test = read("apps/decodexd/tests/account_route_process.rs")
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
        self.assertIn('CARGO_BIN_EXE_decodexd', process_test)
        self.assertIn('actual_daemon_routes_a_b_a', process_test)
        self.assertIn('assert_no_credentials', process_test)
        self.assertIn('process_listener_loss_restarts_exact_owned_daemon', supervisor_test)
        self.assertIn('process_recovery_never_terminates_independently_managed_daemon', supervisor_test)

    def test_conversation_restart_contract_is_executable(self) -> None:
        test = read("database/tests/conversation_restart.rs")
        self.assertIn("read_ordinary_runtime_session_for_resume", test)
        self.assertIn("ContinuationPlanKind::SameThread", test)
        self.assertIn("PrepareProviderAttemptOutcome::Replayed", test)
        self.assertIn("restart must not create a duplicate dispatch intent", test)

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
        managed_repository = read("crates/decodex-runtime/src/managed_repository_disabled.rs")
        reset_card = read("crates/decodex-runtime/src/account_launch/api_reset_card_disabled.rs")
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
        self.assertIn("ManagedRepositoryUnavailableReason", managed_repository)
        self.assertIn("ProductStateUnavailable", reset_card)
        for retired in (
            "managed_repository_runtime.rs",
            "managed_repository_saga.rs",
            "managed_repository_executor.rs",
            "work_item_board.rs",
            "local_account_authority.rs",
        ):
            self.assertFalse((ROOT / "crates/decodex-runtime/src" / retired).exists())

    def test_current_openwiki_declares_sqlite_authority(self) -> None:
        quickstart = read("openwiki/quickstart.md")
        self.assertIn("bundled SQLite", quickstart)
        self.assertIn("database/", quickstart)
        self.assertIn("same Codex thread", quickstart)
        self.assertNotIn("accepted no-migration reset", quickstart)


if __name__ == "__main__":
    unittest.main()
