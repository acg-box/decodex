//! Versioned Execution Program aggregate.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{
	super::{
		contract::{decision_contract_fingerprint, ensure_accepted_contract},
		evaluation::{EvaluateNodeInput, ExecutionProgramEvaluation, evaluate_node},
		intake::{ProgramIntakeKind, ProgramIntakePlan},
		policy::{ExecutionProgramReadinessContext, ExecutionWorkflowPolicy},
		validation::{
			execution_program_record_version, execution_program_schema, validate_optional,
			validate_required,
		},
	},
	EXECUTION_PROGRAM_RECORD_VERSION, EXECUTION_PROGRAM_SCHEMA, ExecutionProgramNode,
};
use crate::{
	loop_contract::DecisionContract,
	prelude::{Result, eyre},
};

/// Versioned internal Execution Program derived from an accepted Decision Contract.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionProgram {
	#[serde(default = "execution_program_schema")]
	schema: String,
	#[serde(default = "execution_program_record_version")]
	record_version: u16,
	pub(in crate::execution_program) program_id: String,
	pub(in crate::execution_program) service_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::execution_program) source_contract_id: Option<String>,
	pub(in crate::execution_program) accepted_contract_fingerprint: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	program_intake_plan: Option<ProgramIntakePlan>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(in crate::execution_program) nodes: Vec<ExecutionProgramNode>,
}
impl ExecutionProgram {
	/// Build an internal Execution Program from an accepted Decision Contract.
	pub(crate) fn from_accepted_contract(
		program_id: impl Into<String>,
		service_id: impl Into<String>,
		contract: &DecisionContract,
		mut nodes: Vec<ExecutionProgramNode>,
	) -> Result<Self> {
		ensure_accepted_contract(contract)?;

		let program_id = program_id.into();
		let service_id = service_id.into();
		let fingerprint = decision_contract_fingerprint(contract)?;

		for node in &mut nodes {
			node.bind_contract_fingerprint(&fingerprint);
		}

		let program = Self {
			schema: execution_program_schema(),
			record_version: EXECUTION_PROGRAM_RECORD_VERSION,
			program_id: program_id.clone(),
			service_id: service_id.clone(),
			source_contract_id: Some(contract.contract_id().to_owned()),
			accepted_contract_fingerprint: fingerprint.clone(),
			program_intake_plan: Some(ProgramIntakePlan::goal_intake(
				program_id,
				service_id,
				contract,
				fingerprint.clone(),
			)?),
			nodes,
		};

		program.validate()?;

		Ok(program)
	}

	/// Build an internal Execution Program from an accepted issue-batch intake boundary.
	pub(crate) fn from_issue_batch_intake(
		program_id: impl Into<String>,
		service_id: impl Into<String>,
		accepted_batch_fingerprint: impl Into<String>,
		public_summary: impl Into<String>,
		mut nodes: Vec<ExecutionProgramNode>,
	) -> Result<Self> {
		let program_id = program_id.into();
		let service_id = service_id.into();
		let fingerprint = accepted_batch_fingerprint.into();

		for node in &mut nodes {
			node.bind_contract_fingerprint(&fingerprint);
		}

		let program = Self {
			schema: execution_program_schema(),
			record_version: EXECUTION_PROGRAM_RECORD_VERSION,
			program_id: program_id.clone(),
			service_id: service_id.clone(),
			source_contract_id: None,
			accepted_contract_fingerprint: fingerprint.clone(),
			program_intake_plan: Some(ProgramIntakePlan::issue_batch_intake(
				program_id,
				service_id,
				fingerprint,
				public_summary,
			)?),
			nodes,
		};

		program.validate()?;

		Ok(program)
	}

	/// Stable internal program id.
	pub(crate) fn program_id(&self) -> &str {
		&self.program_id
	}

	/// Service id that owns queue-label decisions.
	pub(crate) fn service_id(&self) -> &str {
		&self.service_id
	}

	/// Accepted Decision Contract id that authorized this program, for goal intake.
	pub(crate) fn source_contract_id(&self) -> Option<&str> {
		self.source_contract_id.as_deref()
	}

