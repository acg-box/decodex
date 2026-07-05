use crate::orchestrator::{
	self, IssueRunPlan,
	execution_lifecycle::model::TerminalFailureLifecycle,
	records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
};

pub(crate) fn terminal_failure_lifecycle_event(
	service_id: &str,
	issue_run: &IssueRunPlan,
	failure: TerminalFailureLifecycle<'_>,
) -> LinearExecutionEventRecord {
	let retained_partial_progress = failure.error_class == "partial_progress_retained";
	let event_type = if failure.manual_attention_requested || retained_partial_progress {
		"needs_attention"
	} else {
		"terminal_failure"
	};
	let anchor =
		records::stable_event_anchor(&[event_type, failure.error_class, failure.target_state]);
	let mut record = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id,
			issue_id: &issue_run.issue.id,
			issue_identifier: &issue_run.issue.identifier,
			run_id: &issue_run.run_id,
			attempt_number: issue_run.attempt_number,
		},
		event_type,
		orchestrator::current_timestamp(),
		&anchor,
	);

	record.branch = Some(issue_run.worktree.branch_name.clone());
	record.worktree_path = Some(failure.worktree_path.to_owned());
	record.error_class = Some(failure.error_class.to_owned());
	record.next_action = Some(failure.next_action.to_owned());

	if retained_partial_progress {
		let mut evidence = vec![format!(
			"Attempt {} stopped with tracked worktree changes retained.",
			issue_run.attempt_number
		)];

		if let Some(source_error_class) = failure.retained_source_error_class {
			evidence.push(format!(
				"Source failure class `{source_error_class}` was preserved for recovery context."
			));
		}

		record.blockers = Some(vec![String::from(
			"Retained tracked worktree changes require operator recovery.",
		)]);
		record.evidence = Some(evidence);
		record.summary =
			Some(String::from("Decodex retained partial progress and needs attention."));
		record.terminal_path = Some(String::from("retained_partial_progress"));
	} else {
		record.blockers = Some(vec![format!("Run failed with `{}`.", failure.error_class)]);
		record.evidence = Some(vec![format!(
			"Attempt {} reached terminal failure handling.",
			issue_run.attempt_number
		)]);
		record.summary = Some(String::from("Decodex run failed and needs attention."));
	}

	record.pr_url = failure.pr_url.map(ToOwned::to_owned);
	record.target_state = Some(failure.target_state.to_owned());

	if failure.manual_attention_requested {
		record.terminal_path = Some(String::from("manual_attention"));
	}

	record
}
