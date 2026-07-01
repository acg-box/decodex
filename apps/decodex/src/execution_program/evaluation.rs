//! Readiness evaluation and operator summaries for execution programs.

mod node;
mod summary;

pub(super) use self::node::{EvaluateNodeInput, evaluate_node};
pub(crate) use self::{
	node::ExecutionNodeEvaluation,
	summary::{ExecutionProgramEvaluation, ExecutionProgramOperatorSummary},
};
