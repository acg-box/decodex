use std::collections::{BTreeMap, HashSet};

use crate::{
	execution_program::{
		model::{
			ExecutionConflictDomain, ExecutionDispatchAction, ExecutionLinearIssueMapping,
			ExecutionProgram, ExecutionProgramNode, ExecutionProgramNodeLifecycleState,
			ExecutionProgramNodeStage, ExecutionReadinessState,
		},
		policy::{ExecutionDependencySnapshot, ExecutionWorkflowPolicy},
	},
	loop_contract::DecisionContract,
};

/// Readiness result for one program node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionNodeEvaluation {
	pub(in crate::execution_program::evaluation) node_id: String,
	pub(in crate::execution_program::evaluation) stage: ExecutionProgramNodeStage,
	pub(in crate::execution_program::evaluation) state: ExecutionReadinessState,
	pub(in crate::execution_program::evaluation) lifecycle_state:
		ExecutionProgramNodeLifecycleState,
	pub(in crate::execution_program::evaluation) reasons: Vec<String>,
	pub(in crate::execution_program::evaluation) dispatch_action: Option<ExecutionDispatchAction>,
	pub(in crate::execution_program::evaluation) linear_issue: Option<ExecutionLinearIssueMapping>,
}
impl ExecutionNodeEvaluation {
	/// Node id.
	pub(crate) fn node_id(&self) -> &str {
		&self.node_id
	}

	/// Program node stage.
	pub(crate) fn stage(&self) -> ExecutionProgramNodeStage {
		self.stage
	}

	/// Normalized readiness state.
	pub(crate) fn state(&self) -> ExecutionReadinessState {
		self.state
	}

	/// Durable lifecycle state used for operator program-intake readback.
	pub(crate) fn lifecycle_state(&self) -> ExecutionProgramNodeLifecycleState {
		self.lifecycle_state
	}

	/// Human-readable readiness reasons.
	pub(crate) fn reasons(&self) -> &[String] {
		&self.reasons
	}

	/// Direct dispatch action, if any.
	pub(crate) fn dispatch_action(&self) -> Option<ExecutionDispatchAction> {
		self.dispatch_action
	}

	/// Whether this node can be dispatched directly by the program scheduler.
	pub(crate) fn dispatchable(&self) -> bool {
		matches!(self.dispatch_action, Some(ExecutionDispatchAction::Dispatch))
	}

	/// Mapped Linear issue, when present.
	pub(crate) fn linear_issue(&self) -> Option<&ExecutionLinearIssueMapping> {
		self.linear_issue.as_ref()
	}
}

pub(crate) struct EvaluateNodeInput<'a> {
	pub(crate) program: &'a ExecutionProgram,
	pub(crate) node: &'a ExecutionProgramNode,
	pub(crate) current_contract: Option<&'a DecisionContract>,
	pub(crate) current_fingerprint: &'a str,
	pub(crate) policy: &'a ExecutionWorkflowPolicy,
	pub(crate) node_lookup: &'a BTreeMap<&'a str, &'a ExecutionProgramNode>,
	pub(crate) dependency_lookup: &'a BTreeMap<&'a str, &'a ExecutionDependencySnapshot>,
	pub(crate) occupied_conflicts: &'a HashSet<&'a ExecutionConflictDomain>,
	pub(crate) active_issue_ids: &'a HashSet<&'a str>,
}
