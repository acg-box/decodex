use crate::orchestrator::types::{ChildRunRef, PathBuf, WorkflowDocument};

#[derive(Clone)]
pub(crate) struct CachedWorkflowDocument {
	pub(crate) path: PathBuf,
	pub(crate) document: WorkflowDocument,
}

#[derive(Clone, Copy)]
pub(crate) struct ActiveWorkflowOverride<'a> {
	pub(crate) child: ChildRunRef<'a>,
	pub(crate) workflow: &'a WorkflowDocument,
}
