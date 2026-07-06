"""Module-boundary lifecycle hook hint text."""

from __future__ import annotations

ANTI_MONOLITH_HINT = (
    "Large, generated, or growing implementation files are present. Treat size as "
    "a module-boundary review trigger, not a split rule. Before commit, push, or "
    "ready/done claims, load $codebase:work and check ownership: split files that "
    "mix unrelated concerns, or state why the current owner boundary is deliberate. "
    "Use $deliberation:skeptic when the structure is material."
)
MODULE_BOUNDARY_HINT = (
    "Module-boundary work is in scope. Load $codebase:work and use ownership rules "
    "before judging or editing: split or merge by responsibility, public contract, "
    "state ownership, change cadence, validation surface, and reader navigation. "
    "Do not use fixed line counts as the decision rule."
)
FAKE_MODULARIZATION_HINT = (
    "The current diff has signs of pseudo-modularization such as textual includes, "
    "original-scope fragment wiring, compatibility shims, or files that only move "
    "code without creating an owner boundary. Do not claim that as modularization. "
    "Replace it with explicit owner APIs and visibility boundaries, or document it "
    "as generated/FFI adapter plumbing that does not count toward the refactor."
)
