//! Constants for Radar artifact validation.

pub(super) const RADAR_ARCHIVE_MANIFEST_SCHEMA: &str = "radar_archive_manifest/v1";
pub(super) const ANALYSIS_MODES: &[&str] = &["commit_only", "pr_first"];
pub(super) const SIGNAL_IMPACT: &[&str] = &["high", "low", "medium"];
pub(super) const SIGNAL_KINDS: &[&str] = &["behavior_change", "capability", "try_now"];
pub(super) const SOCIAL_BLOCK_REASONS: &[&str] =
	&["daily_cap_exceeded", "duplicate", "insufficient_evidence", "policy_block"];
pub(super) const SOCIAL_POST_MODES: &[&str] = &[
	"operator_impact",
	"practical_explainer",
	"release_pulse",
	"release_rollup",
	"thread",
	"watch_note",
];
pub(super) const SOCIAL_POST_PRIORITIES: &[&str] = &["critical", "high", "low", "normal"];
pub(super) const SOCIAL_POST_STATUSES: &[&str] = &["blocked", "failed", "published", "skipped"];
pub(super) const SOCIAL_POST_WORTHINESS: &[&str] = &["block", "publish", "skip"];
pub(super) const SOCIAL_POST_LIFECYCLE_STATES: &[&str] = &[
	"deleted_by_operator",
	"live",
	"superseded_failed_attempt",
	"superseded_published",
	"superseded_text_only",
];
pub(super) const SOCIAL_PUBLISH_RESERVATION_STATUSES: &[&str] =
	&["active", "canceled", "consumed", "expired"];
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
pub(super) const UPSTREAM_REVIEW_ACTION_TYPES: &[&str] = &[
	"control_plane_upgrade_candidate",
	"none",
	"signal_entry",
	"social_candidate",
	"upstream_impact",
];
pub(super) const UPSTREAM_REVIEW_NEXT_STEPS: &[&str] = &["ai_review_required"];
pub(super) const UPSTREAM_REVIEW_PRIORITIES: &[&str] = &["critical", "high", "low", "normal"];
pub(super) const UPSTREAM_SOURCE_STATES: &[&str] = &["closed", "commit_only", "merged", "open"];
