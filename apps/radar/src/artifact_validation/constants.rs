//! Constants for Radar artifact validation.

pub(super) const ANALYSIS_MODES: &[&str] = &["commit_only", "pr_first"];
pub(super) const SIGNAL_IMPACT: &[&str] = &["high", "low", "medium"];
pub(super) const SIGNAL_KINDS: &[&str] = &["behavior_change", "capability", "try_now"];
pub(super) const SOURCE_ITEM_KINDS: &[&str] = &["commit", "pull_request"];
pub(super) const CONTROL_PLANE_UPGRADE_IMPACTS: &[&str] =
	&["adopt_now", "candidate", "compat_risk"];
pub(super) const CONTROL_PLANE_UPGRADE_PATHS: &[&str] =
	&["adopt_now", "compat_risk_mitigation", "discovery"];
pub(super) const CONTROL_PLANE_UPGRADE_STATUSES: &[&str] =
	&["blocked", "deferred", "proposed", "superseded"];
pub(super) const CODEX_COMPATIBILITY_STATUSES: &[&str] =
	&["compatible", "incompatible", "needs_review", "not_tested", "unknown"];
pub(super) const CODEX_TARGET_CHANNELS: &[&str] = &["main", "preview", "stable"];
pub(super) const UPSTREAM_IMPACT_KINDS: &[&str] =
	&["browser_observation", "changelog", "commit", "pull_request", "release", "signal"];
pub(super) const UPSTREAM_REVIEW_ACTION_TYPES: &[&str] =
	&["control_plane_upgrade_candidate", "none", "signal_entry", "upstream_impact"];
pub(super) const UPSTREAM_REVIEW_NEXT_STEPS: &[&str] = &["ai_review_required"];
pub(super) const UPSTREAM_REVIEW_PRIORITIES: &[&str] = &["critical", "high", "low", "normal"];
pub(super) const UPSTREAM_SOURCE_STATES: &[&str] = &["closed", "commit_only", "merged", "open"];
