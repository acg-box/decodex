//! Internal Execution Program model and readiness evaluator.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
	loop_contract::{DecisionContract, DecisionContractStatus},
	prelude::{Result, eyre},
	tracker,
	workflow::WorkflowDocument,
};

pub(crate) const EXECUTION_PROGRAM_SCHEMA: &str = "decodex.execution_program/1";
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

/// Queue intent for one internal Execution Program node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionQueueIntent {
	/// The node is intentionally not ready for queueing.
	NotReady,
	/// The node is ready to receive the service queue label once mapped to a startable issue.
	ReadyToQueue,
	/// The node should retain the service queue label while it remains startable.
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
	fn is_terminal(self) -> bool {
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
	fn as_str(self) -> &'static str {
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
	/// Node is startable and may be mapped to queue action.
	Ready,
	/// Node cannot start until a concrete blocker clears.
	Blocked,
	/// Node is intentionally paused.
	Paused,
	/// Node is already active and should not retain the queue label.
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

/// Queue-label action allowed for a mapped node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionQueueLabelAction {
	/// Apply the service queue label.
	Apply,
	/// Retain the service queue label.
	Retain,
	/// Remove the service queue label.
	Remove,
}

/// Conflict-domain key for one program node.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionConflictDomain {
	kind: ExecutionConflictDomainKind,
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

	fn validate(&self) -> Result<()> {
		validate_required("execution program conflict_domain.key", &self.key)
	}
}

/// Dependency edge for one program node.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionProgramDependency {
	dependency_id: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	required_terminal_states: Vec<String>,
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

	fn validate(&self) -> Result<()> {
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
	issue_id: String,
	issue_identifier: String,
	issue_state: String,
	has_queue_label: bool,
	has_active_label: bool,
	has_opt_out_label: bool,
	has_needs_attention_label: bool,
	has_generic_dispatch_briefing: bool,
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
			has_queue_label: false,
			has_active_label: false,
			has_opt_out_label: false,
			has_needs_attention_label: false,
			has_generic_dispatch_briefing: true,
		};

		mapping.validate()?;

		Ok(mapping)
	}

	/// Mark whether the issue currently carries the service queue label.
	pub(crate) fn with_queue_label(mut self, present: bool) -> Self {
		self.has_queue_label = present;

		self
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

	/// Mark whether the issue description remains a generic dispatch briefing.
	pub(crate) fn with_generic_dispatch_briefing(mut self, present: bool) -> Self {
		self.has_generic_dispatch_briefing = present;

		self
	}

	/// Linear issue identifier such as `XY-853`.
	pub(crate) fn issue_identifier(&self) -> &str {
		&self.issue_identifier
	}

	/// Tracker workflow state for the mapped issue.
	pub(crate) fn issue_state(&self) -> &str {
		&self.issue_state
	}

	/// Whether the service queue label is currently present.
	pub(crate) fn has_queue_label(&self) -> bool {
		self.has_queue_label
	}

	fn validate(&self) -> Result<()> {
		validate_required("execution program issue_mapping.issue_id", &self.issue_id)?;
		validate_required(
			"execution program issue_mapping.issue_identifier",
			&self.issue_identifier,
		)?;

		validate_required("execution program issue_mapping.issue_state", &self.issue_state)
	}
}

