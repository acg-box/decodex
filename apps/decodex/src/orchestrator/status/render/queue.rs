use crate::orchestrator::{
	self, OperatorQueuedIssueStatus, OperatorStatusSnapshot, ServiceConfig, status_render::activity,
};

pub(crate) fn render_queue_explain(
	config: &ServiceConfig,
	queued_candidates: &[OperatorQueuedIssueStatus],
) -> String {
	let mut output = String::new();

	output.push_str(&format!("Project: {}\n", config.service_id()));
	output.push_str("Mode: dry-run queue explain\n");
	output.push_str(&format!("Queued candidates: {}\n", queued_candidates.len()));
	output.push_str(&format!(
		"Ready: {}\n",
		queued_candidates.iter().filter(|candidate| candidate.classification == "ready").count()
	));
	output.push_str(&format!(
		"Waiting: {}\n",
		queued_candidates.iter().filter(|candidate| candidate.classification == "waiting").count()
	));
	output.push_str(&format!(
		"Blocked: {}\n",
		queued_candidates.iter().filter(|candidate| candidate.classification == "blocked").count()
	));
	output.push_str(&format!(
		"Claimed: {}\n",
		queued_candidates.iter().filter(|candidate| candidate.classification == "claimed").count()
	));
	output.push_str(&format!(
		"Closed: {}\n",
		queued_candidates.iter().filter(|candidate| candidate.classification == "closed").count()
	));
	output.push_str("\nQueued Candidate Reasons\n");

	if queued_candidates.is_empty() {
		output.push_str("- none\n");
		output.push_str(&format!(
			"  {}\n",
			orchestrator::format_status_no_eligible_issue_hint(config.service_id())
		));

		return output;
	}

	for queued_issue in queued_candidates {
		append_rendered_queued_issue(&mut output, queued_issue, None);
	}

	output
}

pub(super) fn rendered_backlog_queue_groups(
	queued_candidates: Vec<&OperatorQueuedIssueStatus>,
) -> (Vec<&OperatorQueuedIssueStatus>, Vec<&OperatorQueuedIssueStatus>) {
	let (stale_closed_queue_labels, non_closed_queue_candidates): (Vec<_>, Vec<_>) =
		queued_candidates
			.into_iter()
			.partition(|queued_issue| queued_issue.classification == "closed");
	let backlog_candidates = non_closed_queue_candidates
		.into_iter()
		.filter(|queued_issue| {
			orchestrator::queued_candidate_counts_as_waiting_intake(queued_issue)
		})
		.collect::<Vec<_>>();

	(stale_closed_queue_labels, backlog_candidates)
}

pub(super) fn append_rendered_queued_issue_section(
	output: &mut String,
	title: &str,
	queued_issues: &[&OperatorQueuedIssueStatus],
	snapshot: &OperatorStatusSnapshot,
	show_running_owner: bool,
) {
	output.push_str(&format!("\n{title}\n"));

	if queued_issues.is_empty() {
		output.push_str("- none\n");

		if title == "Backlog" {
			output.push_str(&format!(
				"  {}\n",
				orchestrator::format_status_no_eligible_issue_hint(&snapshot.project_id)
			));
		}

		return;
	}

	for queued_issue in queued_issues {
		let running_owner = show_running_owner
			.then(|| current_lane_run_id_for_queue_candidate(queued_issue, snapshot))
			.flatten();

		append_rendered_queued_issue(output, queued_issue, running_owner);
	}
}

pub(super) fn queue_claim_belongs_to_current_lane(
	queued_issue: &OperatorQueuedIssueStatus,
	snapshot: &OperatorStatusSnapshot,
) -> bool {
	queued_issue.classification == "claimed"
		&& current_lane_run_id_for_queue_candidate(queued_issue, snapshot).is_some()
}

fn current_lane_run_id_for_queue_candidate<'a>(
	queued_issue: &OperatorQueuedIssueStatus,
	snapshot: &'a OperatorStatusSnapshot,
) -> Option<&'a str> {
	snapshot
		.current_lanes
		.iter()
		.find(|run| run.issue_id == queued_issue.issue_id && run.counts_as_running)
		.map(|run| run.run_id.as_str())
}

fn append_rendered_queued_issue(
	output: &mut String,
	queued_issue: &OperatorQueuedIssueStatus,
	current_lane_run_id: Option<&str>,
) {
	let priority =
		queued_issue.priority.map_or_else(|| String::from("none"), |value| value.to_string());
	let blockers = if queued_issue.blocker_identifiers.is_empty() {
		String::from("none")
	} else {
		queued_issue.blocker_identifiers.join(", ")
	};
	let running_owner = current_lane_run_id.unwrap_or("none");

	output.push_str(&format!(
		"- issue_id: {}\n  issue: {}\n  title: {}\n  state: {}\n  priority: {}\n  created_at: {}\n  classification: {}\n  reason: {}\n  running_owner_run: {}\n  blockers: {}\n",
		queued_issue.issue_id,
		queued_issue.issue_identifier,
		queued_issue.title,
		queued_issue.state,
		priority,
		queued_issue.created_at,
		queued_issue.classification,
		queued_issue.reason,
		running_owner,
		blockers,
	));

	if let Some(attention) = &queued_issue.attention {
		let loop_status = activity::render_loop_status_summary(attention.loop_status.as_ref());
		let loop_review = activity::render_loop_review_summary(attention.loop_status.as_ref());
		let loop_architecture_recovery =
			activity::render_loop_architecture_recovery_summary(attention.loop_status.as_ref());
		let loop_boundary = activity::render_loop_boundary_summary(attention.loop_status.as_ref());

		output.push_str(&format!(
			"  attention: {}\n  attention_run: {}\n  attention_attempt: {}\n  attention_operation: {}\n  attention_thread: {}\n  attention_cause: {}\n  attention_next_action: {}\n  attention_auto_retry: {}\n  attention_retry_budget_attempts: {}\n  attention_worktree: {}\n  attention_last_activity: {}\n  loop_status: {}\n  loop_review: {}\n  loop_architecture_recovery: {}\n  loop_boundary: {}\n",
			attention.summary,
			attention.run_id.as_deref().unwrap_or("none"),
			attention
				.attempt_number
				.map_or_else(|| String::from("none"), |value| value.to_string()),
			attention.current_operation.as_deref().unwrap_or("none"),
			attention.thread_status.as_deref().unwrap_or("none"),
			attention.attention_error_class.as_deref().unwrap_or("none"),
			attention.attention_next_action.as_deref().unwrap_or("none"),
			attention.auto_retry_blocked_reason.as_deref().unwrap_or("none"),
			attention
				.retry_budget_attempt_count
				.map_or_else(|| String::from("none"), |value| value.to_string()),
			attention.worktree_path.as_deref().unwrap_or("none"),
			attention.last_activity_at.as_deref().unwrap_or("none"),
			loop_status,
			loop_review,
			loop_architecture_recovery,
			loop_boundary
		));

		if let Some(decision_request) = attention.decision_request.as_ref() {
			output.push_str(&format!(
				"  decision_request_phase: {}\n  decision_request_reason: {}\n  decision_request_boundary: {}\n  decision_request_id: {}\n  decision_request_next_action: {}\n",
				decision_request.phase,
				decision_request.reason,
				decision_request.boundary,
				decision_request.decision_request_id,
				decision_request.next_action
			));
		}
	}
}
