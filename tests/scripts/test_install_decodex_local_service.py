from __future__ import annotations

import base64
import importlib.util
import json
import os
import plistlib
import pwd
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts/macos/install_decodex_local_service.py"


def load_module():
    spec = importlib.util.spec_from_file_location("install_decodex_local_service", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def jwt(claims: dict[str, object]) -> str:
    payload = base64.urlsafe_b64encode(
        json.dumps(claims, separators=(",", ":")).encode()
    ).decode().rstrip("=")
    return f"header.{payload}.signature"


class LocalServiceInstallerTests(unittest.TestCase):
    def setUp(self):
        self.module = load_module()

    def account_record(
        self,
        *,
        account_id: str = "provider-account-1",
        email: str = "one@example.test",
        plan_type: str = "pro",
        access_token: str = "private-access-token",
    ) -> dict[str, object]:
        id_token = jwt(
            {
                "email": email,
                "https://api.openai.com/auth": {
                    "chatgpt_account_id": account_id,
                    "chatgpt_plan_type": plan_type,
                },
            }
        )
        return {
            "email": email,
            "tokens": {
                "access_token": access_token,
                "account_id": account_id,
                "id_token": id_token,
                "refresh_token": "private-refresh-token",
            },
        }

    def paths(self, root: Path):
        return self.module.InstallPaths(
            repository=REPO_ROOT,
            root=root,
            config=root / "config.toml",
            mapping=root / "reset-card-legacy-map.json",
            data_directory=root / "postgres/data",
            socket_directory=root / "postgres/socket",
            log_directory=root / "logs",
            postgres_log=root / "logs/postgres.log",
            service_log=root / "logs/local-service.log",
            legacy_accounts=root / "legacy/accounts.jsonl",
            launch_agent=root / "space.decodex.local-service.plist",
            decodexd=root / "bin/decodexd",
            decodex_cli=root / "bin/decodex",
            codex=root / "codex-bin/codex",
            postgres=root / "bin/postgres",
            initdb=root / "bin/initdb",
            pg_isready=root / "bin/pg_isready",
            psql=root / "bin/psql",
        )

    def test_account_parser_cross_checks_identity_without_exposing_values(self):
        record = self.account_record()
        account = self.module.account_from_record(record)

        self.assertEqual("provider-account-1", account.provider_account_id)
        self.assertEqual("one@example.test", account.email)
        self.assertEqual("pro", account.plan_type)
        self.assertRegex(account.provider_account_id_sha256, r"^[0-9a-f]{64}$")

        changed = self.account_record()
        changed["tokens"]["account_id"] = "other-provider-account"
        with self.assertRaisesRegex(
            self.module.InstallError, "identity claims are inconsistent"
        ):
            self.module.account_from_record(changed)

        malformed_shape = self.account_record()
        malformed_shape["auth"] = {"tokens": malformed_shape["tokens"]}
        with self.assertRaisesRegex(self.module.InstallError, "shape is invalid"):
            self.module.account_from_record(malformed_shape)

        malformed_token = self.account_record()
        malformed_token["tokens"]["id_token"] += ".extra"
        with self.assertRaisesRegex(self.module.InstallError, "token is malformed"):
            self.module.account_from_record(malformed_token)

        nested = self.account_record()
        nested = {
            "email": " invalid@example.test",
            "auth": {
                "email": "one@example.test",
                "tokens": nested["tokens"],
            },
        }
        with self.assertRaisesRegex(self.module.InstallError, "credentials are unavailable"):
            self.module.account_from_record(nested)

    def test_config_mapping_and_plist_are_credential_negative(self):
        account = self.module.account_from_record(self.account_record())
        enrollment = self.module.build_enrollments(
            [account],
            {},
            {},
            {
                account.email: {
                    "email": account.email,
                    "plan_type": "pro",
                    "random_name": "Amber Otter",
                    "reset_credits_available_count": 2,
                }
            },
        )
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            config = self.module.render_config(paths, 501, enrollment)
            mapping = self.module.render_mapping(enrollment).decode()
            plist_bytes = self.module.render_launch_agent(paths)
            plist = plist_bytes.decode()
            plist_document = plistlib.loads(plist_bytes)

        combined = config + mapping + plist
        self.assertNotIn(account.provider_account_id, combined)
        self.assertNotIn(account.email, combined)
        self.assertNotIn("private-access-token", combined)
        self.assertNotIn("secret-run", combined)
        self.assertIn("DECODEX_RESET_CARD_SLOT_01_ACCESS_TOKEN", config)
        self.assertIn('"schema":"decodex/reset-card-legacy-bridge/1"', mapping)
        self.assertIn("supervise-local", plist)
        self.assertIn("Amber Otter", config)
        self.assertEqual(
            {"HOME", "PATH"},
            set(plist_document["EnvironmentVariables"]),
        )
        self.assertEqual(
            str(paths.root.parent),
            plist_document["EnvironmentVariables"]["HOME"],
        )
        self.assertEqual(
            str(paths.codex.parent),
            plist_document["EnvironmentVariables"]["PATH"].split(os.pathsep)[0],
        )
        self.assertNotIn("--secret-run", plist_document["ProgramArguments"])
        self.assertEqual({"SuccessfulExit": False}, plist_document["KeepAlive"])
        self.assertEqual(60, plist_document["ExitTimeOut"])

    def test_parser_discovers_codex_without_copying_the_process_environment(self):
        discovered = Path("/Applications/ChatGPT.app/Contents/Resources/codex")
        with mock.patch.object(self.module.shutil, "which", return_value=str(discovered)):
            arguments = self.module.parse_args([])

        self.assertEqual(discovered, arguments.codex)

    def test_existing_mapping_is_stable_and_account_set_change_fails_closed(self):
        account = self.module.account_from_record(self.account_record())
        digest = account.provider_account_id_sha256
        account_id = "10000000-0000-4000-8000-000000000001"
        existing = self.module.ExistingEnrollment(account_id, "Pinned Label")
        enrollment = self.module.build_enrollments(
            [account],
            {digest: 1},
            {1: existing},
            {account.email: {"random_name": "Changed Label"}},
        )

        self.assertEqual(account_id, enrollment[0].account_id)
        self.assertEqual("Pinned Label", enrollment[0].display_label)
        recovered = self.module.build_enrollments(
            [account],
            {digest: 1},
            {},
            {},
        )
        self.assertEqual(1, recovered[0].slot)
        self.assertRegex(recovered[0].account_id, self.module.UUID_PATTERN)
        with self.assertRaisesRegex(
            self.module.InstallError,
            "enrollment lacks its bridge mapping",
        ):
            self.module.build_enrollments(
                [account],
                {},
                {1: existing},
                {},
            )
        second = self.module.account_from_record(
            self.account_record(
                account_id="provider-account-2",
                email="two@example.test",
            )
        )
        with self.assertRaisesRegex(
            self.module.InstallError, "explicit reconciliation is required"
        ):
            self.module.build_enrollments(
                [account, second],
                {digest: 1},
                {1: existing},
                {},
            )

    def test_rerun_pins_display_label_across_presentation_changes(self):
        account = self.module.account_from_record(self.account_record())
        mapping = {account.provider_account_id_sha256: 1}
        presentation = {
            account.email: {
                "plan_type": account.plan_type,
                "random_name": "Amber Otter",
            }
        }
        cases = [
            (presentation, {}, "Amber Otter"),
            ({}, presentation, "Account 01"),
        ]

        for initial_presentation, rerun_presentation, expected_label in cases:
            with self.subTest(expected_label=expected_label):
                initial = self.module.build_enrollments(
                    [account],
                    {},
                    {},
                    initial_presentation,
                )
                with tempfile.TemporaryDirectory() as temp:
                    paths = self.paths(Path(temp))
                    paths.config.write_text(
                        self.module.render_config(paths, os.geteuid(), initial),
                        encoding="utf-8",
                    )
                    paths.config.chmod(0o600)
                    existing = self.module.existing_enrollments(
                        paths.config,
                        os.geteuid(),
                    )

                rerun = self.module.build_enrollments(
                    [account],
                    mapping,
                    existing,
                    rerun_presentation,
                )

                self.assertEqual(initial[0].account_id, rerun[0].account_id)
                self.assertEqual(expected_label, rerun[0].display_label)

        recovered = self.module.build_enrollments(
            [account],
            mapping,
            {},
            presentation,
        )
        self.assertEqual("Amber Otter", recovered[0].display_label)
        self.assertRegex(recovered[0].account_id, self.module.UUID_PATTERN)

    def test_mapping_parser_rejects_unknown_fields_and_noncontiguous_slots(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "mapping.json"
            path.write_text(
                json.dumps(
                    {
                        "schema": self.module.MAPPING_SCHEMA,
                        "accounts": [
                            {
                                "slot": 2,
                                "provider_account_id_sha256": "a" * 64,
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                self.module.InstallError, "slots are not contiguous"
            ):
                self.module.load_existing_mapping(path, os.geteuid())

    def test_bounded_descriptor_read_collects_partial_reads_and_surfaces_overflow(self):
        with mock.patch.object(
            self.module.os,
            "read",
            side_effect=[b"ab", b"cd", b""],
        ) as read:
            self.assertEqual(
                b"abcd",
                self.module.read_bounded_descriptor(19, 8),
            )
        self.assertEqual(3, read.call_count)

        with mock.patch.object(
            self.module.os,
            "read",
            side_effect=[b"ab", b"cd", b"e"],
        ):
            self.assertEqual(
                5,
                len(self.module.read_bounded_descriptor(19, 4)),
            )

    def test_legacy_read_repairs_exact_file_and_lock_permissions(self):
        with tempfile.TemporaryDirectory() as temp:
            parent = Path(temp) / "legacy"
            parent.mkdir(mode=0o700)
            account_path = parent / "accounts.jsonl"
            account_path.write_text(
                json.dumps(self.account_record()) + "\n",
                encoding="utf-8",
            )
            account_path.chmod(0o644)

            accounts = self.module.read_legacy_accounts(account_path, os.geteuid())

            self.assertEqual(1, len(accounts))
            self.assertEqual(0o600, stat.S_IMODE(account_path.stat().st_mode))
            self.assertEqual(
                0o600,
                stat.S_IMODE((parent / ".accounts.jsonl.lock").stat().st_mode),
            )

    def test_managed_mapping_symlink_is_rejected_without_following_it(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            target = root / "target.json"
            target.write_text(
                json.dumps({"schema": self.module.MAPPING_SCHEMA, "accounts": []}),
                encoding="utf-8",
            )
            link = root / "mapping.json"
            link.symlink_to(target)

            with self.assertRaisesRegex(self.module.InstallError, "mapping is malformed"):
                self.module.load_existing_mapping(link, os.geteuid())

    def test_atomic_write_replaces_without_backup_and_cleans_failed_candidate(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            target = root / "config.toml"
            target.write_bytes(b"old")

            self.module.atomic_write(target, b"new", 0o600)

            self.assertEqual(b"new", target.read_bytes())
            self.assertEqual(0o600, stat.S_IMODE(target.stat().st_mode))
            self.assertEqual(["config.toml"], sorted(path.name for path in root.iterdir()))

            with mock.patch.object(
                self.module.os,
                "replace",
                side_effect=OSError("injected replace failure"),
            ):
                with self.assertRaises(OSError):
                    self.module.atomic_write(target, b"not-installed", 0o600)

            self.assertEqual(b"new", target.read_bytes())
            self.assertEqual(["config.toml"], sorted(path.name for path in root.iterdir()))

    def test_missing_postgres_data_directory_is_created_and_initialized_once(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            paths = self.paths(root)
            self.module.ensure_directories(paths, os.geteuid())
            share = paths.initdb.parent.parent / "share/postgresql"
            share.mkdir(parents=True)

            def initialize(command, **_kwargs):
                self.assertIn("--auth-local=trust", command)
                self.assertIn("--auth-host=reject", command)
                (paths.data_directory / "PG_VERSION").write_text(
                    "18\n",
                    encoding="ascii",
                )
                return subprocess.CompletedProcess(command, 0, "", "")

            with mock.patch.object(
                self.module,
                "run",
                side_effect=initialize,
            ) as run:
                self.module.initialize_cluster(paths, os.geteuid())
                self.module.initialize_cluster(paths, os.geteuid())

            self.assertEqual(1, run.call_count)
            self.assertEqual(0o700, stat.S_IMODE(paths.data_directory.stat().st_mode))
            self.assertEqual(
                0o600,
                stat.S_IMODE((paths.data_directory / "PG_VERSION").stat().st_mode),
            )
            self.assertEqual(0o600, stat.S_IMODE(paths.service_log.stat().st_mode))

    def test_psql_environment_uses_os_identity_and_drops_inherited_pg_settings(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            with mock.patch.dict(
                os.environ,
                {
                    "USER": "spoofed-user",
                    "PGPASSWORD": "must-not-propagate",
                    "PGOPTIONS": "must-not-propagate",
                },
                clear=False,
            ):
                environment = self.module.psql_environment(paths)

        self.assertEqual(pwd.getpwuid(os.geteuid()).pw_name, environment["PGUSER"])
        self.assertNotIn("PGPASSWORD", environment)
        self.assertNotIn("PGOPTIONS", environment)

    def test_service_readiness_requires_every_expected_inventory(self):
        first_account = "10000000-0000-4000-8000-000000000001"
        second_account = "10000000-0000-4000-8000-000000000002"
        accounts = {
            "schema": "decodex/reset-card-cli/1",
            "command": "accounts",
            "outcome": "available",
            "result": {
                "outcome": "available",
                "data": {
                    "accounts": [
                        {"account_id": first_account},
                        {"account_id": second_account},
                    ]
                },
            },
        }

        def inventory(account_id):
            return {
                "schema": "decodex/reset-card-cli/1",
                "command": "list",
                "outcome": "available",
                "result": {
                    "outcome": "available",
                    "data": {"account_id": account_id},
                },
            }

        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            doctor = mock.patch.object(
                self.module,
                "query_doctor",
                return_value=True,
            )
            reset_card = mock.patch.object(
                self.module,
                "query_reset_card",
                side_effect=[
                    accounts,
                    inventory(first_account),
                    inventory(second_account),
                ],
            )
            with doctor, reset_card as query:
                self.module.wait_for_service(paths, {first_account, second_account})

        self.assertEqual(
            [
                mock.call(paths, ["accounts"]),
                mock.call(paths, ["list", "--account", first_account]),
                mock.call(paths, ["list", "--account", second_account]),
            ],
            query.call_args_list,
        )

    def test_doctor_accepts_report_shape_with_only_designed_unknown_checks(self):
        required = [
            "configuration",
            "database",
            "protocol",
            "protocol_version",
            "server_identity",
            "server_repositories",
            "credential_vault",
        ]
        document = {
            "schema": "decodex/cli-diagnostics/1",
            "command": "doctor",
            "outcome": "report",
            "status": "unknown",
            "checks": 9,
            "report": {
                "checks": [
                    {
                        "component": {"kind": kind},
                        "status": {"state": "ready"},
                    }
                    for kind in required
                ]
                + [
                    {
                        "component": {"kind": "plugin_readiness"},
                        "status": {"state": "unknown", "issue": "plugin"},
                    }
                ],
            },
        }

        self.assertTrue(self.module.critical_doctor_is_ready(document))
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            completed = subprocess.CompletedProcess(
                [str(paths.decodex_cli)],
                1,
                json.dumps(document),
                "",
            )
            with mock.patch.object(self.module, "run", return_value=completed):
                self.assertTrue(self.module.query_doctor(paths))
        document["outcome"] = "success"
        self.assertFalse(self.module.critical_doctor_is_ready(document))
        document["outcome"] = "report"
        document["report"]["checks"][1]["status"] = {
            "state": "unavailable",
            "issue": "database_unreachable",
        }
        self.assertFalse(self.module.critical_doctor_is_ready(document))

    def test_query_uses_explicit_root_and_does_not_log_captured_failure(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            completed = subprocess.CompletedProcess(
                [str(paths.decodex_cli)],
                2,
                '{"outcome":"failure"}',
                "private-marker",
            )
            with mock.patch.object(self.module, "run", return_value=completed) as run:
                self.assertIsNone(self.module.query_reset_card(paths, ["accounts"]))

        command = run.call_args.args[0]
        self.assertEqual(str(paths.root), command[command.index("--root") + 1])
        self.assertNotIn("private-marker", str(run.call_args))

    def test_installed_launch_agent_contract_requires_exact_drain_settings(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.launch_agent.write_bytes(self.module.render_launch_agent(paths))
            paths.launch_agent.chmod(0o600)
            self.assertTrue(
                self.module.installed_launch_agent_supports_graceful_drain(
                    paths.launch_agent, os.geteuid()
                )
            )

            legacy = plistlib.loads(self.module.render_launch_agent(paths))
            legacy["KeepAlive"] = True
            legacy["ExitTimeOut"] = 360
            paths.launch_agent.write_bytes(plistlib.dumps(legacy))
            paths.launch_agent.chmod(0o600)
            self.assertFalse(
                self.module.installed_launch_agent_supports_graceful_drain(
                    paths.launch_agent, os.geteuid()
                )
            )

    def test_graceful_contract_drains_loaded_service_before_bootout(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            active = subprocess.CompletedProcess(
                ["launchctl"], 0, "service = {\n\tpid = 100\n}\n", ""
            )
            initial_processes = subprocess.CompletedProcess(
                ["/bin/ps"],
                0,
                "100 1 Sat Jul 25 20:00:00 2026\n"
                "101 100 Sat Jul 25 20:00:01 2026\n"
                "200 1 Sat Jul 25 19:00:00 2026\n",
                "",
            )
            signaled = subprocess.CompletedProcess(["launchctl"], 0, "", "")
            inactive = subprocess.CompletedProcess(
                ["launchctl"], 0, "service = {\n\tstate = exited\n}\n", ""
            )
            stopped = subprocess.CompletedProcess(["launchctl"], 0, "", "")
            settled = subprocess.CompletedProcess(
                ["/bin/ps"], 0, "200 1 Sat Jul 25 19:00:00 2026\n", ""
            )

            with mock.patch.object(
                self.module,
                "installed_launch_agent_supports_graceful_drain",
                return_value=True,
            ):
                with mock.patch.object(
                    self.module,
                    "run",
                    side_effect=[
                        active,
                        initial_processes,
                        signaled,
                        inactive,
                        stopped,
                        settled,
                    ],
                ) as run:
                    with mock.patch.object(self.module.time, "sleep") as sleep:
                        self.module.bootout_service(paths, os.geteuid())

        commands = [call.args[0] for call in run.call_args_list]
        self.assertEqual(
            [
                "/bin/launchctl",
                "kill",
                "SIGTERM",
                f"gui/{os.geteuid()}/{self.module.LAUNCH_AGENT_LABEL}",
            ],
            commands[2],
        )
        self.assertEqual("bootout", commands[4][1])
        sleep.assert_called_once_with(
            self.module.LOCAL_SERVICE_SETTLEMENT_POLL_SECONDS
        )

    def test_legacy_contract_boots_out_then_waits_for_captured_generation(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            active = subprocess.CompletedProcess(
                ["launchctl"], 0, "service = {\n\tpid = 100\n}\n", ""
            )
            initial_processes = subprocess.CompletedProcess(
                ["/bin/ps"],
                0,
                "100 1 Sat Jul 25 20:00:00 2026\n"
                "101 100 Sat Jul 25 20:00:01 2026\n",
                "",
            )
            stopped = subprocess.CompletedProcess(["launchctl"], 0, "", "")
            child_still_exiting = subprocess.CompletedProcess(
                ["/bin/ps"],
                0,
                "101 1 Sat Jul 25 20:00:01 2026\n",
                "",
            )
            settled = subprocess.CompletedProcess(["/bin/ps"], 0, "", "")

            with mock.patch.object(
                self.module,
                "installed_launch_agent_supports_graceful_drain",
                return_value=False,
            ):
                with mock.patch.object(
                    self.module,
                    "run",
                    side_effect=[
                        active,
                        initial_processes,
                        stopped,
                        child_still_exiting,
                        settled,
                    ],
                ) as run:
                    with mock.patch.object(self.module.time, "sleep") as sleep:
                        self.module.bootout_service(paths, os.geteuid())

        commands = [call.args[0] for call in run.call_args_list]
        self.assertEqual("bootout", commands[2][1])
        self.assertGreater(
            run.call_args_list[2].kwargs["timeout"],
            self.module.LOCAL_SERVICE_CONTROL_TIMEOUT_SECONDS,
        )
        self.assertNotIn(
            "kill", [argument for command in commands for argument in command]
        )
        sleep.assert_called_once_with(
            self.module.LOCAL_SERVICE_SETTLEMENT_POLL_SECONDS
        )

    def test_nonzero_bootout_with_final_absence_still_waits_for_generation(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            active = subprocess.CompletedProcess(
                ["launchctl"], 0, "service = {\n\tpid = 100\n}\n", ""
            )
            initial_processes = subprocess.CompletedProcess(
                ["/bin/ps"], 0, "100 1 Sat Jul 25 20:00:00 2026\n", ""
            )
            bootout_absent = subprocess.CompletedProcess(
                ["launchctl"], 3, "", "not found"
            )
            print_absent = subprocess.CompletedProcess(
                ["launchctl"],
                self.module.LAUNCHCTL_PRINT_NOT_FOUND_STATUS,
                "",
                "not found",
            )
            still_exiting = subprocess.CompletedProcess(
                ["/bin/ps"], 0, "100 1 Sat Jul 25 20:00:00 2026\n", ""
            )
            settled = subprocess.CompletedProcess(["/bin/ps"], 0, "", "")

            with mock.patch.object(
                self.module,
                "installed_launch_agent_supports_graceful_drain",
                return_value=False,
            ):
                with mock.patch.object(
                    self.module,
                    "run",
                    side_effect=[
                        active,
                        initial_processes,
                        bootout_absent,
                        print_absent,
                        still_exiting,
                        settled,
                    ],
                ):
                    with mock.patch.object(self.module.time, "sleep") as sleep:
                        self.module.bootout_service(paths, os.geteuid())

        sleep.assert_called_once_with(
            self.module.LOCAL_SERVICE_SETTLEMENT_POLL_SECONDS
        )

    def test_concurrent_bootout_during_graceful_drain_waits_for_initial_generation(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            active = subprocess.CompletedProcess(
                ["launchctl"], 0, "service = {\n\tpid = 100\n}\n", ""
            )
            initial_processes = subprocess.CompletedProcess(
                ["/bin/ps"], 0, "100 1 Sat Jul 25 20:00:00 2026\n", ""
            )
            kill_absent = subprocess.CompletedProcess(
                ["launchctl"], 3, "", "not found"
            )
            bootout_absent = subprocess.CompletedProcess(
                ["launchctl"], 3, "", "not found"
            )
            print_absent = subprocess.CompletedProcess(
                ["launchctl"],
                self.module.LAUNCHCTL_PRINT_NOT_FOUND_STATUS,
                "",
                "not found",
            )
            still_exiting = subprocess.CompletedProcess(
                ["/bin/ps"], 0, "100 1 Sat Jul 25 20:00:00 2026\n", ""
            )
            settled = subprocess.CompletedProcess(["/bin/ps"], 0, "", "")

            with mock.patch.object(
                self.module,
                "installed_launch_agent_supports_graceful_drain",
                return_value=True,
            ):
                with mock.patch.object(
                    self.module,
                    "run",
                    side_effect=[
                        active,
                        initial_processes,
                        kill_absent,
                        print_absent,
                        bootout_absent,
                        print_absent,
                        still_exiting,
                        settled,
                    ],
                ):
                    with mock.patch.object(self.module.time, "sleep") as sleep:
                        self.module.bootout_service(paths, os.geteuid())

        sleep.assert_called_once_with(
            self.module.LOCAL_SERVICE_SETTLEMENT_POLL_SECONDS
        )

    def test_pid_reuse_with_a_different_lstart_is_not_the_captured_process(self):
        captured = {
            self.module.ProcessIdentity(
                process_id=100, started_at="Sat Jul 25 20:00:00 2026"
            )
        }
        reused = subprocess.CompletedProcess(
            ["/bin/ps"], 0, "100 1 Sat Jul 25 20:01:00 2026\n", ""
        )
        with mock.patch.object(self.module, "run", return_value=reused) as run:
            with mock.patch.object(self.module.time, "sleep") as sleep:
                self.module.wait_for_process_generation_exit(
                    captured, self.module.time.monotonic() + 300
                )

        self.assertLessEqual(
            run.call_args.kwargs["timeout"],
            self.module.LOCAL_SERVICE_CONTROL_TIMEOUT_SECONDS,
        )
        sleep.assert_not_called()

    def test_process_inventory_timeout_is_value_free_and_prevents_bootout(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            active = subprocess.CompletedProcess(
                ["launchctl"], 0, "service = {\n\tpid = 100\n}\n", ""
            )
            timeout = subprocess.TimeoutExpired(["/bin/ps"], 5, "private-output")
            with mock.patch.object(
                self.module,
                "installed_launch_agent_supports_graceful_drain",
                return_value=False,
            ):
                with mock.patch.object(
                    self.module, "run", side_effect=[active, timeout]
                ) as run:
                    with self.assertRaisesRegex(
                        self.module.InstallError,
                        "process inventory is unavailable",
                    ):
                        self.module.bootout_service(paths, os.geteuid())

        self.assertEqual(2, run.call_count)
        self.assertEqual("print", run.call_args_list[0].args[0][1])

    def test_launchctl_state_error_is_not_misclassified_as_absence(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            unavailable = subprocess.CompletedProcess(
                ["launchctl"], 5, "", "private-error"
            )
            with mock.patch.object(
                self.module, "run", return_value=unavailable
            ) as run:
                with self.assertRaisesRegex(
                    self.module.InstallError, "service state is unavailable"
                ):
                    self.module.bootout_service(paths, os.geteuid())

        self.assertEqual(1, run.call_count)

    def test_ambiguous_bootout_timeout_waits_for_captured_generation(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            active = subprocess.CompletedProcess(
                ["launchctl"], 0, "service = {\n\tpid = 100\n}\n", ""
            )
            initial_processes = subprocess.CompletedProcess(
                ["/bin/ps"], 0, "100 1 Sat Jul 25 20:00:00 2026\n", ""
            )
            timeout = subprocess.TimeoutExpired(
                ["/bin/launchctl", "bootout"], 5, "private-output"
            )
            settled = subprocess.CompletedProcess(["/bin/ps"], 0, "", "")
            with mock.patch.object(
                self.module,
                "installed_launch_agent_supports_graceful_drain",
                return_value=False,
            ):
                with mock.patch.object(
                    self.module,
                    "run",
                    side_effect=[active, initial_processes, timeout, settled],
                ) as run:
                    with self.assertRaisesRegex(
                        self.module.InstallError, "could not be stopped"
                    ):
                        self.module.bootout_service(paths, os.geteuid())

        self.assertEqual(4, run.call_count)
        self.assertGreater(
            run.call_args_list[2].kwargs["timeout"],
            self.module.LOCAL_SERVICE_CONTROL_TIMEOUT_SECONDS,
        )
        self.assertEqual("/bin/ps", run.call_args_list[-1].args[0][0])

    def test_bootout_distinguishes_absent_service_from_stop_failure(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            bootout_absent = subprocess.CompletedProcess(
                ["launchctl"], 3, "", "not found"
            )
            print_absent = subprocess.CompletedProcess(
                ["launchctl"],
                self.module.LAUNCHCTL_PRINT_NOT_FOUND_STATUS,
                "",
                "not found",
            )
            with mock.patch.object(
                self.module,
                "installed_launch_agent_supports_graceful_drain",
                return_value=False,
            ):
                with mock.patch.object(
                    self.module,
                    "run",
                    side_effect=[print_absent, bootout_absent, print_absent],
                ):
                    self.module.bootout_service(paths, os.geteuid())

            loaded = subprocess.CompletedProcess(["launchctl"], 0, "loaded", "")
            with mock.patch.object(
                self.module,
                "installed_launch_agent_supports_graceful_drain",
                return_value=False,
            ):
                with mock.patch.object(
                    self.module,
                    "run",
                    side_effect=[loaded, bootout_absent, loaded],
                ):
                    with self.assertRaisesRegex(
                        self.module.InstallError, "could not be stopped"
                    ):
                        self.module.bootout_service(paths, os.geteuid())

    def test_bootstrap_does_not_kill_the_freshly_started_generation(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            with mock.patch.object(self.module, "run") as run:
                self.module.bootstrap_service(paths, os.geteuid())

        commands = [call.args[0] for call in run.call_args_list]
        self.assertEqual("bootstrap", commands[0][1])
        self.assertEqual("kickstart", commands[1][1])
        self.assertNotIn("-k", commands[1])
        self.assertTrue(
            all(
                call.kwargs["timeout"]
                == self.module.LOCAL_SERVICE_CONTROL_TIMEOUT_SECONDS
                for call in run.call_args_list
            )
        )

    def test_postgres_readiness_uses_the_existing_postgres_database(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            process = mock.Mock()
            process.poll.return_value = None
            ready = subprocess.CompletedProcess([str(paths.pg_isready)], 0, "", "")
            with mock.patch.object(self.module, "run", return_value=ready) as run:
                self.module.wait_for_postgres(paths, process)

        command = run.call_args.args[0]
        self.assertEqual("postgres", command[command.index("-d") + 1])


if __name__ == "__main__":
    unittest.main()
