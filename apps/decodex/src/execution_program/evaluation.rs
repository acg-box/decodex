//! Readiness evaluation and operator summaries for execution programs.

mod node;
mod summary;
pub(crate) use self::{
	node::{EvaluateNodeInput, ExecutionNodeEvaluation, evaluate_node},
	summary::{ExecutionProgramEvaluation, ExecutionProgramOperatorSummary},
};
