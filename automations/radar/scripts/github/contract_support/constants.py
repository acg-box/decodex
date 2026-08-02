from __future__ import annotations

import re

BUNDLE_SCHEMA = "github_change_bundle/v1"
SIGNAL_SCHEMA = "signal_entry/v1"
RELEASE_DELTA_SCHEMA = "release_delta/v1"
UPSTREAM_REVIEW_QUEUE_SCHEMA = "upstream_review_queue/v1"
UPSTREAM_REVIEW_SCHEMA = "upstream_review/v1"
ANALYSIS_MODES = {"pr_first", "commit_only"}
SIGNAL_KINDS = {"capability", "behavior_change", "try_now"}
SIGNAL_CONFIDENCE = {"confirmed", "likely", "weak"}
SIGNAL_IMPACT = {"low", "medium", "high"}
SOURCE_ITEM_KINDS = {"pull_request", "commit"}
UPSTREAM_SUBJECT_KINDS = {"commit", "pr"}
UPSTREAM_REVIEW_PRIORITIES = {"critical", "high", "normal", "low"}
UPSTREAM_REVIEW_NEXT_STEPS = {"ai_review_required"}
UPSTREAM_SOURCE_STATES = {"open", "closed", "merged", "commit_only"}
UPSTREAM_REVIEW_ACTION_TYPES = {
    "none",
    "upstream_impact",
    "signal_entry",
}
ISSUE_REF_RE = re.compile(r"(?:^|[^\w])((?:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#\d+)")
FLAG_RE = re.compile(
    r"(?<![\w-])(--[a-zA-Z0-9][\w-]*|[A-Z][A-Z0-9_]{2,}(?:=[^\s,`]+)?)"
)
GENERIC_COMMIT_TITLES = {
    "update",
    "fix",
    "fix.",
    "fix tests",
    "fix tests.",
    "merge fixes",
    "flaky syntax",
}
