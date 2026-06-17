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
pub(crate) const PROGRAM_INTAKE_PLAN_SCHEMA: &str = "decodex.program_intake_plan/1";
pub(crate) const PROGRAM_INTAKE_PLAN_RECORD_VERSION: u16 = 1;

/// Source shape for a Program Intake Plan.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProgramIntakeKind {
	/// Natural-language goal promoted through an accepted Decision Contract.
	GoalIntake,
	/// Operator-supplied batch of normal issue briefs.
	IssueBatchIntake,
}
impl ProgramIntakeKind {
	/// Stable machine-readable intake kind.
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::GoalIntake => "goal_intake",
			Self::IssueBatchIntake => "issue_batch_intake",
		}
	}
}

/// Durable planning metadata for first-class program intake.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ProgramIntakePlan {
	#[serde(default = "program_intake_plan_schema")]
	schema: String,
	#[serde(default = "program_intake_plan_record_version")]
	record_version: u16,
	plan_id: String,
	service_id: String,
	intake_kind: ProgramIntakeKind,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_contract_id: Option<String>,
	accepted_contract_fingerprint: String,
	public_summary: String,
}
impl ProgramIntakePlan {
	/// Build program-intake metadata for a promoted natural-language goal.
	pub(crate) fn goal_intake(
		plan_id: impl Into<String>,
		service_id: impl Into<String>,
		contract: &DecisionContract,
		accepted_contract_fingerprint: impl Into<String>,
	) -> Result<Self> {
		ensure_accepted_contract(contract)?;

		let public_summary =
			contract.accepted_authority().accepted_objectives().first().cloned().unwrap_or_else(
				|| format!("Accepted Decision Contract `{}`.", contract.contract_id()),
			);
		let plan = Self {
			schema: program_intake_plan_schema(),
			record_version: PROGRAM_INTAKE_PLAN_RECORD_VERSION,
			plan_id: plan_id.into(),
			service_id: service_id.into(),
			intake_kind: ProgramIntakeKind::GoalIntake,
			source_contract_id: Some(contract.contract_id().to_owned()),
			accepted_contract_fingerprint: accepted_contract_fingerprint.into(),
			public_summary,
		};

		plan.validate()?;

		Ok(plan)
	}

	/// Build program-intake metadata for an accepted issue batch.
	#[allow(dead_code)]
	pub(crate) fn issue_batch_intake(
		plan_id: impl Into<String>,
		service_id: impl Into<String>,
		accepted_contract_fingerprint: impl Into<String>,
		public_summary: impl Into<String>,
	) -> Result<Self> {
		let plan = Self {
			schema: program_intake_plan_schema(),
			record_version: PROGRAM_INTAKE_PLAN_RECORD_VERSION,
			plan_id: plan_id.into(),
			service_id: service_id.into(),
			intake_kind: ProgramIntakeKind::IssueBatchIntake,
			source_contract_id: None,
			accepted_contract_fingerprint: accepted_contract_fingerprint.into(),
			public_summary: public_summary.into(),
		};

		plan.validate()?;

		Ok(plan)
	}

	/// Program intake plan id.
	pub(crate) fn plan_id(&self) -> &str {
		&self.plan_id
	}

	/// Service id that owns this intake plan.
	pub(crate) fn service_id(&self) -> &str {
		&self.service_id
	}

	/// Intake source kind.
	pub(crate) fn intake_kind(&self) -> ProgramIntakeKind {
		self.intake_kind
	}

	/// Accepted Decision Contract id for goal intake.
	pub(crate) fn source_contract_id(&self) -> Option<&str> {
		self.source_contract_id.as_deref()
	}

	/// Stable authority fingerprint for this intake boundary.
	pub(crate) fn accepted_contract_fingerprint(&self) -> &str {
		&self.accepted_contract_fingerprint
	}

	/// Public-safe summary suitable for operator readback.
	pub(crate) fn public_summary(&self) -> &str {
		&self.public_summary
	}

