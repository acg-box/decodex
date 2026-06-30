use crate::{
	autonomy_objective::{AutonomyObjectiveContract, AutonomyObjectiveState},
	autonomy_proposal::{AutonomyProposal, AutonomyProposalState},
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalKind, AutonomySignalPrivacy,
	},
	execution_program::ExecutionProgram,
	loop_contract::{DecisionContract, DecisionContractStatus},
};

/// SQLite-backed Loop/Decision Contract retained by the local runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecisionContractRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) source_issue_id: Option<String>,
	pub(in crate::state) contract: DecisionContract,
	pub(in crate::state) status: DecisionContractStatus,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl DecisionContractRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn source_issue_id(&self) -> Option<&str> {
		self.source_issue_id.as_deref()
	}

	pub(crate) fn contract(&self) -> &DecisionContract {
		&self.contract
	}

	pub(crate) fn contract_id(&self) -> &str {
		self.contract.contract_id()
	}

	pub(crate) fn status(&self) -> DecisionContractStatus {
		self.status
	}

	pub(crate) fn created_at(&self) -> &str {
		&self.created_at
	}

	pub(crate) fn created_at_unix(&self) -> i64 {
		self.created_at_unix
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

/// SQLite-backed Objective Contract authority version retained by the local runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomyObjectiveRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) objective: AutonomyObjectiveContract,
	pub(in crate::state) state: AutonomyObjectiveState,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl AutonomyObjectiveRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn objective(&self) -> &AutonomyObjectiveContract {
		&self.objective
	}

	pub(crate) fn objective_id(&self) -> &str {
		self.objective.id()
	}

	pub(crate) fn version(&self) -> u64 {
		self.objective.version()
	}

	pub(crate) fn state(&self) -> AutonomyObjectiveState {
		self.state
	}

	pub(crate) fn created_at(&self) -> &str {
		&self.created_at
	}

	pub(crate) fn created_at_unix(&self) -> i64 {
		self.created_at_unix
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

/// SQLite-backed autonomy signal evidence retained by the local runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomySignalRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) signal: AutonomySignal,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl AutonomySignalRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn signal(&self) -> &AutonomySignal {
		&self.signal
	}

	pub(crate) fn signal_id(&self) -> &str {
		self.signal.id()
	}

	pub(crate) fn objective_id(&self) -> &str {
		self.signal.objective_id()
	}

	pub(crate) fn objective_version(&self) -> u64 {
		self.signal.objective_version()
	}

	pub(crate) fn kind(&self) -> AutonomySignalKind {
		self.signal.kind()
	}

	pub(crate) fn freshness(&self) -> AutonomySignalFreshness {
		self.signal.freshness()
	}

	pub(crate) fn evidence_class(&self) -> AutonomySignalEvidenceClass {
		self.signal.evidence_class()
	}

	pub(crate) fn confidence(&self) -> AutonomySignalConfidence {
		self.signal.confidence()
	}

	pub(crate) fn privacy(&self) -> AutonomySignalPrivacy {
		self.signal.privacy()
	}

	pub(crate) fn created_at(&self) -> &str {
		&self.created_at
	}

	pub(crate) fn created_at_unix(&self) -> i64 {
		self.created_at_unix
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

/// SQLite-backed autonomy proposal dry-run evidence retained by the local runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomyProposalRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) proposal: AutonomyProposal,
	pub(in crate::state) state: AutonomyProposalState,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl AutonomyProposalRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn proposal(&self) -> &AutonomyProposal {
		&self.proposal
	}

	pub(crate) fn proposal_id(&self) -> &str {
		self.proposal.id()
	}

	pub(crate) fn objective_id(&self) -> &str {
		self.proposal.objective_id()
	}

	pub(crate) fn objective_version(&self) -> u64 {
		self.proposal.objective_version()
	}

	pub(crate) fn state(&self) -> AutonomyProposalState {
		self.state
	}

	pub(crate) fn created_at(&self) -> &str {
		&self.created_at
	}

	pub(crate) fn created_at_unix(&self) -> i64 {
		self.created_at_unix
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

/// SQLite-backed internal Execution Program retained by the local runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionProgramRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program: ExecutionProgram,
	pub(in crate::state) source_contract_id: Option<String>,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl ExecutionProgramRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn program(&self) -> &ExecutionProgram {
		&self.program
	}

	pub(crate) fn program_id(&self) -> &str {
		self.program.program_id()
	}

	pub(crate) fn source_contract_id(&self) -> Option<&str> {
		self.source_contract_id.as_deref()
	}

	pub(crate) fn created_at(&self) -> &str {
		&self.created_at
	}

	pub(crate) fn created_at_unix(&self) -> i64 {
		self.created_at_unix
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

/// SQLite-backed Program Intake Plan projection retained by the local runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProgramIntakePlanRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program_id: String,
	pub(in crate::state) plan_id: String,
	pub(in crate::state) intake_kind: String,
	pub(in crate::state) source_contract_id: Option<String>,
	pub(in crate::state) accepted_contract_fingerprint: String,
	pub(in crate::state) public_summary: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl ProgramIntakePlanRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn program_id(&self) -> &str {
		&self.program_id
	}

	pub(crate) fn plan_id(&self) -> &str {
		&self.plan_id
	}

	pub(crate) fn intake_kind(&self) -> &str {
		&self.intake_kind
	}

	pub(crate) fn source_contract_id(&self) -> Option<&str> {
		self.source_contract_id.as_deref()
	}

	pub(crate) fn accepted_contract_fingerprint(&self) -> &str {
		&self.accepted_contract_fingerprint
	}

	pub(crate) fn public_summary(&self) -> &str {
		&self.public_summary
	}

	pub(crate) fn created_at(&self) -> &str {
		&self.created_at
	}

	pub(crate) fn created_at_unix(&self) -> i64 {
		self.created_at_unix
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

/// SQLite-backed normal Linear issue mapping for one internal program node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProgramIssueMappingRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program_id: String,
	pub(in crate::state) node_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) issue_identifier: String,
	pub(in crate::state) issue_state: String,
	pub(in crate::state) queue_intent: String,
	pub(in crate::state) has_active_label: bool,
	pub(in crate::state) has_opt_out_label: bool,
	pub(in crate::state) has_needs_attention_label: bool,
	pub(in crate::state) has_generic_dispatch_briefing: bool,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl ProgramIssueMappingRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn program_id(&self) -> &str {
		&self.program_id
	}

	pub(crate) fn node_id(&self) -> &str {
		&self.node_id
	}

	pub(crate) fn issue_id(&self) -> &str {
		&self.issue_id
	}

	pub(crate) fn issue_identifier(&self) -> &str {
		&self.issue_identifier
	}

	pub(crate) fn issue_state(&self) -> &str {
		&self.issue_state
	}

	pub(crate) fn queue_intent(&self) -> &str {
		&self.queue_intent
	}

	pub(crate) fn has_active_label(&self) -> bool {
		self.has_active_label
	}

	pub(crate) fn has_opt_out_label(&self) -> bool {
		self.has_opt_out_label
	}

	pub(crate) fn has_needs_attention_label(&self) -> bool {
		self.has_needs_attention_label
	}

	pub(crate) fn has_generic_dispatch_briefing(&self) -> bool {
		self.has_generic_dispatch_briefing
	}

	pub(crate) fn created_at(&self) -> &str {
		&self.created_at
	}

	pub(crate) fn created_at_unix(&self) -> i64 {
		self.created_at_unix
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}
