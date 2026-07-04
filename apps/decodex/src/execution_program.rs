//! Internal Execution Program model and readiness evaluator.

mod contract;
mod evaluation;
mod intake;
mod model;
mod policy;
mod validation;

pub(crate) use self::{
	evaluation::{
		ExecutionNodeEvaluation, ExecutionProgramEvaluation, ExecutionProgramOperatorSummary,
	},
	model::{
		ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionDispatchAction,
		ExecutionLinearIssueMapping, ExecutionProgram, ExecutionProgramDependency,
		ExecutionProgramNode, ExecutionProgramNodeLifecycleState, ExecutionProgramNodeStage,
		ExecutionQueueIntent, ExecutionReadinessState,
	},
	policy::{
		ExecutionDependencySnapshot, ExecutionProgramReadinessContext, ExecutionWorkflowPolicy,
	},
};

#[cfg(test)] mod tests;