/// Internal node in an Execution Program.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ExecutionProgramNode {
	node_id: String,
	stage: ExecutionProgramNodeStage,
	objective: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	objective_lineage: Vec<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	dependencies: Vec<ExecutionProgramDependency>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	conflict_domains: Vec<ExecutionConflictDomain>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	acceptance_expectations: Vec<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	validation_expectations: Vec<String>,
	queue_intent: ExecutionQueueIntent,
	#[serde(skip_serializing_if = "Option::is_none")]
	linear_issue: Option<ExecutionLinearIssueMapping>,
	contract_fingerprint: String,
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

	/// Node queue intent.
	pub(crate) fn queue_intent(&self) -> ExecutionQueueIntent {
		self.queue_intent
	}

	/// Linked normal Linear issue, when the node is executable.
	pub(crate) fn linear_issue(&self) -> Option<&ExecutionLinearIssueMapping> {
		self.linear_issue.as_ref()
	}

	fn bind_contract_fingerprint(&mut self, fingerprint: &str) {
		if self.contract_fingerprint.is_empty() {
			self.contract_fingerprint = fingerprint.to_owned();
		}
	}

	fn validate(&self) -> Result<()> {
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
	program_id: String,
	service_id: String,
	source_contract_id: String,
	accepted_contract_fingerprint: String,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	nodes: Vec<ExecutionProgramNode>,
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

		let fingerprint = decision_contract_fingerprint(contract)?;

		for node in &mut nodes {
			node.bind_contract_fingerprint(&fingerprint);
		}

		let program = Self {
			schema: execution_program_schema(),
			record_version: EXECUTION_PROGRAM_RECORD_VERSION,
			program_id: program_id.into(),
			service_id: service_id.into(),
			source_contract_id: contract.contract_id().to_owned(),
			accepted_contract_fingerprint: fingerprint,
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

	/// Accepted Decision Contract id that authorized this program.
	pub(crate) fn source_contract_id(&self) -> &str {
		&self.source_contract_id
	}

	/// Program nodes.
	pub(crate) fn nodes(&self) -> &[ExecutionProgramNode] {
		&self.nodes
	}

	/// Evaluate every node against the current contract, workflow policy, and runtime context.
	pub(crate) fn evaluate(
		&self,
		current_contract: &DecisionContract,
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

		let current_fingerprint = decision_contract_fingerprint(current_contract)?;
		let node_lookup =
			self.nodes.iter().map(|node| (node.node_id.as_str(), node)).collect::<BTreeMap<_, _>>();
		let dependency_lookup = context.dependency_lookup();
		let occupied_conflicts = context.occupied_conflict_domains.iter().collect::<HashSet<_>>();
		let mut nodes = Vec::new();

		for node in &self.nodes {
			nodes.push(evaluate_node(EvaluateNodeInput {
				program: self,
				node,
				current_contract,
				current_fingerprint: &current_fingerprint,
				policy,
				node_lookup: &node_lookup,
				dependency_lookup: &dependency_lookup,
				occupied_conflicts: &occupied_conflicts,
			})?);
		}

		Ok(ExecutionProgramEvaluation { program_id: self.program_id.clone(), nodes })
	}

	/// Validate the serialized program payload.
	pub(crate) fn validate(&self) -> Result<()> {
		validate_required("execution program schema", &self.schema)?;
		validate_required("execution program program_id", &self.program_id)?;
		validate_required("execution program service_id", &self.service_id)?;
		validate_required("execution program source_contract_id", &self.source_contract_id)?;
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
		for node in &self.nodes {
			for dependency in &node.dependencies {
				if !node_ids.contains(dependency.dependency_id.as_str()) {
					eyre::bail!(
						"Execution program node `{}` depends on unknown node `{}`.",
						node.node_id,
						dependency.dependency_id
					);
				}
			}
		}

		Ok(())
	}
}

/// Workflow policy needed for Execution Program readiness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionWorkflowPolicy {
	service_id: String,
	queue_label: String,
	active_label: String,
	startable_states: Vec<String>,
	terminal_states: Vec<String>,
	opt_out_label: String,
	needs_attention_label: String,
}
impl ExecutionWorkflowPolicy {
	/// Build readiness policy from the registered project workflow.
	pub(crate) fn from_workflow(service_id: &str, workflow: &WorkflowDocument) -> Result<Self> {
		Self::new(
			service_id,
			workflow.frontmatter().tracker().startable_states().to_vec(),
			workflow.frontmatter().tracker().terminal_states().to_vec(),
			workflow.frontmatter().tracker().opt_out_label().to_owned(),
			workflow.frontmatter().tracker().needs_attention_label().to_owned(),
		)
	}

	/// Build readiness policy directly.
	pub(crate) fn new(
		service_id: impl Into<String>,
		startable_states: Vec<String>,
		terminal_states: Vec<String>,
		opt_out_label: impl Into<String>,
		needs_attention_label: impl Into<String>,
	) -> Result<Self> {
		let service_id = service_id.into();
		let policy = Self {
			queue_label: tracker::automation_queue_label(&service_id),
			active_label: tracker::automation_active_label(&service_id),
			service_id,
			startable_states,
			terminal_states,
			opt_out_label: opt_out_label.into(),
			needs_attention_label: needs_attention_label.into(),
		};

		policy.validate()?;

		Ok(policy)
	}

	/// Service-scoped queue label.
	pub(crate) fn queue_label(&self) -> &str {
		&self.queue_label
	}

	/// Workflow terminal states.
	pub(crate) fn terminal_states(&self) -> &[String] {
		&self.terminal_states
	}

	fn issue_is_startable(&self, issue: &ExecutionLinearIssueMapping) -> bool {
		self.startable_states.iter().any(|state| state == issue.issue_state())
	}

