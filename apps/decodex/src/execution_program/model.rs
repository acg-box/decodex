//! Durable execution-program model types and constructors.

mod graph;
mod issue_mapping;
mod program;
mod state;

pub(crate) use self::{
	graph::{ExecutionConflictDomain, ExecutionProgramDependency, ExecutionProgramNode},
	issue_mapping::ExecutionLinearIssueMapping,
	program::ExecutionProgram,
	state::{
		ExecutionConflictDomainKind, ExecutionDispatchAction, ExecutionProgramNodeLifecycleState,
		ExecutionProgramNodeStage, ExecutionQueueIntent, ExecutionReadinessState,
	},
};

/// Stable schema identifier for serialized Execution Programs.
pub(crate) const EXECUTION_PROGRAM_SCHEMA: &str = "decodex.execution_program/1";
/// Stable record version for serialized Execution Programs.
pub(crate) const EXECUTION_PROGRAM_RECORD_VERSION: u16 = 1;
