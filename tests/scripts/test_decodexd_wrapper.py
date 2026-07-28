from __future__ import annotations

import datetime
import importlib.util
import json
import os
import plistlib
import shutil
import stat
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts/macos/decodexd_wrapper.py"


def load_module():
    spec = importlib.util.spec_from_file_location("decodexd_wrapper", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


class DecodexdWrapperTests(unittest.TestCase):
    def setUp(self):
        self.module = load_module()
        self.commands: list[list[str]] = []
        self.leaf_certificate = b"\x30\x03\x30\x01\x00"
        self.profile = self.valid_profile()
        self.entitlements = dict(self.module.EXPECTED_ENTITLEMENTS)

    def valid_profile(self):
        return {
            "TeamIdentifier": [self.module.TEAM_IDENTIFIER],
            "ApplicationIdentifierPrefix": [self.module.TEAM_IDENTIFIER],
            "ExpirationDate": datetime.datetime(2099, 1, 1),
            "ProvisionedDevices": ["device"],
            "DeveloperCertificates": [self.leaf_certificate],
            "Entitlements": {
                **self.module.EXPECTED_ENTITLEMENTS,
                "get-task-allow": True,
            },
        }

    def tool_result(self, command):
        self.commands.append(command)
        if command[:4] == [str(self.module.SECURITY), "cms", "-D", "-i"]:
            return plistlib.dumps(self.profile), b""
        if command[0] != str(self.module.CODESIGN):
            raise AssertionError("unexpected platform tool")
        if "--force" in command:
            wrapper = Path(command[-1])
            signature = wrapper / "Contents/_CodeSignature"
            signature.mkdir(mode=0o755)
            (signature / "CodeResources").write_bytes(b"sealed")
            return b"", b""
        if "--verify" in command:
            return b"", b""
        if "--entitlements" in command:
            return plistlib.dumps(self.entitlements), b""
        if "--extract-certificates" in command:
            prefix = Path(command[command.index("--extract-certificates") + 1])
            prefix.with_name(prefix.name + "0").write_bytes(
                self.leaf_certificate
            )
            return b"", b""
        if "--verbose=4" in command:
            return (
                b"",
                (
                    "Identifier=box.acg.decodex.daemon\n"
                    "TeamIdentifier=T54QFA7W2S\n"
                    "CDHash=ABCDEF0123456789ABCDEF0123456789ABCDEF01\n"
                    "CodeDirectory v=20500 size=512 flags=0x10000(runtime) hashes=8\n"
                    "Authority=Apple Development: Decodex Test\n"
                    "Authority=Apple Worldwide Developer Relations Certification Authority\n"
                ).encode(),
            )
        if "-r-" in command:
            return (
                b"",
                (
                    'designated => anchor apple generic and identifier '
                    '"box.acg.decodex.daemon"\n'
                ).encode(),
            )
        raise AssertionError("unexpected codesign command")

    def make_wrapper(self, root: Path) -> Path:
        wrapper = root / self.module.WRAPPER_NAME
        contents = wrapper / "Contents"
        macos = contents / "MacOS"
        signature = contents / "_CodeSignature"
        macos.mkdir(parents=True)
        signature.mkdir()
        shutil.copyfile(
            self.module.INFO_PLIST_SOURCE,
            contents / "Info.plist",
        )
        (contents / "embedded.provisionprofile").write_bytes(b"profile")
        executable = macos / "decodexd"
        executable.write_bytes(b"\xcf\xfa\xed\xfe" + b"daemon")
        executable.chmod(0o755)
        (signature / "CodeResources").write_bytes(b"sealed")
        return wrapper

    def inspect(self, wrapper):
        with mock.patch.object(
            self.module,
            "run_tool",
            side_effect=self.tool_result,
        ):
            return self.module.inspect_wrapper(wrapper)

    def test_fixed_resources_are_closed_and_daemon_specific(self):
        info = plistlib.loads(self.module.INFO_PLIST_SOURCE.read_bytes())
        entitlements = plistlib.loads(self.module.ENTITLEMENTS_SOURCE.read_bytes())

        self.assertEqual(
            {
                "CFBundleDevelopmentRegion": "en",
                "CFBundleExecutable": "decodexd",
                "CFBundleIdentifier": "box.acg.decodex.daemon",
                "CFBundleInfoDictionaryVersion": "6.0",
                "CFBundleName": "decodexd",
                "CFBundlePackageType": "APPL",
                "LSBackgroundOnly": True,
            },
            info,
        )
        self.assertEqual(self.module.EXPECTED_ENTITLEMENTS, entitlements)
        self.assertEqual(
            ["T54QFA7W2S.box.acg.decodex.daemon"],
            entitlements["keychain-access-groups"],
        )
        self.assertEqual(
            "T54QFA7W2S.box.acg.decodex.daemon",
            entitlements["com.apple.application-identifier"],
        )

    def test_inspector_emits_only_the_strict_nonsecret_descriptor(self):
        with tempfile.TemporaryDirectory() as temp:
            wrapper = self.make_wrapper(Path(temp))
            descriptor = self.inspect(wrapper)

        self.assertEqual(self.module.DESCRIPTOR_FIELDS, set(descriptor))
        self.assertEqual("decodex/daemon-wrapper/1", descriptor["schema"])
        self.assertEqual(str(wrapper), descriptor["wrapper_path"])
        self.assertEqual(
            str(wrapper / "Contents/MacOS/decodexd"),
            descriptor["executable_path"],
        )
        self.assertEqual("development", descriptor["profile_channel"])
        self.assertEqual(
            ["T54QFA7W2S.box.acg.decodex.daemon"],
            descriptor["keychain_access_groups"],
        )
        result = self.module.finite_result(descriptor)
        self.assertEqual(
            self.module.canonical_sha256(descriptor),
            result["descriptor_sha256"],
        )
        self.assertIn(temp, json.dumps(result))
        self.assertNotIn("ProvisionedDevices", json.dumps(result))
        self.assertNotIn("DeveloperCertificates", json.dumps(result))
        self.assertNotIn("Authority=", json.dumps(result))
        expected_identity = {
            "identifier": "box.acg.decodex.daemon",
            "team_identifier": "T54QFA7W2S",
            "cdhash": "abcdef0123456789abcdef0123456789abcdef01",
            "code_directory": (
                "v=20500 size=512 flags=0x10000(runtime) hashes=8"
            ),
            "designated_requirement": (
                'anchor apple generic and identifier '
                '"box.acg.decodex.daemon"'
            ),
            "certificate_authorities": [
                "Apple Development: Decodex Test",
                (
                    "Apple Worldwide Developer Relations "
                    "Certification Authority"
                ),
            ],
            "leaf_certificate_sha256": self.module.sha256_bytes(
                self.leaf_certificate
            ),
        }
        self.assertEqual(
            self.module.canonical_sha256(expected_identity),
            descriptor["signature_identity_sha256"],
        )

    def test_composer_uses_only_fixed_layout_signing_and_verification(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable = root / "decodexd"
            executable.write_bytes(b"\xcf\xfa\xed\xfe" + b"daemon")
            executable.chmod(0o755)
            profile = root / "profile.mobileprovision"
            profile.write_bytes(b"profile")
            output_parent = root / "output"
            output_parent.mkdir(mode=0o700)
            output = output_parent / self.module.WRAPPER_NAME
            with mock.patch.object(
                self.module,
                "run_tool",
                side_effect=self.tool_result,
            ):
                descriptor = self.module.compose_wrapper(
                    executable,
                    profile,
                    "Apple Development: Decodex Test",
                    output,
                )

            self.assertTrue(output.is_dir())
            self.assertEqual(self.module.DESCRIPTOR_FIELDS, set(descriptor))
            self.assertEqual(
                {"Info.plist", "MacOS", "_CodeSignature", "embedded.provisionprofile"},
                {member.name for member in (output / "Contents").iterdir()},
            )
            self.assertEqual(
                0o755,
                stat.S_IMODE((output / "Contents/MacOS/decodexd").stat().st_mode),
            )

        sign = next(command for command in self.commands if "--force" in command)
        self.assertEqual("/usr/bin/codesign", sign[0])
        self.assertIn("runtime", sign)
        self.assertIn(str(self.module.ENTITLEMENTS_SOURCE), sign)
        verify = next(command for command in self.commands if "--verify" in command)
        self.assertEqual(
            [
                "/usr/bin/codesign",
                "--verify",
                "--strict",
                "--all-architectures",
                "--verbose=2",
            ],
            verify[:-1],
        )
        security = next(
            command for command in self.commands if command[0].endswith("security")
        )
        self.assertEqual(["/usr/bin/security", "cms", "-D", "-i"], security[:-1])
        certificate_commands = [
            command
            for command in self.commands
            if "--extract-certificates" in command
        ]
        self.assertTrue(certificate_commands)
        for command in certificate_commands:
            prefix = Path(
                command[command.index("--extract-certificates") + 1]
            )
            self.assertFalse(prefix.parent.exists())

    def test_descriptor_rejects_missing_extra_duplicate_and_wrong_fields(self):
        with tempfile.TemporaryDirectory() as temp:
            descriptor = self.inspect(self.make_wrapper(Path(temp)))
        for mutation in ("missing", "extra", "wrong"):
            changed = dict(descriptor)
            if mutation == "missing":
                del changed["team_identifier"]
            elif mutation == "extra":
                changed["other"] = True
            else:
                changed["team_identifier"] = "WRONG"
            with self.subTest(mutation=mutation), self.assertRaises(
                self.module.WrapperError
            ):
                self.module.validate_descriptor(changed)
        duplicate = self.module.canonical_json(descriptor)
        duplicate = duplicate[:-1] + b',"schema":"decodex/daemon-wrapper/1"}'
        with self.assertRaisesRegex(self.module.WrapperError, "duplicate"):
            self.module.load_descriptor(duplicate)

    def test_profile_rejects_wrong_team_key_expiry_channel_and_certificates(self):
        variants = {}
        wrong_team = self.valid_profile()
        wrong_team["TeamIdentifier"] = ["WRONG"]
        variants["team"] = wrong_team
        expired = self.valid_profile()
        expired["ExpirationDate"] = datetime.datetime(2000, 1, 1)
        variants["expiry"] = expired
        distribution = self.valid_profile()
        distribution.pop("ProvisionedDevices")
        distribution["ProvisionsAllDevices"] = True
        variants["channel"] = distribution
        wrong_key = self.valid_profile()
        wrong_key["Entitlements"] = {
            **wrong_key["Entitlements"],
            "application-identifier": self.module.APPLICATION_IDENTIFIER,
        }
        del wrong_key["Entitlements"]["com.apple.application-identifier"]
        variants["legacy_application_key"] = wrong_key
        missing_certificates = self.valid_profile()
        del missing_certificates["DeveloperCertificates"]
        variants["missing_certificates"] = missing_certificates
        duplicate_certificates = self.valid_profile()
        duplicate_certificates["DeveloperCertificates"] = [
            self.leaf_certificate,
            self.leaf_certificate,
        ]
        variants["duplicate_certificates"] = duplicate_certificates
        wrong_leaf = self.valid_profile()
        wrong_leaf["DeveloperCertificates"] = [b"\x30\x03\x30\x01\x01"]
        variants["leaf_not_in_profile"] = wrong_leaf
        with tempfile.TemporaryDirectory() as temp:
            wrapper = self.make_wrapper(Path(temp))
            for name, profile in variants.items():
                self.profile = profile
                with self.subTest(name=name), self.assertRaises(
                    self.module.WrapperError
                ):
                    self.inspect(wrapper)

    def test_entitlement_and_signature_disagreement_refuse(self):
        with tempfile.TemporaryDirectory() as temp:
            wrapper = self.make_wrapper(Path(temp))
            variants = {
                "missing": {
                    key: value
                    for key, value in self.module.EXPECTED_ENTITLEMENTS.items()
                    if key != "keychain-access-groups"
                },
                "extra": {
                    **self.module.EXPECTED_ENTITLEMENTS,
                    "get-task-allow": True,
                },
                "wrong": {
                    **self.module.EXPECTED_ENTITLEMENTS,
                    "com.apple.application-identifier": "WRONG",
                },
                "legacy": {
                    key: value
                    for key, value in self.module.EXPECTED_ENTITLEMENTS.items()
                    if key != "com.apple.application-identifier"
                },
            }
            variants["legacy"]["application-identifier"] = (
                self.module.APPLICATION_IDENTIFIER
            )
            for name, entitlements in variants.items():
                self.entitlements = entitlements
                with self.subTest(name=name), self.assertRaisesRegex(
                    self.module.WrapperError,
                    "entitlements differ",
                ):
                    self.inspect(wrapper)
            self.entitlements = dict(self.module.EXPECTED_ENTITLEMENTS)

            def wrong_signature(command):
                stdout, stderr = self.tool_result(command)
                if "--verbose=4" in command:
                    stderr = stderr.replace(
                        b"TeamIdentifier=T54QFA7W2S",
                        b"TeamIdentifier=WRONG",
                    )
                return stdout, stderr

            with (
                mock.patch.object(
                    self.module,
                    "run_tool",
                    side_effect=wrong_signature,
                ),
                self.assertRaisesRegex(self.module.WrapperError, "identity differs"),
            ):
                self.module.inspect_wrapper(wrapper)

    def test_raw_binary_layout_symlink_and_executable_drift_refuse(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            wrapper = self.make_wrapper(root)
            expected = self.inspect(wrapper)
            with self.assertRaises(self.module.WrapperError):
                self.module.inspect_wrapper(wrapper / "Contents/MacOS/decodexd")

            extra = wrapper / "Contents/MacOS/other"
            extra.write_bytes(b"other")
            extra.chmod(0o755)
            with self.assertRaisesRegex(self.module.WrapperError, "layout"):
                self.module.inspect_wrapper(wrapper)
            extra.unlink()

            code_resources = wrapper / "Contents/_CodeSignature/CodeResources"
            code_resources.write_bytes(
                b"x" * (self.module.MAX_PLIST_BYTES + 1)
            )
            with self.assertRaisesRegex(
                self.module.WrapperError,
                "CodeResources",
            ):
                self.module.inspect_wrapper(wrapper)
            code_resources.write_bytes(b"sealed")

            executable = wrapper / "Contents/MacOS/decodexd"
            executable.write_bytes(executable.read_bytes() + b"drift")
            executable.chmod(0o755)
            with (
                mock.patch.object(
                    self.module,
                    "run_tool",
                    side_effect=self.tool_result,
                ),
                self.assertRaisesRegex(self.module.WrapperError, "identity differs"),
            ):
                self.module.verify_wrapper(wrapper, expected)

            profile = wrapper / "Contents/embedded.provisionprofile"
            profile.unlink()
            profile.symlink_to(root / "outside-profile")
            with self.assertRaises(self.module.WrapperError):
                self.module.inspect_wrapper(wrapper)

    def test_relocation_and_absolute_cross_binding_refuse(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            original_parent = root / "original"
            moved_parent = root / "moved"
            other_parent = root / "other"
            original_parent.mkdir()
            moved_parent.mkdir()
            other_parent.mkdir()
            wrapper = self.make_wrapper(original_parent)
            other = self.make_wrapper(other_parent)
            expected = self.inspect(wrapper)
            moved = moved_parent / self.module.WRAPPER_NAME
            os.rename(wrapper, moved)
            moved_descriptor = self.inspect(moved)
            self.assertNotEqual(
                expected["wrapper_path"],
                moved_descriptor["wrapper_path"],
            )
            with self.assertRaises(self.module.WrapperError):
                self.module.verify_wrapper(moved, expected)
            cross_bound = dict(moved_descriptor)
            cross_bound["executable_path"] = str(
                other / "Contents/MacOS/decodexd"
            )
            with self.assertRaisesRegex(self.module.WrapperError, "fields differ"):
                self.module.validate_descriptor(cross_bound)

    def test_post_publication_inspection_failure_removes_only_new_output(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable = root / "decodexd"
            executable.write_bytes(b"\xcf\xfa\xed\xfe" + b"daemon")
            executable.chmod(0o755)
            profile = root / "profile"
            profile.write_bytes(b"profile")
            output_parent = root / "output"
            output_parent.mkdir(mode=0o700)
            output = output_parent / self.module.WRAPPER_NAME
            marker = output_parent / "preserve"
            marker.write_text("preserve", encoding="ascii")

            def fail_published_inspection(command):
                if (
                    command[:4]
                    == [str(self.module.SECURITY), "cms", "-D", "-i"]
                    and str(output) in command[-1]
                ):
                    raise self.module.WrapperError("injected published inspection")
                return self.tool_result(command)

            with (
                mock.patch.object(
                    self.module,
                    "run_tool",
                    side_effect=fail_published_inspection,
                ),
                self.assertRaisesRegex(
                    self.module.WrapperError,
                    "published inspection",
                ),
            ):
                self.module.compose_wrapper(
                    executable,
                    profile,
                    "Apple Development: Decodex Test",
                    output,
                )
            self.assertFalse(output.exists())
            self.assertEqual("preserve", marker.read_text(encoding="ascii"))

    def test_platform_tool_output_is_bounded_and_cleanup_is_owned(self):
        stdout_read, stdout_write = os.pipe()
        stderr_read, stderr_write = os.pipe()
        os.write(stdout_write, b"12345")
        os.close(stdout_write)
        os.close(stderr_write)
        process = mock.Mock()
        process.stdout = os.fdopen(stdout_read, "rb", buffering=0)
        process.stderr = os.fdopen(stderr_read, "rb", buffering=0)
        process.poll.return_value = None
        process.returncode = None
        with (
            mock.patch.object(
                self.module.subprocess,
                "Popen",
                return_value=process,
            ),
            mock.patch.object(self.module, "MAX_TOOL_OUTPUT_BYTES", 4),
            self.assertRaisesRegex(
                self.module.WrapperError,
                "output exceeded",
            ),
        ):
            self.module.run_tool(["/usr/bin/codesign", "--fixed-test"])
        process.kill.assert_called_once_with()
        process.wait.assert_called_once_with(timeout=10)
        self.assertTrue(process.stdout.closed)
        self.assertTrue(process.stderr.closed)

    def test_composer_rejects_unsafe_output_sources_identity_and_overrides(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            executable = root / "decodexd"
            executable.write_bytes(b"\xcf\xfa\xed\xfe" + b"daemon")
            executable.chmod(0o755)
            profile = root / "profile"
            profile.write_bytes(b"profile")
            output_parent = root / "output"
            output_parent.mkdir(mode=0o700)
            output = output_parent / self.module.WRAPPER_NAME
            output.mkdir()
            with self.assertRaisesRegex(self.module.WrapperError, "already exists"):
                self.module.compose_wrapper(
                    executable,
                    profile,
                    "Apple Development: Decodex Test",
                    output,
                )
            output.rmdir()
            with self.assertRaisesRegex(self.module.WrapperError, "identity"):
                self.module.compose_wrapper(executable, profile, "-", output)
            profile_link = root / "profile-link"
            profile_link.symlink_to(profile)
            with self.assertRaisesRegex(self.module.WrapperError, "profile input"):
                self.module.compose_wrapper(
                    executable,
                    profile_link,
                    "Apple Development: Decodex Test",
                    output,
                )
        with self.assertRaisesRegex(self.module.WrapperError, "arguments"):
            self.module.parse_args(
                [
                    "compose",
                    "--decodexd",
                    "/tmp/decodexd",
                    "--profile",
                    "/tmp/profile",
                    "--signing-identity",
                    "identity",
                    "--output",
                    "/tmp/decodexd.app",
                    "--team",
                    "OTHER",
                ]
            )

    def test_duplicate_entitlement_key_is_rejected(self):
        duplicate = b"""<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>application-identifier</key><string>one</string>
<key>application-identifier</key><string>two</string>
</dict></plist>"""
        with self.assertRaisesRegex(self.module.WrapperError, "duplicate"):
            self.module.load_plist(duplicate)


if __name__ == "__main__":
    unittest.main()