	fn issue_is_terminal(&self, issue: &ExecutionLinearIssueMapping) -> bool {
		self.terminal_states.iter().any(|state| state == issue.issue_state())
	}

	fn validate(&self) -> Result<()> {
		validate_required("execution workflow service_id", &self.service_id)?;
		validate_required("execution workflow queue_label", &self.queue_label)?;
		validate_required("execution workflow active_label", &self.active_label)?;
		validate_required("execution workflow opt_out_label", &self.opt_out_label)?;
		validate_required("execution workflow needs_attention_label", &self.needs_attention_label)?;
		validate_string_list("execution workflow startable_states", &self.startable_states)?;
		validate_string_list("execution workflow terminal_states", &self.terminal_states)?;

		if self.startable_states.is_empty() {
			eyre::bail!("Execution workflow startable_states must not be empty.");
		}
		if self.terminal_states.is_empty() {
			eyre::bail!("Execution workflow terminal_states must not be empty.");
		}

		Ok(())
	}
}

/// Runtime dependency observation used by readiness evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionDependencySnapshot {
	dependency_id: String,
	tracker_state: Option<String>,
	queue_intent: Option<ExecutionQueueIntent>,
}
impl ExecutionDependencySnapshot {
	/// Observe a dependency through a tracker state.
	pub(crate) fn tracker_state(
		dependency_id: impl Into<String>,
		state: impl Into<String>,
	) -> Result<Self> {
		let snapshot = Self {
			dependency_id: dependency_id.into(),
			tracker_state: Some(state.into()),
			queue_intent: None,
		};

		snapshot.validate()?;

		Ok(snapshot)
	}

	/// Observe a dependency through another internal node queue intent.
	pub(crate) fn queue_intent(
		dependency_id: impl Into<String>,
		queue_intent: ExecutionQueueIntent,
	) -> Result<Self> {
		let snapshot = Self {
			dependency_id: dependency_id.into(),
			tracker_state: None,
			queue_intent: Some(queue_intent),
		};

		snapshot.validate()?;

		Ok(snapshot)
	}

	fn validate(&self) -> Result<()> {
		validate_required("execution dependency snapshot.dependency_id", &self.dependency_id)?;

		validate_optional(
			"execution dependency snapshot.tracker_state",
			self.tracker_state.as_deref(),
		)
	}
}

/// Runtime context supplied to readiness evaluation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ExecutionProgramReadinessContext {
	dependency_snapshots: Vec<ExecutionDependencySnapshot>,
	occupied_conflict_domains: Vec<ExecutionConflictDomain>,
}
impl ExecutionProgramReadinessContext {
	/// Build an empty readiness context.
	pub(crate) fn new() -> Self {
		Self::default()
	}

	/// Add dependency observations.
	pub(crate) fn with_dependency_snapshots(
		mut self,
		snapshots: impl IntoIterator<Item = ExecutionDependencySnapshot>,
	) -> Self {
		self.dependency_snapshots = snapshots.into_iter().collect();

		self
	}

	/// Add conflict domains already occupied by active or retained work.
	pub(crate) fn with_occupied_conflict_domains(
		mut self,
		domains: impl IntoIterator<Item = ExecutionConflictDomain>,
	) -> Self {
		self.occupied_conflict_domains = domains.into_iter().collect();

		self
	}

	fn dependency_lookup(&self) -> BTreeMap<&str, &ExecutionDependencySnapshot> {
		self.dependency_snapshots
			.iter()
			.map(|snapshot| (snapshot.dependency_id.as_str(), snapshot))
			.collect()
	}
}

/// Readiness result for one program node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionNodeEvaluation {
	node_id: String,
	state: ExecutionReadinessState,
	reasons: Vec<String>,
	queue_label_action: Option<ExecutionQueueLabelAction>,
	linear_issue: Option<ExecutionLinearIssueMapping>,
}
impl ExecutionNodeEvaluation {
	/// Node id.
	pub(crate) fn node_id(&self) -> &str {
		&self.node_id
	}

	/// Normalized readiness state.
	pub(crate) fn state(&self) -> ExecutionReadinessState {
		self.state
	}

	/// Human-readable readiness reasons.
	pub(crate) fn reasons(&self) -> &[String] {
		&self.reasons
	}

	/// Queue-label action, if any.
	pub(crate) fn queue_label_action(&self) -> Option<ExecutionQueueLabelAction> {
		self.queue_label_action
	}

