use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
	autonomy_objective::{AutonomyObjectiveContract, AutonomyObjectiveState},
	autonomy_proposal::{AutonomyProposal, AutonomyProposalState},
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalKind, AutonomySignalPrivacy,
	},
	config::ServiceConfig,
	execution_program::ExecutionProgram,
	loop_contract::{DecisionContract, DecisionContractStatus},
	state::timestamp_parts,
};

pub(crate) const WORKTREE_PROVENANCE_FILESYSTEM_SCAN: &str = "filesystem_scan";
pub(crate) const WORKTREE_PROVENANCE_GIT_HYGIENE_SCAN: &str = "git_hygiene_scan";
pub(crate) const WORKTREE_PROVENANCE_LEGACY_UNKNOWN: &str = "legacy_unknown";
pub(crate) const WORKTREE_PROVENANCE_RUNTIME_RECOVERED: &str = "runtime_recovered";
pub(crate) const WORKTREE_PROVENANCE_RUNTIME_RECORDED: &str = "runtime_recorded";

/// Active lease for one issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssueLease {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) run_id: String,
	pub(super) issue_state: String,
}
impl IssueLease {
	/// Local project identifier owning this lease.
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Issue identifier owning the lease.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Run identifier holding the lease.
	pub fn run_id(&self) -> &str {
		&self.run_id
	}

	/// Tracker state representing the dispatched run.
	pub fn issue_state(&self) -> &str {
		&self.issue_state
	}
}

/// Persistent run attempt metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunAttempt {
	pub(super) run_id: String,
	pub(super) issue_id: String,
	pub(super) attempt_number: i64,
	pub(super) status: String,
	pub(super) thread_id: Option<String>,
	pub(super) turn_id: Option<String>,
}
impl RunAttempt {
	/// Stable run identifier.
	pub fn run_id(&self) -> &str {
		&self.run_id
	}

	/// Issue identifier for the run.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Attempt number for this run.
	pub fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	/// Current local status for the run.
	pub fn status(&self) -> &str {
		&self.status
	}

	/// Thread identifier returned by `app-server`, when known.
	pub fn thread_id(&self) -> Option<&str> {
		self.thread_id.as_deref()
	}

	/// Latest turn identifier returned by `app-server`, when known.
	pub fn turn_id(&self) -> Option<&str> {
		self.turn_id.as_deref()
	}
}

/// Local control capability published by one running run attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunControlChannel {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) transport: String,
	pub(super) channel_path: PathBuf,
	pub(super) status: String,
	pub(super) published_at: String,
	pub(super) published_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
impl RunControlChannel {
	/// Local project identifier owning this control channel.
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Issue identifier owning this control channel.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Stable run identifier owning this control channel.
	pub fn run_id(&self) -> &str {
		&self.run_id
	}

	/// Attempt number owning this control channel.
	pub fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	/// Local transport mechanism for this control channel.
	pub fn transport(&self) -> &str {
		&self.transport
	}

	/// Local path used by this control channel.
	pub fn channel_path(&self) -> &Path {
		&self.channel_path
	}

	/// Runtime status for this control channel.
	pub fn status(&self) -> &str {
		&self.status
	}

	/// UTC timestamp when this control channel was first published.
	pub fn published_at(&self) -> &str {
		&self.published_at
	}

	/// UTC timestamp when this control channel was last updated.
	pub fn updated_at(&self) -> &str {
		&self.updated_at
	}
}

/// Local run-control request resolution and first audit row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunControlActionReceipt {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) thread_id: Option<String>,
	pub(super) turn_id: Option<String>,
	pub(super) current_thread_id: Option<String>,
	pub(super) current_turn_id: Option<String>,
	pub(super) source: String,
	pub(super) action: String,
	pub(super) outcome: String,
	pub(super) reason: String,
	pub(super) audit_record_id: i64,
	pub(super) metadata: Option<Value>,
	pub(super) context: Option<Value>,
	pub(super) channel: Option<RunControlChannel>,
}
#[allow(dead_code)]
impl RunControlActionReceipt {
	/// Project identifier used for the local audit scope.
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Issue identifier used for the local audit scope.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Run identifier used for the local audit scope.
	pub fn run_id(&self) -> &str {
		&self.run_id
	}

	/// Attempt number used for the local audit scope.
	pub fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	/// Requested thread identifier, when supplied.
	pub fn thread_id(&self) -> Option<&str> {
		self.thread_id.as_deref()
	}

	/// Requested turn identifier, when supplied.
	pub fn turn_id(&self) -> Option<&str> {
		self.turn_id.as_deref()
	}

	/// Current thread identifier observed while resolving the request.
	pub fn current_thread_id(&self) -> Option<&str> {
		self.current_thread_id.as_deref()
	}

	/// Current turn identifier observed while resolving the request.
	pub fn current_turn_id(&self) -> Option<&str> {
		self.current_turn_id.as_deref()
	}

	/// Local source that requested the action.
	pub fn source(&self) -> &str {
		&self.source
	}

	/// Requested control action.
	pub fn action(&self) -> &str {
		&self.action
	}

