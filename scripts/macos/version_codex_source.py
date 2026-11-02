#!/usr/bin/env python3
"""Set the reviewed source build version without changing external dependencies."""

import argparse
import json
from pathlib import Path
import re
import subprocess
import tomllib


def set_version(source: Path, version: str) -> None:
    workspace = source / "codex-rs"
    manifest = workspace / "Cargo.toml"
    lock = workspace / "Cargo.lock"
    contents = manifest.read_text()
    previous = tomllib.loads(contents)["workspace"]["package"]["version"]
    before = tomllib.loads(lock.read_text())
    section = re.search(r"(?ms)^\[workspace\.package\]\n.*?(?=^\[|\Z)", contents)
    if section is None:
        raise ValueError("Upstream workspace package metadata is missing")
    updated, count = re.subn(
        r'(?m)^version[ \t]*=[ \t]*"[^"\n]+"[ \t]*$',
        f"version = {json.dumps(version)}",
        section.group(),
        count=1,
    )
    if count != 1:
        raise ValueError("Upstream workspace version is missing")
    manifest.write_text(contents[:section.start()] + updated + contents[section.end():])
    subprocess.run(
        ["cargo", "+stable", "update", "--workspace"],
        cwd=workspace,
        check=True,
    )
    after = tomllib.loads(lock.read_text())
    for package in after["package"]:
        if "source" not in package and package["version"] == version:
            package["version"] = previous
    if before != after:
        raise ValueError("Version preparation changed the upstream dependency graph")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("version")
    args = parser.parse_args()
    set_version(args.source, args.version)
