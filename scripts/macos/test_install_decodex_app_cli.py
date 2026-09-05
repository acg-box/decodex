from __future__ import annotations

import importlib.util
import os
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("install_decodex_app_cli.py")


def load_module():
    spec = importlib.util.spec_from_file_location("install_decodex_app_cli", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class AppCliInstallTests(unittest.TestCase):
    def test_default_contract_is_the_bundled_helper(self) -> None:
        module = load_module()
        self.assertEqual(
            module.APP_HELPER,
            Path("/Applications/Decodex.app/Contents/Helpers/decodex"),
        )

    def test_install_is_an_exact_symlink_and_is_idempotent(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            helper = root / "Decodex.app/Contents/Helpers/decodex"
            helper.parent.mkdir(parents=True)
            helper.write_bytes(b"fixture")
            helper.chmod(0o755)
            destination = root / "home/.local/bin/decodex"

            module.install_symlink(destination, helper)
            module.install_symlink(destination, helper)

            self.assertTrue(destination.is_symlink())
            self.assertEqual(os.readlink(destination), str(helper))
            self.assertEqual(list(destination.parent.iterdir()), [destination])

    def test_install_refuses_a_link_to_other_bytes(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            helper = root / "decodex"
            helper.write_bytes(b"fixture")
            helper.chmod(0o755)
            destination = root / "bin/decodex"
            destination.parent.mkdir()
            destination.symlink_to(root / "other")
            with self.assertRaisesRegex(module.InstallError, "points elsewhere"):
                module.install_symlink(destination, helper)

    def test_install_refuses_a_non_executable_helper(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            helper = root / "decodex"
            helper.write_bytes(b"fixture")
            helper.chmod(0o644)
            with self.assertRaisesRegex(module.InstallError, "helper is unavailable"):
                module.install_symlink(root / "bin/decodex", helper)


if __name__ == "__main__":
    unittest.main()
