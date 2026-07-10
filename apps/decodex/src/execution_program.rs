//! Internal Execution Program model and readiness evaluator.

mod contract;
mod evaluation;
mod intake;
mod model;
mod policy;
mod validation;

#[cfg(test)] pub(crate) use self::model::ExecutionReadinessState;
pub(crate) use self::{
	contract::decision_contract_fingerprint,
	evaluation::{
		ExecutionNodeEvaluation, ExecutionProgramEvaluation, ExecutionProgramOperatorSummary,
	},
	model::{
		ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionDispatchAction,
		ExecutionLinearIssueMapping, ExecutionProgram, ExecutionProgramDependency,
		ExecutionProgramNode, ExecutionProgramNodeLifecycleState, ExecutionProgramNodeStage,
		ExecutionQueueIntent,
	},
	policy::{
		ExecutionDependencySnapshot, ExecutionProgramReadinessContext, ExecutionWorkflowPolicy,
	},
};

#[cfg(test)] mod tests;
