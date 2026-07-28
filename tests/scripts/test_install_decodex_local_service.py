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
from contextlib import ExitStack
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
        self.inspect_wrapper_patcher = mock.patch.object(
            self.module,
            "inspect_daemon_wrapper",
            side_effect=lambda paths: self.daemon_wrapper(paths),
        )
        self.verify_wrapper_patcher = mock.patch.object(
            self.module,
            "verify_daemon_wrapper",
            side_effect=lambda _paths, expected, **_kwargs: expected,
        )
        self.inspect_wrapper_patcher.start()
        self.verify_wrapper_patcher.start()
        self.addCleanup(self.inspect_wrapper_patcher.stop)
        self.addCleanup(self.verify_wrapper_patcher.stop)

    def account_record(
        self,
        *,
        account_id: str = "provider-account-1",
        email: str = "one@example.test",
        plan_type: str = "pro",
        access_token: str | None = None,
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
        if access_token is None:
            access_token = jwt({"exp": 4102444800})
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
            vnext_config_source=root / "account-migration-vnext-source.toml",
            staging_config=root / ".account-migration-runtime.toml",
            mapping=root / "reset-card-legacy-map.json",
            migration_manifest=root / "account-migration-manifest.json",
            credential_directory=root / ".account-migration-credentials",
            data_directory=root / "postgres/data",
            socket_directory=root / "postgres/socket",
            log_directory=root / "logs",
            postgres_log=root / "logs/postgres.log",
            service_log=root / "logs/local-service.log",
            legacy_accounts=root / "legacy/accounts.jsonl",
            legacy_config=root / "legacy/config.toml",
            launch_agent=root / "space.decodex.local-service.plist",
            decodexd=(
                root
                / "bin"
                / "decodexd.app"
                / "Contents"
                / "MacOS"
                / "decodexd"
            ),
            decodex_cli=root / "bin/decodex",
            codex=root / "codex-bin/codex",
            postgres=root / "bin/postgres",
            initdb=root / "bin/initdb",
            pg_isready=root / "bin/pg_isready",
            psql=root / "bin/psql",
        )

    def daemon_wrapper(self, paths):
        wrapper = paths.daemon_wrapper
        return {
            "schema": "decodex/daemon-wrapper/1",
            "wrapper_path": str(wrapper),
            "executable_path": str(paths.decodexd),
            "executable_sha256": "1" * 64,
            "executable_byte_count": 1024,
            "info_plist_path": str(wrapper / "Contents/Info.plist"),
            "info_plist_sha256": "2" * 64,
            "bundle_identifier": "box.acg.decodex.daemon",
            "bundle_executable": "decodexd",
            "bundle_package_type": "APPL",
            "background_only": True,
            "embedded_profile_path": str(
                wrapper / "Contents/embedded.provisionprofile"
            ),
            "embedded_profile_sha256": "3" * 64,
            "team_identifier": "T54QFA7W2S",
            "application_identifier": "T54QFA7W2S.box.acg.decodex.daemon",
            "profile_expires_at": "2099-01-01T00:00:00Z",
            "profile_channel": "development",
            "signed_entitlements_sha256": "4" * 64,
            "keychain_access_groups": [
                "T54QFA7W2S.box.acg.decodex.daemon"
            ],
            "signature_identity_sha256": "5" * 64,
        }

    def namespace_lock(self, root: Path):
        paths = self.paths(root)
        paths.server_directory.mkdir(parents=True, mode=0o700)
        os.chmod(paths.server_directory, 0o700)
        return paths, self.module.InstallerNamespaceLock.acquire(
            paths,
            os.geteuid(),
        )

    def assert_fresh_namespace_lock(self, paths):
        acquired = self.module.InstallerNamespaceLock.acquire(
            paths,
            os.geteuid(),
        )
        acquired.close()

    def test_namespace_lock_borrow_stays_non_inheritable(self):
        with tempfile.TemporaryDirectory() as temp:
            paths, namespace_lock = self.namespace_lock(Path(temp))
            descriptor = namespace_lock.borrow()
            try:
                self.assertFalse(os.get_inheritable(descriptor))
            finally:
                os.close(descriptor)
                namespace_lock.close()
            self.assert_fresh_namespace_lock(paths)

    def test_transition_gate_dup_failure_closes_namespace_duplicate(self):
        with tempfile.TemporaryDirectory() as temp:
            paths, namespace_lock = self.namespace_lock(Path(temp))
            borrowed = os.dup(namespace_lock.lock_descriptor)
            with (
                mock.patch.object(namespace_lock, "borrow", return_value=borrowed),
                mock.patch.object(
                    self.module.os,
                    "dup",
                    side_effect=OSError("injected transition-gate dup failure"),
                ),
                self.assertRaisesRegex(OSError, "transition-gate dup failure"),
            ):
                self.module.run_installer_child(
                    ["child"],
                    namespace_lock,
                    cwd=Path(temp),
                    capture=True,
                    transition_gate_fd=namespace_lock.lock_descriptor,
                )
            with self.assertRaises(OSError):
                os.fstat(borrowed)
            namespace_lock.close()
            self.assert_fresh_namespace_lock(paths)

    def test_popen_failure_closes_installer_child_duplicates(self):
        with tempfile.TemporaryDirectory() as temp:
            paths, namespace_lock = self.namespace_lock(Path(temp))
            transition_read, transition_write = os.pipe()
            duplicates: list[int] = []
            real_dup = os.dup

            def record_dup(descriptor):
                duplicate = real_dup(descriptor)
                duplicates.append(duplicate)
                return duplicate

            try:
                with (
                    mock.patch.object(self.module.os, "dup", side_effect=record_dup),
                    mock.patch.object(
                        self.module.subprocess,
                        "Popen",
                        side_effect=OSError("injected spawn failure"),
                    ) as popen,
                    self.assertRaisesRegex(OSError, "injected spawn failure"),
                ):
                    self.module.run_installer_child(
                        ["child"],
                        namespace_lock,
                        cwd=Path(temp),
                        capture=True,
                        transition_gate_fd=transition_read,
                    )
                self.assertEqual(2, len(duplicates))
                for descriptor in duplicates:
                    with self.assertRaises(OSError):
                        os.fstat(descriptor)
                self.assertTrue(popen.call_args.kwargs["close_fds"])
                self.assertEqual(
                    tuple(duplicates),
                    popen.call_args.kwargs["pass_fds"],
                )
            finally:
                os.close(transition_read)
                os.close(transition_write)
                namespace_lock.close()
            self.assert_fresh_namespace_lock(paths)

    def test_post_spawn_failure_reaps_owned_child_and_closes_duplicates(self):
        primary_error = self.module.InstallError("injected post-spawn failure")
        with tempfile.TemporaryDirectory() as temp:
            paths, namespace_lock = self.namespace_lock(Path(temp))
            process = mock.Mock()
            process.poll.return_value = None
            process.stdout.closed = False
            process.stderr.closed = False
            duplicates: list[int] = []
            real_dup = os.dup

            def record_dup(descriptor):
                duplicate = real_dup(descriptor)
                duplicates.append(duplicate)
                return duplicate

            with (
                mock.patch.object(self.module.os, "dup", side_effect=record_dup),
                mock.patch.object(
                    self.module.subprocess,
                    "Popen",
                    return_value=process,
                ),
                mock.patch.object(
                    self.module,
                    "communicate_bounded",
                    side_effect=primary_error,
                ),
                mock.patch.object(
                    self.module,
                    "terminate_bounded_process",
                ) as terminate,
                self.assertRaisesRegex(
                    self.module.InstallError,
                    "injected post-spawn failure",
                ) as raised,
            ):
                self.module.run_installer_child(
                    ["child"],
                    namespace_lock,
                    cwd=Path(temp),
                    capture=True,
                )
            self.assertIs(primary_error, raised.exception)
            terminate.assert_called_once_with(process)
            process.stdout.close.assert_called_once_with()
            process.stderr.close.assert_called_once_with()
            for descriptor in duplicates:
                with self.assertRaises(OSError):
                    os.fstat(descriptor)
            namespace_lock.close()
            self.assert_fresh_namespace_lock(paths)

    def test_cleanup_failure_is_secondary_to_installer_child_failure(self):
        primary_error = self.module.InstallError("injected post-spawn failure")
        cleanup_error = self.module.InstallError("injected cleanup failure")
        with tempfile.TemporaryDirectory() as temp:
            paths, namespace_lock = self.namespace_lock(Path(temp))
            process = mock.Mock()
            process.poll.return_value = None
            with (
                mock.patch.object(
                    self.module.subprocess,
                    "Popen",
                    return_value=process,
                ),
                mock.patch.object(
                    self.module,
                    "communicate_bounded",
                    side_effect=primary_error,
                ),
                mock.patch.object(
                    self.module,
                    "terminate_bounded_process",
                    side_effect=cleanup_error,
                ),
                self.assertRaisesRegex(
                    self.module.InstallError,
                    "injected post-spawn failure",
                ) as raised,
            ):
                self.module.run_installer_child(
                    ["child"],
                    namespace_lock,
                    cwd=Path(temp),
                    capture=True,
                )
            self.assertIs(primary_error, raised.exception)
            self.assertIs(cleanup_error, raised.exception.__cause__)
            namespace_lock.close()
            self.assert_fresh_namespace_lock(paths)

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

    def test_config_manifest_and_plist_are_credential_negative(self):
        account = self.module.account_from_record(self.account_record())
        enrollments, fixed_account_id, order = self.module.build_enrollments(
            [account],
            {},
            {},
            {},
            None,
            {},
        )
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            config = self.module.render_config(paths, 501)
            sources = [
                self.module.source_record(role, paths.root / f"absent-{index}", None)
                for index, role in enumerate(
                    [
                        "legacy_account_pool",
                        "legacy_account_config",
                        "vnext_uuid_bridge",
                        "vnext_account_config",
                    ]
                )
            ]
            manifest = self.module.render_migration_manifest(
                paths,
                501,
                sources,
                enrollments,
                {enrollments[0].account_id: "a" * 64},
                fixed_account_id,
                order,
                self.daemon_wrapper(paths),
            ).decode()
            plist_bytes = self.module.render_launch_agent(
                paths,
                self.daemon_wrapper(paths),
            )
            plist = plist_bytes.decode()
            manifest_document = json.loads(manifest)
            plist_document = plistlib.loads(plist_bytes)

        combined = config + manifest + plist
        self.assertNotIn(account.provider_account_id, combined)
        self.assertNotIn(account.email, combined)
        self.assertNotIn(account.access_token, combined)
        self.assertNotIn("secret-run", combined)
        self.assertNotIn("DECODEX_RESET_CARD_SLOT_", combined)
        self.assertIn('"schema":"decodex/account-migration-manifest/1"', manifest)
        self.assertIn('"quota_policy":"reset_to_unknown"', manifest)
        self.assertIn('"history_policy":"do_not_import"', manifest)
        self.assertIn('"daemon_wrapper":', manifest)
        self.assertEqual(
            self.daemon_wrapper(paths),
            manifest_document["daemon_wrapper"],
        )
        self.assertNotIn(
            "daemon_wrapper",
            manifest_document["decision_fingerprints"],
        )
        self.assertIn("supervise-local", plist)
        self.assertEqual(
            str(paths.decodexd),
            plist_document["ProgramArguments"][0],
        )
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
        enrollments, fixed_account_id, order = self.module.build_enrollments(
            [account],
            {digest: 1},
            {1: existing},
            {},
            None,
            {},
        )

        self.assertEqual(account_id, enrollments[0].account_id)
        self.assertEqual("Pinned Label", enrollments[0].display_label)
        self.assertIsNone(fixed_account_id)
        self.assertEqual([account_id], order)
        recovered, _, _ = self.module.build_enrollments(
            [account],
            {digest: 1},
            {},
            {},
            None,
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
                None,
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
                None,
                {},
            )

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

    def test_legacy_read_rejects_non_private_file_without_repair(self):
        with tempfile.TemporaryDirectory() as temp:
            parent = Path(temp) / "legacy"
            parent.mkdir(mode=0o700)
            account_path = parent / "accounts.jsonl"
            account_path.write_text(
                json.dumps(self.account_record()) + "\n",
                encoding="utf-8",
            )
            account_path.chmod(0o644)

            with (
                mock.patch.object(
                    self.module,
                    "require_private_legacy_source_chain",
                ),
                self.assertRaisesRegex(
                    self.module.InstallError,
                    "legacy account file authority is unsafe",
                ),
            ):
                self.module.lock_and_read_legacy_accounts(
                        account_path,
                        os.geteuid(),
                    )

            self.assertEqual(0o644, stat.S_IMODE(account_path.stat().st_mode))
            self.assertEqual(
                0o600,
                stat.S_IMODE((parent / ".accounts.jsonl.lock").stat().st_mode),
            )

    def test_legacy_source_chain_uses_login_home_and_allows_read_execute_bits(self):
        login_home = Path(pwd.getpwuid(os.geteuid()).pw_dir)
        with tempfile.TemporaryDirectory(dir=login_home) as temp:
            private_home = Path(temp) / "login-home"
            private_home.mkdir(mode=0o750)
            shared_parent = private_home / ".codex"
            shared_parent.mkdir(mode=0o755)
            direct_parent = shared_parent / "gate-run"
            direct_parent.mkdir(mode=0o700)
            source = direct_parent / "accounts.jsonl"

            passwd_record = mock.Mock(pw_dir=str(private_home))
            with (
                mock.patch.object(
                    self.module.pwd,
                    "getpwuid",
                    return_value=passwd_record,
                ) as getpwuid,
                mock.patch.dict(
                    self.module.os.environ,
                    {"HOME": str(login_home / "ambient-home-is-not-authority")},
                ),
            ):
                self.module.require_private_legacy_source_chain(
                    source,
                    os.geteuid(),
                )

            getpwuid.assert_called_once_with(os.geteuid())

    def test_legacy_source_chain_rejects_writable_or_foreign_ancestor(self):
        login_home = Path(pwd.getpwuid(os.geteuid()).pw_dir)
        with tempfile.TemporaryDirectory(dir=login_home) as temp:
            private_home = Path(temp) / "login-home"
            private_home.mkdir(mode=0o700)
            ancestor = private_home / ".codex"
            ancestor.mkdir(mode=0o755)
            direct_parent = ancestor / "gate-run"
            direct_parent.mkdir(mode=0o700)
            source = direct_parent / "accounts.jsonl"
            passwd_record = mock.Mock(pw_dir=str(private_home))

            ancestor.chmod(0o720)
            with (
                mock.patch.object(
                    self.module.pwd,
                    "getpwuid",
                    return_value=passwd_record,
                ),
                self.assertRaisesRegex(
                    self.module.InstallError,
                    "legacy account source parent is unsafe",
                ),
            ):
                self.module.require_private_legacy_source_chain(
                    source,
                    os.geteuid(),
                )
            ancestor.chmod(0o755)

            original_lstat = Path.lstat

            def lstat_with_foreign_owner(path):
                metadata = original_lstat(path)
                if path != ancestor:
                    return metadata
                foreign = mock.Mock()
                foreign.st_mode = metadata.st_mode
                foreign.st_uid = os.geteuid() + 100_000
                return foreign

            with (
                mock.patch.object(
                    self.module.pwd,
                    "getpwuid",
                    return_value=passwd_record,
                ),
                mock.patch.object(
                    Path,
                    "lstat",
                    autospec=True,
                    side_effect=lstat_with_foreign_owner,
                ),
                self.assertRaisesRegex(
                    self.module.InstallError,
                    "legacy account source parent is unsafe",
                ),
            ):
                self.module.require_private_legacy_source_chain(
                    source,
                    os.geteuid(),
                )

    def test_absent_legacy_pool_is_a_valid_fresh_install_source(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "missing" / "accounts.jsonl"

            accounts, body, lock_descriptor = self.module.lock_and_read_legacy_accounts(
                path,
                os.geteuid(),
            )

        self.assertEqual([], accounts)
        self.assertIsNone(body)
        self.assertIsNone(lock_descriptor)

    def test_staging_owner_removes_only_exact_account_files(self):
        account_id = "10000000-0000-4000-8000-000000000001"
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.credential_directory.mkdir(parents=True)
            expected = paths.credential_directory / f"{account_id}.json"
            unexpected = paths.credential_directory / "unexpected.json"
            metadata = paths.root / "other-staging-metadata.json"
            expected.write_text("secret", encoding="ascii")
            unexpected.write_text("leave", encoding="ascii")
            paths.staging_config.write_text("staging", encoding="ascii")
            paths.migration_manifest.write_text("manifest", encoding="ascii")
            metadata.write_text("metadata", encoding="ascii")
            owner = self.module.MigrationStagingOwner.for_accounts(paths, [account_id])

            with self.assertRaisesRegex(
                self.module.InstallError,
                "staging could not be retired",
            ):
                owner.cleanup()

            self.assertFalse(expected.exists())
            self.assertFalse(paths.staging_config.exists())
            self.assertEqual("leave", unexpected.read_text(encoding="ascii"))
            self.assertEqual(
                "manifest",
                paths.migration_manifest.read_text(encoding="ascii"),
            )
            self.assertEqual("metadata", metadata.read_text(encoding="ascii"))

    def test_prepared_resume_verifier_failure_cleans_exact_staging_before_release(self):
        account_id = "10000000-0000-4000-8000-000000000001"
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.credential_directory.mkdir(parents=True)
            credential = paths.credential_directory / f"{account_id}.json"
            credential.write_text("secret", encoding="ascii")
            paths.staging_config.write_text("staging", encoding="ascii")
            namespace_lock = mock.Mock()
            with (
                mock.patch.object(
                    self.module,
                    "managed_path_exists",
                    return_value=True,
                ),
                mock.patch.object(
                    self.module,
                    "load_existing_manifest_accounts",
                    return_value={1: {"account_id": account_id}},
                ),
                mock.patch.object(
                    self.module,
                    "run_prepared_account_migration_verifier",
                    side_effect=self.module.InstallError("verifier failed"),
                ),
                mock.patch.object(
                    self.module,
                    "retire_active_migration_sources",
                ) as retire,
                mock.patch.object(
                    self.module,
                    "run_account_migration_finalizer",
                ) as finalize,
                self.assertRaisesRegex(self.module.InstallError, "verifier failed"),
            ):
                self.module.resume_prepared_account_migration(
                    paths,
                    os.geteuid(),
                    namespace_lock,
                    checkpoint=lambda _name: None,
                    transition_gate_fd=None,
                )

            self.assertFalse(credential.exists())
            self.assertFalse(paths.credential_directory.exists())
            self.assertFalse(paths.staging_config.exists())
            retire.assert_not_called()
            finalize.assert_not_called()

    def test_prepared_resume_preserves_unknown_staging_file_and_primary_failure(self):
        account_id = "10000000-0000-4000-8000-000000000001"
        verifier_failure = self.module.InstallError("destination mismatch")
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.credential_directory.mkdir(parents=True)
            credential = paths.credential_directory / f"{account_id}.json"
            unknown = paths.credential_directory / "unknown.json"
            credential.write_text("secret", encoding="ascii")
            unknown.write_text("preserve", encoding="ascii")
            paths.staging_config.write_text("staging", encoding="ascii")
            with (
                mock.patch.object(self.module, "managed_path_exists", return_value=True),
                mock.patch.object(
                    self.module,
                    "load_existing_manifest_accounts",
                    return_value={1: {"account_id": account_id}},
                ),
                mock.patch.object(
                    self.module,
                    "run_prepared_account_migration_verifier",
                    side_effect=verifier_failure,
                ),
                self.assertRaisesRegex(
                    self.module.InstallError,
                    "destination mismatch",
                ) as raised,
            ):
                self.module.resume_prepared_account_migration(
                    paths,
                    os.geteuid(),
                    mock.Mock(),
                    checkpoint=lambda _name: None,
                    transition_gate_fd=None,
                )

            self.assertIs(raised.exception, verifier_failure)
            self.assertIsInstance(raised.exception.__cause__, self.module.InstallError)
            self.assertIn("staging could not be retired", str(raised.exception.__cause__))
            self.assertFalse(credential.exists())
            self.assertFalse(paths.staging_config.exists())
            self.assertEqual("preserve", unknown.read_text(encoding="ascii"))

    def test_prepared_resume_cleanup_only_failure_blocks_retirement_and_finalization(self):
        account_id = "10000000-0000-4000-8000-000000000001"
        cleanup_failure = self.module.InstallError("cleanup failed")
        cleanup_retry_failure = self.module.InstallError("cleanup retry failed")
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            with (
                mock.patch.object(self.module, "managed_path_exists", return_value=True),
                mock.patch.object(
                    self.module,
                    "load_existing_manifest_accounts",
                    return_value={1: {"account_id": account_id}},
                ),
                mock.patch.object(
                    self.module,
                    "run_prepared_account_migration_verifier",
                ),
                mock.patch.object(
                    self.module.MigrationStagingOwner,
                    "cleanup",
                    autospec=True,
                    side_effect=[cleanup_failure, cleanup_retry_failure],
                ) as cleanup,
                mock.patch.object(
                    self.module,
                    "retire_active_migration_sources",
                ) as retire,
                mock.patch.object(
                    self.module,
                    "run_account_migration_finalizer",
                ) as finalize,
                self.assertRaisesRegex(
                    self.module.InstallError,
                    "cleanup failed",
                ) as raised,
            ):
                self.module.resume_prepared_account_migration(
                    paths,
                    os.geteuid(),
                    mock.Mock(),
                    checkpoint=lambda _name: None,
                    transition_gate_fd=None,
                )

        self.assertIs(raised.exception, cleanup_failure)
        self.assertIs(raised.exception.__cause__, cleanup_retry_failure)
        self.assertEqual(2, cleanup.call_count)
        retire.assert_not_called()
        finalize.assert_not_called()

    def test_prepared_resume_destination_mismatch_never_retires_or_finalizes(self):
        account_id = "10000000-0000-4000-8000-000000000001"
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.credential_directory.mkdir(parents=True)
            credential = paths.credential_directory / f"{account_id}.json"
            credential.write_text("secret", encoding="ascii")
            paths.staging_config.write_text("staging", encoding="ascii")
            with (
                mock.patch.object(self.module, "managed_path_exists", return_value=True),
                mock.patch.object(
                    self.module,
                    "load_existing_manifest_accounts",
                    return_value={1: {"account_id": account_id}},
                ),
                mock.patch.object(
                    self.module,
                    "run_prepared_account_migration_verifier",
                    side_effect=self.module.InstallError(
                        "prepared account migration destination did not verify"
                    ),
                ),
                mock.patch.object(
                    self.module,
                    "retire_active_migration_sources",
                ) as retire,
                mock.patch.object(
                    self.module,
                    "run_account_migration_finalizer",
                ) as finalize,
                self.assertRaisesRegex(
                    self.module.InstallError,
                    "destination did not verify",
                ),
            ):
                self.module.resume_prepared_account_migration(
                    paths,
                    os.geteuid(),
                    mock.Mock(),
                    checkpoint=lambda _name: None,
                    transition_gate_fd=None,
                )

            self.assertFalse(credential.exists())
            self.assertFalse(paths.staging_config.exists())
            retire.assert_not_called()
            finalize.assert_not_called()

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
            "schema": "decodex/cli-account/1",
            "command": "list",
            "outcome": "success",
            "result": {
                "accounts": [
                    {"account_id": first_account},
                    {"account_id": second_account},
                ],
            },
        }

        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            doctor = mock.patch.object(
                self.module,
                "query_doctor",
                return_value=True,
            )
            accounts_query = mock.patch.object(
                self.module,
                "query_accounts",
                return_value=accounts,
            )
            with doctor, accounts_query as query:
                self.module.wait_for_service(paths, {first_account, second_account})

        query.assert_called_once_with(paths)

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
                self.assertIsNone(self.module.query_accounts(paths))

        command = run.call_args.args[0]
        self.assertEqual(str(paths.root), command[command.index("--root") + 1])
        self.assertEqual(["account", "list"], command[-2:])
        self.assertNotIn("private-marker", str(run.call_args))

    def test_cutover_probe_classifies_only_postgres_receipt_phase(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            with mock.patch.object(
                self.module,
                "psql_scalar",
                side_effect=[
                    "decodex.account_migration_receipts",
                    "prepared",
                    "decodex.account_migration_receipts",
                    "completed",
                ],
            ) as query:
                self.assertEqual(
                    "prepared",
                    self.module.migration_receipt_phase(paths, {"PGUSER": "owner"}),
                )
                self.assertEqual(
                    "completed",
                    self.module.migration_receipt_phase(paths, {"PGUSER": "owner"}),
                )

        statements = [call.args[2] for call in query.call_args_list]
        self.assertIn("to_regclass", statements[0])
        self.assertIn("WHERE singleton", statements[1])
        self.assertNotIn(str(paths.legacy_accounts), "".join(statements))
        self.assertNotIn(str(paths.mapping), "".join(statements))
        self.assertNotIn(str(paths.legacy_config), "".join(statements))

    def test_prepared_verifier_failure_cleans_before_postgres_stop_and_lock_release(self):
        events: list[str] = []
        verifier_failure = self.module.InstallError("prepared verifier failed")
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            process = mock.Mock()
            namespace_lock = mock.Mock()
            namespace_lock.close.side_effect = lambda: events.append("lock_release")

            def fail_verifier(*_args, **_kwargs):
                events.append("verify")
                raise verifier_failure

            with (
                mock.patch.object(self.module, "install_paths", return_value=paths),
                mock.patch.object(
                    self.module,
                    "validate_host",
                    return_value=os.geteuid(),
                ),
                mock.patch.object(self.module, "ensure_installer_namespace_layout"),
                mock.patch.object(
                    self.module.InstallerNamespaceLock,
                    "acquire",
                    return_value=namespace_lock,
                ),
                mock.patch.object(self.module, "ensure_directories"),
                mock.patch.object(self.module, "postgres_major", return_value=18),
                mock.patch.object(self.module, "bootout_service"),
                mock.patch.object(
                    self.module,
                    "read_optional_owned_source",
                    return_value=b"version = 1\n",
                ),
                mock.patch.object(self.module, "initialize_cluster"),
                mock.patch.object(
                    self.module,
                    "start_temporary_postgres",
                    return_value=process,
                ),
                mock.patch.object(
                    self.module,
                    "psql_environment",
                    return_value={"PGUSER": "owner"},
                ),
                mock.patch.object(self.module, "ensure_roles_and_database"),
                mock.patch.object(
                    self.module,
                    "migration_receipt_phase",
                    return_value="prepared",
                ),
                mock.patch.object(self.module, "managed_path_exists", return_value=True),
                mock.patch.object(
                    self.module,
                    "load_existing_manifest_accounts",
                    return_value={},
                ),
                mock.patch.object(
                    self.module,
                    "run_prepared_account_migration_verifier",
                    side_effect=fail_verifier,
                ),
                mock.patch.object(
                    self.module.MigrationStagingOwner,
                    "cleanup",
                    autospec=True,
                    side_effect=lambda _owner: events.append("cleanup"),
                ),
                mock.patch.object(
                    self.module,
                    "stop_temporary_postgres",
                    side_effect=lambda _process: events.append("stop_pg"),
                ),
                mock.patch.object(
                    self.module,
                    "retire_active_migration_sources",
                ) as retire,
                mock.patch.object(
                    self.module,
                    "run_account_migration_finalizer",
                ) as finalize,
                mock.patch.object(self.module, "atomic_write") as config_write,
                mock.patch.object(self.module, "bootstrap_service") as bootstrap,
                self.assertRaisesRegex(
                    self.module.InstallError,
                    "prepared verifier failed",
                ) as raised,
            ):
                self.module.main([])

        self.assertIs(raised.exception, verifier_failure)
        self.assertEqual(["verify", "cleanup", "stop_pg", "lock_release"], events)
        retire.assert_not_called()
        finalize.assert_not_called()
        config_write.assert_not_called()
        bootstrap.assert_not_called()

    def test_fresh_verifier_failure_cleans_before_postgres_stop_and_lock_release(self):
        events: list[str] = []
        verifier_failure = self.module.InstallError("fresh verifier failed")
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            process = mock.Mock()
            namespace_lock = mock.Mock()
            namespace_lock.close.side_effect = lambda: events.append("lock_release")

            def fail_verifier(*_args, **_kwargs):
                events.append("verify")
                raise verifier_failure

            with (
                mock.patch.object(self.module, "install_paths", return_value=paths),
                mock.patch.object(
                    self.module,
                    "validate_host",
                    return_value=os.geteuid(),
                ),
                mock.patch.object(self.module, "ensure_installer_namespace_layout"),
                mock.patch.object(
                    self.module.InstallerNamespaceLock,
                    "acquire",
                    return_value=namespace_lock,
                ),
                mock.patch.object(self.module, "ensure_directories"),
                mock.patch.object(self.module, "postgres_major", return_value=18),
                mock.patch.object(self.module, "bootout_service"),
                mock.patch.object(
                    self.module,
                    "read_optional_owned_source",
                    return_value=None,
                ),
                mock.patch.object(
                    self.module,
                    "lock_and_read_legacy_accounts",
                    return_value=([], None, None),
                ),
                mock.patch.object(
                    self.module,
                    "prepare_vnext_config_source",
                    return_value=None,
                ),
                mock.patch.object(self.module, "managed_path_exists", return_value=False),
                mock.patch.object(
                    self.module,
                    "load_existing_mapping",
                    return_value={},
                ),
                mock.patch.object(
                    self.module,
                    "parse_legacy_account_config",
                    return_value=(None, {}),
                ),
                mock.patch.object(
                    self.module,
                    "load_existing_manifest_accounts",
                    return_value={},
                ),
                mock.patch.object(
                    self.module,
                    "build_enrollments",
                    return_value=([], None, []),
                ),
                mock.patch.object(
                    self.module,
                    "render_config",
                    return_value="version = 1\n",
                ),
                mock.patch.object(
                    self.module,
                    "render_launch_agent",
                    return_value=b"launch-agent",
                ),
                mock.patch.object(
                    self.module,
                    "credential_sources",
                    return_value={},
                ),
                mock.patch.object(
                    self.module,
                    "render_migration_manifest",
                    return_value=b"manifest",
                ),
                mock.patch.object(self.module, "atomic_write") as writes,
                mock.patch.object(self.module, "initialize_cluster"),
                mock.patch.object(
                    self.module,
                    "start_temporary_postgres",
                    return_value=process,
                ),
                mock.patch.object(
                    self.module,
                    "psql_environment",
                    return_value={"PGUSER": "owner"},
                ),
                mock.patch.object(self.module, "ensure_roles_and_database"),
                mock.patch.object(
                    self.module,
                    "run_offline_account_migration",
                    side_effect=fail_verifier,
                ),
                mock.patch.object(
                    self.module.MigrationStagingOwner,
                    "cleanup",
                    autospec=True,
                    side_effect=lambda _owner: events.append("cleanup"),
                ),
                mock.patch.object(
                    self.module,
                    "stop_temporary_postgres",
                    side_effect=lambda _process: events.append("stop_pg"),
                ),
                mock.patch.object(
                    self.module,
                    "retire_active_migration_sources",
                ) as retire,
                mock.patch.object(
                    self.module,
                    "run_account_migration_finalizer",
                ) as finalize,
                mock.patch.object(self.module, "bootstrap_service") as bootstrap,
                self.assertRaisesRegex(
                    self.module.InstallError,
                    "fresh verifier failed",
                ) as raised,
            ):
                self.module.main([])

        self.assertIs(raised.exception, verifier_failure)
        self.assertEqual(["verify", "cleanup", "stop_pg", "lock_release"], events)
        self.assertFalse(
            any(call.args[0] == paths.config for call in writes.call_args_list)
        )
        retire.assert_not_called()
        finalize.assert_not_called()
        bootstrap.assert_not_called()

    def test_prepared_cutover_resumes_finalization_without_legacy_reads(self):
        result = {
            "schema": "decodex/account-migration-result/1",
            "outcome": "verified",
            "manifest_sha256": "a" * 64,
            "account_count": 0,
            "account_ids": [],
            "intent_recorded": False,
            "receipt_completed": True,
        }
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.migration_manifest.write_text(
                json.dumps(
                    {
                        "schema": self.module.MIGRATION_MANIFEST_SCHEMA,
                        "accounts": [],
                    }
                ),
                encoding="utf-8",
            )
            paths.migration_manifest.chmod(0o600)
            process = mock.Mock()
            namespace_lock = mock.Mock()
            patches = [
                mock.patch.object(self.module, "install_paths", return_value=paths),
                mock.patch.object(self.module, "validate_host", return_value=os.geteuid()),
                mock.patch.object(self.module, "ensure_installer_namespace_layout"),
                mock.patch.object(
                    self.module.InstallerNamespaceLock,
                    "acquire",
                    return_value=namespace_lock,
                ),
                mock.patch.object(self.module, "ensure_directories"),
                mock.patch.object(self.module, "postgres_major", return_value=18),
                mock.patch.object(self.module, "bootout_service"),
                mock.patch.object(
                    self.module,
                    "read_optional_owned_source",
                    return_value=b"version = 1\n",
                ),
                mock.patch.object(self.module, "initialize_cluster"),
                mock.patch.object(
                    self.module,
                    "start_temporary_postgres",
                    return_value=process,
                ),
                mock.patch.object(
                    self.module,
                    "psql_environment",
                    return_value={"PGUSER": "owner"},
                ),
                mock.patch.object(self.module, "ensure_roles_and_database"),
                mock.patch.object(
                    self.module,
                    "migration_receipt_phase",
                    return_value="prepared",
                ),
                mock.patch.object(
                    self.module,
                    "run_prepared_account_migration_verifier",
                ),
                mock.patch.object(
                    self.module,
                    "run_account_migration_finalizer",
                    return_value=result,
                ),
                mock.patch.object(
                    self.module,
                    "run_completed_account_migration_verifier",
                ),
                mock.patch.object(self.module, "stop_temporary_postgres"),
                mock.patch.object(
                    self.module,
                    "lock_and_read_legacy_accounts",
                    side_effect=AssertionError("legacy account pool was reopened"),
                ),
                mock.patch("builtins.print"),
            ]
            with ExitStack() as stack:
                entered = [stack.enter_context(patch) for patch in patches]
                self.assertEqual(0, self.module.main(["--no-launch"]))

        entered[13].assert_called_once_with(
            paths,
            namespace_lock,
            transition_gate_fd=None,
        )
        entered[14].assert_called_once_with(
            paths,
            namespace_lock,
            transition_gate_fd=None,
        )
        entered[15].assert_not_called()
        entered[17].assert_not_called()
        namespace_lock.close.assert_called_once_with()
        output = json.loads(entered[18].call_args.args[0])
        self.assertEqual(0, output["account_count"])
        self.assertEqual("a" * 64, output["migration_manifest_sha256"])

    def test_fresh_install_without_legacy_pool_completes_empty(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            process = mock.Mock()
            namespace_lock = mock.Mock()

            def migration_result(_paths, _namespace_lock, **_kwargs):
                manifest = json.loads(paths.migration_manifest.read_text(encoding="utf-8"))
                return {
                    "schema": "decodex/account-migration-result/1",
                    "outcome": "destinations_verified",
                    "manifest_sha256": self.module.decision_digest(manifest),
                    "account_count": 0,
                    "account_ids": [],
                    "intent_recorded": True,
                    "receipt_completed": False,
                }

            def final_result(_paths, _namespace_lock, **_kwargs):
                result = migration_result(_paths, _namespace_lock)
                result.update(
                    {
                        "outcome": "verified",
                        "intent_recorded": False,
                        "receipt_completed": True,
                    }
                )
                return result

            patches = [
                mock.patch.object(self.module, "install_paths", return_value=paths),
                mock.patch.object(self.module, "validate_host", return_value=os.geteuid()),
                mock.patch.object(self.module, "ensure_installer_namespace_layout"),
                mock.patch.object(
                    self.module.InstallerNamespaceLock,
                    "acquire",
                    return_value=namespace_lock,
                ),
                mock.patch.object(self.module, "ensure_directories"),
                mock.patch.object(self.module, "postgres_major", return_value=18),
                mock.patch.object(self.module, "bootout_service"),
                mock.patch.object(self.module, "initialize_cluster"),
                mock.patch.object(
                    self.module,
                    "start_temporary_postgres",
                    return_value=process,
                ),
                mock.patch.object(
                    self.module,
                    "psql_environment",
                    return_value={"PGUSER": "owner"},
                ),
                mock.patch.object(self.module, "ensure_roles_and_database"),
                mock.patch.object(
                    self.module,
                    "run_offline_account_migration",
                    side_effect=migration_result,
                ),
                mock.patch.object(
                    self.module,
                    "run_account_migration_finalizer",
                    side_effect=final_result,
                ),
                mock.patch.object(self.module, "stop_temporary_postgres"),
                mock.patch("builtins.print"),
            ]
            with ExitStack() as stack:
                entered = [stack.enter_context(patch) for patch in patches]
                self.assertEqual(0, self.module.main(["--no-launch"]))

        entered[11].assert_called_once_with(
            paths,
            namespace_lock,
            transition_gate_fd=None,
        )
        entered[12].assert_called_once_with(
            paths,
            namespace_lock,
            transition_gate_fd=None,
        )
        namespace_lock.close.assert_called_once_with()
        output = json.loads(entered[14].call_args.args[0])
        self.assertEqual("success", output["outcome"])
        self.assertEqual(0, output["account_count"])

    def test_completed_cutover_verifier_does_not_reopen_legacy_sources(self):
        account_id = "10000000-0000-4000-8000-000000000001"
        result = {
            "schema": "decodex/account-migration-result/1",
            "outcome": "verified",
            "manifest_sha256": "a" * 64,
            "account_count": 1,
            "account_ids": [account_id],
            "intent_recorded": False,
            "receipt_completed": True,
        }
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            completed = subprocess.CompletedProcess(
                [str(paths.decodexd)],
                0,
                json.dumps(result),
                "",
            )
            namespace_lock = mock.Mock()
            descriptor = self.daemon_wrapper(paths)
            with (
                mock.patch.object(
                    self.module,
                    "inspect_daemon_wrapper",
                    return_value=descriptor,
                ),
                mock.patch.object(
                    self.module,
                    "verify_daemon_wrapper",
                    return_value=descriptor,
                ),
                mock.patch.object(
                    self.module,
                    "run_installer_child",
                    return_value=completed,
                ) as run,
            ):
                self.assertEqual(
                    result,
                    self.module.run_completed_account_migration_verifier(
                        paths,
                        namespace_lock,
                    ),
                )

        command = run.call_args.args[0]
        self.assertIs(namespace_lock, run.call_args.args[1])
        self.assertIn("verify-account-migration", command)
        self.assertNotIn(str(paths.migration_manifest), command)
        self.assertNotIn(str(paths.legacy_accounts), command)
        self.assertIn(str(paths.mapping), command)
        self.assertIn(str(paths.vnext_config_source), command)
        self.assertNotIn(str(paths.legacy_config), command)

    def test_installed_launch_agent_contract_requires_exact_drain_settings(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = self.paths(Path(temp))
            paths.launch_agent.write_bytes(
                self.module.render_launch_agent(
                    paths,
                    self.daemon_wrapper(paths),
                )
            )
            paths.launch_agent.chmod(0o600)
            self.assertTrue(
                self.module.installed_launch_agent_supports_graceful_drain(
                    paths.launch_agent, os.geteuid()
                )
            )

            legacy = plistlib.loads(
                self.module.render_launch_agent(
                    paths,
                    self.daemon_wrapper(paths),
                )
            )
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