	/// Normalized audit outcome for the request resolution.
	pub fn outcome(&self) -> &str {
		&self.outcome
	}

	/// Normalized reason for the request resolution.
	pub fn reason(&self) -> &str {
		&self.reason
	}

	/// Private execution event row id for the request-resolution audit.
	pub fn audit_record_id(&self) -> i64 {
		self.audit_record_id
	}

	/// Optional compact action metadata captured with the audit event.
	pub fn metadata(&self) -> Option<&Value> {
		self.metadata.as_ref()
	}

	/// Optional compact lane context captured with the audit event.
	pub fn context(&self) -> Option<&Value> {
		self.context.as_ref()
	}

	/// Control channel selected for an accepted request.
	pub fn channel(&self) -> Option<&RunControlChannel> {
		self.channel.as_ref()
	}
}

/// One private, local-only execution event retained in the runtime SQLite ledger.
#[derive(Clone, Debug, PartialEq)]
pub struct PrivateExecutionEvent {
	pub(super) record_id: i64,
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) event_type: String,
	pub(super) payload: Value,
	pub(super) recorded_at: String,
	pub(super) recorded_at_unix: i64,
}
impl PrivateExecutionEvent {
	/// Monotonic local row id assigned by the runtime store.
	pub fn record_id(&self) -> i64 {
		self.record_id
	}

	/// Local project identifier owning the evidence row.
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Issue identifier for this private evidence row.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Run identifier for this private evidence row.
	pub fn run_id(&self) -> &str {
		&self.run_id
	}

	/// Attempt number for this private evidence row.
	pub fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	/// Private event type chosen by the runtime or issue-scoped tool path.
	pub fn event_type(&self) -> &str {
		&self.event_type
	}

	/// Structured JSON payload kept local to the runtime store.
	pub fn payload(&self) -> &Value {
		&self.payload
	}

	/// UTC timestamp when the runtime store recorded this row.
	pub fn recorded_at(&self) -> &str {
		&self.recorded_at
	}

	/// Unix timestamp when the runtime store recorded this row.
	pub fn recorded_at_unix(&self) -> i64 {
		self.recorded_at_unix
	}
}

/// Project-scoped operator view of one run attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectRunStatus {
	pub(super) run_id: String,
	pub(super) issue_id: String,
	pub(super) attempt_number: i64,
	pub(super) status: String,
	pub(super) thread_id: Option<String>,
	pub(super) turn_id: Option<String>,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
	pub(super) branch_name: Option<String>,
	pub(super) worktree_path: Option<PathBuf>,
	pub(super) run_lease: bool,
	pub(super) event_count: i64,
	pub(super) last_event_type: Option<String>,
	pub(super) last_event_at: Option<String>,
	pub(super) last_event_at_unix: Option<i64>,
	pub(super) control_channel: Option<RunControlChannel>,
	pub(super) child_agent_activity: Option<ChildAgentActivitySummary>,
	pub(super) protocol_activity: Option<ProtocolActivitySummary>,
	pub(super) recovery_source: String,
	pub(super) recovery_evidence: Vec<String>,
	pub(super) recovery_gaps: Vec<String>,
}
impl ProjectRunStatus {
	/// Stable run identifier.
	pub fn run_id(&self) -> &str {
		&self.run_id
	}

	/// Issue identifier for the run.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Attempt number for this run.
	pub fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	/// Current local status for the run.
	pub fn status(&self) -> &str {
		&self.status
	}

	/// Thread identifier returned by `app-server`, when known.
	pub fn thread_id(&self) -> Option<&str> {
		self.thread_id.as_deref()
	}

	/// Latest turn identifier returned by `app-server`, when known.
	pub fn turn_id(&self) -> Option<&str> {
		self.turn_id.as_deref()
	}

	/// Timestamp of the latest run-attempt status update.
	pub fn updated_at(&self) -> &str {
		&self.updated_at
	}

	/// Branch name for the retained lane, when known.
	pub fn branch_name(&self) -> Option<&str> {
		self.branch_name.as_deref()
	}

	/// Filesystem path to the retained worktree, when known.
	pub fn worktree_path(&self) -> Option<&Path> {
		self.worktree_path.as_deref()
	}

	/// Whether this run still holds the active local lease.
	pub fn run_lease(&self) -> bool {
		self.run_lease
	}

	/// Number of recorded protocol events for the run.
	pub fn event_count(&self) -> i64 {
		self.event_count
	}

	/// Latest recorded protocol event type, when one exists.
	pub fn last_event_type(&self) -> Option<&str> {
		self.last_event_type.as_deref()
	}

	/// Timestamp of the latest recorded protocol event, when one exists.
	pub fn last_event_at(&self) -> Option<&str> {
		self.last_event_at.as_deref()
	}

	/// Local control capability published by this run attempt, when one exists.
	pub fn control_channel(&self) -> Option<&RunControlChannel> {
		self.control_channel.as_ref()
	}

	pub(crate) fn child_agent_activity(&self) -> Option<&ChildAgentActivitySummary> {
		self.child_agent_activity.as_ref()
	}

