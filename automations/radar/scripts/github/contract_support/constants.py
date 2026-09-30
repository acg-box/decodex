from __future__ import annotations

BUNDLE_SCHEMA = "github_change_bundle/v1"
ANALYSIS_MODES = {"pr_first", "commit_only"}
SIGNAL_KINDS = {"capability", "behavior_change", "try_now"}
SIGNAL_CONFIDENCE = {"confirmed", "likely", "weak"}
SIGNAL_IMPACT = {"low", "medium", "high"}
