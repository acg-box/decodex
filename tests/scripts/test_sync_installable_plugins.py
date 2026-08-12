from __future__ import annotations

import importlib.util
import json
import os
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
    def test_plugin_roots_match_their_runtime_package_contract(self):
        module = load_sync_module()

        for plugin_root in module.plugin_sources(REPO_ROOT):
            physical_files = {
                path.relative_to(plugin_root)
                for path in plugin_root.rglob("*")
                if path.is_file()
            }
            packaged_files = {
                path.relative_to(plugin_root)
                for path in module.package_files(plugin_root)
            }

            self.assertEqual(physical_files, packaged_files, plugin_root)

    def test_sync_installs_plugins_without_global_repo_local_skills(self):
        module = load_sync_module()
        with tempfile.TemporaryDirectory() as temp_dir:
            codex_home = Path(temp_dir) / ".codex"
            global_skill = codex_home / "skills/codex-code-analysis"
            shutil.copytree(REPO_ROOT / "automations/radar/skills/codex-code-analysis", global_skill)

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
                    / "plugins/cache/acg-box/decodex/0.2.0/.codex-plugin/plugin.json"
                ).is_file()
            )
            hooks_path = codex_home / "plugins/cache/acg-box/decodex/0.2.0/hooks/hooks.json"
            hook_script = codex_home / "plugins/cache/acg-box/decodex/0.2.0/scripts/decodex_lifecycle_hook"
            self.assertTrue(hooks_path.is_file())
            self.assertTrue(hook_script.is_file())
            self.assertTrue(os.access(hook_script, os.X_OK))
            hooks = json.loads(hooks_path.read_text(encoding="utf-8"))["hooks"]
            self.assertEqual({"PreToolUse"}, set(hooks))
            commands = [
                hook["command"]
                for entry in hooks["PreToolUse"]
                for hook in entry["hooks"]
            ]
            self.assertEqual(1, len(commands))
            self.assertIn("scripts/decodex_lifecycle_hook", commands[0])
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
