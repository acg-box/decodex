#!/usr/bin/env python3
"""Install the app-owned CLI symlink, mutually exclusive with standalone service mode."""

from __future__ import annotations

import os
import stat
import sys
from pathlib import Path


APP_HELPER = Path("/Applications/Decodex.app/Contents/Helpers/decodex")


class InstallError(RuntimeError):
    """The app CLI link could not be installed safely."""


def install_symlink(destination: Path, helper: Path) -> None:
    try:
        metadata = helper.lstat()
    except OSError as error:
        raise InstallError("the bundled Decodex helper is unavailable") from error
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_mode & 0o111 == 0:
        raise InstallError("the bundled Decodex helper is unavailable")

    destination.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
    try:
        destination_metadata = destination.lstat()
    except FileNotFoundError:
        destination_metadata = None
    except OSError as error:
        raise InstallError("the Decodex CLI destination is unavailable") from error
    if destination_metadata is not None and not stat.S_ISLNK(destination_metadata.st_mode):
        raise InstallError("the Decodex CLI destination is not a symbolic link")
    if destination_metadata is not None and os.readlink(destination) == str(helper):
        return
    if destination_metadata is not None:
        raise InstallError("the Decodex CLI destination points elsewhere")
    try:
        destination.symlink_to(helper)
    except OSError as error:
        raise InstallError("the Decodex CLI link could not be installed") from error

    if not destination.is_symlink() or os.readlink(destination) != str(helper):
        raise InstallError("the Decodex CLI link readback differs")


def main() -> int:
    destination = Path.home() / ".local" / "bin" / "decodex"
    install_symlink(destination, APP_HELPER)
    print(destination)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except InstallError as error:
        print(f"decodex app CLI install failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
