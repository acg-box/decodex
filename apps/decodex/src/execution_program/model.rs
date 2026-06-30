//! Durable execution-program model types and constructors.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{
	contract::{decision_contract_fingerprint, ensure_accepted_contract},
	evaluation::{EvaluateNodeInput, ExecutionProgramEvaluation, evaluate_node},
	intake::{ProgramIntakeKind, ProgramIntakePlan},
	policy::{ExecutionProgramReadinessContext, ExecutionWorkflowPolicy},
	validation::{
		execution_program_record_version, execution_program_schema, is_false, non_empty_optional,
		validate_optional, validate_required, validate_string_list,
	},
};
use crate::{
	loop_contract::DecisionContract,
	prelude::{Result, eyre},
};

/// Stable schema identifier for serialized Execution Programs.
pub(crate) const EXECUTION_PROGRAM_SCHEMA: &str = "decodex.execution_program/1";
/// Stable record version for serialized Execution Programs.
pub(crate) const EXECUTION_PROGRAM_RECORD_VERSION: u16 = 1;

/// Stage for one internal Execution Program node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionProgramNodeStage {
	/// Research or evidence-gathering work.
	Research,
	/// Design or architecture-shaping work.
	Design,
	/// Normative specification work.
	Spec,
	/// Runtime schema, storage, or serialization work.
	Schema,
	/// Runtime implementation work.
	Runtime,
	/// Agent/plugin skill or integration work.
	Plugin,
	/// Evaluation, harness, or validation work.
	Eval,
	/// Review, PR, delivery, or handoff work.
	Handoff,
}
impl ExecutionProgramNodeStage {
	/// Stable machine-readable stage name.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Research => "research",
			Self::Design => "design",
			Self::Spec => "spec",
			Self::Schema => "schema",
			Self::Runtime => "runtime",
			Self::Plugin => "plugin",
			Self::Eval => "eval",
			Self::Handoff => "handoff",
		}
	}
}

/// Dispatch intent for one internal Execution Program node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionQueueIntent {
	/// The node is intentionally not ready for dispatch.
	NotReady,
	/// The node is ready for direct dispatch once mapped to a startable issue.
	ReadyToQueue,
	/// The node is retained in a ready-to-dispatch position.
	Queued,
	/// The node is already active in a lane.
	Active,
	/// The node is intentionally paused.
	Paused,
	/// The node is complete.
	Done,
	/// The node was canceled.
	Canceled,
}
impl ExecutionQueueIntent {
	/// Stable machine-readable dispatch-intent name.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::NotReady => "not_ready",
			Self::ReadyToQueue => "ready_to_queue",
			Self::Queued => "queued",
			Self::Active => "active",
			Self::Paused => "paused",
			Self::Done => "done",
			Self::Canceled => "canceled",
		}
	}

	pub(super) fn is_terminal(self) -> bool {
		matches!(self, Self::Done | Self::Canceled)
	}
}

/// Conflict-domain class for one program node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionConflictDomainKind {
	/// A concrete file or path family.
	File,
	/// A module, crate, package, or app surface.
	Module,
	/// Local runtime or repository state.
	State,
	/// Credential, account, or auth-owned surface.
	Credentials,
	/// Tracker ownership, labels, comments, or workflow state.
	TrackerOwnership,
	/// Pull request, review, or landing surface.
	ReviewSurface,
}
impl ExecutionConflictDomainKind {
	/// Stable machine-readable conflict-domain class name.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::File => "file",
			Self::Module => "module",
			Self::State => "state",
			Self::Credentials => "credentials",
			Self::TrackerOwnership => "tracker_ownership",
			Self::ReviewSurface => "review_surface",
		}
	}
}

/// Normalized readiness state for one program node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionReadinessState {
	/// Node is intentionally not ready yet.
	NotReady,
	/// Node is startable and may be dispatched directly.
	Ready,
	/// Node cannot start until a concrete blocker clears.
	Blocked,
	/// Node is intentionally paused.
	Paused,
	/// Node is already active.
	Active,
	/// Node is terminal.
	Completed,
	/// Node no longer matches the accepted contract.
	Stale,
}
impl ExecutionReadinessState {
	/// Stable machine-readable state name.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::NotReady => "not_ready",
			Self::Ready => "ready",
			Self::Blocked => "blocked",
			Self::Paused => "paused",
			Self::Active => "active",
			Self::Completed => "completed",
			Self::Stale => "stale",
		}
	}
}

