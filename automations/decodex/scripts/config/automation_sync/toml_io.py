"""TOML and scalar rendering helpers for automation sync."""

from __future__ import annotations

import json
import tomllib
from pathlib import Path
from typing import Any


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)
