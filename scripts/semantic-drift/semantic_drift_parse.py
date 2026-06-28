from __future__ import annotations

from typing import Any

from semantic_drift_paths import is_docs_path, is_executable_path
from semantic_drift_terms import CLAIM_PATTERN, extract_executable_terms


def parse_diff(diff_text: str) -> dict[str, Any]:
    state = new_state()
    for line in diff_text.splitlines():
        handle_diff_line(line, state)
    return {
        "changed_docs": sorted(state["changed_docs"]),
        "changed_executable": sorted(state["changed_executable"]),
        "added_claims": state["added_claims"],
        "removed_terms": sorted(state["removed_terms"] - state["added_executable_terms"]),
    }


def new_state() -> dict[str, Any]:
    return {
        "changed_docs": set(),
        "changed_executable": set(),
        "added_claims": [],
        "removed_terms": set(),
        "added_executable_terms": set(),
        "old_path": None,
        "current_path": None,
    }


def handle_diff_line(line: str, state: dict[str, Any]) -> None:
    if line.startswith("diff --git "):
        state["old_path"] = None
        state["current_path"] = None
    elif line.startswith("--- a/"):
        state["old_path"] = line.removeprefix("--- a/")
        state["current_path"] = state["old_path"]
    elif line.startswith("+++ b/"):
        state["current_path"] = line.removeprefix("+++ b/")
        mark_changed(state["current_path"], state)
    elif line.startswith("+++ /dev/null") and state["old_path"]:
        state["current_path"] = state["old_path"]
        mark_changed(state["current_path"], state)
    elif state["current_path"]:
        handle_content_line(line, state["current_path"], state)


def mark_changed(path: str, state: dict[str, Any]) -> None:
    if is_docs_path(path):
        state["changed_docs"].add(path)
    if is_executable_path(path):
        state["changed_executable"].add(path)


def handle_content_line(line: str, current_path: str, state: dict[str, Any]) -> None:
    if line.startswith("+"):
        handle_added_line(line[1:], current_path, state)
    elif line.startswith("-"):
        handle_removed_line(line[1:], current_path, state)


def handle_added_line(text: str, current_path: str, state: dict[str, Any]) -> None:
    stripped = text.strip()
    if all((stripped, is_docs_path(current_path), CLAIM_PATTERN.search(stripped))):
        state["added_claims"].append({"path": current_path, "text": stripped})
    if is_executable_path(current_path) and not is_docs_path(current_path):
        state["added_executable_terms"].update(extract_executable_terms(text))


def handle_removed_line(text: str, current_path: str, state: dict[str, Any]) -> None:
    if is_executable_path(current_path) and not is_docs_path(current_path):
        state["removed_terms"].update(extract_executable_terms(text))
