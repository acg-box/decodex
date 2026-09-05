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
            database=root / "server/decodex.sqlite3",
            retired_vault=root / "server/credentials.redb",
            log_directory=root / "logs",
            service_log=root / "logs/local-service.log",
            launch_agent=root / "space.decodex.local-service.plist",
            decodex=root / "bin/decodex",
            database_transfer=root / "bin/decodex-database-transfer",
            codex=root / "codex-bin/codex",
        )

    def service_executable(self):
        return {
            "identifier": "box.acg.decodex.cli",
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

    def test_source_has_one_direct_sqlite_install_path(self) -> None:
        source = SCRIPT_PATH.read_text(encoding="utf-8")
        for retired_term in (
            '"--initdb"',
            '"--pg-isready"',
            '"supervise-local"',
            "initialize_cluster",
            "ensure_roles_and_database",
        ):
            with self.subTest(term=retired_term):
                self.assertNotIn(retired_term, source)
        self.assertIn('[str(paths.decodex), "serve"]', source)
        self.assertNotIn("decodexd", source)
        self.assertNotIn("artifact_cohort", source)
        self.assertIn('"initialize-local-database"', source)
        self.assertIn('"validate-local-database"', source)

    def test_parser_exposes_only_current_arguments(self) -> None:
        args = self.module.parse_args(["--no-launch"])
        self.assertTrue(args.no_launch)
        for field in ("initdb", "pg_isready"):
            self.assertFalse(hasattr(args, field))

    def test_install_paths_are_local_database_owned(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            args = argparse.Namespace(
                repository=REPO_ROOT,
                root=root,
                launch_agent=root / "agent.plist",
                decodex=root / "decodex",
                database_transfer=root / "decodex-database-transfer",
                codex=root / "codex",
            )
            paths = self.module.install_paths(args)
        self.assertEqual(paths.database, root.resolve() / "server/decodex.sqlite3")
        self.assertEqual(paths.retired_vault, root.resolve() / "server/credentials.redb")
        for retired_field in ("data_directory", "socket_directory", "postgres_log"):
            self.assertFalse(hasattr(paths, retired_field))

    def test_config_and_launch_agent_have_no_database_endpoint_or_secret(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            config = self.module.render_config(paths, 501)
            config_document = tomllib.loads(config.decode("utf-8"))
            launch_agent = plistlib.loads(self.module.render_launch_agent(paths))

        self.assertEqual(config_document["active_profile"], "local")
        self.assertEqual(config_document["profiles"]["local"]["policy"], "same_uid")
        self.assertNotIn("database", config_document)
        self.assertEqual(
            launch_agent["ProgramArguments"],
            [str(paths.decodex), "serve"],
        )
        self.assertEqual(set(launch_agent["EnvironmentVariables"]), {"HOME", "PATH"})
        self.assertEqual(launch_agent["KeepAlive"], {"SuccessfulExit": False})
        self.assertEqual(launch_agent["ExitTimeOut"], 60)
        self.assertEqual(launch_agent["WorkingDirectory"], str(paths.root))
        self.assertNotEqual(launch_agent["WorkingDirectory"], str(paths.repository))
        serialized = config + plistlib.dumps(launch_agent)
        for secret_projection in (
            b"access_token",
            b"refresh_token",
            b"id_token",
            b"auth.json",
            b"credentials.redb",
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

    def test_service_digest_refuses_symlink_alias_and_unsafe_mode(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable = root / "decodex"
            executable.write_bytes(b"signed-service-fixture")
            executable.chmod(0o755)
            expected = self.module.hashlib.sha256(b"signed-service-fixture").hexdigest()
            self.assertEqual(
                self.module.executable_sha256(executable, "Decodex service"),
                expected,
            )

            symlink = root / "decodex-symlink"
            symlink.symlink_to(executable)
            with self.assertRaisesRegex(self.module.InstallError, "authority is unsafe"):
                self.module.executable_sha256(symlink, "Decodex service")

            alias = root / "decodex-alias"
            os.link(executable, alias)
            with self.assertRaisesRegex(self.module.InstallError, "authority is unsafe"):
                self.module.executable_sha256(executable, "Decodex service")
            alias.unlink()

            executable.chmod(0o775)
            with self.assertRaisesRegex(self.module.InstallError, "authority is unsafe"):
                self.module.executable_sha256(executable, "Decodex service")

    def test_signed_peer_must_match_service_team_and_optional_identifier(self) -> None:
        descriptor = {
            "identifier": "box.acg.decodex.database-transfer",
            "team_identifier": "T54QFA7W2S",
            "sha256": "0" * 64,
        }
        with mock.patch.object(
            self.module,
            "inspect_signed_executable",
            return_value=descriptor,
        ):
            self.assertEqual(
                self.module.verify_signed_peer(
                    Path("/private/transfer"),
                    "Decodex database transfer",
                    "T54QFA7W2S",
                    "box.acg.decodex.database-transfer",
                ),
                descriptor,
            )
            with self.assertRaisesRegex(self.module.InstallError, "signature did not verify"):
                self.module.verify_signed_peer(
                    Path("/private/transfer"),
                    "Decodex database transfer",
                    "OTHERTEAM",
                    "box.acg.decodex.database-transfer",
                )

    def test_launch_agent_service_binding_is_exact(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.launch_agent.write_bytes(self.module.render_launch_agent(paths))
            paths.launch_agent.chmod(0o600)
            expected = self.service_executable()
            with mock.patch.object(
                self.module,
                "inspect_service_executable",
                return_value=expected,
            ):
                self.assertEqual(
                    self.module.verify_service_executable(
                        paths,
                        expected,
                        require_launch_agent=True,
                    ),
                    expected,
                )

    def test_account_list_parser_requires_routing_order_to_match_rows(self) -> None:
        first = "10000000-0000-4000-8000-000000000001"
        second = "10000000-0000-4000-8000-000000000002"
        document = self.account_document([first, second])
        self.assertEqual(self.module.account_ids_from_result(document), [first, second])
        document["result"]["data"]["routing"]["order"] = [second, first]
        self.assertIsNone(self.module.account_ids_from_result(document))

    def test_account_list_parser_accepts_current_cli_shape_and_rejects_retired_pending(self) -> None:
        first = "10000000-0000-4000-8000-000000000001"
        second = "10000000-0000-4000-8000-000000000002"
        document = self.account_document([first, second])
        self.assertEqual(self.module.account_ids_from_result(document), [first, second])

        document["result"]["data"]["pending_route"] = {
            "operation_id": "20000000-0000-4000-8000-000000000001",
            "account_id": second,
            "routing_revision": 1,
        }
        self.assertIsNone(self.module.account_ids_from_result(document))

    @unittest.skipUnless(
        (REPO_ROOT / "target/debug/decodex").is_file(),
        "build the unified debug CLI before the real-output installer check",
    )
    def test_current_real_cli_account_list_output_is_accepted(self) -> None:
        executable = REPO_ROOT / "target/debug/decodex"
        with tempfile.TemporaryDirectory(dir=Path.home()) as temporary:
            home = Path(temporary).resolve()
            home.chmod(0o700)
            root = home / ".decodex"
            subprocess.run(
                [
                    str(executable),
                    "initialize-local-database",
                    "--root",
                    str(root),
                ],
                check=True,
                capture_output=True,
                text=True,
            )
            config = root / "config.toml"
            config.write_text(
                "\n".join(
                    [
                        "version = 1",
                        'active_profile = "local"',
                        "",
                        "[profiles.local]",
                        'kind = "local"',
                        'policy = "same_uid"',
                        f"service_owner_uid = {os.geteuid()}",
                        "",
                        "[cache]",
                        "max_entries = 16",
                        "max_bytes = 1048576",
                        "max_entry_bytes = 65536",
                        "",
                    ]
                ),
                encoding="utf-8",
            )
            config.chmod(0o600)
            environment = os.environ.copy()
            environment["HOME"] = str(home)
            service = subprocess.Popen(
                [str(executable), "serve"],
                cwd=root,
                env=environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            try:
                self.assertEqual(
                    service.stdout.readline().strip(),
                    "decodex serving WebSocket /v1/ws over same-UID local transport",
                )
                completed = subprocess.run(
                    [
                        str(executable),
                        "--root",
                        str(root),
                        "--output",
                        "json",
                        "account",
                        "list",
                    ],
                    cwd=root,
                    env=environment,
                    check=False,
                    capture_output=True,
                    text=True,
                )
                if completed.returncode != 0:
                    self.fail(completed.stderr or completed.stdout)
                document = json.loads(completed.stdout)
                self.assertEqual(self.module.account_ids_from_result(document), [])
                self.assertEqual(
                    set(document["result"]["data"]),
                    {"accounts", "routing"},
                )
            finally:
                if service.poll() is None:
                    service.terminate()
                service.communicate(timeout=10)

    def test_retired_snapshot_is_captured_only_before_sqlite_exists(self) -> None:
        account_id = "10000000-0000-4000-8000-000000000001"
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.server_directory.mkdir(parents=True)
            paths.retired_vault.write_bytes(b"retired-vault-fixture")
            paths.retired_vault.chmod(0o600)
            with mock.patch.object(
                self.module,
                "query_accounts",
                return_value=self.account_document([account_id]),
            ) as query:
                snapshot = self.module.capture_retired_account_snapshot(
                    paths,
                    os.geteuid(),
                )
                self.assertEqual(json.loads(snapshot), self.account_document([account_id]))
                query.assert_called_once_with(paths)

            paths.database.write_bytes(b"sqlite-fixture")
            paths.database.chmod(0o600)
            with mock.patch.object(self.module, "query_accounts") as query:
                self.assertIsNone(
                    self.module.capture_retired_account_snapshot(paths, os.geteuid())
                )
                query.assert_not_called()

    def test_transfer_invocation_is_value_suppressing_and_validated(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.database_transfer.parent.mkdir(parents=True)
            paths.database_transfer.write_bytes(b"fixture")
            paths.database_transfer.chmod(0o755)
            result = {
                "schema": "decodex/local-account-transfer/1",
                "outcome": "imported",
                "account_count": 6,
                "source_vault_retained": True,
            }
            completed = subprocess.CompletedProcess(
                ["transfer"],
                0,
                json.dumps(result),
                "",
            )
            with (
                mock.patch.object(self.module, "verify_signed_peer"),
                mock.patch.object(self.module, "run", return_value=completed) as run,
            ):
                self.assertEqual(
                    self.module.transfer_retired_accounts(
                        paths,
                        b'{"credential_negative":true}',
                        "T54QFA7W2S",
                    ),
                    6,
                )
        self.assertEqual(
            run.call_args.kwargs["input_bytes"],
            b'{"credential_negative":true}',
        )
        self.assertNotIn("credentials.redb", " ".join(run.call_args.args[0]))

    def test_database_commands_use_only_the_installed_service_and_root(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            completed = subprocess.CompletedProcess([], 0, "", "")
            with mock.patch.object(self.module, "run", return_value=completed) as run:
                self.module.initialize_local_database(paths)
                self.module.validate_local_database(paths)
        self.assertEqual(
            [call.args[0] for call in run.call_args_list],
            [
                [
                    str(paths.decodex),
                    "initialize-local-database",
                    "--root",
                    str(paths.root),
                ],
                [
                    str(paths.decodex),
                    "validate-local-database",
                    "--root",
                    str(paths.root),
                ],
            ],
        )

    def test_fresh_and_transfer_install_paths_are_separate(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.server_directory.mkdir(parents=True, mode=0o700)
            os.chmod(paths.server_directory, 0o700)
            namespace_lock = self.module.InstallerNamespaceLock.acquire(
                paths,
                os.geteuid(),
            )
            service = self.service_executable()
            try:
                with (
                    mock.patch.object(self.module, "ensure_directories"),
                    mock.patch.object(self.module, "verify_service_executable"),
                    mock.patch.object(self.module, "verify_signed_peer"),
                    mock.patch.object(self.module, "initialize_local_database") as initialize,
                    mock.patch.object(self.module, "transfer_retired_accounts", return_value=6) as transfer,
                    mock.patch.object(self.module, "validate_local_database"),
                    mock.patch.object(self.module, "require_owned_private_file"),
                    mock.patch.object(self.module, "atomic_write"),
                    mock.patch.object(
                        self.module,
                        "read_owned_file",
                        side_effect=lambda path, *_args, **_kwargs: (
                            self.module.render_config(paths, os.geteuid())
                            if path == paths.config
                            else self.module.render_launch_agent(paths)
                        ),
                    ),
                ):
                    self.assertEqual(
                        self.module.install_under_namespace_lock(
                            paths,
                            os.geteuid(),
                            namespace_lock,
                            service,
                            None,
                        ),
                        0,
                    )
                    initialize.assert_called_once_with(paths)
                    transfer.assert_not_called()

                    self.assertEqual(
                        self.module.install_under_namespace_lock(
                            paths,
                            os.geteuid(),
                            namespace_lock,
                            service,
                            b"snapshot",
                        ),
                        6,
                    )
                    transfer.assert_called_once_with(
                        paths,
                        b"snapshot",
                        service["team_identifier"],
                    )
            finally:
                namespace_lock.close()

    def test_build_info_is_diagnostic_identity_from_the_one_executable(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            document = {
                "schema": "decodex/build-info/1",
                "version": "0.2.0",
                "commit": "1" * 40,
                "dirty": False,
            }
            completed = subprocess.CompletedProcess([], 0, json.dumps(document), "")
            with mock.patch.object(self.module, "run", return_value=completed) as run:
                self.assertEqual(self.module.read_build_info(paths), document)
        self.assertEqual(
            run.call_args.args[0],
            [str(paths.decodex), "--output", "json", "build-info"],
        )

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

    def test_installed_launch_agent_contract_enables_graceful_drain(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.launch_agent.write_bytes(self.module.render_launch_agent(paths))
            paths.launch_agent.chmod(0o600)
            self.assertTrue(
                self.module.installed_launch_agent_supports_graceful_drain(
                    paths.launch_agent,
                    os.geteuid(),
                )
            )

    def test_main_reports_sqlite_success(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            args = argparse.Namespace(no_launch=False)
            service = self.service_executable()
            namespace_lock = FakeNamespaceLock()
            output = io.StringIO()
            account_id = "10000000-0000-4000-8000-000000000001"
            with (
                mock.patch.object(self.module, "parse_args", return_value=args),
                mock.patch.object(self.module, "install_paths", return_value=paths),
                mock.patch.object(self.module, "validate_host", return_value=501),
                mock.patch.object(
                    self.module,
                    "inspect_service_executable",
                    return_value=service,
                ),
                mock.patch.object(self.module, "verify_signed_peer"),
                mock.patch.object(self.module, "ensure_installer_namespace_layout"),
                mock.patch.object(
                    self.module,
                    "read_build_info",
                    return_value={
                        "schema": "decodex/build-info/1",
                        "version": "0.2.0",
                        "commit": "1" * 40,
                        "dirty": False,
                    },
                ),
                mock.patch.object(
                    self.module,
                    "capture_retired_account_snapshot",
                    return_value=b"snapshot",
                ),
                mock.patch.object(self.module, "bootout_service"),
                mock.patch.object(
                    self.module.InstallerNamespaceLock,
                    "acquire",
                    return_value=namespace_lock,
                ),
                mock.patch.object(
                    self.module,
                    "install_under_namespace_lock",
                    return_value=1,
                ),
                mock.patch.object(self.module, "bootstrap_service"),
                mock.patch.object(
                    self.module,
                    "wait_for_service",
                    return_value=[account_id],
                ),
                contextlib.redirect_stdout(output),
            ):
                self.assertEqual(self.module.main([]), 0)

        result = json.loads(output.getvalue())
        self.assertEqual(result["database"], "sqlite")
        self.assertEqual(result["account_count"], 1)
        self.assertEqual(result["account_transfer"], "completed")
        self.assertTrue(result["retired_sources_retained"])
        self.assertNotIn("artifact_cohort", result)
        self.assertNotIn("protocol", result)
        self.assertEqual(
            result["build"],
            {"version": "0.2.0", "commit": "1" * 40, "dirty": False},
        )
        self.assertTrue(result["launched"])
        self.assertTrue(namespace_lock.closed)


if __name__ == "__main__":
    unittest.main()