	pub(crate) fn protocol_activity(&self) -> Option<&ProtocolActivitySummary> {
		self.protocol_activity.as_ref()
	}

	pub(crate) fn recovery_source(&self) -> &str {
		&self.recovery_source
	}

	pub(crate) fn recovery_evidence(&self) -> &[String] {
		&self.recovery_evidence
	}

	pub(crate) fn recovery_gaps(&self) -> &[String] {
		&self.recovery_gaps
	}

	/// Unix timestamp of the latest recorded protocol event, when one exists.
	pub(crate) fn last_event_at_unix(&self) -> Option<i64> {
		self.last_event_at_unix
	}

	pub(crate) fn last_run_activity_unix_epoch(&self) -> i64 {
		match self.last_event_at_unix {
			Some(last_event_at_unix) => self.updated_at_unix.max(last_event_at_unix),
			None => self.updated_at_unix,
		}
	}
}

/// Worktree mapping for one issue lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeMapping {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) branch_name: String,
	pub(super) worktree_path: PathBuf,
	pub(super) provenance: WorktreeProvenance,
}
impl WorktreeMapping {
	/// Local project identifier owning this lane.
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Issue identifier for this lane.
	pub fn issue_id(&self) -> &str {
		&self.issue_id
	}

	/// Branch name used for the lane.
	pub fn branch_name(&self) -> &str {
		&self.branch_name
	}

	/// Filesystem path to the worktree checkout.
	pub fn worktree_path(&self) -> &Path {
		&self.worktree_path
	}

	/// Durable provenance captured when Decodex recorded or migrated this mapping.
	pub fn provenance(&self) -> &WorktreeProvenance {
		&self.provenance
	}
}

/// Durable provenance for a retained worktree mapping.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorktreeProvenance {
	pub(super) source: String,
	pub(super) created_at_unix: Option<i64>,
	pub(super) updated_at_unix: Option<i64>,
}
impl WorktreeProvenance {
	/// Source that created or last classified this mapping.
	pub fn source(&self) -> &str {
		&self.source
	}

	/// Unix timestamp for when this mapping was first recorded, when available.
	pub fn created_at_unix(&self) -> Option<i64> {
		self.created_at_unix
	}

	/// Unix timestamp for when this mapping was last refreshed, when available.
	pub fn updated_at_unix(&self) -> Option<i64> {
		self.updated_at_unix
	}

	/// Whether this mapping was migrated from a legacy row without durable provenance.
	pub fn is_legacy_unknown(&self) -> bool {
		self.source == WORKTREE_PROVENANCE_LEGACY_UNKNOWN
	}
}

/// Project-scoped external connector backoff retained in the runtime store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectorBackoff {
	pub(super) project_id: String,
	pub(super) connector: String,
	pub(super) sync_phase: String,
	pub(super) quota_class: String,
	pub(super) reset_unix_epoch: i64,
	pub(super) reset_source: String,
	pub(super) warning: String,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
#[allow(dead_code)]
impl ConnectorBackoff {
	/// Local project identifier affected by this connector backoff.
	pub fn project_id(&self) -> &str {
		&self.project_id
	}

	/// Connector name, such as `linear`.
	pub fn connector(&self) -> &str {
		&self.connector
	}

	/// Runtime phase that last observed the connector backoff.
	pub fn sync_phase(&self) -> &str {
		&self.sync_phase
	}

	/// Quota class backing the pause.
	pub fn quota_class(&self) -> &str {
		&self.quota_class
	}

	/// Unix epoch when Decodex may retry the connector.
	pub fn reset_unix_epoch(&self) -> i64 {
		self.reset_unix_epoch
	}

	/// Source for the reset time.
	pub fn reset_source(&self) -> &str {
		&self.reset_source
	}

	/// Snapshot warning represented by this backoff.
	pub fn warning(&self) -> &str {
		&self.warning
	}

	/// Timestamp when Decodex stored the backoff.
	pub fn updated_at(&self) -> &str {
		&self.updated_at
	}

