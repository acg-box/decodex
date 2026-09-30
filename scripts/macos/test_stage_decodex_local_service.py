"""Exercise local-service staging without compiling or signing user binaries."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("stage_decodex_local_service.sh")


def executable(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    path.chmod(0o755)


class LocalServiceStageTests(unittest.TestCase):
    def test_stages_the_current_cargo_output_instead_of_stale_default_bytes(self) -> None:
        for custom in (False, True):
            with self.subTest(custom_target=custom), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                script = root / "scripts/macos/stage_decodex_local_service.sh"
                script.parent.mkdir(parents=True)
                shutil.copyfile(SCRIPT, script)
                target = root / ("custom build output" if custom else "target")
                tools = root / "tools"
                metadata = shlex.quote(json.dumps({"target_directory": str(target)}))
                executable(
                    tools / "cargo",
                    "#!/bin/sh\nif [ \"$2\" = metadata ]; then\n"
                    "    printf '%s\\n' " + metadata + "\nfi\n",
                )
                executable(tools / "codesign", "#!/bin/sh\nexit 0\n")
                identity = shlex.quote(json.dumps({
                    "schema": "decodex/build-info/1", "version": "0.2.0",
                    "commit": "0123456789012345678901234567890123456789", "dirty": False,
                }))
                fresh = "#!/bin/sh\n# fresh build\nprintf '%s\\n' " + identity + "\n"
                for name in ("decodex", "decodex-database-transfer"):
                    executable(
                        root / "target/release" / name,
                        fresh.replace("fresh build", "stale build"),
                    )
                    executable(target / "release" / name, fresh)
                stage = root / "stage"
                environment = dict(
                    os.environ,
                    PATH=str(tools) + os.pathsep + os.environ["PATH"],
                    DECODEX_LOCAL_SERVICE_STAGE_DIR=str(stage),
                    DECODEX_LOCAL_SERVICE_SIGN_IDENTITY="fixture-identity",
                )
                if custom:
                    environment["CARGO_TARGET_DIR"] = str(target)
                else:
                    environment.pop("CARGO_TARGET_DIR", None)
                result = subprocess.run(
                    ["bash", str(script)], cwd=root, env=environment,
                    capture_output=True, text=True, check=False,
                )
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                for name in ("decodex", "decodex-database-transfer"):
                    self.assertEqual((stage / name).read_text(), fresh)


if __name__ == "__main__":
    unittest.main()
