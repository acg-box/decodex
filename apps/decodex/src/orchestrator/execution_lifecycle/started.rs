use crate::orchestrator::{
	self, IssueRunPlan, IssueTracker, Result, ServiceConfig, StateStore, WorkflowDocument,
	execution_lifecycle::{identity, model::RunStartedLifecycleFields, writer},
	eyre,
	records::{self, LinearExecutionEventRecord},
};

pub(crate) fn write_prepare_lifecycle_events<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let worktree_path = orchestrator::relative_worktree_path(project, &issue_run.worktree);
	let privacy_classifier =
		orchestrator::configured_public_projection_privacy_classifier(project)?;
	let commit_sha =
		orchestrator::worktree_head_oid(&issue_run.worktree.path)?.ok_or_else(|| {
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

pub(crate) fn write_run_started_lifecycle_event<T>(
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
	let mut record = LinearExecutionEventRecord::new(
		identity::lifecycle_event_identity(project, issue_run),
		"run_started",
		orchestrator::current_timestamp(),
		&anchor,
	);

	record.branch = Some(issue_run.worktree.branch_name.clone());
	record.worktree_path = Some(fields.worktree_path.to_owned());
	record.commit_sha = Some(fields.commit_sha.to_owned());
	record.transport = Some(transport.to_owned());
	record.summary =
		Some(format!("Decodex started a {} run for this issue.", issue_run.dispatch_mode.as_str()));

	writer::write_lifecycle_event(
		tracker,
		state_store,
		&issue_run.issue.id,
		&record,
		fields.privacy_classifier,
	)
}
