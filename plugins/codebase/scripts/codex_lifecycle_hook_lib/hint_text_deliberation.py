"""Deliberation lifecycle hook hint text."""

from __future__ import annotations

DELIBERATION_GATE_HINT = (
    "When the task involves design, architecture, refactor, root-cause debugging, "
    "research, option comparison, or important ready/done claims, use compact "
    "first-principles framing with $deliberation:grill, source-backed evidence "
    "with $deliberation:scout when facts are not local and obvious, and skeptic "
    "review with $deliberation:skeptic before material conclusions. Inline only "
    "for one local question that fits in 1-2 files or one command and cannot affect "
    "architecture, debugging, review repair, public contracts, docs drift, "
    "commit/land, or ready/done decisions. Do not wait for the user to explicitly "
    "request subagents; when the inline exception fails and subagent tools "
    "are allowed, dispatch bounded read-only scout/skeptic subagents. If "
    "subagent tools are unavailable, name the inline fallback."
)