	/// Whether this node may receive or retain the service queue label.
	pub(crate) fn queue_label_eligible(&self) -> bool {
		matches!(
			self.queue_label_action,
			Some(ExecutionQueueLabelAction::Apply | ExecutionQueueLabelAction::Retain)
		)
	}

	/// Mapped Linear issue, when present.
	pub(crate) fn linear_issue(&self) -> Option<&ExecutionLinearIssueMapping> {
		self.linear_issue.as_ref()
	}
}

/// Full readiness result for one Execution Program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionProgramEvaluation {
	program_id: String,
	nodes: Vec<ExecutionNodeEvaluation>,
}
impl ExecutionProgramEvaluation {
	/// Node evaluations.
	pub(crate) fn nodes(&self) -> &[ExecutionNodeEvaluation] {
		&self.nodes
	}

	/// Nodes that are internally ready.
	pub(crate) fn ready_node_ids(&self) -> Vec<&str> {
		self.nodes
			.iter()
			.filter(|node| node.state == ExecutionReadinessState::Ready)
			.map(|node| node.node_id.as_str())
			.collect()
	}

	/// Nodes that may receive or retain the service queue label.
	pub(crate) fn startable_node_ids(&self) -> Vec<&str> {
		self.nodes
			.iter()
			.filter(|node| node.queue_label_eligible())
			.map(|node| node.node_id.as_str())
			.collect()
	}

	/// Operator-facing progress summary without exposing graph operations as workflow.
	pub(crate) fn operator_summary(&self) -> ExecutionProgramOperatorSummary {
		let mut summary = ExecutionProgramOperatorSummary {
			program_id: self.program_id.clone(),
			ready_count: 0,
			blocked_count: 0,
			paused_count: 0,
			active_count: 0,
			completed_count: 0,
			stale_count: 0,
			queue_label_eligible_count: 0,
			mapped_issue_identifiers: Vec::new(),
		};

		for node in &self.nodes {
			match node.state {
				ExecutionReadinessState::Ready => summary.ready_count += 1,
				ExecutionReadinessState::Blocked | ExecutionReadinessState::NotReady =>
					summary.blocked_count += 1,
				ExecutionReadinessState::Paused => summary.paused_count += 1,
				ExecutionReadinessState::Active => summary.active_count += 1,
				ExecutionReadinessState::Completed => summary.completed_count += 1,
				ExecutionReadinessState::Stale => summary.stale_count += 1,
			}

			if node.queue_label_eligible() {
				summary.queue_label_eligible_count += 1;
			}

			if let Some(issue) = &node.linear_issue {
				summary.mapped_issue_identifiers.push(issue.issue_identifier.clone());
			}
		}

		summary.mapped_issue_identifiers.sort();
		summary.mapped_issue_identifiers.dedup();

		summary
	}
}

/// Compact operator readback for Execution Program progress.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionProgramOperatorSummary {
	/// Program id.
	pub(crate) program_id: String,
	/// Count of ready nodes.
	pub(crate) ready_count: usize,
	/// Count of blocked or intentionally not-ready nodes.
	pub(crate) blocked_count: usize,
	/// Count of paused nodes.
	pub(crate) paused_count: usize,
	/// Count of active nodes.
	pub(crate) active_count: usize,
	/// Count of done or canceled nodes.
	pub(crate) completed_count: usize,
	/// Count of stale nodes.
	pub(crate) stale_count: usize,
	/// Count of nodes eligible to receive or retain the service queue label.
	pub(crate) queue_label_eligible_count: usize,
	/// Normal Linear issue identifiers linked to the program.
	pub(crate) mapped_issue_identifiers: Vec<String>,
}

struct EvaluateNodeInput<'a> {
	program: &'a ExecutionProgram,
	node: &'a ExecutionProgramNode,
	current_contract: &'a DecisionContract,
	current_fingerprint: &'a str,
	policy: &'a ExecutionWorkflowPolicy,
	node_lookup: &'a BTreeMap<&'a str, &'a ExecutionProgramNode>,
	dependency_lookup: &'a BTreeMap<&'a str, &'a ExecutionDependencySnapshot>,
	occupied_conflicts: &'a HashSet<&'a ExecutionConflictDomain>,
}

