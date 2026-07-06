"""Git commit-subject helpers for lifecycle hook checks."""

from __future__ import annotations

import json

from .constants import COMMIT_SCHEMA
from .git_core import git_output


def commit_subject_is_valid(subject: str) -> bool:
    if "\n" in subject:
        return False
    try:
        value = json.loads(subject)
    except json.JSONDecodeError:
        return False
    if not isinstance(value, dict):
        return False
    return (
        value.get("schema") == COMMIT_SCHEMA
        and isinstance(value.get("summary"), str)
        and bool(value["summary"].strip())
        and isinstance(value.get("authority"), str)
        and bool(value["authority"].strip())
    )


def ahead_commit_subjects(limit: int = 20) -> list[str]:
    output = git_output(["git", "rev-list", "--format=%s", "@{u}..HEAD"], timeout=4)
    if output is None:
        return []
    subjects = [
        line.strip()
        for line in output.splitlines()
        if line.strip() and not line.startswith("commit ")
    ]
    return subjects[:limit]


def invalid_ahead_commit_subjects() -> list[str]:
    return [subject for subject in ahead_commit_subjects() if not commit_subject_is_valid(subject)]
