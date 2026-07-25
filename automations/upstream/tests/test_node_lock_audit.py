import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]
SCRIPT = ROOT / "scripts/audit_node_lock.py"


def load_module():
    spec = importlib.util.spec_from_file_location("audit_node_lock", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class NodeLockAuditTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.audit = load_module()

    def lock_value(self, **values):
        return {
            "version": "1.2.3",
            "resolved": "https://registry.npmjs.org/example/-/example-1.2.3.tgz",
            "integrity": "sha512-QUJDRA==",
            **values,
        }

    def write_package(self, site, lock_path, **values):
        package_root = site / lock_path
        package_root.mkdir(parents=True)
        manifest = {
            "name": self.audit.package_name_from_lock_path(lock_path),
            "version": "1.2.3",
            **values,
        }
        (package_root / "package.json").write_text(
            json.dumps(manifest),
            encoding="utf-8",
        )
        return package_root

    def test_installed_manifest_identity_matches_lock_path(self):
        with tempfile.TemporaryDirectory() as directory:
            site = Path(directory)
            self.write_package(site, "node_modules/@scope/example")
            report = self.audit.audit_package_graph(
                site,
                {
                    "node_modules/@scope/example": self.lock_value(
                        resolved=(
                            "https://registry.npmjs.org/@scope/example/"
                            "-/example-1.2.3.tgz"
                        )
                    )
                },
            )
        self.assertEqual(report["audited_packages"], 1)
        self.assertEqual(report["installed_packages"], 1)

    def test_substituted_installed_package_name_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            site = Path(directory)
            self.write_package(
                site,
                "node_modules/example",
                name="substituted",
            )
            with self.assertRaisesRegex(
                self.audit.AuditError,
                "node_installed_package_identity_invalid",
            ):
                self.audit.audit_package_graph(
                    site,
                    {"node_modules/example": self.lock_value()},
                )

    def test_unrecorded_install_lifecycle_script_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            site = Path(directory)
            self.write_package(
                site,
                "node_modules/example",
                scripts={"postinstall": "node install.js"},
            )
            with self.assertRaisesRegex(
                self.audit.AuditError,
                "node_install_script_metadata_mismatch",
            ):
                self.audit.audit_package_graph(
                    site,
                    {"node_modules/example": self.lock_value()},
                )

    def test_optional_absence_is_allowed_but_required_absence_is_not(self):
        with tempfile.TemporaryDirectory() as directory:
            site = Path(directory)
            optional = self.audit.audit_package_graph(
                site,
                {
                    "node_modules/example": self.lock_value(
                        optional=True,
                        os=["other"],
                    )
                },
            )
            self.assertEqual(optional["optional_packages_absent"], 1)
            with self.assertRaisesRegex(
                self.audit.AuditError,
                "node_installed_package_missing",
            ):
                self.audit.audit_package_graph(
                    site,
                    {"node_modules/example": self.lock_value()},
                )

    def test_installed_package_symlink_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            site = Path(directory)
            target = site / "target"
            target.mkdir()
            (target / "package.json").write_text(
                json.dumps({"name": "example", "version": "1.2.3"}),
                encoding="utf-8",
            )
            (site / "node_modules").mkdir()
            os.symlink(target, site / "node_modules/example")
            with self.assertRaisesRegex(
                self.audit.AuditError,
                "node_installed_package_invalid",
            ):
                self.audit.audit_package_graph(
                    site,
                    {"node_modules/example": self.lock_value()},
                )

    def test_native_fingerprint_binds_name_and_integrity(self):
        with tempfile.TemporaryDirectory() as directory:
            site = Path(directory)
            self.write_package(
                site,
                "node_modules/example",
                os=["darwin"],
                cpu=["arm64"],
            )
            first = self.audit.audit_package_graph(
                site,
                {
                    "node_modules/example": self.lock_value(
                        optional=True,
                        os=["darwin"],
                        cpu=["arm64"],
                    )
                },
            )
            second = self.audit.audit_package_graph(
                site,
                {
                    "node_modules/example": self.lock_value(
                        optional=True,
                        os=["darwin"],
                        cpu=["arm64"],
                        integrity="sha512-RUZHRw==",
                    )
                },
            )
        self.assertNotEqual(
            first["native_package_set_sha256"],
            second["native_package_set_sha256"],
        )

    def test_lock_only_rejects_a_changed_root_build_script(self):
        with tempfile.TemporaryDirectory() as directory:
            site = Path(directory)
            for name in ("package.json", "package-lock.json", ".nvmrc"):
                shutil.copy2(ROOT / "site" / name, site / name)
            package_path = site / "package.json"
            package = json.loads(package_path.read_text(encoding="utf-8"))
            package["scripts"]["build"] = "node steal-environment.js"
            package_path.write_text(
                json.dumps(package),
                encoding="utf-8",
            )

            with self.assertRaisesRegex(
                self.audit.AuditError,
                "node_root_scripts_changed",
            ):
                self.audit.audit_site(site, inspect_installed=False)

    def test_rejected_lock_never_invokes_npm_ci(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            site = root / "site"
            scripts = root / "scripts"
            fake_bin = root / "bin"
            site.mkdir()
            scripts.mkdir()
            fake_bin.mkdir()
            for name in ("package.json", "package-lock.json", ".nvmrc"):
                shutil.copy2(ROOT / "site" / name, site / name)
            shutil.copy2(SCRIPT, scripts / SCRIPT.name)

            package_path = site / "package.json"
            package = json.loads(package_path.read_text(encoding="utf-8"))
            package["scripts"]["check"] = "node untrusted.js"
            package_path.write_text(
                json.dumps(package),
                encoding="utf-8",
            )
            fake_npm = fake_bin / "npm"
            log = root / "npm.log"
            fake_npm.write_text(
                "#!/bin/sh\n"
                "printf '%s\\n' \"$*\" >> \"$NPM_LOG\"\n"
                "if [ \"$1\" = \"--version\" ]; then\n"
                "  printf '11.17.0\\n'\n"
                "  exit 0\n"
                "fi\n"
                "exit 99\n",
                encoding="utf-8",
            )
            fake_npm.chmod(0o700)
            environment = dict(os.environ)
            environment["PATH"] = f"{fake_bin}:{environment['PATH']}"
            environment["NPM_LOG"] = str(log)

            completed = subprocess.run(
                [
                    "cargo",
                    "make",
                    "--makefile",
                    str(ROOT / "Makefile.toml"),
                    "prepare-node",
                ],
                cwd=root,
                env=environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                timeout=30,
                check=False,
            )

            self.assertNotEqual(completed.returncode, 0)
            self.assertEqual(
                log.read_text(encoding="utf-8").splitlines(),
                ["--version"],
            )

    def test_wrong_npm_version_fails_before_npm_ci(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake_npm = root / "npm"
            log = root / "npm.log"
            fake_npm.write_text(
                "#!/bin/sh\n"
                "printf '%s\\n' \"$*\" >> \"$NPM_LOG\"\n"
                "if [ \"$1\" = \"--version\" ]; then\n"
                "  printf '0.0.0\\n'\n"
                "  exit 0\n"
                "fi\n"
                "exit 99\n",
                encoding="utf-8",
            )
            fake_npm.chmod(0o700)
            environment = dict(os.environ)
            environment["PATH"] = f"{root}:{environment['PATH']}"
            environment["NPM_LOG"] = str(log)

            completed = subprocess.run(
                ["cargo", "make", "prepare-node"],
                cwd=ROOT,
                env=environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                timeout=30,
                check=False,
            )

            self.assertNotEqual(completed.returncode, 0)
            self.assertEqual(log.read_text(encoding="utf-8").splitlines(), ["--version"])
            self.assertNotIn(" ci", log.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
