"""Low-level Git command helpers."""

from __future__ import annotations

import subprocess
from pathlib import Path


def git_output(args: list[str], timeout: int = 3, cwd: Path | None = None) -> str | None:
    try:
        result = subprocess.run(
            args,
            check=True,
            cwd=str(cwd) if cwd is not None else None,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            timeout=timeout,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    return result.stdout


def git_root() -> str | None:
    output = git_output(["git", "rev-parse", "--show-toplevel"], timeout=2)
    return output.strip() if output else None