	/// Stable authority fingerprint for this program.
	pub(crate) fn accepted_contract_fingerprint(&self) -> &str {
		&self.accepted_contract_fingerprint
	}

	/// Durable program-intake plan metadata, when the payload is not a legacy row.
	pub(crate) fn program_intake_plan(&self) -> Option<&ProgramIntakePlan> {
		self.program_intake_plan.as_ref()
	}

	/// Program nodes.
	pub(crate) fn nodes(&self) -> &[ExecutionProgramNode] {
		&self.nodes
	}

	/// Replace program nodes after runtime reconciliation refreshes tracker issue facts.
	pub(crate) fn with_nodes(mut self, nodes: Vec<ExecutionProgramNode>) -> Result<Self> {
		self.nodes = nodes;

		self.validate()?;

		Ok(self)
	}

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

		let current_fingerprint = decision_contract_fingerprint(current_contract)?;

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
			nodes.push(evaluate_node(EvaluateNodeInput {
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

	/// Validate the serialized program payload.
	pub(crate) fn validate(&self) -> Result<()> {
		validate_required("execution program schema", &self.schema)?;
		validate_required("execution program program_id", &self.program_id)?;
		validate_required("execution program service_id", &self.service_id)?;
		validate_optional(
			"execution program source_contract_id",
			self.source_contract_id.as_deref(),
		)?;
		validate_required(
			"execution program accepted_contract_fingerprint",
			&self.accepted_contract_fingerprint,
		)?;

		if self.schema != EXECUTION_PROGRAM_SCHEMA {
			eyre::bail!(
				"Execution program `{}` has unsupported schema `{}`.",
				self.program_id,
				self.schema
			);
		}
		if self.record_version != EXECUTION_PROGRAM_RECORD_VERSION {
			eyre::bail!(
				"Execution program `{}` has unsupported record_version `{}`.",
				self.program_id,
				self.record_version
			);
		}

		if let Some(plan) = &self.program_intake_plan {
			plan.validate()?;

			if plan.service_id != self.service_id {
				eyre::bail!(
					"Execution program `{}` belongs to service `{}` but intake plan belongs to `{}`.",
					self.program_id,
					self.service_id,
					plan.service_id
				);
			}

			if let Some(source_contract_id) = plan.source_contract_id()
				&& Some(source_contract_id) != self.source_contract_id.as_deref()
			{
				eyre::bail!(
					"Execution program `{}` belongs to source contract `{}` but intake plan belongs to `{}`.",
					self.program_id,
					self.source_contract_id.as_deref().unwrap_or("none"),
					source_contract_id
				);
			}

			if plan.intake_kind == ProgramIntakeKind::GoalIntake
				&& self.source_contract_id.as_deref().is_none_or(str::is_empty)
			{
				eyre::bail!(
					"Goal intake execution program `{}` must reference a source contract.",
					self.program_id
				);
			}
			if plan.intake_kind == ProgramIntakeKind::IssueBatchIntake
				&& self.source_contract_id.as_deref().is_some_and(|id| !id.is_empty())
			{
				eyre::bail!(
					"Issue-batch execution program `{}` must not reference a source contract.",
					self.program_id
				);
			}
			if plan.accepted_contract_fingerprint != self.accepted_contract_fingerprint {
				eyre::bail!(
					"Execution program `{}` has an intake plan fingerprint mismatch.",
					self.program_id
				);
			}
		}

		let mut node_ids = HashSet::new();

		for node in &self.nodes {
			node.validate()?;

			if !node_ids.insert(node.node_id.as_str()) {
				eyre::bail!(
					"Execution program `{}` contains duplicate node `{}`.",
					self.program_id,
					node.node_id
				);
			}
		}

		if self.program_intake_plan.is_none() && self.source_contract_id.as_deref().is_none() {
			eyre::bail!(
				"Execution program `{}` without a source contract must carry an issue-batch intake plan.",
				self.program_id
			);
		}

		Ok(())
	}
}
