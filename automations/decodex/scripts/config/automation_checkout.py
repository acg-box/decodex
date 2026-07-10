"""Resolve and validate the primary checkout used by Codex automations."""

from __future__ import annotations

import subprocess
from pathlib import Path


def _git(repo_root: Path, *args: str) -> str:
	completed = subprocess.run(
		["git", "-C", str(repo_root), *args],
		check=True,
		capture_output=True,
		text=True,
	)
	return completed.stdout.strip()


def parse_worktree_list(value: str) -> list[dict[str, str]]:
	records: list[dict[str, str]] = []
	record: dict[str, str] = {}
	for line in value.splitlines() + [""]:
		if not line:
			if record:
				records.append(record)
				record = {}
			continue
		key, _, field_value = line.partition(" ")
		record[key] = field_value
	return records


def primary_checkout_for_branch(source_root: Path, branch: str = "main") -> Path:
	ref = f"refs/heads/{branch}"
	records = parse_worktree_list(_git(source_root, "worktree", "list", "--porcelain"))
	matches = [Path(record["worktree"]).resolve() for record in records if record.get("branch") == ref]
	if len(matches) != 1:
		raise ValueError(f"expected exactly one checkout for {ref}, found {len(matches)}")
	return matches[0]


def is_linked_worktree(repo_root: Path) -> bool:
	git_dir = Path(_git(repo_root, "rev-parse", "--path-format=absolute", "--git-dir")).resolve()
	common_dir = Path(
		_git(repo_root, "rev-parse", "--path-format=absolute", "--git-common-dir")
	).resolve()
	return git_dir != common_dir


def validate_runtime_checkout(repo_root: Path, branch: str = "main") -> None:
	repo_root = repo_root.resolve()
	if ".worktrees" in repo_root.parts or is_linked_worktree(repo_root):
		raise ValueError("automation runtime cwd must not be a linked worktree")
	current_branch = _git(repo_root, "branch", "--show-current")
	if current_branch != branch:
		raise ValueError(
			f"automation runtime cwd must use branch {branch!r}, got {current_branch!r}"
		)


def resolve_runtime_checkout(
	source_root: Path,
	explicit_root: str | None,
	branch: str = "main",
) -> Path:
	repo_root = (
		Path(explicit_root).expanduser().resolve()
		if explicit_root
		else primary_checkout_for_branch(source_root, branch)
	)
	validate_runtime_checkout(repo_root, branch)
	return repo_root