fn evaluate_node(input: EvaluateNodeInput<'_>) -> Result<ExecutionNodeEvaluation> {
	let EvaluateNodeInput {
		program,
		node,
		current_contract,
		current_fingerprint,
		policy,
		node_lookup,
		dependency_lookup,
		occupied_conflicts,
	} = input;
	let mut reasons = Vec::new();
	let mut state = ExecutionReadinessState::Ready;

	if current_contract.status() != DecisionContractStatus::AcceptedPromoted
		|| current_contract.contract_id() != program.source_contract_id
		|| current_fingerprint != program.accepted_contract_fingerprint
		|| current_fingerprint != node.contract_fingerprint
	{
		state = ExecutionReadinessState::Stale;

		reasons.push(String::from("node no longer matches the accepted Decision Contract"));
	} else {
		match node.queue_intent {
			ExecutionQueueIntent::NotReady => {
				state = ExecutionReadinessState::NotReady;

				reasons.push(String::from("node queue intent is not-ready"));
			},
			ExecutionQueueIntent::Paused => {
				state = ExecutionReadinessState::Paused;

				reasons.push(String::from("node queue intent is paused"));
			},
			ExecutionQueueIntent::Active => {
				state = ExecutionReadinessState::Active;

				reasons.push(String::from("node already has an active lane"));
			},
			ExecutionQueueIntent::Done | ExecutionQueueIntent::Canceled => {
				state = ExecutionReadinessState::Completed;

				reasons.push(String::from("node queue intent is terminal"));
			},
			ExecutionQueueIntent::ReadyToQueue | ExecutionQueueIntent::Queued => {
				collect_blocking_readiness_reasons(
					node,
					policy,
					node_lookup,
					dependency_lookup,
					occupied_conflicts,
					&mut reasons,
				);

				if !reasons.is_empty() {
					state = ExecutionReadinessState::Blocked;
				}
			},
		}
	}
	if state == ExecutionReadinessState::Ready {
		reasons.push(String::from("node is ready for normal Linear issue execution"));
	}

	let queue_label_action = queue_label_action_for(node, state, policy);

	Ok(ExecutionNodeEvaluation {
		node_id: node.node_id.clone(),
		state,
		reasons,
		queue_label_action,
		linear_issue: node.linear_issue.clone(),
	})
}

fn collect_blocking_readiness_reasons(
	node: &ExecutionProgramNode,
	policy: &ExecutionWorkflowPolicy,
	node_lookup: &BTreeMap<&str, &ExecutionProgramNode>,
	dependency_lookup: &BTreeMap<&str, &ExecutionDependencySnapshot>,
	occupied_conflicts: &HashSet<&ExecutionConflictDomain>,
	reasons: &mut Vec<String>,
) {
	if node.acceptance_expectations.is_empty() {
		reasons.push(String::from("node has no acceptance expectations"));
	}
	if node.validation_expectations.is_empty() {
		reasons.push(String::from("node has no validation expectations"));
	}

	for dependency in &node.dependencies {
		if !dependency_is_satisfied(dependency, policy, node_lookup, dependency_lookup) {
			reasons.push(format!(
				"dependency `{}` has not reached a required terminal state",
				dependency.dependency_id()
			));
		}
	}
	for domain in &node.conflict_domains {
		if occupied_conflicts.contains(domain) {
			reasons.push(format!(
				"conflict domain `{}:{}` is already occupied",
				domain.kind.as_str(),
				domain.key()
			));
		}
	}

	if let Some(issue) = &node.linear_issue {
		collect_issue_mapping_reasons(issue, policy, reasons);
	} else {
		reasons.push(String::from("node has no normal Linear issue mapping"));
	}
}

fn collect_issue_mapping_reasons(
	issue: &ExecutionLinearIssueMapping,
	policy: &ExecutionWorkflowPolicy,
	reasons: &mut Vec<String>,
) {
	if policy.issue_is_terminal(issue) {
		reasons.push(format!(
			"mapped issue `{}` is already terminal in `{}`",
			issue.issue_identifier(),
			issue.issue_state()
		));
	}
	if !policy.issue_is_startable(issue) {
		reasons.push(format!(
			"mapped issue `{}` is not in a startable state",
			issue.issue_identifier()
		));
	}
	if issue.has_active_label {
		reasons.push(format!(
			"mapped issue `{}` already carries `{}`",
			issue.issue_identifier(),
			policy.active_label
		));
	}
	if issue.has_opt_out_label {
		reasons.push(format!(
			"mapped issue `{}` carries `{}`",
			issue.issue_identifier(),
			policy.opt_out_label
		));
	}
	if issue.has_needs_attention_label {
		reasons.push(format!(
			"mapped issue `{}` carries `{}`",
			issue.issue_identifier(),
			policy.needs_attention_label
		));
	}
	if !issue.has_generic_dispatch_briefing {
		reasons.push(format!(
			"mapped issue `{}` is missing a generic dispatch briefing",
			issue.issue_identifier()
		));
	}
}

