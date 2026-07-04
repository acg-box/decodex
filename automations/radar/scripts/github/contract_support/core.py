from __future__ import annotations

import json
import re
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from contract_support.constants import FLAG_RE, ISSUE_REF_RE


@dataclass
class ValidationResult:
    ok: bool
    errors: list[str]


def load_json(path: str | Path) -> Any:
    return json.loads(Path(path).read_text(encoding="utf-8"))


def dump_json(path: str | Path, payload: Any) -> None:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def utc_now_iso() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def slugify(value: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", value.lower()).strip("-")
    return slug or "signal"


def first_line(value: str) -> str:
    return value.strip().splitlines()[0] if value.strip() else ""


def truncate_patch(value: str | None, limit: int = 900) -> str | None:
    if not value:
        return None
    compact = value.strip()
    return compact[:limit] + "..." if len(compact) > limit else compact


def collect_issue_refs(*texts: str) -> list[str]:
    found: list[str] = []
    for text in texts:
        for match in ISSUE_REF_RE.findall(text or ""):
            if match not in found:
                found.append(match)
    return found


def collect_flags(*texts: str) -> list[str]:
    found: list[str] = []
    for text in texts:
        for match in FLAG_RE.findall(text or ""):
            if match not in found:
                found.append(match)
    return found