	fn validate(&self) -> Result<()> {
		validate_required("program intake plan schema", &self.schema)?;
		validate_required("program intake plan plan_id", &self.plan_id)?;
		validate_required("program intake plan service_id", &self.service_id)?;
		validate_required(
			"program intake plan accepted_contract_fingerprint",
			&self.accepted_contract_fingerprint,
		)?;
		validate_required("program intake plan public_summary", &self.public_summary)?;

		if self.schema != PROGRAM_INTAKE_PLAN_SCHEMA {
			eyre::bail!(
				"Program intake plan `{}` has unsupported schema `{}`.",
				self.plan_id,
				self.schema
			);
		}
		if self.record_version != PROGRAM_INTAKE_PLAN_RECORD_VERSION {
			eyre::bail!(
				"Program intake plan `{}` has unsupported record_version `{}`.",
				self.plan_id,
				self.record_version
			);
		}
		if self.intake_kind == ProgramIntakeKind::GoalIntake
			&& self.source_contract_id.as_deref().is_none_or(str::is_empty)
		{
			eyre::bail!("Goal intake plan `{}` must reference a source contract.", self.plan_id);
		}
		if self.intake_kind == ProgramIntakeKind::IssueBatchIntake
			&& self.source_contract_id.as_deref().is_some_and(|id| !id.is_empty())
		{
			eyre::bail!(
				"Issue-batch intake plan `{}` must not reference a source contract.",
				self.plan_id
			);
		}

		Ok(())
	}
}

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

	/// Stable conflict-domain kind.
	pub(crate) fn kind(&self) -> ExecutionConflictDomainKind {
		self.kind
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
	has_active_label: bool,
	has_opt_out_label: bool,
	has_needs_attention_label: bool,
	#[serde(default, skip_serializing_if = "is_false")]
	has_open_tracker_blockers: bool,
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
			has_active_label: false,
			has_opt_out_label: false,
			has_needs_attention_label: false,
			has_open_tracker_blockers: false,
			has_generic_dispatch_briefing: true,
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

	fn validate(&self) -> Result<()> {
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
	#[serde(skip_serializing_if = "Option::is_none")]
	source_contract_id: Option<String>,
	accepted_contract_fingerprint: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	program_intake_plan: Option<ProgramIntakePlan>,
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

	/// Observe a dependency through another internal node dispatch intent.
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
	stage: ExecutionProgramNodeStage,
	state: ExecutionReadinessState,
	lifecycle_state: ExecutionProgramNodeLifecycleState,
	reasons: Vec<String>,
	dispatch_action: Option<ExecutionDispatchAction>,
	linear_issue: Option<ExecutionLinearIssueMapping>,
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

	/// Nodes that can be dispatched directly by the program scheduler.
	pub(crate) fn dispatchable_node_ids(&self) -> Vec<&str> {
		self.nodes
			.iter()
			.filter(|node| node.dispatchable())
			.map(|node| node.node_id.as_str())
			.collect()
	}

	/// Operator-facing progress summary without exposing graph operations as workflow.
	pub(crate) fn operator_summary(&self) -> ExecutionProgramOperatorSummary {
		let mut summary = ExecutionProgramOperatorSummary {
			program_id: self.program_id.clone(),
			planned_count: 0,
			mapped_count: 0,
			ready_count: 0,
			queued_count: 0,
			blocked_count: 0,
			held_count: 0,
			active_count: 0,
			needs_attention_count: 0,
			completed_count: 0,
			stale_count: 0,
			superseded_count: 0,
			dispatchable_count: 0,
			mapped_issue_identifiers: Vec::new(),
		};

		for node in &self.nodes {
			match node.lifecycle_state {
				ExecutionProgramNodeLifecycleState::Planned => {
					summary.planned_count += 1;
					summary.held_count += 1;
				},
				ExecutionProgramNodeLifecycleState::Mapped => {
					summary.mapped_count += 1;
					summary.held_count += 1;
				},
				ExecutionProgramNodeLifecycleState::Ready => summary.ready_count += 1,
				ExecutionProgramNodeLifecycleState::Queued => summary.queued_count += 1,
				ExecutionProgramNodeLifecycleState::Blocked => summary.blocked_count += 1,
				ExecutionProgramNodeLifecycleState::Active => {
					summary.active_count += 1;
					summary.held_count += 1;
				},
				ExecutionProgramNodeLifecycleState::NeedsAttention => {
					summary.needs_attention_count += 1;
				},
				ExecutionProgramNodeLifecycleState::Completed => summary.completed_count += 1,
				ExecutionProgramNodeLifecycleState::Stale => summary.stale_count += 1,
				ExecutionProgramNodeLifecycleState::Superseded => summary.superseded_count += 1,
			}

			if node.dispatchable() {
				summary.dispatchable_count += 1;
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
	/// Count of planned nodes without a normal Linear issue mapping.
	pub(crate) planned_count: usize,
	/// Count of mapped nodes that are intentionally held from queueing.
	pub(crate) mapped_count: usize,
	/// Count of ready nodes.
	pub(crate) ready_count: usize,
	/// Count of queued nodes.
	pub(crate) queued_count: usize,
	/// Count of blocked or intentionally not-ready nodes.
	pub(crate) blocked_count: usize,
	/// Count of held nodes that are planned, mapped, or active.
	pub(crate) held_count: usize,
	/// Count of active nodes.
	pub(crate) active_count: usize,
	/// Count of human-attention nodes.
	pub(crate) needs_attention_count: usize,
	/// Count of done or canceled nodes.
	pub(crate) completed_count: usize,
	/// Count of stale nodes.
	pub(crate) stale_count: usize,
	/// Count of superseded nodes.
	pub(crate) superseded_count: usize,
	/// Count of nodes the program scheduler can dispatch directly.
	pub(crate) dispatchable_count: usize,
	/// Normal Linear issue identifiers linked to the program.
	pub(crate) mapped_issue_identifiers: Vec<String>,
}

struct EvaluateNodeInput<'a> {
	program: &'a ExecutionProgram,
	node: &'a ExecutionProgramNode,
	current_contract: Option<&'a DecisionContract>,
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
	let authority_matches =
		current_contract.map_or(program.source_contract_id.is_none(), |contract| {
			contract.status() == DecisionContractStatus::AcceptedPromoted
				&& Some(contract.contract_id()) == program.source_contract_id.as_deref()
		});
	let mut reasons = Vec::new();
	let mut state = ExecutionReadinessState::Ready;
	let mut lifecycle_state = None;

	if !authority_matches
		|| current_fingerprint != program.accepted_contract_fingerprint
		|| current_fingerprint != node.contract_fingerprint
	{
		state = ExecutionReadinessState::Stale;
		lifecycle_state = Some(
			if current_contract.is_some_and(|contract| {
				contract.status() == DecisionContractStatus::RejectedSuperseded
			}) {
				ExecutionProgramNodeLifecycleState::Superseded
			} else {
				ExecutionProgramNodeLifecycleState::Stale
			},
		);

		reasons.push(String::from("node no longer matches the accepted Decision Contract"));
	} else if let Some(issue) = node.linear_issue()
		&& policy.issue_is_terminal(issue)
	{
		state = ExecutionReadinessState::Completed;
		lifecycle_state = Some(ExecutionProgramNodeLifecycleState::Completed);

		reasons.push(format!(
			"mapped issue `{}` is already terminal in `{}`",
			issue.issue_identifier(),
			issue.issue_state()
		));
	} else {
		match node.queue_intent {
			ExecutionQueueIntent::NotReady => {
				state = ExecutionReadinessState::NotReady;

				reasons.push(String::from("node dispatch intent is not-ready"));
			},
			ExecutionQueueIntent::Paused => {
				state = ExecutionReadinessState::Paused;

				reasons.push(String::from("node dispatch intent is paused"));
			},
			ExecutionQueueIntent::Active => {
				state = ExecutionReadinessState::Active;

				reasons.push(String::from("node already has a current lane"));
			},
			ExecutionQueueIntent::Done | ExecutionQueueIntent::Canceled => {
				state = ExecutionReadinessState::Completed;

				reasons.push(String::from("node dispatch intent is terminal"));
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

	let dispatch_action = dispatch_action_for(node, state, policy);
	let lifecycle_state = lifecycle_state.unwrap_or_else(|| lifecycle_state_for(node, state));

	Ok(ExecutionNodeEvaluation {
		node_id: node.node_id.clone(),
		stage: node.stage,
		state,
		lifecycle_state,
		reasons,
		dispatch_action,
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
	if issue.has_open_tracker_blockers {
		reasons.push(format!(
			"mapped issue `{}` has open tracker dependency blockers",
			issue.issue_identifier()
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

fn dispatch_action_for(
	node: &ExecutionProgramNode,
	state: ExecutionReadinessState,
	policy: &ExecutionWorkflowPolicy,
) -> Option<ExecutionDispatchAction> {
	let issue = node.linear_issue()?;

	if state != ExecutionReadinessState::Ready {
		return None;
	}
	if !matches!(
		node.queue_intent(),
		ExecutionQueueIntent::ReadyToQueue | ExecutionQueueIntent::Queued
	) || !policy.issue_is_startable(issue)
	{
		return None;
	}

	Some(ExecutionDispatchAction::Dispatch)
}

fn lifecycle_state_for(
	node: &ExecutionProgramNode,
	state: ExecutionReadinessState,
) -> ExecutionProgramNodeLifecycleState {
	if let Some(issue) = node.linear_issue()
		&& issue.has_needs_attention_label
	{
		return ExecutionProgramNodeLifecycleState::NeedsAttention;
	}
	if let Some(issue) = node.linear_issue()
		&& issue.has_active_label
	{
		return ExecutionProgramNodeLifecycleState::Active;
	}

	match state {
		ExecutionReadinessState::NotReady | ExecutionReadinessState::Paused =>
			if node.linear_issue().is_some() {
				ExecutionProgramNodeLifecycleState::Mapped
			} else {
				ExecutionProgramNodeLifecycleState::Planned
			},
		ExecutionReadinessState::Ready => ExecutionProgramNodeLifecycleState::Ready,
		ExecutionReadinessState::Blocked => ExecutionProgramNodeLifecycleState::Blocked,
		ExecutionReadinessState::Active => ExecutionProgramNodeLifecycleState::Active,
		ExecutionReadinessState::Completed => ExecutionProgramNodeLifecycleState::Completed,
		ExecutionReadinessState::Stale => ExecutionProgramNodeLifecycleState::Stale,
	}
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

fn program_intake_plan_schema() -> String {
	PROGRAM_INTAKE_PLAN_SCHEMA.to_owned()
}

fn program_intake_plan_record_version() -> u16 {
	PROGRAM_INTAKE_PLAN_RECORD_VERSION
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

fn is_false(value: &bool) -> bool {
	!*value
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
			ExecutionDispatchAction, ExecutionLinearIssueMapping, ExecutionProgram,
			ExecutionProgramDependency, ExecutionProgramNode, ExecutionProgramNodeStage,
			ExecutionProgramReadinessContext, ExecutionQueueIntent, ExecutionReadinessState,
			ExecutionWorkflowPolicy, ProgramIntakeKind,
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
		assert_eq!(evaluation.dispatchable_node_ids(), vec!["node-ready"]);
		assert_eq!(
			evaluation.nodes()[0].dispatch_action(),
			Some(ExecutionDispatchAction::Dispatch)
		);
		assert_eq!(evaluation.operator_summary().ready_count, 1);
		assert_eq!(evaluation.operator_summary().blocked_count, 1);
	}

	#[test]
	fn accepted_contract_program_carries_goal_intake_metadata() {
		let (contract, program) = program_with(vec![ready_node("node-ready", "XY-900")]);
		let plan = program.program_intake_plan().expect("new programs should carry intake plan");

		assert_eq!(plan.plan_id(), "program-1");
		assert_eq!(plan.intake_kind(), ProgramIntakeKind::GoalIntake);
		assert_eq!(plan.source_contract_id(), Some(contract.contract_id()));
	}

	#[test]
	fn legacy_execution_program_payload_without_intake_plan_still_validates() {
		let (_contract, program) = program_with(vec![ready_node("node-ready", "XY-900")]);
		let mut payload =
			serde_json::to_value(&program).expect("program payload should serialize to json");

		payload
			.as_object_mut()
			.expect("program payload should be an object")
			.remove("program_intake_plan");

		let legacy_program: ExecutionProgram =
			serde_json::from_value(payload).expect("legacy program should deserialize");

		legacy_program.validate().expect("legacy program should validate");

		assert!(legacy_program.program_intake_plan().is_none());
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

		assert!(ready.dispatchable_node_ids().contains(&"node-dependent"));
	}

	#[test]
	fn stale_contract_drift_blocks_direct_dispatch() {
		let stale_node = ready_node("node-stale", "XY-903")
			.with_contract_fingerprint("stale-contract-fingerprint")
			.expect("fingerprint should override");
		let (contract, program) = program_with(vec![stale_node]);
		let evaluation = program
			.evaluate(&contract, &workflow_policy(), &ExecutionProgramReadinessContext::new())
			.expect("program should evaluate");
		let node = &evaluation.nodes()[0];

		assert_eq!(node.state(), ExecutionReadinessState::Stale);
		assert_eq!(node.dispatch_action(), None);
		assert!(evaluation.dispatchable_node_ids().is_empty());
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
		assert!(evaluation.dispatchable_node_ids().is_empty());
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
