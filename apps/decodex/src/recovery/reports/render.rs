use crate::recovery::reports::{
	ghost_lane::{GhostLaneDiagnostic, GhostLaneRecoveryReport},
	review_handoff::ReviewHandoffRecoveryReport,
	stale_active::StaleActiveRecoveryReport,
};

pub(in crate::recovery) fn render_review_handoff_recovery_report(
	report: &ReviewHandoffRecoveryReport,
) -> String {
	let mut output =
		format!("Review handoff recovery diagnostics for project {}\n", report.project_id);

	if report.diagnostics.is_empty() {
		output.push_str("- none\n");

		return output;
	}

	for diagnostic in &report.diagnostics {
		output.push_str(&format!(
			"- issue: {}\n  state: {}\n  classification: {}\n  reason: {}\n  branch: {}\n  worktree_path: {}\n  local_branch: {}\n  local_head: {}\n  worktree_clean: {}\n  existing_pr_url: {}\n  existing_lifecycle_handoff_head: {}\n  existing_lifecycle_phase_head: {}\n  pr_base_ref: {}\n  pr_head: {}\n  pr_read_error: {}\n  mismatched_field: {}\n  active_label_present: {}\n  next_action: {}\n",
			diagnostic.issue_identifier,
			diagnostic.issue_state,
			diagnostic.classification,
			diagnostic.reason,
			diagnostic.branch_name,
			diagnostic.worktree_path,
			optional_text(diagnostic.local_branch_name.as_deref()),
			optional_text(diagnostic.local_head_oid.as_deref()),
			diagnostic.worktree_clean.map_or_else(|| String::from("unknown"), |clean| clean.to_string()),
			optional_text(diagnostic.existing_pr_url.as_deref()),
			optional_text(diagnostic.existing_lifecycle_handoff_head_oid.as_deref()),
			optional_text(diagnostic.existing_lifecycle_phase_head_oid.as_deref()),
			optional_text(diagnostic.pr_base_ref.as_deref()),
			optional_text(diagnostic.pr_head_oid.as_deref()),
			optional_text(diagnostic.pr_read_error.as_deref()),
			optional_text(diagnostic.mismatched_field.as_deref()),
			diagnostic.active_label_present.map_or_else(|| String::from("unknown"), |present| present.to_string()),
			diagnostic.next_action,
		));
	}

	output
}

pub(in crate::recovery) fn render_ghost_lane_recovery_report(
	report: &GhostLaneRecoveryReport,
) -> String {
	let mut output = format!("Ghost lane recovery diagnostics for project {}\n", report.project_id);

	if report.diagnostics.is_empty() {
		output.push_str("- none\n");

		return output;
	}

	for diagnostic in &report.diagnostics {
		output.push_str(&format!(
			"- issue: {}\n  local_issue_id: {}\n  run_id: {}\n  attempt: {}\n  attempt_status: {}\n  classification: {}\n  reason: {}\n  run_lease: {}\n  control_channel: {}\n  evidence: {}\n  blockers: {}\n  next_action: {}\n",
			render_ghost_lane_issue(diagnostic),
			diagnostic.issue_id,
			diagnostic.run_id,
			diagnostic.attempt_number,
			diagnostic.attempt_status,
			diagnostic.classification,
			diagnostic.reason,
			diagnostic.run_lease,
			diagnostic.control_channel,
			render_string_list(&diagnostic.evidence),
			render_string_list(&diagnostic.blockers),
			diagnostic.next_action,
		));
	}

	output
}

pub(in crate::recovery) fn render_stale_active_recovery_report(
	report: &StaleActiveRecoveryReport,
) -> String {
	let mut output =
		format!("Stale active recovery diagnostics for project {}\n", report.project_id);

	if report.diagnostics.is_empty() {
		output.push_str("- none\n");

		return output;
	}

	for diagnostic in &report.diagnostics {
		output.push_str(&format!(
			"- issue: {}\n  issue_id: {}\n  issue_state: {}\n  classification: {}\n  reason: {}\n  queue_label_present: {}\n  active_label_present: {}\n  needs_attention_label_present: {}\n  latest_run_id: {}\n  latest_attempt: {}\n  latest_attempt_status: {}\n  run_lease: {}\n  active_shared_claim: {}\n  control_channel: {}\n  worktree_path: {}\n  worktree_state: {}\n  evidence: {}\n  blockers: {}\n  next_action: {}\n",
			diagnostic.issue_identifier,
			diagnostic.issue_id,
			diagnostic.issue_state,
			diagnostic.classification,
			diagnostic.reason,
			diagnostic.queue_label_present,
			diagnostic.active_label_present,
			diagnostic.needs_attention_label_present,
			diagnostic.latest_run_id.as_deref().unwrap_or("none"),
			diagnostic
				.latest_attempt_number
				.map(|attempt| attempt.to_string())
				.unwrap_or_else(|| String::from("none")),
			diagnostic.latest_attempt_status.as_deref().unwrap_or("none"),
			diagnostic.run_lease,
			diagnostic.active_shared_claim,
			diagnostic.control_channel,
			diagnostic.worktree_path.as_deref().unwrap_or("none"),
			diagnostic.worktree_state,
			render_string_list(&diagnostic.evidence),
			render_string_list(&diagnostic.blockers),
			diagnostic.next_action,
		));
	}

	output
}

pub(in crate::recovery) fn render_ghost_lane_issue(diagnostic: &GhostLaneDiagnostic) -> &str {
	diagnostic.issue_identifier.as_deref().unwrap_or(diagnostic.issue_id.as_str())
}

fn optional_text(value: Option<&str>) -> &str {
	value.unwrap_or("none")
}

fn render_string_list(values: &[String]) -> String {
	if values.is_empty() { String::from("none") } else { values.join(",") }
}