/// Durable lifecycle state for one program node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionProgramNodeLifecycleState {
	/// Node exists only as an internal plan and has no normal Linear issue yet.
	Planned,
	/// Node is mapped to a normal Linear issue but is intentionally held.
	Mapped,
	/// Node is ready for direct dispatch.
	Ready,
	/// Node was retained in a ready-to-dispatch position.
	Queued,
	/// Node already has a current lane.
	Active,
	/// Node is owned by a retained post-review lane.
	PostReview,
	/// Node is blocked by dependency, conflict, issue, or briefing evidence.
	Blocked,
	/// Node is stopped on human-required issue attention.
	NeedsAttention,
	/// Node is terminal.
	Completed,
	/// Node no longer matches the accepted contract.
	Stale,
	/// Node belongs to a superseded contract.
	Superseded,
}
impl ExecutionProgramNodeLifecycleState {
	/// Stable machine-readable state name.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Planned => "planned",
			Self::Mapped => "mapped",
			Self::Ready => "ready",
			Self::Queued => "queued",
			Self::Active => "active",
			Self::PostReview => "post_review",
			Self::Blocked => "blocked",
			Self::NeedsAttention => "needs_attention",
			Self::Completed => "completed",
			Self::Stale => "stale",
			Self::Superseded => "superseded",
		}
	}
}

/// Direct dispatch action allowed for a mapped node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionDispatchAction {
	/// Start this mapped node directly from the Execution Program scheduler.
	Dispatch,
}
impl ExecutionDispatchAction {
	/// Stable machine-readable direct-dispatch action.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Dispatch => "dispatch",
		}
	}
}

/// Conflict-domain key for one program node.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionConflictDomain {
	pub(super) kind: ExecutionConflictDomainKind,
	key: String,
}
impl ExecutionConflictDomain {
	/// Build a conflict-domain key.
	pub(crate) fn new(kind: ExecutionConflictDomainKind, key: impl Into<String>) -> Result<Self> {
		let domain = Self { kind, key: key.into() };

		domain.validate()?;

		Ok(domain)
	}

	/// Stable conflict-domain key.
	pub(crate) fn key(&self) -> &str {
		&self.key
	}

	/// Stable conflict-domain kind.
	pub(crate) fn kind(&self) -> ExecutionConflictDomainKind {
		self.kind
	}

	pub(super) fn validate(&self) -> Result<()> {
		validate_required("execution program conflict_domain.key", &self.key)
	}
}

/// Dependency edge for one program node.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionProgramDependency {
	pub(super) dependency_id: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(super) required_terminal_states: Vec<String>,
}
impl ExecutionProgramDependency {
	/// Build a dependency edge using the registered workflow terminal states.
	pub(crate) fn new(dependency_id: impl Into<String>) -> Result<Self> {
		let dependency =
			Self { dependency_id: dependency_id.into(), required_terminal_states: Vec::new() };

		dependency.validate()?;

		Ok(dependency)
	}

	/// Override the terminal tracker states that satisfy this dependency.
	pub(crate) fn with_required_terminal_states(
		mut self,
		states: impl IntoIterator<Item = impl Into<String>>,
	) -> Result<Self> {
		self.required_terminal_states = states.into_iter().map(Into::into).collect();

		self.validate()?;

		Ok(self)
	}

	/// Dependency node or issue identifier.
	pub(crate) fn dependency_id(&self) -> &str {
		&self.dependency_id
	}

	pub(super) fn validate(&self) -> Result<()> {
		validate_required("execution program dependency.dependency_id", &self.dependency_id)?;

		validate_string_list(
			"execution program dependency.required_terminal_states",
			&self.required_terminal_states,
		)
	}
}

/// Normal Linear issue mapping for an executable program node.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionLinearIssueMapping {
	pub(super) issue_id: String,
	pub(super) issue_identifier: String,
	pub(super) issue_state: String,
	pub(super) has_active_label: bool,
	pub(super) has_opt_out_label: bool,
	pub(super) has_needs_attention_label: bool,
	#[serde(default, skip_serializing_if = "is_false")]
	pub(super) has_open_tracker_blockers: bool,
	pub(super) has_generic_dispatch_briefing: bool,
	#[serde(default, skip_serializing_if = "is_false")]
	pub(super) has_post_review_lifecycle: bool,
}
impl ExecutionLinearIssueMapping {
	/// Build a Linear issue mapping with no automation labels and a generic dispatch briefing.
	pub(crate) fn new(
		issue_id: impl Into<String>,
		issue_identifier: impl Into<String>,
		issue_state: impl Into<String>,
	) -> Result<Self> {
		let mapping = Self {
			issue_id: issue_id.into(),
			issue_identifier: issue_identifier.into(),
			issue_state: issue_state.into(),
			has_active_label: false,
			has_opt_out_label: false,
			has_needs_attention_label: false,
			has_open_tracker_blockers: false,
			has_generic_dispatch_briefing: true,
			has_post_review_lifecycle: false,
		};

		mapping.validate()?;

		Ok(mapping)
	}

