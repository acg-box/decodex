#!/usr/bin/env python3
"""Sync installable Decodex plugins without installing repo-local skills globally."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path
from pathlib import PurePosixPath


NAMESPACE = "acg-box"


@dataclass(frozen=True)
class RepoLocalSkill:
    name: str
    source: Path


def find_repo_root(start: Path) -> Path:
    for candidate in [start, *start.parents]:
        if (candidate / "plugins").is_dir() and (candidate / "Cargo.toml").is_file():
            return candidate
    raise RuntimeError(f"could not find repo root from {start}")


def workspace_version(repo_root: Path) -> str:
    manifest = repo_root / "Cargo.toml"
    in_workspace_package = False
    version_pattern = re.compile(r'^version\s*=\s*"([^"]+)"')

    for line in manifest.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped == "[workspace.package]":
            in_workspace_package = True
            continue
        if in_workspace_package and stripped.startswith("["):
            break
        if in_workspace_package:
            match = version_pattern.match(stripped)
            if match:
                return match.group(1)

    raise RuntimeError("workspace package version not found in Cargo.toml")


def plugin_sources(repo_root: Path) -> list[Path]:
    plugin_root = repo_root / "plugins"
    return sorted(
        candidate
        for candidate in plugin_root.iterdir()
        if (candidate / ".codex-plugin" / "plugin.json").is_file()
    )


def package_contract(plugin_root: Path) -> tuple[list[str], list[str]]:
    manifest_path = plugin_root / ".codex-plugin" / "plugin.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    package = manifest.get("package")
    if not isinstance(package, dict):
        raise RuntimeError(f"{manifest_path} is missing package contract")

    include = package.get("include", [])
    exclude = package.get("exclude", [])
    if not isinstance(include, list) or not include:
        raise RuntimeError(f"{manifest_path} package.include must be a non-empty list")
    if not isinstance(exclude, list):
        raise RuntimeError(f"{manifest_path} package.exclude must be a list")

    for pattern in include + exclude:
        if not isinstance(pattern, str):
            raise RuntimeError(f"{manifest_path} package patterns must be strings")
        path = PurePosixPath(pattern)
        if path.is_absolute() or ".." in path.parts:
            raise RuntimeError(f"{manifest_path} has unsafe package pattern: {pattern}")

    return include, exclude


def package_files(plugin_root: Path) -> list[Path]:
    include, exclude = package_contract(plugin_root)
    repo_root = find_repo_root(plugin_root)
    plugin_relative = plugin_root.resolve().relative_to(repo_root).as_posix()
    output = subprocess.check_output(
        [
            "git",
            "-C",
            str(repo_root),
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            plugin_relative,
        ],
        text=True,
    )
    files = []
    for line in output.splitlines():
        path = repo_root / line
        relative = path.relative_to(plugin_root).as_posix()
        if any(fnmatch(relative, pattern) for pattern in include) and not any(
            fnmatch(relative, pattern) for pattern in exclude
        ):
            files.append(path)
    if not files:
        raise RuntimeError(f"{plugin_root} package contract matched no files")
    return sorted(files)


def copy_plugin_package(source: Path, destination: Path) -> None:
    if destination.exists():
        shutil.rmtree(destination)
    destination.mkdir(parents=True, exist_ok=True)

    for file_path in package_files(source):
        relative = file_path.relative_to(source)
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(file_path, target)


def repo_local_skills(repo_root: Path) -> list[RepoLocalSkill]:
    skills: list[RepoLocalSkill] = []
    for skill_file in sorted((repo_root / "automations").glob("*/skills/*/SKILL.md")):
        skills.append(RepoLocalSkill(name=skill_file.parent.name, source=skill_file.parent))
    return skills


def sync_plugins(repo_root: Path, codex_home: Path, version: str, apply: bool) -> list[str]:
    actions: list[str] = []
    cache_root = codex_home / "plugins" / "cache" / NAMESPACE

    for source in plugin_sources(repo_root):
        destination = cache_root / source.name / version
        package_files(source)
        actions.append(f"sync plugin {source.name} -> {destination}")
        if apply:
            copy_plugin_package(source, destination)

    return actions


def clean_repo_local_global_skills(repo_root: Path, codex_home: Path, apply: bool) -> list[str]:
    actions: list[str] = []
    global_skill_root = codex_home / "skills"

    for skill in repo_local_skills(repo_root):
        destination = global_skill_root / skill.name
        destination_skill = destination / "SKILL.md"
        if not destination.exists():
            continue
        if not destination_skill.is_file():
            raise RuntimeError(f"refusing to remove non-skill global path: {destination}")

        source_text = (skill.source / "SKILL.md").read_text(encoding="utf-8")
        destination_text = destination_skill.read_text(encoding="utf-8")
        if source_text != destination_text:
            raise RuntimeError(
                f"refusing to remove modified global repo-local skill: {destination}"
            )

        actions.append(f"remove repo-local global skill {skill.name} -> {destination}")
        if apply:
            shutil.rmtree(destination)

    return actions


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Sync installable Decodex plugins to CODEX_HOME without copying "
            "automations/*/skills into global skills."
        )
    )
    parser.add_argument("--apply", action="store_true", help="write changes")
    parser.add_argument(
        "--clean-repo-local-skills",
        action="store_true",
        help=(
            "remove global ~/.codex/skills entries that are exact copies of "
            "repo-local automations/*/skills"
        ),
    )
    parser.add_argument("--codex-home", type=Path, default=None, help="override CODEX_HOME")
    parser.add_argument("--repo-root", type=Path, default=None, help="override repo root")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    script_path = Path(__file__).resolve()
    repo_root = args.repo_root.resolve() if args.repo_root else find_repo_root(script_path)
    codex_home = (
        args.codex_home
        or Path(os.environ.get("CODEX_HOME") or Path.home() / ".codex")
    ).expanduser().resolve()
    version = workspace_version(repo_root)

    actions = sync_plugins(repo_root, codex_home, version, args.apply)
    if args.clean_repo_local_skills:
        actions.extend(clean_repo_local_global_skills(repo_root, codex_home, args.apply))

    mode = "apply" if args.apply else "dry-run"
    for action in actions:
        print(f"{mode}: {action}")

    if not args.apply:
        print("dry-run only; pass --apply to write changes")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
