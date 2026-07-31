"""Exact model assignment for the managed Codex automations."""

from __future__ import annotations


MODEL_BY_AUTOMATION_ID = {
    "codex-upstream-maintainer": "gpt-5.6-sol",
    "codex-upstream-reviewer": "gpt-5.6-sol",
    "codex-upstream-health": "gpt-5.6-terra",
    "decodex-content-manager": "gpt-5.6-terra",
    "decodex-xurl-publisher": "gpt-5.6-luna",
}

DEFAULT_REASONING_EFFORT = "high"
REASONING_EFFORT_BY_AUTOMATION_ID = {
    "codex-upstream-maintainer": "max",
    "codex-upstream-reviewer": "max",
    "codex-upstream-health": "high",
    "decodex-content-manager": "high",
    "decodex-xurl-publisher": "high",
}


def expected_model(automation_id: str) -> str:
    try:
        return MODEL_BY_AUTOMATION_ID[automation_id]
    except KeyError as error:
        raise ValueError(
            f"no model policy exists for automation {automation_id!r}"
        ) from error


def expected_reasoning_effort(automation_id: str) -> str:
    try:
        return REASONING_EFFORT_BY_AUTOMATION_ID[automation_id]
    except KeyError as error:
        raise ValueError(
            f"no reasoning policy exists for automation {automation_id!r}"
        ) from error
