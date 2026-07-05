from __future__ import annotations

import importlib.util
import shutil
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts/config/sync_installable_plugins.py"


def load_sync_module():
    spec = importlib.util.spec_from_file_location("sync_installable_plugins", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules["sync_installable_plugins"] = module
    spec.loader.exec_module(module)
    return module


class SyncInstallablePluginsTests(unittest.TestCase):
    def test_sync_installs_plugins_without_global_repo_local_skills(self):
        module = load_sync_module()
        with tempfile.TemporaryDirectory() as temp_dir:
            codex_home = Path(temp_dir) / ".codex"
            global_skill = codex_home / "skills/x-post-publisher"
            shutil.copytree(REPO_ROOT / "automations/decodex/skills/x-post-publisher", global_skill)

            result = module.main(
                [
                    "--apply",
                    "--clean-repo-local-skills",
                    "--repo-root",
                    str(REPO_ROOT),
                    "--codex-home",
                    str(codex_home),
                ]
            )

            self.assertEqual(result, 0)
            self.assertTrue(
                (
                    codex_home
                    / "plugins/cache/hack-ink/decodex/0.2.0/.codex-plugin/plugin.json"
                ).is_file()
            )
            self.assertFalse(global_skill.exists())

    def test_clean_refuses_modified_global_repo_local_skill(self):
        module = load_sync_module()
        with tempfile.TemporaryDirectory() as temp_dir:
            codex_home = Path(temp_dir) / ".codex"
            global_skill = codex_home / "skills/github-signal"
            shutil.copytree(REPO_ROOT / "automations/radar/skills/github-signal", global_skill)
            (global_skill / "SKILL.md").write_text("modified\n", encoding="utf-8")

            with self.assertRaisesRegex(RuntimeError, "modified global repo-local skill"):
                module.main(
                    [
                        "--apply",
                        "--clean-repo-local-skills",
                        "--repo-root",
                        str(REPO_ROOT),
                        "--codex-home",
                        str(codex_home),
                    ]
                )

            self.assertTrue(global_skill.exists())


if __name__ == "__main__":
    unittest.main()
