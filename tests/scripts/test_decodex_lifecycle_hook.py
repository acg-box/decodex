from __future__ import annotations

import json
import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
PLUGIN_ROOT = REPO_ROOT / "plugins" / "decodex"
HOOK_SCRIPT = PLUGIN_ROOT / "scripts" / "decodex_lifecycle_hook"


class DecodexLifecycleHookTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tempdir = tempfile.TemporaryDirectory()
        self.addCleanup(self.tempdir.cleanup)
        self.root = Path(self.tempdir.name)
        self.home = self.root / "home"
        self.home.mkdir()

    def run_hook(self, cwd: Path, command: str) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        env["CODEX_HOME"] = str(self.home / ".codex")
        env["PYTHONDONTWRITEBYTECODE"] = "1"
        return subprocess.run(
            [str(HOOK_SCRIPT), "--event", "PreToolUse"],
            cwd=cwd,
            input=json.dumps({"tool_name": "exec_command", "tool_input": {"cmd": command}}),
            text=True,
            capture_output=True,
            check=True,
            env=env,
        )

    def make_git_repo(self, name: str) -> Path:
        repo = self.root / name
        repo.mkdir()
        subprocess.run(["git", "init", "-q", "-b", "main"], cwd=repo, check=True)
        return repo

    def make_decodex_repo(self) -> Path:
        repo = self.make_git_repo("decodex")
        (repo / "plugins" / "decodex" / ".codex-plugin").mkdir(parents=True)
        (repo / "plugins" / "decodex" / ".codex-plugin" / "plugin.json").write_text(
            "{}\n",
            encoding="utf-8",
        )
        (repo / "apps" / "decodex").mkdir(parents=True)
        return repo

    def write_project_config(
        self,
        repo: Path | str,
        worktree_root: Path | str | None = None,
        project_name: str = "demo",
    ) -> Path:
        project = self.home / ".codex" / "decodex" / "projects" / project_name
        project.mkdir(parents=True)
        lines = ["[paths]", f'repo_root = "{repo}"']
        if worktree_root:
            lines.append(f'worktree_root = "{worktree_root}"')
        (project / "project.toml").write_text("\n".join(lines) + "\n", encoding="utf-8")
        return project

    def assert_blocked(self, result: subprocess.CompletedProcess[str], expected: str) -> None:
        output = json.loads(result.stdout)
        self.assertEqual(output["decision"], "block")
        self.assertIn(expected, output["reason"])

    def test_blocks_git_commit_inside_decodex_repo(self) -> None:
        repo = self.make_decodex_repo()

        result = self.run_hook(repo, "git commit -m test")

        self.assert_blocked(result, "decodex commit")

    def test_blocks_git_commit_with_git_c_inside_decodex_repo(self) -> None:
        repo = self.make_decodex_repo()
        outside = self.make_git_repo("outside")

        result = self.run_hook(outside, f"git -C {repo} commit --amend --no-edit")

        self.assert_blocked(result, "decodex commit")

    def test_blocks_git_commit_with_git_config_option_inside_decodex_repo(self) -> None:
        repo = self.make_decodex_repo()

        result = self.run_hook(repo, "git -c user.name=test commit -m test")

        self.assert_blocked(result, "decodex commit")

    def test_blocks_git_commit_with_work_tree_option_inside_decodex_repo(self) -> None:
        repo = self.make_decodex_repo()
        outside = self.make_git_repo("outside")

        result = self.run_hook(outside, f"git --work-tree {repo} commit -m test")

        self.assert_blocked(result, "decodex commit")

    def test_blocks_gh_pr_merge_inside_decodex_repo(self) -> None:
        repo = self.make_decodex_repo()

        result = self.run_hook(repo, "gh pr merge 123 --merge")

        self.assert_blocked(result, "decodex land")

    def test_blocks_gh_pr_merge_with_long_repo_option_inside_decodex_repo(self) -> None:
        repo = self.make_decodex_repo()

        result = self.run_hook(repo, "gh --repo acg-box/decodex pr merge 123 --merge")

        self.assert_blocked(result, "decodex land")

    def test_blocks_gh_pr_merge_with_repo_option_outside_decodex_repo(self) -> None:
        repo = self.make_git_repo("ordinary")

        result = self.run_hook(repo, "gh --repo acg-box/decodex pr merge 123 --merge")

        self.assert_blocked(result, "decodex land")

    def test_blocks_gh_pr_merge_with_transferred_repo_alias_outside_decodex_repo(self) -> None:
        repo = self.make_git_repo("ordinary")

        result = self.run_hook(repo, "gh --repo hack-ink/decodex pr merge 123 --merge")

        self.assert_blocked(result, "decodex land")

    def test_blocks_gh_pr_merge_with_host_qualified_repo_option_outside_decodex_repo(self) -> None:
        repo = self.make_git_repo("ordinary")

        result = self.run_hook(repo, "gh --repo github.com/acg-box/decodex pr merge 123 --merge")

        self.assert_blocked(result, "decodex land")

    def test_blocks_gh_pr_merge_with_joined_short_repo_option_outside_decodex_repo(self) -> None:
        repo = self.make_git_repo("ordinary")

        result = self.run_hook(repo, "gh -Rgithub.com/acg-box/decodex pr merge 123 --merge")

        self.assert_blocked(result, "decodex land")

    def test_blocks_gh_pr_merge_with_pr_url_outside_decodex_repo(self) -> None:
        repo = self.make_git_repo("ordinary")

        result = self.run_hook(
            repo,
            "gh pr merge https://github.com/acg-box/decodex/pull/123 --merge",
        )

        self.assert_blocked(result, "decodex land")

    def test_blocks_git_commit_inside_registered_project_repo(self) -> None:
        repo = self.make_git_repo("project-repo")
        self.write_project_config(repo)

        result = self.run_hook(repo, "git commit -m test")

        self.assert_blocked(result, "decodex commit")

    def test_blocks_git_commit_inside_registered_worktree_root(self) -> None:
        repo = self.make_git_repo("project-repo")
        worktree_root = self.root / "worktrees"
        worktree = worktree_root / "ISSUE-1"
        worktree.mkdir(parents=True)
        self.write_project_config(repo, worktree_root)

        result = self.run_hook(worktree, "git commit -m test")

        self.assert_blocked(result, "decodex commit")

    def test_blocks_git_commit_inside_registered_relative_project_repo(self) -> None:
        project = self.write_project_config("relative-repo", project_name="relative-repo")
        repo = project / "relative-repo"
        repo.mkdir()
        subprocess.run(["git", "init", "-q", "-b", "main"], cwd=repo, check=True)

        result = self.run_hook(repo, "git commit -m test")

        self.assert_blocked(result, "decodex commit")

    def test_blocks_git_commit_inside_registered_relative_worktree_root(self) -> None:
        repo = self.make_git_repo("relative-worktree-repo")
        worktree = repo / "lanes" / "ISSUE-1"
        worktree.mkdir(parents=True)
        self.write_project_config(repo, "lanes", project_name="relative-worktree")

        result = self.run_hook(worktree, "git commit -m test")

        self.assert_blocked(result, "decodex commit")

    def test_allows_git_commit_outside_decodex_scope(self) -> None:
        repo = self.make_git_repo("ordinary")

        result = self.run_hook(repo, "git commit -m test")

        self.assertEqual("", result.stdout)

    def test_allows_git_c_commit_outside_decodex_scope_from_decodex_repo(self) -> None:
        repo = self.make_decodex_repo()
        outside = self.make_git_repo("outside")

        result = self.run_hook(repo, f"git -C {outside} commit -m test")

        self.assertEqual("", result.stdout)

    def test_allows_gh_pr_merge_outside_decodex_scope(self) -> None:
        repo = self.make_git_repo("ordinary")

        result = self.run_hook(repo, "gh pr merge 123 --merge")

        self.assertEqual("", result.stdout)

    def test_allows_read_only_git_commands_inside_decodex_repo(self) -> None:
        repo = self.make_decodex_repo()
        for command in ("git status --short", "git diff", "git log --oneline -1"):
            with self.subTest(command=command):
                result = self.run_hook(repo, command)
                self.assertEqual("", result.stdout)

    def test_allows_decodex_commit_and_land(self) -> None:
        repo = self.make_decodex_repo()
        for command in (
            'decodex commit "summary"',
            'decodex commit --manual-authority "summary"',
            'decodex land --manual-authority --pr https://github.com/x/y/pull/1 "summary"',
        ):
            with self.subTest(command=command):
                result = self.run_hook(repo, command)
                self.assertEqual("", result.stdout)

    def test_allows_read_only_mentions_of_git_commit(self) -> None:
        repo = self.make_decodex_repo()

        result = self.run_hook(repo, "rg 'git commit' plugins/decodex/skills")

        self.assertEqual("", result.stdout)

    def test_blocks_wrapped_git_commit(self) -> None:
        repo = self.make_decodex_repo()

        result = self.run_hook(repo, "bash -lc 'git add . && git commit -m test'")

        self.assert_blocked(result, "decodex commit")

    def test_blocks_adjacent_shell_operator_git_commit(self) -> None:
        repo = self.make_decodex_repo()

        result = self.run_hook(repo, "git add .&&git commit -m test")

        self.assert_blocked(result, "decodex commit")

    def test_blocks_multiline_git_commit(self) -> None:
        repo = self.make_decodex_repo()

        result = self.run_hook(repo, "git add .\ngit commit -m test")

        self.assert_blocked(result, "decodex commit")

    def test_blocks_wrapped_git_config_commit(self) -> None:
        repo = self.make_decodex_repo()

        result = self.run_hook(repo, "bash -lc 'git -c user.name=test commit -m test'")

        self.assert_blocked(result, "decodex commit")

    def test_blocks_wrapped_multiline_git_commit(self) -> None:
        repo = self.make_decodex_repo()

        result = self.run_hook(repo, "bash -lc 'git add .\ngit commit -m test'")

        self.assert_blocked(result, "decodex commit")

    def test_blocks_env_wrapped_bash_git_commit(self) -> None:
        repo = self.make_decodex_repo()

        result = self.run_hook(repo, "env FOO=bar bash -lc 'git commit -m test'")

        self.assert_blocked(result, "decodex commit")

    def test_hook_config_uses_workspace_versioned_cache_path(self) -> None:
        hooks = json.loads((PLUGIN_ROOT / "hooks" / "hooks.json").read_text(encoding="utf-8"))["hooks"]
        if (REPO_ROOT / "Cargo.toml").is_file():
            cargo_toml = (REPO_ROOT / "Cargo.toml").read_text(encoding="utf-8")
            version = re.search(r'(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"', cargo_toml)
            self.assertIsNotNone(version)
            version_text = version.group(1)
        else:
            version_text = PLUGIN_ROOT.name
        expected_path = f"plugins/cache/acg-box/decodex/{version_text}/scripts/decodex_lifecycle_hook"

        self.assertEqual({"PreToolUse"}, set(hooks))
        for entries in hooks.values():
            for entry in entries:
                for hook in entry["hooks"]:
                    self.assertIn(expected_path, hook["command"])

    def test_packaged_text_routes_only_decodex_skills(self) -> None:
        surfaces = []
        for root in (PLUGIN_ROOT / "references", PLUGIN_ROOT / "skills"):
            for path in root.rglob("*"):
                if path.is_file() and path.suffix in {".md", ".yaml", ".yml"}:
                    surfaces.append(path.read_text(encoding="utf-8"))
        manifest = json.loads(
            (PLUGIN_ROOT / ".codex-plugin" / "plugin.json").read_text(encoding="utf-8")
        )
        combined = "\n".join(surfaces)
        skill_owners = set(re.findall(r"\$([a-z][a-z0-9-]*):", combined))
        plugin_owners = set(re.findall(r"plugin://([a-z][a-z0-9-]*)@", combined))

        self.assertNotIn("dependencies", manifest)
        self.assertNotRegex(combined.lower(), r"\bplugins?\b")
        self.assertEqual(set(), skill_owners)
        self.assertEqual(set(), plugin_owners)


if __name__ == "__main__":
    unittest.main()
