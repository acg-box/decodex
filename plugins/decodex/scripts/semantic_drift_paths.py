from __future__ import annotations

from pathlib import PurePosixPath


DOC_SUFFIXES = {".md", ".mdx", ".rst", ".txt"}
EXECUTABLE_SUFFIXES = {
    ".bash",
    ".c",
    ".cc",
    ".cpp",
    ".go",
    ".js",
    ".json",
    ".jsx",
    ".py",
    ".rs",
    ".sh",
    ".toml",
    ".ts",
    ".tsx",
    ".yaml",
    ".yml",
}
DOC_DIR_PARTS = {"docs", "doc", "runbook", "reference", "spec", "decisions", "evidence"}


def is_docs_path(path: str) -> bool:
    posix = PurePosixPath(path)
    parts = set(posix.parts)
    return posix.suffix.lower() in DOC_SUFFIXES or bool(parts & DOC_DIR_PARTS)


def is_executable_path(path: str) -> bool:
    posix = PurePosixPath(path)
    if posix.name in {"Makefile", "Makefile.toml", "Cargo.toml", "package.json"}:
        return True
    if posix.suffix.lower() in EXECUTABLE_SUFFIXES:
        return True
    return any(part in {"src", "scripts", "bin", ".github", "tests", "fixtures"} for part in posix.parts)
