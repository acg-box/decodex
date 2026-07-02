use crate::orchestrator::*;
use crate::tracker;

pub(in crate::orchestrator) struct TerminalFailureLifecycle<'a> {
	pub(in crate::orchestrator) error_class: &'a str,
	pub(in crate::orchestrator) next_action: &'a str,
	pub(in crate::orchestrator) pr_url: Option<&'a str>,
	pub(in crate::orchestrator) target_state: &'a str,
	pub(in crate::orchestrator) worktree_path: &'a str,
	pub(in crate::orchestrator) manual_attention_requested: bool,
	pub(in crate::orchestrator) retained_source_error_class: Option<&'a str>,
}

pub(in crate::orchestrator) struct RunStartedLifecycleFields<'a> {
	pub(in crate::orchestrator) worktree_path: &'a str,
	pub(in crate::orchestrator) commit_sha: &'a str,
	pub(in crate::orchestrator) privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
}

pub(in crate::orchestrator) fn lifecycle_event_identity<'a>(
	project: &'a ServiceConfig,
	issue_run: &'a IssueRunPlan,
) -> records::LinearExecutionEventIdentity<'a> {
	records::LinearExecutionEventIdentity {
		service_id: project.service_id(),
		issue_id: &issue_run.issue.id,
		issue_identifier: &issue_run.issue.identifier,
		run_id: &issue_run.run_id,
		attempt_number: issue_run.attempt_number,
	}
}

pub(in crate::orchestrator) fn write_lifecycle_event<T>(
	tracker: &T,
	state_store: &StateStore,
	issue_id: &str,
	record: &records::LinearExecutionEventRecord,
	privacy_classifier: &dyn PublicProjectionPrivacyClassifier,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let retry_budget_attempt_count = state_store.retry_budget_attempt_count(issue_id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body =
		records::render_linear_execution_event_comment_body(record, retry_budget_attempt_count);
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, record, privacy_classifier)?;

	if state_store.record_linear_execution_event(&projection.record)?
		&& let Err(error) =
			tracker::create_prepared_linear_execution_event_comment_without_remote_scan(
				tracker,
				issue_id,
				&projection,
			) {
		state_store.forget_linear_execution_event(&projection.record.idempotency_key)?;

		return Err(error);
	}

	Ok(())
}

pub(in crate::orchestrator) fn write_prepare_lifecycle_events<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let worktree_path = relative_worktree_path(project, &issue_run.worktree);
	let privacy_classifier = configured_public_projection_privacy_classifier(project)?;
	let commit_sha = worktree_head_oid(&issue_run.worktree.path)?.ok_or_else(|| {
		eyre::eyre!(
			"Prepared worktree `{}` for issue `{}` did not expose a HEAD commit.",
			issue_run.worktree.path.display(),
			issue_run.issue.identifier
		)
	})?;

	write_run_started_lifecycle_event(
		tracker,
		project,
		workflow,
		state_store,
		issue_run,
		RunStartedLifecycleFields {
			worktree_path: &worktree_path,
			commit_sha: &commit_sha,
			privacy_classifier: &privacy_classifier,
		},
	)
}

pub(in crate::orchestrator) fn write_run_started_lifecycle_event<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	fields: RunStartedLifecycleFields<'_>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let transport = workflow.frontmatter().agent().transport();
	let anchor = records::stable_event_anchor(&[
		issue_run.dispatch_mode.as_str(),
		&issue_run.worktree.branch_name,
		fields.commit_sha,
		transport,
	]);
	let mut record = records::LinearExecutionEventRecord::new(
		lifecycle_event_identity(project, issue_run),
		"run_started",
		current_timestamp(),
		&anchor,
	);

	record.branch = Some(issue_run.worktree.branch_name.clone());
	record.worktree_path = Some(fields.worktree_path.to_owned());
	record.commit_sha = Some(fields.commit_sha.to_owned());
	record.transport = Some(transport.to_owned());
	record.summary =
		Some(format!("Decodex started a {} run for this issue.", issue_run.dispatch_mode.as_str()));

	write_lifecycle_event(
		tracker,
		state_store,
		&issue_run.issue.id,
		&record,
		fields.privacy_classifier,
	)
}

pub(in crate::orchestrator) fn terminal_failure_lifecycle_event(
	service_id: &str,
	issue_run: &IssueRunPlan,
	failure: TerminalFailureLifecycle<'_>,
) -> records::LinearExecutionEventRecord {
	let retained_partial_progress = failure.error_class == "partial_progress_retained";
	let event_type = if failure.manual_attention_requested || retained_partial_progress {
		"needs_attention"
	} else {
		"terminal_failure"
	};
	let anchor =
		records::stable_event_anchor(&[event_type, failure.error_class, failure.target_state]);
	let mut record = records::LinearExecutionEventRecord::new(
		records::LinearExecutionEventIdentity {
			service_id,
			issue_id: &issue_run.issue.id,
			issue_identifier: &issue_run.issue.identifier,
			run_id: &issue_run.run_id,
			attempt_number: issue_run.attempt_number,
		},
		event_type,
		current_timestamp(),
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

pub(in crate::orchestrator) fn write_cleanup_complete_lifecycle_event<T>(
	tracker: &T,
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	pr_url: Option<&str>,
	commit_sha: Option<&str>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let worktree_path = relative_worktree_path(project, &issue_run.worktree);
	let privacy_classifier = configured_public_projection_privacy_classifier(project)?;
	let anchor = records::stable_event_anchor(&[
		&issue_run.worktree.branch_name,
		commit_sha.unwrap_or_default(),
		"cleanup_complete",
	]);
	let mut record = records::LinearExecutionEventRecord::new(
		lifecycle_event_identity(project, issue_run),
		"cleanup_complete",
		current_timestamp(),
		&anchor,
	);

	record.branch = Some(issue_run.worktree.branch_name.clone());
	record.worktree_path = Some(worktree_path);
	record.cleanup_status = Some(String::from("completed"));
	record.summary = Some(String::from("Decodex cleaned up the retained lane worktree."));
	record.pr_url = pr_url.map(ToOwned::to_owned);
	record.commit_sha = commit_sha.map(ToOwned::to_owned);

	write_lifecycle_event(tracker, state_store, &issue_run.issue.id, &record, &privacy_classifier)
}
