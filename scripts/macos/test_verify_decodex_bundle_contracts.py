from __future__ import annotations

import importlib.util
import plistlib
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("verify_decodex_bundle_contracts.py")


def load_module():
    spec = importlib.util.spec_from_file_location("verify_decodex_bundle_contracts", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class BundleIdentityTests(unittest.TestCase):
    def test_build_identity_is_stamped_from_the_one_service_executable(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            service = root / "decodex"
            service.write_text(
                "#!/bin/sh\n"
                "printf '%s\\n' "
                "'{\"schema\":\"decodex/build-info/1\",\"version\":\"0.2.0\","
                "\"commit\":\"0123456789012345678901234567890123456789\",\"dirty\":false}'\n",
                encoding="utf-8",
            )
            service.chmod(0o755)
            info = root / "Info.plist"
            with info.open("wb") as info_file:
                plistlib.dump({"CFBundleShortVersionString": "old"}, info_file)

            identity = module.executable_identity(service)
            module.stamp_app_identity(info, identity)

            document = module.read_app_info(info)
            self.assertEqual(document["CFBundleShortVersionString"], "0.2.0")
            self.assertEqual(
                document["DecodexBuildCommit"],
                "0123456789012345678901234567890123456789",
            )
            self.assertFalse(document["DecodexBuildDirty"])


if __name__ == "__main__":
    unittest.main()
