//! Evidence predicates for explicit operator recovery diagnostics.

mod ghost_lane;
mod json;
mod stale_active;

pub(super) use self::{
	ghost_lane::{
		ghost_lane_events_are_mcp_test_recovery_evidence, ghost_lane_has_mcp_test_fixture_identity,
		ghost_lane_mcp_test_fixture_allowed_live_blocker,
		ghost_lane_private_event_is_cleanup_audit,
		ghost_lane_private_events_are_cleanup_audit_evidence,
		ghost_lane_record_has_pr_or_review_lineage,
	},
	stale_active::{
		stale_active_private_event_allows_release,
		stale_active_private_event_is_release_audit_for_run,
	},
};
