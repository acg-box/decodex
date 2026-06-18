from __future__ import annotations

import subprocess
from pathlib import Path

from semantic_drift_paths import is_docs_path


def stale_phrase_hits(repo_root: Path, removed_terms: list[str]) -> list[dict[str, str]]:
    if not removed_terms:
        return []
    tracked = subprocess.check_output(["git", "ls-files"], cwd=repo_root, text=True).splitlines()
    docs_paths = [path for path in tracked if is_docs_path(path)]
    hits: list[dict[str, str]] = []
    for term in removed_terms:
        for relative in docs_paths:
            path = repo_root / relative
            if not path.is_file():
                continue
            try:
                text = path.read_text(encoding="utf-8")
            except UnicodeDecodeError:
                continue
            for line_number, line in enumerate(text.splitlines(), start=1):
                if term in line:
                    hits.append({"term": term, "path": relative, "line": str(line_number)})
    return hits