fn dependency_is_satisfied(
	dependency: &ExecutionProgramDependency,
	policy: &ExecutionWorkflowPolicy,
	node_lookup: &BTreeMap<&str, &ExecutionProgramNode>,
	dependency_lookup: &BTreeMap<&str, &ExecutionDependencySnapshot>,
) -> bool {
	if let Some(snapshot) = dependency_lookup.get(dependency.dependency_id()) {
		if let Some(state) = &snapshot.tracker_state {
			return dependency_terminal_states(dependency, policy)
				.iter()
				.any(|terminal| terminal == state);
		}
		if let Some(queue_intent) = snapshot.queue_intent {
			return queue_intent.is_terminal();
		}
	}

	node_lookup
		.get(dependency.dependency_id())
		.is_some_and(|node| node.queue_intent().is_terminal())
}

fn dependency_terminal_states<'a>(
	dependency: &'a ExecutionProgramDependency,
	policy: &'a ExecutionWorkflowPolicy,
) -> &'a [String] {
	if dependency.required_terminal_states.is_empty() {
		policy.terminal_states()
	} else {
		&dependency.required_terminal_states
	}
}

fn queue_label_action_for(
	node: &ExecutionProgramNode,
	state: ExecutionReadinessState,
	policy: &ExecutionWorkflowPolicy,
) -> Option<ExecutionQueueLabelAction> {
	let issue = node.linear_issue()?;

	if state != ExecutionReadinessState::Ready {
		return issue.has_queue_label().then_some(ExecutionQueueLabelAction::Remove);
	}
	if !matches!(
		node.queue_intent(),
		ExecutionQueueIntent::ReadyToQueue | ExecutionQueueIntent::Queued
	) || !policy.issue_is_startable(issue)
	{
		return issue.has_queue_label().then_some(ExecutionQueueLabelAction::Remove);
	}

	Some(if issue.has_queue_label() {
		ExecutionQueueLabelAction::Retain
	} else {
		ExecutionQueueLabelAction::Apply
	})
}

fn ensure_accepted_contract(contract: &DecisionContract) -> Result<()> {
	contract.validate()?;

	if contract.status() != DecisionContractStatus::AcceptedPromoted {
		eyre::bail!(
			"Execution Programs can only derive from accepted Decision Contracts; `{}` is `{}`.",
			contract.contract_id(),
			contract.status().as_str()
		);
	}

	Ok(())
}

fn decision_contract_fingerprint(contract: &DecisionContract) -> Result<String> {
	contract.validate()?;

	let payload = serde_json::to_vec(contract)?;
	let digest = Sha256::digest(payload);

	Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>())
}

fn execution_program_schema() -> String {
	EXECUTION_PROGRAM_SCHEMA.to_owned()
}

fn execution_program_record_version() -> u16 {
	EXECUTION_PROGRAM_RECORD_VERSION
}

fn validate_required(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(())
}

fn validate_optional(name: &str, value: Option<&str>) -> Result<()> {
	if let Some(value) = value {
		validate_required(name, value)?;
	}

	Ok(())
}

fn non_empty_optional(value: &str) -> Option<&str> {
	if value.is_empty() { None } else { Some(value) }
}

