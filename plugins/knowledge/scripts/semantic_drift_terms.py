from __future__ import annotations

import re


CLAIM_PATTERN = re.compile(
    r"\b("
    r"must|should|will|does|own|owns|route|routes|validate|validates|report|reports|"
    r"emit|emits|support|supports|require|requires|return|returns|write|writes|read|"
    r"reads|sync|syncs|clean|cleans|delete|deletes|land|lands|commit|commits"
    r")\b|`[^`]+`|--[A-Za-z0-9][A-Za-z0-9_-]*",
    flags=re.IGNORECASE,
)
REMOVED_TERM_PATTERN = re.compile(
    r"--[A-Za-z0-9][A-Za-z0-9_-]*|"
    r"`[^`]{6,}`|"
    r"['\"][A-Za-z0-9_:/.-]{6,}['\"]|"
    r"\b[A-Z][A-Z0-9_]{5,}\b|"
    r"\b[a-z][a-z0-9]+(?:_[a-z0-9]+)+\b"
)
COMMON_TERMS = {
    "agents",
    "agents_text",
    "assert_contains",
    "description",
    "longdescription",
    "return",
    "returns",
    "shortdescription",
    "should",
    "support",
    "supports",
    "validate",
    "validates",
}


def extract_executable_terms(text: str) -> set[str]:
    terms: set[str] = set()
    for match in REMOVED_TERM_PATTERN.findall(text):
        term = match.strip("`\"'")
        if len(term) >= 6 and term.lower() not in COMMON_TERMS:
            terms.add(term)
    return terms
