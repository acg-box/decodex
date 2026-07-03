//! Reentry predicates for stale-active release recovery.

use crate::{
	recovery::{GHOST_LANE_TERMINAL_STATUS, stale_active_labels::StaleActiveLabelSnapshot},
	state::ProjectRunStatus,
	tracker::TrackerIssue,
	workflow::WorkflowTracker,
};

pub(super) struct StaleActiveReleaseReentryInput<'a> {
	pub(super) run: Option<&'a ProjectRunStatus>,
	pub(super) run_lease: bool,
	pub(super) active_shared_claim: bool,
	pub(super) labels: &'a StaleActiveLabelSnapshot,
	pub(super) issue: &'a TrackerIssue,
	pub(super) tracker_policy: &'a WorkflowTracker,
	pub(super) worktree_state: &'a str,
	pub(super) control_channel: &'a str,
}

struct StaleActiveStartableStateRestoreReentryInput<'a> {
	run: Option<&'a ProjectRunStatus>,
	run_lease: bool,
	active_shared_claim: bool,
	queue_label_present: bool,
	active_label_present: bool,
	needs_attention_label_present: bool,
	issue_state: &'a str,
	in_progress_state: &'a str,
	startable_state_id_present: bool,
	worktree_state: &'a str,
	control_channel: &'a str,
}

struct StaleActiveLocalCleanupReentryInput<'a> {
	run: Option<&'a ProjectRunStatus>,
	run_lease: bool,
	active_shared_claim: bool,
	active_label_present: bool,
	needs_attention_label_present: bool,
	worktree_state: &'a str,
	control_channel: &'a str,
}

pub(super) fn apply_stale_active_release_reentries(
	input: StaleActiveReleaseReentryInput<'_>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	apply_stale_active_local_cleanup_reentry(
		StaleActiveLocalCleanupReentryInput {
			run: input.run,
			run_lease: input.run_lease,
			active_shared_claim: input.active_shared_claim,
			active_label_present: input.labels.active_label_present,
			needs_attention_label_present: input.labels.needs_attention_label_present,
			worktree_state: input.worktree_state,
			control_channel: input.control_channel,
		},
		evidence,
		blockers,
	);
	apply_stale_active_startable_state_restore_reentry(
		StaleActiveStartableStateRestoreReentryInput {
			run: input.run,
			run_lease: input.run_lease,
			active_shared_claim: input.active_shared_claim,
			queue_label_present: input.labels.queue_label_present,
			active_label_present: input.labels.active_label_present,
			needs_attention_label_present: input.labels.needs_attention_label_present,
			issue_state: &input.issue.state.name,
			in_progress_state: input.tracker_policy.in_progress_state(),
			startable_state_id_present: stale_active_startable_state_id_present(&input),
			worktree_state: input.worktree_state,
			control_channel: input.control_channel,
		},
		evidence,
		blockers,
	);
}

pub(super) fn evidence_contains(evidence: &[String], expected: &str) -> bool {
	evidence.iter().any(|entry| entry == expected)
}

fn stale_active_startable_state_id_present(input: &StaleActiveReleaseReentryInput<'_>) -> bool {
	input
		.tracker_policy
		.startable_states()
		.first()
		.and_then(|state_name| input.issue.state_id_for_name(state_name))
		.is_some()
}

fn apply_stale_active_local_cleanup_reentry(
	input: StaleActiveLocalCleanupReentryInput<'_>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if stale_active_local_cleanup_reentry_allowed(input, evidence, blockers) {
		evidence.push(String::from("stale_active_local_cleanup_complete"));
		blockers.retain(|blocker| !stale_active_local_cleanup_reentry_blocker(blocker));
	}
}

fn apply_stale_active_startable_state_restore_reentry(
	input: StaleActiveStartableStateRestoreReentryInput<'_>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if stale_active_startable_state_restore_reentry_allowed(input, evidence, blockers) {
		evidence.push(String::from("stale_active_startable_state_restore_pending"));
		blockers.retain(|blocker| !stale_active_startable_state_restore_reentry_blocker(blocker));
	}
}

fn stale_active_startable_state_restore_reentry_allowed(
	input: StaleActiveStartableStateRestoreReentryInput<'_>,
	evidence: &[String],
	blockers: &[String],
) -> bool {
	if blockers.is_empty()
		|| !blockers
			.iter()
			.all(|blocker| stale_active_startable_state_restore_reentry_blocker(blocker.as_str()))
	{
		return false;
	}

	let Some(run) = input.run else {
		return false;
	};

	input.queue_label_present
		&& !input.active_label_present
		&& !input.needs_attention_label_present
		&& !input.run_lease
		&& !input.active_shared_claim
		&& input.issue_state == input.in_progress_state
		&& input.startable_state_id_present
		&& stale_active_attempt_status_allows_release_reentry(run.status())
		&& input.worktree_state == "missing"
		&& stale_active_reentry_control_channel_inactive_or_absent(input.control_channel, evidence)
		&& evidence_contains(evidence, "only_stale_active_or_failed_control_evidence_present")
		&& evidence_contains(evidence, "review_lineage_missing")
		&& evidence_contains(evidence, "stale_active_release_audit_present")
		&& evidence_contains(evidence, "worktree_mapping_missing")
		&& evidence_contains(evidence, "worktree_missing")
}

fn stale_active_startable_state_restore_reentry_blocker(blocker: &str) -> bool {
	matches!(
		blocker,
		"active_label_missing"
			| "child_agent_activity_present"
			| "protocol_activity_present"
			| "protocol_event_evidence_present"
	)
}

fn stale_active_local_cleanup_reentry_allowed(
	input: StaleActiveLocalCleanupReentryInput<'_>,
	evidence: &[String],
	blockers: &[String],
) -> bool {
	if blockers.is_empty()
		|| !blockers
			.iter()
			.all(|blocker| stale_active_local_cleanup_reentry_blocker(blocker.as_str()))
	{
		return false;
	}

	let Some(run) = input.run else {
		return false;
	};

	input.active_label_present
		&& !input.needs_attention_label_present
		&& !input.run_lease
		&& !input.active_shared_claim
		&& stale_active_attempt_status_allows_release_reentry(run.status())
		&& input.worktree_state == "missing"
		&& stale_active_reentry_control_channel_inactive_or_absent(input.control_channel, evidence)
		&& evidence_contains(evidence, "only_stale_active_or_failed_control_evidence_present")
		&& evidence_contains(evidence, "review_lineage_missing")
		&& evidence_contains(evidence, "stale_active_release_audit_present")
		&& evidence_contains(evidence, "worktree_mapping_missing")
		&& evidence_contains(evidence, "worktree_missing")
}

fn stale_active_attempt_status_allows_release_reentry(status: &str) -> bool {
	matches!(status, GHOST_LANE_TERMINAL_STATUS | "failed" | "interrupted")
}

fn stale_active_local_cleanup_reentry_blocker(blocker: &str) -> bool {
	matches!(
		blocker,
		"child_agent_activity_present"
			| "protocol_activity_present"
			| "protocol_event_evidence_present"
	)
}

fn stale_active_reentry_control_channel_inactive_or_absent(
	control_channel: &str,
	evidence: &[String],
) -> bool {
	if control_channel == "missing" {
		return evidence_contains(evidence, "control_channel_missing");
	}

	!control_channel.ends_with(":active")
		&& evidence_contains(evidence, "control_channel_inactive_or_file_missing")
}
