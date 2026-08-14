from __future__ import annotations

import argparse
import contextlib
import importlib.util
import io
import json
import os
import plistlib
import stat
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path
from unittest import mock


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts/macos/install_decodex_local_service.py"


def load_module():
    spec = importlib.util.spec_from_file_location(
        "install_decodex_local_service",
        SCRIPT_PATH,
    )
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


class FakeNamespaceLock:
    def __init__(self) -> None:
        self.closed = False

    def verify(self) -> None:
        if self.closed:
            raise AssertionError("closed namespace lock was used")

    def close(self) -> None:
        self.closed = True


class LocalServiceInstallerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = load_module()

    def paths(self, root: Path):
        root = root.resolve()
        return self.module.InstallPaths(
            repository=REPO_ROOT,
            root=root,
            config=root / "config.toml",
            data_directory=root / "postgres/data",
            socket_directory=root / "postgres/socket",
            log_directory=root / "logs",
            postgres_log=root / "logs/postgres.log",
            service_log=root / "logs/local-service.log",
            launch_agent=root / "space.decodex.local-service.plist",
            decodexd=root / "bin/decodexd",
            decodex_cli=root / "bin/decodex",
            codex=root / "codex-bin/codex",
            postgres=root / "bin/postgres",
            initdb=root / "bin/initdb",
            pg_isready=root / "bin/pg_isready",
            psql=root / "bin/psql",
        )

    def daemon_executable(self, paths):
        return {
            "identifier": "box.acg.decodex.daemon",
            "team_identifier": "T54QFA7W2S",
            "sha256": "1" * 64,
        }

    def account_document(self, account_ids: list[str]) -> dict[str, object]:
        accounts = [{"account_id": account_id} for account_id in account_ids]
        return {
            "schema": "decodex/cli-account/1",
            "command": "list",
            "outcome": "success",
            "result": {
                "outcome": "available",
                "data": {
                    "accounts": accounts,
                    "routing": {
                        "revision": 1,
                        "mode": {"mode": "balanced"},
                        "order": account_ids,
                    },
                },
            },
        }

    def doctor_document(self) -> dict[str, object]:
        required = (
            "configuration",
            "product_store",
            "protocol",
            "protocol_version",
            "server_identity",
        )
        return {
            "schema": "decodex/cli-diagnostics/1",
            "command": "doctor",
            "outcome": "report",
            "report": {
                "checks": [
                    {
                        "component": {"kind": component},
                        "status": {"state": "ready"},
                    }
                    for component in required
                ]
            },
        }

    def namespace_lock(self, root: Path):
        paths = self.paths(root)
        paths.server_directory.mkdir(parents=True, mode=0o700)
        os.chmod(paths.server_directory, 0o700)
        return paths, self.module.InstallerNamespaceLock.acquire(
            paths,
            os.geteuid(),
        )

    def test_source_has_only_the_current_install_path(self) -> None:
        source = SCRIPT_PATH.read_text(encoding="utf-8")
        for retired_term in (
            "legacy",
            "account-migration",
            "migration_manifest",
            "transition_gate",
            "staging_config",
            "receipt_phase",
            "finalize_account",
            "daemon_wrapper",
            "embedded.provisionprofile",
            "keychain-access-groups",
        ):
            with self.subTest(term=retired_term):
                self.assertNotIn(retired_term, source.lower())
        self.assertIn('"supervise-local"', source)
        self.assertIn('"account",\n                "list"', source)

    def test_parser_has_no_old_account_source_argument(self) -> None:
        args = self.module.parse_args(["--no-launch"])
        self.assertTrue(args.no_launch)
        self.assertFalse(hasattr(args, "legacy_accounts"))
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                self.module.parse_args(
                    ["--legacy-accounts", "/private/retired/accounts.jsonl"]
                )

    def test_install_paths_exposes_only_current_service_assets(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            args = argparse.Namespace(
                repository=REPO_ROOT,
                root=root,
                launch_agent=root / "agent.plist",
                decodexd=root / "decodexd",
                decodex_cli=root / "decodex",
                codex=root / "codex",
                postgres=root / "postgres",
                initdb=root / "initdb",
                pg_isready=root / "pg_isready",
                psql=root / "psql",
            )
            paths = self.module.install_paths(args)
        self.assertEqual(paths.config, root.resolve() / "config.toml")
        for retired_field in (
            "legacy_accounts",
            "legacy_config",
            "mapping",
            "migration_manifest",
            "credential_directory",
            "staging_config",
        ):
            self.assertFalse(hasattr(paths, retired_field))

    def test_config_and_launch_agent_are_credential_negative(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            config = self.module.render_config(paths, 501)
            config_document = tomllib.loads(config.decode("utf-8"))
            launch_agent = plistlib.loads(self.module.render_launch_agent(paths))

        self.assertEqual(config_document["active_profile"], "local")
        self.assertEqual(config_document["profiles"]["local"]["policy"], "same_uid")
        self.assertNotIn("migration", config_document["postgres"])
        self.assertNotIn(self.module.POSTGRES_SCHEMA_ROLE, config.decode("utf-8"))
        self.assertEqual(
            config_document["postgres"]["runtime"]["user"],
            self.module.POSTGRES_RUNTIME_ROLE,
        )
        self.assertEqual(
            launch_agent["ProgramArguments"][0:2],
            [str(paths.decodexd), "supervise-local"],
        )
        self.assertEqual(
            set(launch_agent["EnvironmentVariables"]),
            {"HOME", "PATH"},
        )
        self.assertEqual(
            launch_agent["KeepAlive"],
            {"SuccessfulExit": False},
        )
        self.assertEqual(launch_agent["ExitTimeOut"], 60)
        serialized = config + plistlib.dumps(launch_agent)
        for secret_projection in (
            b"access_token",
            b"refresh_token",
            b"id_token",
            b"auth.json",
            b"accounts.jsonl",
        ):
            self.assertNotIn(secret_projection, serialized)

    def test_atomic_write_replaces_without_backup_and_keeps_exact_mode(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            destination = Path(temp) / "config.toml"
            destination.write_bytes(b"old")
            destination.chmod(0o600)
            self.module.atomic_write(destination, b"new", 0o600)

            self.assertEqual(destination.read_bytes(), b"new")
            self.assertEqual(stat.S_IMODE(destination.stat().st_mode), 0o600)
            self.assertEqual(
                [entry.name for entry in destination.parent.iterdir()],
                ["config.toml"],
            )

    def test_daemon_digest_refuses_symlink_link_alias_and_unsafe_mode(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable = root / "decodexd"
            executable.write_bytes(b"signed-daemon-fixture")
            executable.chmod(0o755)
            expected = self.module.hashlib.sha256(
                b"signed-daemon-fixture"
            ).hexdigest()

            self.assertEqual(
                self.module.executable_sha256(executable, "Decodex daemon"),
                expected,
            )

            symlink = root / "decodexd-symlink"
            symlink.symlink_to(executable)
            with self.assertRaisesRegex(
                self.module.InstallError,
                "executable authority is unsafe",
            ):
                self.module.executable_sha256(symlink, "Decodex daemon")

            alias = root / "decodexd-alias"
            os.link(executable, alias)
            with self.assertRaisesRegex(
                self.module.InstallError,
                "executable authority is unsafe",
            ):
                self.module.executable_sha256(executable, "Decodex daemon")
            alias.unlink()

            executable.chmod(0o775)
            with self.assertRaisesRegex(
                self.module.InstallError,
                "executable authority is unsafe",
            ):
                self.module.executable_sha256(executable, "Decodex daemon")

            executable.chmod(0o644)
            with self.assertRaisesRegex(
                self.module.InstallError,
                "executable authority is unsafe",
            ):
                self.module.executable_sha256(executable, "Decodex daemon")

    def test_cli_signature_must_match_daemon_team_and_hardened_runtime(self) -> None:
        verify = subprocess.CompletedProcess(
            ["/usr/bin/codesign"],
            0,
            "",
            "",
        )
        details = subprocess.CompletedProcess(
            ["/usr/bin/codesign"],
            0,
            "",
            (
                "Identifier=box.acg.decodex.cli\n"
                "TeamIdentifier=T54QFA7W2S\n"
                "CodeDirectory v=20500 size=512 flags=0x10000(runtime) hashes=8\n"
            ),
        )
        with (
            mock.patch.object(
                self.module,
                "executable_sha256",
                return_value="0" * 64,
            ),
            mock.patch.object(
                self.module,
                "run",
                side_effect=[verify, details],
            ) as run,
        ):
            result = self.module.verify_signed_cli(
                Path("/private/decodex"),
                "T54QFA7W2S",
            )
        self.assertEqual(
            result,
            {
                "identifier": "box.acg.decodex.cli",
                "team_identifier": "T54QFA7W2S",
            },
        )
        self.assertEqual(run.call_count, 2)

        wrong_team = subprocess.CompletedProcess(
            ["/usr/bin/codesign"],
            0,
            "",
            (
                "Identifier=box.acg.decodex.cli\n"
                "TeamIdentifier=OTHERTEAM\n"
                "CodeDirectory v=20500 size=512 flags=0x10000(runtime) hashes=8\n"
            ),
        )
        with (
            mock.patch.object(
                self.module,
                "executable_sha256",
                return_value="0" * 64,
            ),
            mock.patch.object(
                self.module,
                "run",
                side_effect=[verify, wrong_team],
            ),
        ):
            with self.assertRaisesRegex(
                self.module.InstallError,
                "CLI signature did not verify",
            ):
                self.module.verify_signed_cli(
                    Path("/private/decodex"),
                    "T54QFA7W2S",
                )

    def test_launch_agent_daemon_binding_is_verified(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.launch_agent.parent.mkdir(parents=True, exist_ok=True)
            paths.launch_agent.write_bytes(
                self.module.render_launch_agent(paths)
            )
            paths.launch_agent.chmod(0o600)
            expected = self.daemon_executable(paths)
            with mock.patch.object(
                self.module,
                "inspect_daemon_executable",
                return_value=expected,
            ):
                self.assertEqual(
                    self.module.verify_daemon_executable(
                        paths,
                        expected,
                        require_launch_agent=True,
                    ),
                    expected,
                )

    def test_missing_postgres_cluster_is_initialized_once(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.data_directory.mkdir(parents=True)
            share = paths.initdb.parent.parent / "share/postgresql"
            share.mkdir(parents=True)
            with (
                mock.patch.object(
                    self.module,
                    "postgres_version",
                    side_effect=[None, "18"],
                ),
                mock.patch.object(self.module, "run") as run,
            ):
                self.module.initialize_cluster(paths, os.geteuid())
            run.assert_called_once()
            command = run.call_args.args[0]
            self.assertEqual(command[0], str(paths.initdb))
            self.assertIn("--auth-local=trust", command)
            self.assertIn("--auth-host=reject", command)

        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.data_directory.mkdir(parents=True)
            with (
                mock.patch.object(
                    self.module,
                    "postgres_version",
                    return_value="18",
                ),
                mock.patch.object(self.module, "run") as run,
            ):
                self.module.initialize_cluster(paths, os.geteuid())
            run.assert_not_called()

    def test_psql_environment_drops_inherited_postgres_settings(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            with (
                mock.patch.dict(
                    os.environ,
                    {
                        "PGPASSWORD": "must-not-survive",
                        "PGSERVICE": "must-not-survive",
                        "KEPT": "yes",
                    },
                    clear=True,
                ),
                mock.patch.object(
                    self.module.pwd,
                    "getpwuid",
                    return_value=mock.Mock(pw_name="local-user"),
                ),
            ):
                environment = self.module.psql_environment(paths)
        self.assertNotIn("PGPASSWORD", environment)
        self.assertNotIn("PGSERVICE", environment)
        self.assertEqual(environment["PGUSER"], "local-user")
        self.assertEqual(environment["KEPT"], "yes")

    def test_roles_and_database_are_current_service_only(self) -> None:
        statements: list[str] = []

        def scalar(_paths, _database, sql, _environment):
            statements.append(sql)
            if "SELECT 1 FROM pg_catalog.pg_roles" in sql:
                return "1"
            if "SELECT CASE WHEN role.rolcanlogin" in sql:
                return "1"
            if "SELECT 1 FROM pg_catalog.pg_database" in sql:
                return "1"
            if "SELECT role.rolname FROM pg_catalog.pg_database" in sql:
                return self.module.POSTGRES_SCHEMA_ROLE
            return ""

        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            with mock.patch.object(
                self.module,
                "psql_scalar",
                side_effect=scalar,
            ):
                self.module.ensure_roles_and_database(paths, {"PGUSER": "owner"})

        combined = "\n".join(statements)
        self.assertIn(self.module.POSTGRES_SCHEMA_ROLE, combined)
        self.assertIn(self.module.POSTGRES_RUNTIME_ROLE, combined)
        self.assertNotIn("account_migration", combined)
        self.assertNotIn("receipt", combined)

    def test_account_list_parser_accepts_empty_current_registry(self) -> None:
        document = self.account_document([])
        self.assertEqual(self.module.account_ids_from_result(document), [])

        account_id = "10000000-0000-4000-8000-000000000001"
        populated = self.account_document([account_id])
        self.assertEqual(
            self.module.account_ids_from_result(populated),
            [account_id],
        )

        other_id = "10000000-0000-4000-8000-000000000002"
        reordered = self.account_document([account_id, other_id])
        reordered["result"]["data"]["routing"]["order"] = [other_id, account_id]
        self.assertEqual(
            self.module.account_ids_from_result(reordered),
            [account_id, other_id],
        )

        old_shape = self.account_document([])
        old_shape["result"] = {"accounts": []}
        self.assertIsNone(self.module.account_ids_from_result(old_shape))

    def test_wait_for_service_accepts_empty_registry_after_doctor(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            with (
                mock.patch.object(self.module, "query_doctor", return_value=True),
                mock.patch.object(
                    self.module,
                    "query_accounts",
                    return_value=self.account_document([]),
                ),
            ):
                self.assertEqual(self.module.wait_for_service(paths), [])

    def test_doctor_requires_the_service_foundation_to_be_ready(self) -> None:
        document = self.doctor_document()
        self.assertTrue(self.module.service_foundation_is_ready(document))
        product_store = next(
            check
            for check in document["report"]["checks"]
            if check["component"]["kind"] == "product_store"
        )
        product_store["status"] = {"state": "unavailable"}
        self.assertFalse(self.module.service_foundation_is_ready(document))

    def test_query_accounts_uses_explicit_root(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            completed = subprocess.CompletedProcess(
                ["decodex"],
                0,
                json.dumps(self.account_document([])),
                "",
            )
            with mock.patch.object(
                self.module,
                "run",
                return_value=completed,
            ) as run:
                result = self.module.query_accounts(paths)
        self.assertEqual(result, self.account_document([]))
        command = run.call_args.args[0]
        self.assertEqual(
            command,
            [
                str(paths.decodex_cli),
                "--root",
                str(paths.root),
                "--output",
                "json",
                "account",
                "list",
            ],
        )

    def test_installed_launch_agent_contract_enables_graceful_drain(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.launch_agent.write_bytes(
                self.module.render_launch_agent(paths)
            )
            paths.launch_agent.chmod(0o600)
            self.assertTrue(
                self.module.installed_launch_agent_supports_graceful_drain(
                    paths.launch_agent,
                    os.geteuid(),
                )
            )

            document = plistlib.loads(paths.launch_agent.read_bytes())
            document["KeepAlive"] = True
            paths.launch_agent.write_bytes(plistlib.dumps(document))
            paths.launch_agent.chmod(0o600)
            self.assertFalse(
                self.module.installed_launch_agent_supports_graceful_drain(
                    paths.launch_agent,
                    os.geteuid(),
                )
            )

    def test_bootout_drains_current_contract_before_removal(self) -> None:
        identity = self.module.ProcessIdentity(42, "Mon Jul 27 00:00:00 2026")
        initial = self.module.ServiceObservation(
            True,
            42,
            identity,
            frozenset({identity}),
        )
        inactive = self.module.ServiceObservation(True, None, None, frozenset())
        completed = subprocess.CompletedProcess(["launchctl"], 0, "", "")
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            with (
                mock.patch.object(
                    self.module,
                    "observe_service",
                    return_value=initial,
                ),
                mock.patch.object(
                    self.module,
                    "installed_launch_agent_supports_graceful_drain",
                    return_value=True,
                ),
                mock.patch.object(
                    self.module,
                    "drain_service",
                    return_value=inactive,
                ) as drain,
                mock.patch.object(
                    self.module,
                    "run_settlement_command",
                    return_value=completed,
                ),
                mock.patch.object(
                    self.module,
                    "wait_for_process_generation_exit",
                ) as wait,
            ):
                self.module.bootout_service(paths, 501)
        drain.assert_called_once()
        wait.assert_called_once()

    def test_install_bootstraps_fresh_and_validates_existing_database(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths, namespace_lock = self.namespace_lock(Path(temp))
            daemon = self.daemon_executable(paths)
            process = object()
            try:
                with (
                    mock.patch.object(
                        self.module,
                        "verify_daemon_executable",
                        return_value=daemon,
                    ) as verify_daemon,
                    mock.patch.object(
                        self.module,
                        "verify_signed_cli",
                    ) as verify_cli,
                    mock.patch.object(self.module, "initialize_cluster") as initialize,
                    mock.patch.object(
                        self.module,
                        "start_temporary_postgres",
                        return_value=process,
                    ) as start,
                    mock.patch.object(
                        self.module,
                        "stop_temporary_postgres",
                    ) as stop,
                    mock.patch.object(
                        self.module,
                        "psql_environment",
                        return_value={"PGUSER": "owner"},
                    ) as environment,
                    mock.patch.object(
                        self.module,
                        "ensure_roles_and_database",
                        side_effect=[True, False],
                    ) as ensure_database,
                    mock.patch.object(
                        self.module,
                        "bootstrap_latest_schema",
                    ) as bootstrap_schema,
                    mock.patch.object(
                        self.module,
                        "validate_current_authority",
                    ) as validate_authority,
                ):
                    ordered = mock.Mock()
                    ordered.attach_mock(initialize, "initialize_cluster")
                    ordered.attach_mock(start, "start_temporary_postgres")
                    ordered.attach_mock(environment, "psql_environment")
                    ordered.attach_mock(
                        ensure_database,
                        "ensure_roles_and_database",
                    )
                    ordered.attach_mock(bootstrap_schema, "bootstrap_latest_schema")
                    ordered.attach_mock(validate_authority, "validate_current_authority")
                    ordered.attach_mock(stop, "stop_temporary_postgres")
                    for _ in range(2):
                        self.module.install_under_namespace_lock(
                            paths,
                            os.geteuid(),
                            namespace_lock,
                            daemon,
                        )
            finally:
                namespace_lock.close()

            config = paths.config.read_text(encoding="utf-8")
            launch_agent = plistlib.loads(paths.launch_agent.read_bytes())

        self.assertIn("[postgres.runtime]", config)
        self.assertNotIn("accounts.jsonl", config)
        self.assertEqual(
            launch_agent["ProgramArguments"][0:2],
            [str(paths.decodexd), "supervise-local"],
        )
        self.assertEqual(verify_daemon.call_count, 4)
        self.assertEqual(verify_cli.call_count, 4)
        self.assertEqual(
            ordered.mock_calls,
            [
                mock.call.initialize_cluster(paths, os.geteuid()),
                mock.call.start_temporary_postgres(paths),
                mock.call.psql_environment(paths),
                mock.call.ensure_roles_and_database(paths, {"PGUSER": "owner"}),
                mock.call.bootstrap_latest_schema(paths),
                mock.call.stop_temporary_postgres(process),
                mock.call.initialize_cluster(paths, os.geteuid()),
                mock.call.start_temporary_postgres(paths),
                mock.call.psql_environment(paths),
                mock.call.ensure_roles_and_database(paths, {"PGUSER": "owner"}),
                mock.call.validate_current_authority(paths),
                mock.call.stop_temporary_postgres(process),
            ],
        )

    def test_latest_schema_bootstrap_uses_only_the_installed_daemon(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            with mock.patch.object(
                self.module,
                "run",
                return_value=subprocess.CompletedProcess([], 0, "", ""),
            ) as run:
                self.module.bootstrap_latest_schema(paths)

        run.assert_called_once_with(
            [
                str(paths.decodexd),
                "bootstrap-latest-schema",
                "--root",
                str(paths.root),
                "--schema-owner-user",
                self.module.POSTGRES_SCHEMA_ROLE,
            ],
            cwd=paths.repository,
            capture=True,
        )

    def test_current_authority_validation_uses_only_the_installed_daemon(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            with mock.patch.object(
                self.module,
                "run",
                return_value=subprocess.CompletedProcess([], 0, "", ""),
            ) as run:
                self.module.validate_current_authority(paths)

        run.assert_called_once_with(
            [
                str(paths.decodexd),
                "validate-current-authority",
                "--root",
                str(paths.root),
            ],
            cwd=paths.repository,
            capture=True,
        )

    def test_installed_config_readback_must_match_before_database_start(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            daemon = self.daemon_executable(paths)
            namespace_lock = FakeNamespaceLock()
            with (
                mock.patch.object(self.module, "ensure_directories"),
                mock.patch.object(
                    self.module,
                    "verify_daemon_executable",
                    return_value=daemon,
                ),
                mock.patch.object(self.module, "verify_signed_cli"),
                mock.patch.object(self.module, "atomic_write"),
                mock.patch.object(
                    self.module,
                    "read_owned_file",
                    return_value=b"different",
                ),
                mock.patch.object(self.module, "initialize_cluster") as initialize,
                mock.patch.object(
                    self.module,
                    "start_temporary_postgres",
                ) as start,
            ):
                with self.assertRaisesRegex(
                    self.module.InstallError,
                    "installed Decodex config differs",
                ):
                    self.module.install_under_namespace_lock(
                        paths,
                        os.geteuid(),
                        namespace_lock,
                        daemon,
                    )
            initialize.assert_not_called()
            start.assert_not_called()

    def test_main_reports_success_for_empty_latest_registry(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            args = argparse.Namespace(no_launch=False)
            daemon = self.daemon_executable(paths)
            namespace_lock = FakeNamespaceLock()
            output = io.StringIO()
            with (
                mock.patch.object(self.module, "parse_args", return_value=args),
                mock.patch.object(self.module, "install_paths", return_value=paths),
                mock.patch.object(self.module, "validate_host", return_value=501),
                mock.patch.object(
                    self.module,
                    "inspect_daemon_executable",
                    return_value=daemon,
                ),
                mock.patch.object(self.module, "verify_signed_cli"),
                mock.patch.object(self.module, "ensure_installer_namespace_layout"),
                mock.patch.object(self.module, "postgres_major", return_value=18),
                mock.patch.object(self.module, "bootout_service"),
                mock.patch.object(
                    self.module.InstallerNamespaceLock,
                    "acquire",
                    return_value=namespace_lock,
                ),
                mock.patch.object(self.module, "install_under_namespace_lock"),
                mock.patch.object(self.module, "bootstrap_service"),
                mock.patch.object(self.module, "wait_for_service", return_value=[]),
                contextlib.redirect_stdout(output),
            ):
                self.assertEqual(self.module.main([]), 0)

        result = json.loads(output.getvalue())
        self.assertEqual(
            result,
            {
                "schema": "decodex/local-service-install/1",
                "outcome": "success",
                "account_count": 0,
                "postgres_major": 18,
                "launch_agent": self.module.LAUNCH_AGENT_LABEL,
                "launched": True,
            },
        )
        self.assertTrue(namespace_lock.closed)


if __name__ == "__main__":
    unittest.main()
