"""Shell classification constants for lifecycle hook safeguards."""

from __future__ import annotations

ALWAYS_MUTATING_TOOL_NAMES = {
    "apply_patch",
    "edit",
    "write",
}
MUTATING_COMMAND_TERMS = (
    "add",
    "am",
    "apply",
    "cherry-pick",
    "clean",
    "commit",
    "merge",
    "mv",
    "pull",
    "push",
    "rebase",
    "reset",
    "restore",
    "rm",
    "stash",
    "switch",
    "checkout",
)
GIT_GLOBAL_OPTIONS_WITH_ARG = {
    "-C",
    "-c",
    "--exec-path",
    "--git-dir",
    "--namespace",
    "--super-prefix",
    "--work-tree",
}
GIT_GLOBAL_OPTIONS_WITH_VALUE_PREFIX = (
    "--exec-path=",
    "--git-dir=",
    "--namespace=",
    "--super-prefix=",
    "--work-tree=",
)
GIT_BRANCH_CREATE_OPTIONS = {
    "-b",
    "-B",
    "-c",
    "-C",
    "--create",
    "--force-create",
    "--orphan",
}
GIT_OPTIONS_WITH_ARG = {
    "--conflict",
    "--pathspec-from-file",
}
SHELL_MUTATING_COMMANDS = {
    "cp",
    "install",
    "mkdir",
    "mv",
    "rm",
    "tee",
    "touch",
}
REDIRECTION_OPERATORS = {
    ">",
    ">>",
    "1>",
    "1>>",
    "2>",
    "2>>",
    "&>",
    "&>>",
}
