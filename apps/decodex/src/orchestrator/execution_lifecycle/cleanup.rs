use crate::orchestrator::{
	self, IssueRunPlan, IssueTracker, Result, ServiceConfig, StateStore,
	execution_lifecycle::{identity, writer},
	records::{self, LinearExecutionEventRecord},
};

pub(crate) fn write_cleanup_complete_lifecycle_event<T>(
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
	let worktree_path = orchestrator::relative_worktree_path(project, &issue_run.worktree);
	let privacy_classifier =
		orchestrator::configured_public_projection_privacy_classifier(project)?;
	let anchor = records::stable_event_anchor(&[
		&issue_run.worktree.branch_name,
		commit_sha.unwrap_or_default(),
		"cleanup_complete",
	]);
	let mut record = LinearExecutionEventRecord::new(
		identity::lifecycle_event_identity(project, issue_run),
		"cleanup_complete",
		orchestrator::current_timestamp(),
		&anchor,
	);

	record.branch = Some(issue_run.worktree.branch_name.clone());
	record.worktree_path = Some(worktree_path);
	record.cleanup_status = Some(String::from("completed"));
	record.summary = Some(String::from("Decodex cleaned up the retained lane worktree."));
	record.pr_url = pr_url.map(ToOwned::to_owned);
	record.commit_sha = commit_sha.map(ToOwned::to_owned);

	writer::write_lifecycle_event(
		tracker,
		state_store,
		project.service_id(),
		&issue_run.issue.id,
		&record,
		&privacy_classifier,
	)
}
