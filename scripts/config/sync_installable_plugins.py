#!/usr/bin/env python3
"""Sync installable Decodex plugins without installing repo-local skills globally."""

from __future__ import annotations

import argparse
import os
import re
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path


NAMESPACE = "hack-ink"


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
        actions.append(f"sync plugin {source.name} -> {destination}")
        if apply:
            if destination.exists():
                shutil.rmtree(destination)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copytree(source, destination)

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
