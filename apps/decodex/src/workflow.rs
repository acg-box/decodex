//! Downstream `WORKFLOW.md` parsing and validation.

mod context;
mod document;
mod execution;
mod frontmatter;
mod tracker;
mod validation;

#[cfg(test)] mod tests;

pub use self::{
	context::WorkflowContext,
	document::WorkflowDocument,
	execution::{
		ResolvedRepoGate, WorkflowExecution, WorkflowGateMatchMode, WorkflowGateProfile,
		WorkflowWorkspaceHooks,
	},
	frontmatter::WorkflowFrontmatter,
	tracker::{TrackerProvider, WorkflowAgent, WorkflowTracker},
};
