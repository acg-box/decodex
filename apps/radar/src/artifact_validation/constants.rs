//! Constants for Radar artifact validation.

pub(super) const ANALYSIS_MODES: &[&str] = &["commit_only", "pr_first"];
pub(super) const SIGNAL_IMPACT: &[&str] = &["high", "low", "medium"];
pub(super) const SIGNAL_KINDS: &[&str] = &["behavior_change", "capability", "try_now"];
pub(super) const SOURCE_ITEM_KINDS: &[&str] = &["commit", "pull_request"];
pub(super) const UPSTREAM_IMPACT_KINDS: &[&str] =
	&["browser_observation", "changelog", "commit", "pull_request", "release", "signal"];
pub(super) const UPSTREAM_REVIEW_ACTION_TYPES: &[&str] =
	&["none", "signal_entry", "upstream_impact"];
pub(super) const UPSTREAM_REVIEW_NEXT_STEPS: &[&str] = &["ai_review_required"];
pub(super) const UPSTREAM_REVIEW_PRIORITIES: &[&str] = &["critical", "high", "low", "normal"];
pub(super) const UPSTREAM_SOURCE_STATES: &[&str] = &["closed", "commit_only", "merged", "open"];