	/// Unix timestamp when Decodex stored the backoff.
	pub fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

/// Unix file-descriptor handoff for a daemon-planned lease adopted by a child process.
pub struct PreacquiredLeaseGuards {
	/// The inherited issue-claim lock fd that keeps one issue single-owned across processes.
	pub issue_claim_fd: i32,
	/// The inherited dispatch-slot lock fd used for shared handoff bookkeeping.
	pub dispatch_slot_fd: i32,
	/// The inherited shared dispatch-slot index used for local guard bookkeeping.
	pub dispatch_slot_index: usize,
}

/// SQLite-backed Loop/Decision Contract retained by the local runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecisionContractRecord {
	pub(super) project_id: String,
	pub(super) source_issue_id: Option<String>,
	pub(super) contract: DecisionContract,
	pub(super) status: DecisionContractStatus,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
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
	pub(super) project_id: String,
	pub(super) objective: AutonomyObjectiveContract,
	pub(super) state: AutonomyObjectiveState,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
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
	pub(super) project_id: String,
	pub(super) signal: AutonomySignal,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
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
	pub(super) project_id: String,
	pub(super) proposal: AutonomyProposal,
	pub(super) state: AutonomyProposalState,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
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
	pub(super) project_id: String,
	pub(super) program: ExecutionProgram,
	pub(super) source_contract_id: Option<String>,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
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
	pub(super) project_id: String,
	pub(super) program_id: String,
	pub(super) plan_id: String,
	pub(super) intake_kind: String,
	pub(super) source_contract_id: Option<String>,
	pub(super) accepted_contract_fingerprint: String,
	pub(super) public_summary: String,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
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
	pub(super) project_id: String,
	pub(super) program_id: String,
	pub(super) node_id: String,
	pub(super) issue_id: String,
	pub(super) issue_identifier: String,
	pub(super) issue_state: String,
	pub(super) queue_intent: String,
	pub(super) has_active_label: bool,
	pub(super) has_opt_out_label: bool,
	pub(super) has_needs_attention_label: bool,
	pub(super) has_generic_dispatch_briefing: bool,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
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

/// Latest runtime-owned review-policy checkpoint for one run phase.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewPolicyCheckpoint {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) phase: String,
	pub(super) status: String,
	pub(super) head_sha: String,
	pub(super) nonclean_rounds: i64,
	pub(super) details_json: String,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
#[cfg_attr(not(test), allow(dead_code))]
impl ReviewPolicyCheckpoint {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn issue_id(&self) -> &str {
		&self.issue_id
	}

	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn phase(&self) -> &str {
		&self.phase
	}

	pub(crate) fn status(&self) -> &str {
		&self.status
	}

	pub(crate) fn head_sha(&self) -> &str {
		&self.head_sha
	}

	pub(crate) fn nonclean_rounds(&self) -> i64 {
		self.nonclean_rounds
	}

	pub(crate) fn details_json(&self) -> &str {
		&self.details_json
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

/// Latest loop-guardrail checkpoint for one issue and stop reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LoopGuardrailCheckpoint {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) reason: String,
	pub(super) fingerprint: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) consecutive_count: i64,
	pub(super) details_json: String,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
impl LoopGuardrailCheckpoint {
	#[cfg(test)]
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	#[cfg(test)]
	pub(crate) fn issue_id(&self) -> &str {
		&self.issue_id
	}

	pub(crate) fn reason(&self) -> &str {
		&self.reason
	}

	pub(crate) fn fingerprint(&self) -> &str {
		&self.fingerprint
	}

	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn consecutive_count(&self) -> i64 {
		self.consecutive_count
	}

	pub(crate) fn details_json(&self) -> &str {
		&self.details_json
	}

	#[cfg(test)]
	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	#[cfg(test)]
	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

/// Foundation request for resolving a local run-control action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RunControlActionRequest<'a> {
	/// Requested project identifier.
	pub(crate) project_id: &'a str,
	/// Requested issue identifier.
	pub(crate) issue_id: &'a str,
	/// Requested run identifier.
	pub(crate) run_id: &'a str,
	/// Requested attempt number.
	pub(crate) attempt_number: i64,
	/// Requested app-server thread identifier, when known.
	pub(crate) thread_id: Option<&'a str>,
	/// Requested current app-server turn identifier, when known.
	pub(crate) turn_id: Option<&'a str>,
	/// Local source that requested the action.
	pub(crate) source: &'a str,
	/// Requested control action.
	pub(crate) action: &'a str,
	/// Optional caller timeout budget in milliseconds.
	pub(crate) timeout_ms: Option<i64>,
	/// Optional compact, non-secret action metadata to include in audit evidence.
	pub(crate) metadata: Option<&'a Value>,
	/// Optional compact lane context to include in audit evidence.
	pub(crate) context: Option<&'a Value>,
}

/// Follow-up outcome for a run-control action handled after initial resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RunControlActionOutcomeRequest<'a> {
	/// Project identifier used for local audit scoping.
	pub(crate) project_id: &'a str,
	/// Issue identifier used for local audit scoping.
	pub(crate) issue_id: &'a str,
	/// Run identifier used for local audit scoping.
	pub(crate) run_id: &'a str,
	/// Attempt number used for local audit scoping.
	pub(crate) attempt_number: i64,
	/// Requested app-server thread identifier, when known.
	pub(crate) thread_id: Option<&'a str>,
	/// Requested expected app-server turn identifier, when known.
	pub(crate) turn_id: Option<&'a str>,
	/// Current app-server thread identifier observed while handling the request.
	pub(crate) current_thread_id: Option<&'a str>,
	/// Current app-server turn identifier observed while handling the request.
	pub(crate) current_turn_id: Option<&'a str>,
	/// Local source that requested the action.
	pub(crate) source: &'a str,
	/// Requested control action.
	pub(crate) action: &'a str,
	/// Follow-up outcome.
	pub(crate) outcome: &'a str,
	/// Normalized outcome reason.
	pub(crate) reason: &'a str,
	/// Parent request-resolution audit record id, when known.
	pub(crate) parent_record_id: Option<i64>,
	/// Optional caller timeout budget in milliseconds.
	pub(crate) timeout_ms: Option<i64>,
	/// Optional compact, non-secret action metadata to include in audit evidence.
	pub(crate) metadata: Option<&'a Value>,
	/// Control channel that carried the request, when known.
	pub(crate) channel: Option<&'a RunControlChannel>,
}

/// Registered repo target managed by the local Decodex control plane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProjectRegistration {
	pub(super) service_id: String,
	pub(super) config_path: PathBuf,
	pub(super) repo_root: PathBuf,
	pub(super) worktree_root: PathBuf,
	pub(super) workflow_path: PathBuf,
	pub(super) tracker_api_key_env_var: String,
	pub(super) github_token_env_var: String,
	pub(super) enabled: bool,
	pub(super) config_fingerprint: String,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
impl ProjectRegistration {
	/// Build a registry row from a Decodex project config.
	pub(crate) fn from_config(
		service_id: &str,
		config_path: &Path,
		config: &ServiceConfig,
		enabled: bool,
		config_fingerprint: &str,
	) -> Self {
		let now = timestamp_parts();

		Self {
			service_id: service_id.to_owned(),
			config_path: config_path.to_path_buf(),
			repo_root: config.repo_root().to_path_buf(),
			worktree_root: config.worktree_root().to_path_buf(),
			workflow_path: config.workflow_path().to_path_buf(),
			tracker_api_key_env_var: config.tracker().api_key_env_var().to_owned(),
			github_token_env_var: config.github().token_env_var().to_owned(),
			enabled,
			config_fingerprint: config_fingerprint.to_owned(),
			updated_at: now.text,
			updated_at_unix: now.unix,
		}
	}

