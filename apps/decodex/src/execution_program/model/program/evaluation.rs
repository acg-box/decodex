use std::collections::{BTreeMap, HashSet};

use crate::{
	execution_program::{
		contract,
		evaluation::{self, EvaluateNodeInput, ExecutionProgramEvaluation},
		model::ExecutionProgram,
		policy::{ExecutionProgramReadinessContext, ExecutionWorkflowPolicy},
	},
	loop_contract::DecisionContract,
	prelude::{Result, eyre},
};

impl ExecutionProgram {
	/// Evaluate every node against the current contract, workflow policy, and runtime context.
	pub(crate) fn evaluate(
		&self,
		current_contract: &DecisionContract,
		policy: &ExecutionWorkflowPolicy,
		context: &ExecutionProgramReadinessContext,
	) -> Result<ExecutionProgramEvaluation> {
		if self.source_contract_id.as_deref().is_none() {
			eyre::bail!(
				"Execution program `{}` came from issue-batch intake and must be evaluated without a Decision Contract.",
				self.program_id
			);
		}

		let current_fingerprint = contract::decision_contract_fingerprint(current_contract)?;

		self.evaluate_with_authority(Some(current_contract), &current_fingerprint, policy, context)
	}

	/// Evaluate every node for an issue-batch intake program.
	pub(crate) fn evaluate_issue_batch(
		&self,
		policy: &ExecutionWorkflowPolicy,
		context: &ExecutionProgramReadinessContext,
	) -> Result<ExecutionProgramEvaluation> {
		if self.source_contract_id.as_deref().is_some() {
			eyre::bail!(
				"Execution program `{}` came from goal intake and must be evaluated with a Decision Contract.",
				self.program_id
			);
		}

		self.evaluate_with_authority(None, &self.accepted_contract_fingerprint, policy, context)
	}

	fn evaluate_with_authority(
		&self,
		current_contract: Option<&DecisionContract>,
		current_fingerprint: &str,
		policy: &ExecutionWorkflowPolicy,
		context: &ExecutionProgramReadinessContext,
	) -> Result<ExecutionProgramEvaluation> {
		self.validate()?;

		if self.service_id != policy.service_id {
			eyre::bail!(
				"Execution program `{}` belongs to service `{}` but readiness policy belongs to `{}`.",
				self.program_id,
				self.service_id,
				policy.service_id
			);
		}

		let node_lookup =
			self.nodes.iter().map(|node| (node.node_id.as_str(), node)).collect::<BTreeMap<_, _>>();
		let dependency_lookup = context.dependency_lookup();
		let occupied_conflicts = context.occupied_conflict_domains.iter().collect::<HashSet<_>>();
		let active_issue_ids =
			context.active_issue_ids.iter().map(String::as_str).collect::<HashSet<_>>();
		let mut nodes = Vec::new();

		for node in &self.nodes {
			nodes.push(evaluation::evaluate_node(EvaluateNodeInput {
				program: self,
				node,
				current_contract,
				current_fingerprint,
				policy,
				node_lookup: &node_lookup,
				dependency_lookup: &dependency_lookup,
				occupied_conflicts: &occupied_conflicts,
				active_issue_ids: &active_issue_ids,
			})?);
		}

		Ok(ExecutionProgramEvaluation { program_id: self.program_id.clone(), nodes })
	}
}
