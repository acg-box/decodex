use crate::orchestrator::RunSummary;

pub(crate) fn format_run_once_summary(summary: &RunSummary, dry_run: bool) -> String {
	if dry_run {
		return format!(
			"dry run: project={} issue={} branch={} worktree={} attempt={}",
			summary.project_id,
			summary.issue_identifier,
			summary.branch_name,
			summary.worktree_path.display(),
			summary.attempt_number
		);
	}
	if summary.continuation_pending {
		return format!(
			"run paused at continuation boundary: project={} issue={} run_id={} worktree={} next_action=rerun_or_use_daemon",
			summary.project_id,
			summary.issue_identifier,
			summary.run_id,
			summary.worktree_path.display()
		);
	}

	format!(
		"run complete: project={} issue={} run_id={} worktree={}",
		summary.project_id,
		summary.issue_identifier,
		summary.run_id,
		summary.worktree_path.display()
	)
}