	/// Stable service id from the project config.
	pub(crate) fn service_id(&self) -> &str {
		&self.service_id
	}

	/// Absolute config path registered for this project.
	pub(crate) fn config_path(&self) -> &Path {
		&self.config_path
	}

	/// Absolute repository root for this project.
	pub(crate) fn repo_root(&self) -> &Path {
		&self.repo_root
	}

	/// Absolute worktree root for this project.
	pub(crate) fn worktree_root(&self) -> &Path {
		&self.worktree_root
	}

	/// Absolute workflow path registered for this project.
	pub(crate) fn workflow_path(&self) -> &Path {
		&self.workflow_path
	}

	/// Environment variable name for the tracker API key.
	pub(crate) fn tracker_api_key_env_var(&self) -> &str {
		&self.tracker_api_key_env_var
	}

	/// Environment variable name for the GitHub token.
	pub(crate) fn github_token_env_var(&self) -> &str {
		&self.github_token_env_var
	}

	/// Whether the project participates in `decodex serve`.
	pub(crate) fn enabled(&self) -> bool {
		self.enabled
	}

	/// Last config fingerprint registered for this project.
	pub(crate) fn config_fingerprint(&self) -> &str {
		&self.config_fingerprint
	}

	/// Last registry update timestamp.
	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	/// Last registry update timestamp as Unix epoch seconds.
	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}

