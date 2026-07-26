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
        self.assertEqual(360, plist_document["ExitTimeOut"])

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

    def test_bootout_distinguishes_absent_service_from_stop_failure(self):
        absent = subprocess.CompletedProcess(["launchctl"], 3, "", "not found")
        with mock.patch.object(
            self.module,
            "run",
            side_effect=[absent, absent],
        ):
            self.module.bootout_service(os.geteuid())

        loaded = subprocess.CompletedProcess(["launchctl"], 0, "loaded", "")
        with mock.patch.object(
            self.module,
            "run",
            side_effect=[absent, loaded],
        ):
            with self.assertRaisesRegex(self.module.InstallError, "could not be stopped"):
                self.module.bootout_service(os.geteuid())


if __name__ == "__main__":
    unittest.main()