	/// Mark whether the issue currently carries the service active label.
	pub(crate) fn with_active_label(mut self, present: bool) -> Self {
		self.has_active_label = present;

		self
	}

	/// Mark whether the issue currently carries the opt-out label.
	pub(crate) fn with_opt_out_label(mut self, present: bool) -> Self {
		self.has_opt_out_label = present;

		self
	}

	/// Mark whether the issue currently carries the needs-attention label.
	pub(crate) fn with_needs_attention_label(mut self, present: bool) -> Self {
		self.has_needs_attention_label = present;

		self
	}

	/// Mark whether the mapped issue currently has open tracker dependency blockers.
	pub(crate) fn with_open_tracker_blockers(mut self, present: bool) -> Self {
		self.has_open_tracker_blockers = present;

		self
	}

	/// Mark whether the issue description remains a generic dispatch briefing.
	pub(crate) fn with_generic_dispatch_briefing(mut self, present: bool) -> Self {
		self.has_generic_dispatch_briefing = present;

		self
	}

	/// Mark whether the mapped issue is owned by the retained post-review lifecycle.
	pub(crate) fn with_post_review_lifecycle(mut self, present: bool) -> Self {
		self.has_post_review_lifecycle = present;

		self
	}

	/// Linear issue identifier such as `XY-853`.
	pub(crate) fn issue_identifier(&self) -> &str {
		&self.issue_identifier
	}

	/// Linear issue id used by tracker APIs.
	pub(crate) fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Tracker workflow state for the mapped issue.
	pub(crate) fn issue_state(&self) -> &str {
		&self.issue_state
	}

	/// Whether the service active label is currently present.
	pub(crate) fn has_active_label(&self) -> bool {
		self.has_active_label
	}

	/// Whether the configured opt-out label is currently present.
	pub(crate) fn has_opt_out_label(&self) -> bool {
		self.has_opt_out_label
	}

	/// Whether the configured human-attention label is currently present.
	pub(crate) fn has_needs_attention_label(&self) -> bool {
		self.has_needs_attention_label
	}

	/// Whether the mapped issue currently has open dependency blockers in the tracker.
	pub(crate) fn has_open_tracker_blockers(&self) -> bool {
		self.has_open_tracker_blockers
	}

	/// Whether the issue description is usable as a generic dispatch briefing.
	pub(crate) fn has_generic_dispatch_briefing(&self) -> bool {
		self.has_generic_dispatch_briefing
	}

	/// Whether Review & Landing currently owns the mapped issue.
	pub(crate) fn has_post_review_lifecycle(&self) -> bool {
		self.has_post_review_lifecycle
	}

	pub(super) fn validate(&self) -> Result<()> {
		validate_required("execution program issue_mapping.issue_id", &self.issue_id)?;
		validate_required(
			"execution program issue_mapping.issue_identifier",
			&self.issue_identifier,
		)?;
		validate_required("execution program issue_mapping.issue_state", &self.issue_state)?;

		Ok(())
	}
}

/// Internal node in an Execution Program.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionProgramNode {
	pub(super) node_id: String,
	pub(super) stage: ExecutionProgramNodeStage,
	objective: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	objective_lineage: Vec<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(super) dependencies: Vec<ExecutionProgramDependency>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(super) conflict_domains: Vec<ExecutionConflictDomain>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(super) acceptance_expectations: Vec<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(super) validation_expectations: Vec<String>,
	pub(super) queue_intent: ExecutionQueueIntent,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) linear_issue: Option<ExecutionLinearIssueMapping>,
	pub(super) contract_fingerprint: String,
}
impl ExecutionProgramNode {
	/// Build a program node.
	pub(crate) fn new(
		node_id: impl Into<String>,
		stage: ExecutionProgramNodeStage,
		objective: impl Into<String>,
		queue_intent: ExecutionQueueIntent,
	) -> Result<Self> {
		let node = Self {
			node_id: node_id.into(),
			stage,
			objective: objective.into(),
			objective_lineage: Vec::new(),
			dependencies: Vec::new(),
			conflict_domains: Vec::new(),
			acceptance_expectations: Vec::new(),
			validation_expectations: Vec::new(),
			queue_intent,
			linear_issue: None,
			contract_fingerprint: String::new(),
		};

		node.validate()?;

		Ok(node)
	}