	/// Set whether the registered project is enabled.
	pub(super) fn set_enabled(&mut self, enabled: bool) {
		self.enabled = enabled;

		let now = timestamp_parts();

		self.updated_at = now.text;
		self.updated_at_unix = now.unix;
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ChildAgentActivitySummary {
	pub(crate) buckets: Vec<ChildAgentActivityBucket>,
	pub(crate) current_bucket: Option<String>,
	pub(crate) current_detail: Option<String>,
	pub(crate) current_started_unix_epoch: Option<i64>,
	pub(crate) current_elapsed_seconds: Option<i64>,
	pub(crate) wall_seconds: i64,
	pub(crate) event_count: i64,
	pub(crate) tool_call_count: i64,
	pub(crate) input_tokens_current: Option<i64>,
	pub(crate) input_tokens_max: Option<i64>,
	pub(crate) input_tokens_cumulative: i64,
	pub(crate) output_tokens_cumulative: i64,
	pub(crate) largest_tool_output_bytes: Option<i64>,
	pub(crate) largest_tool_output_tool: Option<String>,
	pub(crate) large_output_warnings: Vec<String>,
}
impl ChildAgentActivitySummary {
	pub(crate) fn sealed_durable(mut self) -> Self {
		self.seal_open_interval();

		self
	}

	pub(crate) fn live_projection(mut self, now_unix_epoch: i64) -> Self {
		let observed_elapsed_seconds =
			self.current_elapsed_seconds.filter(|elapsed| *elapsed >= 0).unwrap_or(0);
		let current_elapsed_seconds = self.current_started_unix_epoch.and_then(|started_at| {
			now_unix_epoch.checked_sub(started_at).filter(|elapsed| *elapsed >= 0)
		});
		let open_delta_seconds = current_elapsed_seconds.and_then(|elapsed| {
			elapsed.checked_sub(observed_elapsed_seconds).filter(|delta| *delta > 0)
		});

		self.current_elapsed_seconds = current_elapsed_seconds;

		let current_bucket = self.current_bucket.clone();

		if let (Some(current_bucket), Some(open_delta_seconds)) =
			(current_bucket, open_delta_seconds)
		{
			let bucket = self.bucket_mut(&current_bucket);

			bucket.wall_seconds = bucket.wall_seconds.saturating_add(open_delta_seconds);
		}

		self
	}

	fn seal_open_interval(&mut self) {
		self.current_bucket = None;
		self.current_detail = None;
		self.current_started_unix_epoch = None;
		self.current_elapsed_seconds = None;
	}

	fn bucket_mut(&mut self, name: &str) -> &mut ChildAgentActivityBucket {
		if let Some(index) = self.buckets.iter().position(|bucket| bucket.name == name) {
			return &mut self.buckets[index];
		}

		self.buckets.push(ChildAgentActivityBucket {
			name: name.to_owned(),
			..ChildAgentActivityBucket::default()
		});

		let last_index = self.buckets.len().saturating_sub(1);

		&mut self.buckets[last_index]
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ChildAgentActivityBucket {
	pub(crate) name: String,
	pub(crate) wall_seconds: i64,
	pub(crate) event_count: i64,
	pub(crate) tool_call_count: i64,
	pub(crate) input_tokens: i64,
	pub(crate) output_tokens: i64,
	pub(crate) output_bytes: i64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProtocolActivitySummary {
	pub(crate) turn_status: Option<String>,
	pub(crate) waiting_reason: Option<String>,
	pub(crate) rate_limit_status: Option<String>,
	pub(crate) recent_events: Vec<ProtocolActivityEventSummary>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProtocolActivityEventSummary {
	pub(crate) event_type: String,
	pub(crate) category: String,
	pub(crate) detail: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct CodexAccountActivitySummary {
	pub(crate) account_fingerprint: String,
	pub(crate) email: Option<String>,
	pub(crate) plan_type: Option<String>,
	pub(crate) status: String,
	pub(crate) refresh_status: String,
	pub(crate) checked_at_unix_epoch: Option<i64>,
	pub(crate) selected_at_unix_epoch: Option<i64>,
	pub(crate) primary_window_seconds: Option<i64>,
	pub(crate) primary_remaining_percent: Option<i64>,
	pub(crate) primary_resets_at_unix_epoch: Option<i64>,
	pub(crate) secondary_window_seconds: Option<i64>,
	pub(crate) secondary_remaining_percent: Option<i64>,
	pub(crate) secondary_resets_at_unix_epoch: Option<i64>,
	pub(crate) credits_has_credits: Option<bool>,
	pub(crate) credits_unlimited: Option<bool>,
	pub(crate) credits_balance: Option<String>,
	pub(crate) rate_limit_reached_type: Option<String>,
	pub(crate) cooldown_until_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_display_name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_username: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_checked_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_lifetime_tokens: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_peak_daily_tokens: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_longest_task_seconds: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_current_streak_days: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_longest_streak_days: Option<i64>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) profile_daily_usage: Vec<CodexAccountProfileDailyUsageSummary>,
	pub(crate) note: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct CodexAccountProfileDailyUsageSummary {
	pub(crate) date: String,
	pub(crate) tokens: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RunActivityMarker {
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) process_id: Option<u32>,
	pub(super) host_boot_id: Option<String>,
	pub(super) process_start_identity: Option<String>,
	pub(super) last_activity_unix_epoch: Option<i64>,
	pub(super) last_protocol_activity_unix_epoch: Option<i64>,
	pub(super) last_progress_unix_epoch: Option<i64>,
	pub(super) current_operation: Option<String>,
	pub(super) thread_id: Option<String>,
	pub(super) turn_id: Option<String>,
	pub(super) thread_status: Option<String>,
	pub(super) thread_active_flags: Vec<String>,
	pub(super) event_count: Option<i64>,
	pub(super) last_event_type: Option<String>,
	pub(super) effective_model: Option<String>,
	pub(super) effective_model_provider: Option<String>,
	pub(super) effective_cwd: Option<String>,
	pub(super) effective_approval_policy: Option<String>,
	pub(super) effective_approvals_reviewer: Option<String>,
	pub(super) effective_sandbox_mode: Option<String>,
	pub(super) child_agent_activity: Option<ChildAgentActivitySummary>,
	pub(super) protocol_activity: Option<ProtocolActivitySummary>,
	pub(super) account: Option<CodexAccountActivitySummary>,
	pub(super) accounts: Vec<CodexAccountActivitySummary>,
	pub(super) retry_budget_attempt_count: Option<i64>,
	pub(super) retry_kind: Option<String>,
	pub(super) retry_ready_at_unix_epoch: Option<i64>,
}
impl RunActivityMarker {
	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn process_id(&self) -> Option<u32> {
		self.process_id
	}

	pub(crate) fn host_boot_id(&self) -> Option<&str> {
		self.host_boot_id.as_deref()
	}

	pub(crate) fn process_start_identity(&self) -> Option<&str> {
		self.process_start_identity.as_deref()
	}

	pub(crate) fn last_activity_unix_epoch(&self) -> Option<i64> {
		self.last_activity_unix_epoch
	}

	pub(crate) fn last_protocol_activity_unix_epoch(&self) -> Option<i64> {
		self.last_protocol_activity_unix_epoch
	}

	pub(crate) fn last_progress_unix_epoch(&self) -> Option<i64> {
		self.last_progress_unix_epoch
	}

	pub(crate) fn current_operation(&self) -> Option<&str> {
		self.current_operation.as_deref()
	}

	pub(crate) fn thread_id(&self) -> Option<&str> {
		self.thread_id.as_deref()
	}

	pub(crate) fn turn_id(&self) -> Option<&str> {
		self.turn_id.as_deref()
	}

	pub(crate) fn thread_status(&self) -> Option<&str> {
		self.thread_status.as_deref()
	}

	pub(crate) fn thread_active_flags(&self) -> &[String] {
		&self.thread_active_flags
	}

	pub(crate) fn event_count(&self) -> i64 {
		self.event_count.unwrap_or(0)
	}

	pub(crate) fn last_event_type(&self) -> Option<&str> {
		self.last_event_type.as_deref()
	}

	pub(crate) fn effective_model(&self) -> Option<&str> {
		self.effective_model.as_deref()
	}

	pub(crate) fn effective_model_provider(&self) -> Option<&str> {
		self.effective_model_provider.as_deref()
	}

	pub(crate) fn effective_cwd(&self) -> Option<&str> {
		self.effective_cwd.as_deref()
	}

	pub(crate) fn effective_approval_policy(&self) -> Option<&str> {
		self.effective_approval_policy.as_deref()
	}

	pub(crate) fn effective_approvals_reviewer(&self) -> Option<&str> {
		self.effective_approvals_reviewer.as_deref()
	}

	pub(crate) fn effective_sandbox_mode(&self) -> Option<&str> {
		self.effective_sandbox_mode.as_deref()
	}

	pub(crate) fn child_agent_activity(&self) -> Option<&ChildAgentActivitySummary> {
		self.child_agent_activity.as_ref()
	}

	pub(crate) fn protocol_activity(&self) -> Option<&ProtocolActivitySummary> {
		self.protocol_activity.as_ref()
	}

	pub(crate) fn account(&self) -> Option<&CodexAccountActivitySummary> {
		self.account.as_ref()
	}

	pub(crate) fn accounts(&self) -> &[CodexAccountActivitySummary] {
		&self.accounts
	}

	pub(crate) fn retry_kind(&self) -> Option<&str> {
		self.retry_kind.as_deref()
	}

	pub(crate) fn retry_ready_at_unix_epoch(&self) -> Option<i64> {
		self.retry_ready_at_unix_epoch
	}

	pub(crate) fn retry_budget_attempt_count(&self) -> Option<i64> {
		self.retry_budget_attempt_count
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewHandoffMarker {
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) branch_name: String,
	pub(super) pr_url: String,
	pub(super) target_base_ref_name: Option<String>,
	pub(super) pr_head_ref_name: String,
	pub(super) pr_head_oid: String,
}
impl ReviewHandoffMarker {
	pub(crate) fn new(
		run_id: impl Into<String>,
		attempt_number: i64,
		branch_name: impl Into<String>,
		pr_url: impl Into<String>,
		target_base_ref_name: impl Into<String>,
		pr_head_ref_name: impl Into<String>,
		pr_head_oid: impl Into<String>,
	) -> Self {
		Self {
			run_id: run_id.into(),
			attempt_number,
			branch_name: branch_name.into(),
			pr_url: pr_url.into(),
			target_base_ref_name: Some(target_base_ref_name.into()),
			pr_head_ref_name: pr_head_ref_name.into(),
			pr_head_oid: pr_head_oid.into(),
		}
	}

	pub(crate) fn branch_name(&self) -> &str {
		&self.branch_name
	}

	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn pr_url(&self) -> &str {
		&self.pr_url
	}

	pub(crate) fn target_base_ref_name(&self) -> Option<&str> {
		self.target_base_ref_name.as_deref()
	}

	pub(crate) fn pr_head_ref_name(&self) -> &str {
		&self.pr_head_ref_name
	}

	pub(crate) fn pr_head_oid(&self) -> &str {
		&self.pr_head_oid
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewOrchestrationMarker {
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) branch_name: String,
	pub(super) pr_url: String,
	pub(super) head_sha: String,
	pub(super) phase: String,
	pub(super) request_comment_database_id: Option<i64>,
	pub(super) request_created_at_unix_epoch: Option<i64>,
	pub(super) request_description_thumbs_up_count: Option<usize>,
	pub(super) request_retry_count: i64,
	pub(super) external_round_count: i64,
	pub(super) auto_merge_enabled_at_unix_epoch: Option<i64>,
}
impl ReviewOrchestrationMarker {
	#[allow(clippy::too_many_arguments)]
	pub(crate) fn new(
		run_id: impl Into<String>,
		attempt_number: i64,
		branch_name: impl Into<String>,
		pr_url: impl Into<String>,
		head_sha: impl Into<String>,
		phase: impl Into<String>,
		request_comment_database_id: Option<i64>,
		request_created_at_unix_epoch: Option<i64>,
		request_description_thumbs_up_count: Option<usize>,
		request_retry_count: i64,
		external_round_count: i64,
		auto_merge_enabled_at_unix_epoch: Option<i64>,
	) -> Self {
		Self {
			run_id: run_id.into(),
			attempt_number,
			branch_name: branch_name.into(),
			pr_url: pr_url.into(),
			head_sha: head_sha.into(),
			phase: phase.into(),
			request_comment_database_id,
			request_created_at_unix_epoch,
			request_description_thumbs_up_count,
			request_retry_count,
			external_round_count,
			auto_merge_enabled_at_unix_epoch,
		}
	}

	pub(crate) fn branch_name(&self) -> &str {
		&self.branch_name
	}

	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn pr_url(&self) -> &str {
		&self.pr_url
	}

	pub(crate) fn head_sha(&self) -> &str {
		&self.head_sha
	}

	pub(crate) fn phase(&self) -> &str {
		&self.phase
	}

	pub(crate) fn request_comment_database_id(&self) -> Option<i64> {
		self.request_comment_database_id
	}

	pub(crate) fn request_created_at_unix_epoch(&self) -> Option<i64> {
		self.request_created_at_unix_epoch
	}

	pub(crate) fn request_description_thumbs_up_count(&self) -> Option<usize> {
		self.request_description_thumbs_up_count
	}

	pub(crate) fn request_retry_count(&self) -> i64 {
		self.request_retry_count
	}

	pub(crate) fn external_round_count(&self) -> i64 {
		self.external_round_count
	}

	pub(crate) fn auto_merge_enabled_at_unix_epoch(&self) -> Option<i64> {
		self.auto_merge_enabled_at_unix_epoch
	}
}

/// Runtime-owned review lifecycle record for one retained PR-backed lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewLifecycleRecord {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) branch_name: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) pr_url: String,
	pub(super) target_base_ref_name: Option<String>,
	pub(super) pr_head_ref_name: String,
	pub(super) pr_head_oid: String,
	pub(super) head_sha: String,
	pub(super) phase: String,
	pub(super) request_comment_database_id: Option<i64>,
	pub(super) request_created_at_unix_epoch: Option<i64>,
	pub(super) request_description_thumbs_up_count: Option<usize>,
	pub(super) request_retry_count: i64,
	pub(super) external_round_count: i64,
	pub(super) auto_merge_enabled_at_unix_epoch: Option<i64>,
	pub(super) landing_state: String,
	pub(super) closeout_state: String,
	pub(super) repair_attempt_count: i64,
	pub(super) evidence_json: String,
	pub(super) next_action: String,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
#[allow(dead_code)]
impl ReviewLifecycleRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn issue_id(&self) -> &str {
		&self.issue_id
	}

	pub(crate) fn branch_name(&self) -> &str {
		&self.branch_name
	}

	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn pr_url(&self) -> &str {
		&self.pr_url
	}

	pub(crate) fn target_base_ref_name(&self) -> Option<&str> {
		self.target_base_ref_name.as_deref()
	}

	pub(crate) fn pr_head_ref_name(&self) -> &str {
		&self.pr_head_ref_name
	}

	pub(crate) fn pr_head_oid(&self) -> &str {
		&self.pr_head_oid
	}

	pub(crate) fn head_sha(&self) -> &str {
		&self.head_sha
	}

	pub(crate) fn phase(&self) -> &str {
		&self.phase
	}

	pub(crate) fn request_comment_database_id(&self) -> Option<i64> {
		self.request_comment_database_id
	}

	pub(crate) fn request_created_at_unix_epoch(&self) -> Option<i64> {
		self.request_created_at_unix_epoch
	}

	pub(crate) fn request_description_thumbs_up_count(&self) -> Option<usize> {
		self.request_description_thumbs_up_count
	}

	pub(crate) fn request_retry_count(&self) -> i64 {
		self.request_retry_count
	}

	pub(crate) fn external_round_count(&self) -> i64 {
		self.external_round_count
	}

	pub(crate) fn auto_merge_enabled_at_unix_epoch(&self) -> Option<i64> {
		self.auto_merge_enabled_at_unix_epoch
	}

	pub(crate) fn landing_state(&self) -> &str {
		&self.landing_state
	}

	pub(crate) fn closeout_state(&self) -> &str {
		&self.closeout_state
	}

	pub(crate) fn repair_attempt_count(&self) -> i64 {
		self.repair_attempt_count
	}

	pub(crate) fn evidence_json(&self) -> &str {
		&self.evidence_json
	}

	pub(crate) fn next_action(&self) -> &str {
		&self.next_action
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}

pub(crate) fn worktree_provenance(
	source: impl Into<String>,
	created_at_unix: Option<i64>,
	updated_at_unix: Option<i64>,
) -> WorktreeProvenance {
	WorktreeProvenance { source: source.into(), created_at_unix, updated_at_unix }
}