fn validate_string_list(name: &str, values: &[String]) -> Result<()> {
	for value in values {
		validate_required(name, value)?;
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use crate::{
		execution_program::{
			ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionDependencySnapshot,
			ExecutionLinearIssueMapping, ExecutionProgram, ExecutionProgramDependency,
			ExecutionProgramNode, ExecutionProgramNodeStage, ExecutionProgramReadinessContext,
			ExecutionQueueIntent, ExecutionQueueLabelAction, ExecutionReadinessState,
			ExecutionWorkflowPolicy,
		},
		loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind},
	};

	fn latent_contract_fixture() -> DecisionContract {
		serde_json::from_str(include_str!(concat!(
			env!("CARGO_MANIFEST_DIR"),
			"/fixtures/decision_contract/research_x_latent_contract.json"
		)))
		.expect("decision contract fixture should deserialize")
	}

	fn accepted_contract_fixture() -> DecisionContract {
		let mut contract = latent_contract_fixture();

		contract
			.promote(
				DecisionPromotion::new(
					"operator",
					DecisionPromotionActorKind::User,
					"2026-06-09T10:00:00Z",
					"conversation",
					Some(String::from("User asked to push this forward.")),
				)
				.expect("promotion should build"),
			)
			.expect("contract should promote");

		contract
	}

	fn workflow_policy() -> ExecutionWorkflowPolicy {
		ExecutionWorkflowPolicy::new(
			"decodex",
			vec![String::from("Todo")],
			vec![String::from("Done"), String::from("Canceled"), String::from("Duplicate")],
			"decodex:manual-only",
			"decodex:needs-attention",
		)
		.expect("workflow policy should build")
	}

	fn issue(identifier: &str, state: &str) -> ExecutionLinearIssueMapping {
		ExecutionLinearIssueMapping::new(
			format!("linear-{identifier}"),
			identifier.to_owned(),
			state.to_owned(),
		)
		.expect("issue mapping should build")
	}

	fn ready_node(id: &str, issue_identifier: &str) -> ExecutionProgramNode {
		ExecutionProgramNode::new(
			id,
			ExecutionProgramNodeStage::Runtime,
			format!("Implement {id}."),
			ExecutionQueueIntent::ReadyToQueue,
		)
		.expect("node should build")
		.with_objective_lineage([String::from("Ship the accepted runtime work.")])
		.expect("lineage should attach")
		.with_acceptance_expectations([String::from("Acceptance is concrete.")])
		.expect("acceptance should attach")
		.with_validation_expectations([String::from("Run the repo gate.")])
		.expect("validation should attach")
		.with_linear_issue(issue(issue_identifier, "Todo"))
		.expect("issue should attach")
	}

	fn program_with(nodes: Vec<ExecutionProgramNode>) -> (DecisionContract, ExecutionProgram) {
		let contract = accepted_contract_fixture();
		let program =
			ExecutionProgram::from_accepted_contract("program-1", "decodex", &contract, nodes)
				.expect("program should derive from accepted contract");

		(contract, program)
	}

	#[test]
	fn readiness_selects_only_startable_ready_nodes() {
		let blocked = ready_node("node-blocked", "XY-901")
			.with_dependencies([
				ExecutionProgramDependency::new("node-ready").expect("dependency should build")
			])
			.expect("dependency should attach");
		let (contract, program) = program_with(vec![ready_node("node-ready", "XY-900"), blocked]);
		let evaluation = program
			.evaluate(&contract, &workflow_policy(), &ExecutionProgramReadinessContext::new())
			.expect("program should evaluate");

		assert_eq!(evaluation.ready_node_ids(), vec!["node-ready"]);
		assert_eq!(evaluation.startable_node_ids(), vec!["node-ready"]);
		assert_eq!(
			evaluation.nodes()[0].queue_label_action(),
			Some(ExecutionQueueLabelAction::Apply)
		);
		assert_eq!(evaluation.operator_summary().ready_count, 1);
		assert_eq!(evaluation.operator_summary().blocked_count, 1);
	}

	#[test]
	fn dependency_blocking_respects_workflow_terminal_states() {
		let dependent = ready_node("node-dependent", "XY-902")
			.with_dependencies([ExecutionProgramDependency::new("node-dependency")
				.expect("dependency should build")])
			.expect("dependency should attach");
		let (contract, program) =
			program_with(vec![ready_node("node-dependency", "XY-901"), dependent.clone()]);
		let blocked_context = ExecutionProgramReadinessContext::new().with_dependency_snapshots([
			ExecutionDependencySnapshot::tracker_state("node-dependency", "In Review")
				.expect("snapshot should build"),
		]);
		let blocked = program
			.evaluate(&contract, &workflow_policy(), &blocked_context)
			.expect("program should evaluate");
		let dependent_evaluation = blocked
			.nodes()
			.iter()
			.find(|node| node.node_id() == "node-dependent")
			.expect("dependent node should exist");

		assert_eq!(dependent_evaluation.state(), ExecutionReadinessState::Blocked);
		assert!(
			dependent_evaluation
				.reasons()
				.iter()
				.any(|reason| reason.contains("required terminal state"))
		);

		let ready_context = ExecutionProgramReadinessContext::new().with_dependency_snapshots([
			ExecutionDependencySnapshot::tracker_state("node-dependency", "Done")
				.expect("snapshot should build"),
		]);
		let ready = program
			.evaluate(&contract, &workflow_policy(), &ready_context)
			.expect("program should evaluate");

		assert!(ready.startable_node_ids().contains(&"node-dependent"));
	}

	#[test]
	fn stale_contract_drift_blocks_queue_retention() {
		let stale_node = ready_node("node-stale", "XY-903")
			.with_contract_fingerprint("stale-contract-fingerprint")
			.expect("fingerprint should override");
		let (contract, program) = program_with(vec![stale_node]);
		let evaluation = program
			.evaluate(&contract, &workflow_policy(), &ExecutionProgramReadinessContext::new())
			.expect("program should evaluate");
		let node = &evaluation.nodes()[0];

		assert_eq!(node.state(), ExecutionReadinessState::Stale);
		assert_eq!(node.queue_label_action(), None);
		assert!(evaluation.startable_node_ids().is_empty());
	}

	#[test]
	fn conflict_domain_blocks_ready_node() {
		let conflict = ExecutionConflictDomain::new(
			ExecutionConflictDomainKind::File,
			"apps/decodex/src/runtime.rs",
		)
		.expect("domain should build");
		let node = ready_node("node-conflict", "XY-904")
			.with_conflict_domains([conflict.clone()])
			.expect("conflict should attach");
		let (contract, program) = program_with(vec![node]);
		let context =
			ExecutionProgramReadinessContext::new().with_occupied_conflict_domains([conflict]);
		let evaluation = program
			.evaluate(&contract, &workflow_policy(), &context)
			.expect("program should evaluate");
		let node = &evaluation.nodes()[0];

		assert_eq!(node.state(), ExecutionReadinessState::Blocked);
		assert!(node.reasons().iter().any(|reason| reason.contains("already occupied")));
	}

	#[test]
	fn linear_issue_mapping_controls_apply_retain_and_remove() {
		let apply = ready_node("node-apply", "XY-905");
		let retain = ready_node("node-retain", "XY-906")
			.with_linear_issue(issue("XY-906", "Todo").with_queue_label(true))
			.expect("issue should attach");
		let remove = ready_node("node-remove", "XY-907")
			.with_linear_issue(issue("XY-907", "In Progress").with_queue_label(true))
			.expect("issue should attach");
		let (contract, program) = program_with(vec![apply, retain, remove]);
		let evaluation = program
			.evaluate(&contract, &workflow_policy(), &ExecutionProgramReadinessContext::new())
			.expect("program should evaluate");
		let action_for = |id: &str| {
			evaluation
				.nodes()
				.iter()
				.find(|node| node.node_id() == id)
				.expect("node should exist")
				.queue_label_action()
		};

		assert_eq!(action_for("node-apply"), Some(ExecutionQueueLabelAction::Apply));
		assert_eq!(action_for("node-retain"), Some(ExecutionQueueLabelAction::Retain));
		assert_eq!(action_for("node-remove"), Some(ExecutionQueueLabelAction::Remove));
		assert_eq!(
			evaluation
				.nodes()
				.iter()
				.find(|node| node.node_id() == "node-remove")
				.expect("node should exist")
				.state(),
			ExecutionReadinessState::Blocked
		);
	}

	#[test]
	fn unmapped_ready_to_queue_node_is_blocked_from_startable_selection() {
		let unmapped = ExecutionProgramNode::new(
			"node-unmapped",
			ExecutionProgramNodeStage::Runtime,
			"Implement unmapped work.",
			ExecutionQueueIntent::ReadyToQueue,
		)
		.expect("node should build")
		.with_acceptance_expectations([String::from("Acceptance is concrete.")])
		.expect("acceptance should attach")
		.with_validation_expectations([String::from("Run the repo gate.")])
		.expect("validation should attach");
		let (contract, program) = program_with(vec![unmapped]);
		let evaluation = program
			.evaluate(&contract, &workflow_policy(), &ExecutionProgramReadinessContext::new())
			.expect("program should evaluate");
		let node = &evaluation.nodes()[0];

		assert_eq!(node.state(), ExecutionReadinessState::Blocked);
		assert!(node.reasons().iter().any(|reason| reason.contains("no normal Linear issue")));
		assert!(evaluation.startable_node_ids().is_empty());
	}

	#[test]
	fn evaluator_rejects_wrong_service_policy() {
		let (contract, program) = program_with(vec![ready_node("node-ready", "XY-908")]);
		let wrong_service_policy = ExecutionWorkflowPolicy::new(
			"other-service",
			vec![String::from("Todo")],
			vec![String::from("Done")],
			"decodex:manual-only",
			"decodex:needs-attention",
		)
		.expect("workflow policy should build");
		let error = program
			.evaluate(&contract, &wrong_service_policy, &ExecutionProgramReadinessContext::new())
			.expect_err("program should reject mismatched service policy");

		assert!(error.to_string().contains("readiness policy belongs to"));
	}
}
