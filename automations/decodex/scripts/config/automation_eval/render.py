"""Render automation evaluation results."""

from __future__ import annotations

from automation_eval.model import AutomationResult


def render_text(results: list[AutomationResult]) -> str:
    lines = []
    for result in results:
        lines.append(f"{result.automation_id}: {result.status}")
        for error in result.errors:
            lines.append(f"  error: {error}")
        for warning in result.warnings:
            lines.append(f"  warning: {warning}")
    return "\n".join(lines) + "\n"
