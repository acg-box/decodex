"""Prompt construction for Radar Codex analysis."""

from __future__ import annotations

from pathlib import Path


def build_prompt(bundle_path: Path, repo_root: Path) -> str:
    relative_bundle = bundle_path.resolve().relative_to(repo_root)
    return "\n".join(
        [
            "Read and follow these repo-local instructions before drafting:",
            "- automations/radar/skills/README.md",
            "- automations/radar/skills/codex-code-analysis/SKILL.md",
            "- automations/radar/skills/github-signal/SKILL.md",
            "",
            f"Analyze the bundle at `{relative_bundle}`.",
            "",
            "Return exactly one JSON object matching the provided output schema.",
            "Use the code-analysis skill as the in-session behavior-reading pass.",
            "Do not invent a separate checked-in code-analysis artifact.",
            "Treat the pull request as the main narrative container and the commits/files as evidence.",
            "Do not summarize every commit independently.",
            "Keep the output publishable for Decodex: concise, user-facing, and evidence-backed.",
            "Include every schema field. Use null when an optional string field does not apply, and use [] when no config flags apply.",
            "If `how_to_try` is not null, `expected_effect` must also be non-null.",
            "Use `impact=low` rather than overstating significance when the change is mostly incremental.",
        ]
    )
