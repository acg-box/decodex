use std::path::PathBuf;

use crate::{
	config::ServiceConfig,
	prelude::Result,
	state::{self, RunActivityMarker, StateStore},
	tracker::TrackerIssue,
};

pub(crate) struct OperatorQueuedIssueWorktreeContext {
	pub(crate) path: PathBuf,
	pub(crate) marker: Option<RunActivityMarker>,
	pub(crate) marker_unreadable: bool,
}

pub(crate) fn operator_queued_issue_worktree_context(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Result<OperatorQueuedIssueWorktreeContext> {
	let worktree_mapping = state_store.worktree_for_issue(&issue.id)?;
	let path = worktree_mapping
		.as_ref()
		.map(|mapping| mapping.worktree_path().to_path_buf())
		.unwrap_or_else(|| project.worktree_root().join(&issue.identifier));
	let marker = state::read_run_activity_marker_snapshot(&path).unwrap_or_default();
	let marker_unreadable = marker.is_none()
		&& matches!(path.join(state::RUN_ACTIVITY_MARKER_FILE).try_exists(), Ok(true));

	Ok(OperatorQueuedIssueWorktreeContext { path, marker, marker_unreadable })
}
