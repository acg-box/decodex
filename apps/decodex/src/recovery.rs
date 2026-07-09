//! Explicit operator recovery surfaces for retained Decodex lanes.

mod closeout;
mod context;
mod events;
mod evidence;
mod ghost_lane;
mod ghost_lane_cleanup;
mod ghost_lane_diagnosis;
mod git_worktree;
mod identifiers;
mod process_liveness;
mod pull_request_inspection;
mod reports;
mod requests;
mod review_handoff;
mod review_handoff_apply;
mod review_handoff_diagnosis;
mod review_handoff_policy;
mod stale_active;
mod stale_active_authority;
mod stale_active_diagnosis;
mod stale_active_guidance;
mod stale_active_labels;
mod stale_active_reentry;
mod stale_active_release;
mod stale_active_runtime;
mod stale_active_worktree;

pub(crate) use self::{
	closeout::{run_legacy_closeout, run_merged_closeout},
	ghost_lane::{run_ghost_lane_cleanup, run_ghost_lane_diagnose},
	requests::{
		GhostLaneCleanupRequest, GhostLaneDiagnoseRequest, LegacyCloseoutRecoveryRequest,
		MergedCloseoutRecoveryRequest, ReviewHandoffAdoptRequest, ReviewHandoffDiagnoseRequest,
		ReviewHandoffRebindRequest, StaleActiveDiagnoseRequest, StaleActiveReleaseRequest,
	},
	review_handoff::commands::{
		run_review_handoff_adopt, run_review_handoff_diagnose, run_review_handoff_rebind,
	},
	stale_active::{run_stale_active_diagnose, run_stale_active_release},
};

use std::collections::BTreeSet;

#[cfg(test)] use crate::state::RUN_CONTROL_CHANNEL_STATUS_FAILED;
use crate::tracker::{
	privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
	records::LinearExecutionEventRecord,
};
#[cfg(test)] use context::LINEAR_RATE_LIMIT_BACKOFF_WARNING;
use context::{
	RecoveryContext, RecoveryRuntimeMutationPolicy, active_recovery_tracker_backoff_message,
	load_recovery_context_for_dry_run, load_recovery_context_read_only,
	remember_recovery_tracker_backoff_message,
};
#[cfg(test)] use events::manual_adopt_run_id;
use events::{
	append_review_handoff_adopt_private_event, append_review_handoff_rebind_private_event,
	review_handoff_adopt_event, review_handoff_rebind_event,
};
#[cfg(test)] use events::{current_timestamp, timestamp_after_seconds};
use ghost_lane_cleanup::{
	apply_ghost_lane_cleanup, apply_ghost_lane_live_status_blockers,
	ensure_ghost_lane_live_status_allows_cleanup,
};
#[cfg(test)]
use ghost_lane_cleanup::{
	apply_ghost_lane_live_status_blockers_with_tracker,
	ensure_ghost_lane_live_status_allows_cleanup_with_tracker,
};
use ghost_lane_diagnosis::{diagnose_ghost_lanes, diagnose_ghost_lanes_read_only};
#[cfg(test)] use git_worktree::worktree_blocking_status_lines;
use pull_request_inspection::{inspect_project_pull_request, landing_url};
#[cfg(test)] use reports::GhostLaneDiagnostic;
use reports::{
	GhostLaneRecoveryReport, StaleActiveRecoveryReport, render_ghost_lane_issue,
	render_ghost_lane_recovery_report, render_stale_active_recovery_report,
};
use review_handoff::{AdoptValidation, RebindValidation, load_issue_by_identifier};
#[cfg(test)]
use review_handoff::{
	validate_adopt_existing_worktree_mapping, validate_existing_handoff_refresh,
	validate_rebind_existing_handoff, validate_rebind_tracker_labels_with_tracker,
};
#[cfg(test)]
use review_handoff_apply::{
	write_review_lifecycle_fixtures_with_rollback, write_review_lifecycle_with_rollback,
};
#[cfg(test)]
use review_handoff_diagnosis::{
	HandoffDiagnosticRequest, diagnose_all_retained_review_worktrees_with_tracker,
	diagnose_issue_with_tracker, diagnostic_binding,
};
#[cfg(test)]
use review_handoff_policy::{
	RebindMode, validate_adopt_issue_state_for_policy, validate_adopt_landing_state,
	validate_rebind_issue_state_for_policy,
};
use stale_active_diagnosis::diagnose_stale_active_issues;
use stale_active_release::{apply_stale_active_release, preflight_stale_active_worktree_cleanup};
#[cfg(test)]
use stale_active_release::{
	apply_stale_active_release_with_tracker, clear_stale_active_dead_run_claims_before_release,
	ensure_stale_active_run_claim_guard,
};

