from __future__ import annotations

import subprocess
from pathlib import Path

from semantic_drift_paths import is_docs_path, is_executable_path


def git_diff(repo_root: Path, rev: str | None) -> str:
    cmd = ["git", "diff", "--no-ext-diff", "--unified=0"]
    if rev:
        cmd.append(rev)
    result = subprocess.run(cmd, cwd=repo_root, text=True, capture_output=True, check=False)
    if result.returncode not in (0, 1):
        raise RuntimeError(result.stderr.strip() or "git diff failed")
    return result.stdout


def changed_candidate_paths(repo_root: Path, rev: str | None) -> list[str]:
    cmd = ["git", "diff", "--name-only"]
    if rev:
        cmd.append(rev)
    diff_text = subprocess.check_output(cmd, cwd=repo_root, text=True)
    return [
        path
        for path in diff_text.splitlines()
        if is_docs_path(path) or is_executable_path(path)
    ]


def tracked_text_paths(repo_root: Path) -> list[Path]:
    tracked = subprocess.check_output(
        ["git", "ls-files"],
        cwd=repo_root,
        text=True,
    ).splitlines()
    paths: list[Path] = []
    for relative in tracked:
        path = repo_root / relative
        if path.is_file() and is_docs_path(relative):
            paths.append(path)
    return paths
