use crate::{
	recovery::{
		self, STALE_ACTIVE_BLOCKED_CLASSIFICATION, STALE_ACTIVE_CLASSIFICATION,
		STALE_ACTIVE_STATE_RESTORE_CLASSIFICATION, reports::StaleActiveDiagnostic,
		stale_active_diagnosis::inspection::inputs::StaleActiveDiagnosticParts,
		stale_active_guidance, stale_active_reentry,
	},
	state::ProjectRunStatus,
};

pub(super) fn stale_active_diagnostic_from_parts(
	parts: StaleActiveDiagnosticParts<'_>,
) -> StaleActiveDiagnostic {
	let (classification, reason, next_action) =
		stale_active_diagnostic_outcome(&parts.issue.identifier, &parts.evidence, &parts.blockers);

	StaleActiveDiagnostic {
		project_id: parts.project_id.to_owned(),
		issue_id: parts.issue.id,
		issue_identifier: parts.issue.identifier,
		issue_state: parts.issue.state.name,
		classification,
		reason,
		queue_label_present: parts.labels.queue_label_present,
		active_label_present: parts.labels.active_label_present,
		needs_attention_label_present: parts.labels.needs_attention_label_present,
		latest_run_id: parts.latest_run.map(|run| run.run_id().to_owned()),
		latest_attempt_number: parts.latest_run.map(ProjectRunStatus::attempt_number),
		latest_attempt_status: parts.latest_run.map(|run| run.status().to_owned()),
		run_lease: parts.run_lease,
		active_shared_claim: parts.active_shared_claim,
		control_channel: parts.control_channel,
		worktree_path: Some(parts.worktree_path.to_string_lossy().to_string()),
		worktree_state: parts.worktree_state,
		evidence: recovery::sorted_unique(parts.evidence),
		blockers: recovery::sorted_unique(parts.blockers),
		next_action,
	}
}

fn stale_active_diagnostic_outcome(
	issue_identifier: &str,
	evidence: &[String],
	blockers: &[String],
) -> (String, String, String) {
	if blockers.is_empty() {
		if stale_active_reentry::evidence_contains(
			evidence,
			"stale_active_startable_state_restore_pending",
		) {
			(
				String::from(STALE_ACTIVE_STATE_RESTORE_CLASSIFICATION),
				String::from(
					"queued_issue_needs_startable_state_restore_after_stale_active_release",
				),
				format!(
					"Run `decodex recover stale-active release {issue_identifier} --dry-run`, then rerun without `--dry-run` if the report stays safe.",
				),
			)
		} else {
			(
				String::from(STALE_ACTIVE_CLASSIFICATION),
				String::from(
					"tracker_issue_has_stale_active_label_without_live_or_retained_progress",
				),
				format!(
					"Run `decodex recover stale-active release {issue_identifier} --dry-run`, then rerun without `--dry-run` if the report stays safe.",
				),
			)
		}
	} else {
		(
			String::from(STALE_ACTIVE_BLOCKED_CLASSIFICATION),
			String::from("safety_check_blocked"),
			stale_active_guidance::blocked_stale_active_next_action(
				issue_identifier,
				blockers,
				evidence,
			),
		)
	}
}
