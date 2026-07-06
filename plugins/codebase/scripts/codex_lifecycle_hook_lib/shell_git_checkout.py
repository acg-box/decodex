"""Git checkout/switch target parsing."""

from __future__ import annotations

from .constants import GIT_BRANCH_CREATE_OPTIONS, GIT_OPTIONS_WITH_ARG
from .git_state import git_branch_ref_exists


def checkout_branch_target(args: list[str], command: str) -> tuple[str, bool] | None:
    args_before_pathspec = args[: args.index("--")] if "--" in args else args
    index = 0
    operands: list[str] = []
    while index < len(args_before_pathspec):
        token = args_before_pathspec[index]
        if token in GIT_BRANCH_CREATE_OPTIONS:
            target = args_before_pathspec[index + 1] if index + 1 < len(args_before_pathspec) else ""
            return target, True
        if token in GIT_OPTIONS_WITH_ARG:
            index += 2
            continue
        if token.startswith("--"):
            index += 1
            continue
        if token.startswith("-") and token not in {"-", "@{-1}"}:
            index += 1
            continue
        operands.append(token)
        index += 1

    if "--" in args or not operands:
        return None
    target = operands[0]
    if command == "switch" or target in {"-", "@{-1}"} or git_branch_ref_exists(target):
        return target, False
    return None


def target_is_default_branch(target: str, default_branch: str) -> bool:
    return target in {default_branch, f"origin/{default_branch}", f"refs/heads/{default_branch}"}
