use crate::orchestrator::types::{Child, IssueDispatchMode, WorkflowDocument};

pub(crate) struct DaemonRunChild {
	pub(crate) child: Child,
	pub(crate) issue_id: String,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) initial_issue_state: String,
	#[allow(dead_code)]
	#[cfg(test)]
	pub(crate) retry_project_slug: String,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) from_retry_queue: bool,
	pub(crate) workflow: WorkflowDocument,
}

#[derive(Clone, Copy)]
pub(crate) struct ChildRunRef<'a> {
	pub(crate) issue_id: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
}

#[derive(Clone, Copy)]
pub(crate) struct CurrentChildRunContext<'a> {
	pub(crate) child: ChildRunRef<'a>,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) dispatch_mode: IssueDispatchMode,
}