const MISSING_HANDOFF_REASON: &str = "missing_review_handoff_record";
const ORPHANED_REVIEW_HANDOFF_CLASSIFICATION: &str = "orphaned_review_handoff";
const REVIEW_HANDOFF_BOUND_CLASSIFICATION: &str = "review_handoff_bound";
const REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION: &str = "review_handoff_ownership_drift";
const REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION: &str = "review_handoff_rebind_required";
const REVIEW_HANDOFF_UNVERIFIED_CLASSIFICATION: &str = "review_handoff_unverified";
const REVIEW_HANDOFF_MISMATCH_CLASSIFICATION: &str = "review_handoff_mismatch";
const STALE_TERMINAL_RESIDUE_CLASSIFICATION: &str = "stale_terminal_local_residue";
const REVIEW_HANDOFF_REBIND_EVENT: &str = "review_handoff_rebind";
const REVIEW_HANDOFF_ADOPT_EVENT: &str = "review_handoff_adopt";
const LEGACY_MANUAL_CLOSEOUT_EVENT: &str = "closeout";
const LEGACY_MANUAL_CLOSEOUT_ANCHOR: &str = "legacy_manual_closeout";
const MERGED_CLOSEOUT_CLOSEOUT_ANCHOR: &str = "merged_closeout";
const MERGED_CLOSEOUT_CLEANUP_ANCHOR: &str = "merged_closeout_cleanup";
const GHOST_LANE_CLASSIFICATION: &str = "missing_issue_ghost_lane";
const MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION: &str = "mcp_test_fixture_ghost_lane";
const GHOST_LANE_BLOCKED_CLASSIFICATION: &str = "ghost_lane_recovery_blocked";
const GHOST_LANE_CLEANUP_EVENT: &str = "ghost_lane_cleanup";
const GHOST_LANE_TERMINAL_STATUS: &str = "terminal_guarded";
const STALE_ACTIVE_CLASSIFICATION: &str = "stale_active_ownership";
const STALE_ACTIVE_BLOCKED_CLASSIFICATION: &str = "stale_active_recovery_blocked";
const STALE_ACTIVE_STATE_RESTORE_CLASSIFICATION: &str = "stale_active_state_restore_pending";
const STALE_ACTIVE_RELEASE_EVENT: &str = "stale_active_release";
const STALE_ACTIVE_RECOVERY_SCHEMA: &str = "decodex.stale_active_recovery_private_event/1";
const MCP_TEST_FIXTURE_SOURCE: &str = "mcp-test";
const MCP_TEST_FIXTURE_PROJECT_ID: &str = "pubfi";
const MCP_TEST_FIXTURE_ISSUE_ID: &str = "PUB-012";
const MCP_TEST_FIXTURE_ALT_ISSUE_IDENTIFIER: &str = "PUBFI-012";
const MCP_TEST_FIXTURE_RUN_ID: &str = "run-12";
const MCP_TEST_FIXTURE_THREAD_ID: &str = "thread-12";
const MCP_TEST_FIXTURE_TURN_ID: &str = "turn-12";
const REBOUND_LIFECYCLE_PHASE: &str = "request_pending";

fn sorted_unique(values: Vec<String>) -> Vec<String> {
	let mut set = BTreeSet::new();

	for value in values {
		set.insert(value);
	}

	set.into_iter().collect()
}

#[cfg(test)] mod tests;
