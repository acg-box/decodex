use crate::{
	recovery::{
		GHOST_LANE_TERMINAL_STATUS,
		stale_active_reentry::{control, types::StaleActiveLocalCleanupReentryInput},
	},
	state::ProjectRunStatus,
};

pub(in crate::recovery::stale_active_reentry) fn apply_stale_active_local_cleanup_reentry(
	input: &StaleActiveLocalCleanupReentryInput<'_>,
	audit_evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if stale_active_local_cleanup_reentry_allowed(input, audit_evidence, blockers) {
		audit_evidence.push(String::from("stale_active_local_cleanup_complete"));
		blockers.retain(|blocker| !stale_active_local_cleanup_reentry_blocker(blocker));
	}
}

pub(in crate::recovery::stale_active_reentry) fn apply_missing_active_label_retained_cleanup(
	input: &StaleActiveLocalCleanupReentryInput<'_>,
	audit_evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if blockers.as_slice() != ["active_label_missing"] {
		return;
	}

	let Some(run) = input.run else {
		return;
	};
	let private_evidence_allows_cleanup =
		super::evidence_contains(
			audit_evidence,
			"only_stale_active_or_failed_control_evidence_present",
		) || super::evidence_contains(audit_evidence, "private_evidence_missing");
	let retained_clean_worktree = input.worktree_state == "clean"
		&& super::evidence_contains(audit_evidence, "worktree_head_reachable_from_default_branch");
	let completed_local_cleanup = input.worktree_state == "missing"
		&& super::evidence_contains(audit_evidence, "stale_active_release_audit_present")
		&& super::evidence_contains(audit_evidence, "worktree_mapping_missing")
		&& super::evidence_contains(audit_evidence, "worktree_missing");
	let process_evidence_allows_cleanup = completed_local_cleanup
		|| super::evidence_contains(audit_evidence, "process_not_alive")
		|| super::evidence_contains(audit_evidence, "activity_marker_missing");

	if !input.queue_label_present
		&& !input.active_label_present
		&& !input.needs_attention_label_present
		&& !input.run_lease
		&& !input.active_shared_claim
		&& attempt_status_allows_release_reentry(ProjectRunStatus::status(run))
		&& (retained_clean_worktree || completed_local_cleanup)
		&& control::reentry_control_channel_inactive_or_absent(
			input.control_channel,
			audit_evidence,
		) && private_evidence_allows_cleanup
		&& process_evidence_allows_cleanup
		&& super::evidence_contains(audit_evidence, "review_lineage_missing")
	{
		blockers.clear();
		audit_evidence.push(String::from("active_label_already_absent_cleanup"));
	}
}

pub(in crate::recovery::stale_active_reentry) fn attempt_status_allows_release_reentry(
	status: &str,
) -> bool {
	matches!(status, GHOST_LANE_TERMINAL_STATUS | "failed" | "interrupted")
}

fn stale_active_local_cleanup_reentry_allowed(
	input: &StaleActiveLocalCleanupReentryInput<'_>,
	audit_evidence: &[String],
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
		&& attempt_status_allows_release_reentry(ProjectRunStatus::status(run))
		&& input.worktree_state == "missing"
		&& control::reentry_control_channel_inactive_or_absent(
			input.control_channel,
			audit_evidence,
		) && super::evidence_contains(
		audit_evidence,
		"only_stale_active_or_failed_control_evidence_present",
	) && super::evidence_contains(audit_evidence, "review_lineage_missing")
		&& super::evidence_contains(audit_evidence, "stale_active_release_audit_present")
		&& super::evidence_contains(audit_evidence, "worktree_mapping_missing")
		&& super::evidence_contains(audit_evidence, "worktree_missing")
}

fn stale_active_local_cleanup_reentry_blocker(blocker: &str) -> bool {
	matches!(
		blocker,
		"child_agent_activity_present"
			| "protocol_activity_present"
			| "protocol_event_evidence_present"
	)
}
