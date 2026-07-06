use crate::{
	recovery::stale_active_reentry::{
		control, local_cleanup, types::StaleActiveStartableStateRestoreReentryInput,
	},
	state::ProjectRunStatus,
};

pub(in crate::recovery::stale_active_reentry) fn apply_stale_active_startable_state_restore_reentry(
	input: StaleActiveStartableStateRestoreReentryInput<'_>,
	audit_evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if stale_active_startable_state_restore_reentry_allowed(input, audit_evidence, blockers) {
		audit_evidence.push(String::from("stale_active_startable_state_restore_pending"));
		blockers.retain(|blocker| !stale_active_startable_state_restore_reentry_blocker(blocker));
	}
}

fn stale_active_startable_state_restore_reentry_allowed(
	input: StaleActiveStartableStateRestoreReentryInput<'_>,
	audit_evidence: &[String],
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
		&& local_cleanup::attempt_status_allows_release_reentry(ProjectRunStatus::status(run))
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

fn stale_active_startable_state_restore_reentry_blocker(blocker: &str) -> bool {
	matches!(
		blocker,
		"active_label_missing"
			| "child_agent_activity_present"
			| "protocol_activity_present"
			| "protocol_event_evidence_present"
	)
}