	/// Add objective-lineage text from the accepted contract.
	pub(crate) fn with_objective_lineage(
		mut self,
		lineage: impl IntoIterator<Item = impl Into<String>>,
	) -> Result<Self> {
		self.objective_lineage = lineage.into_iter().map(Into::into).collect();

		self.validate()?;

		Ok(self)
	}

	/// Add dependencies.
	pub(crate) fn with_dependencies(
		mut self,
		dependencies: impl IntoIterator<Item = ExecutionProgramDependency>,
	) -> Result<Self> {
		self.dependencies = dependencies.into_iter().collect();

		self.validate()?;

		Ok(self)
	}

	/// Add conflict domains.
	pub(crate) fn with_conflict_domains(
		mut self,
		conflict_domains: impl IntoIterator<Item = ExecutionConflictDomain>,
	) -> Result<Self> {
		self.conflict_domains = conflict_domains.into_iter().collect();

		self.validate()?;

		Ok(self)
	}

	/// Add acceptance expectations.
	pub(crate) fn with_acceptance_expectations(
		mut self,
		expectations: impl IntoIterator<Item = impl Into<String>>,
	) -> Result<Self> {
		self.acceptance_expectations = expectations.into_iter().map(Into::into).collect();

		self.validate()?;

		Ok(self)
	}

	/// Add validation expectations.
	pub(crate) fn with_validation_expectations(
		mut self,
		expectations: impl IntoIterator<Item = impl Into<String>>,
	) -> Result<Self> {
		self.validation_expectations = expectations.into_iter().map(Into::into).collect();

		self.validate()?;

		Ok(self)
	}

	/// Link the node to a normal Linear issue.
	pub(crate) fn with_linear_issue(mut self, issue: ExecutionLinearIssueMapping) -> Result<Self> {
		self.linear_issue = Some(issue);

		self.validate()?;

		Ok(self)
	}

	/// Override the accepted-contract fingerprint used for drift detection.
	pub(crate) fn with_contract_fingerprint(
		mut self,
		fingerprint: impl Into<String>,
	) -> Result<Self> {
		self.contract_fingerprint = fingerprint.into();

		self.validate()?;

		Ok(self)
	}

	/// Stable internal node id.
	pub(crate) fn node_id(&self) -> &str {
		&self.node_id
	}

	/// Node execution stage.
	pub(crate) fn stage(&self) -> ExecutionProgramNodeStage {
		self.stage
	}

	/// Node queue intent.
	pub(crate) fn queue_intent(&self) -> ExecutionQueueIntent {
		self.queue_intent
	}

	/// Conflict domains occupied by this node.
	pub(crate) fn conflict_domains(&self) -> &[ExecutionConflictDomain] {
		&self.conflict_domains
	}

	/// Linked normal Linear issue, when the node is executable.
	pub(crate) fn linear_issue(&self) -> Option<&ExecutionLinearIssueMapping> {
		self.linear_issue.as_ref()
	}

	pub(super) fn bind_contract_fingerprint(&mut self, fingerprint: &str) {
		if self.contract_fingerprint.is_empty() {
			self.contract_fingerprint = fingerprint.to_owned();
		}
	}

	pub(super) fn validate(&self) -> Result<()> {
		validate_required("execution program node.node_id", &self.node_id)?;
		validate_required("execution program node.objective", &self.objective)?;
		validate_string_list("execution program node.objective_lineage", &self.objective_lineage)?;
		validate_string_list(
			"execution program node.acceptance_expectations",
			&self.acceptance_expectations,
		)?;
		validate_string_list(
			"execution program node.validation_expectations",
			&self.validation_expectations,
		)?;
		validate_optional(
			"execution program node.contract_fingerprint",
			non_empty_optional(&self.contract_fingerprint),
		)?;

		for dependency in &self.dependencies {
			dependency.validate()?;
		}
		for domain in &self.conflict_domains {
			domain.validate()?;
		}

		if let Some(issue) = &self.linear_issue {
			issue.validate()?;
		}

		Ok(())
	}
}

/// Versioned internal Execution Program derived from an accepted Decision Contract.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionProgram {
	#[serde(default = "execution_program_schema")]
	schema: String,
	#[serde(default = "execution_program_record_version")]
	record_version: u16,
	pub(super) program_id: String,
	pub(super) service_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) source_contract_id: Option<String>,
	pub(super) accepted_contract_fingerprint: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	program_intake_plan: Option<ProgramIntakePlan>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(super) nodes: Vec<ExecutionProgramNode>,
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
