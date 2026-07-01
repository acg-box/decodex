use std::{
	collections::{self, BTreeMap},
	fs::{self, OpenOptions},
	io::Write,
	path::{Path, PathBuf},
	process,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use crate::{
	prelude::{Result, eyre},
	runtime,
};

use super::{
	ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE, ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE,
	ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE, AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE,
	AUTHORITY_DECISION_REQUEST_EVENT_TYPE, EvidenceRequest, OperatorConnectorBackoffStatus,
	OperatorGitHubCliAuthority, OperatorHistoryLaneStatus, OperatorHistoryLedgerOutcome,
	OperatorPostReviewLaneStatus, OperatorProjectStatus, OperatorQueuedIssueStatus,
	OperatorRunStatus, OperatorStatusSnapshot, OperatorWorktreeStatus,
	PHASE_ACCEPTANCE_CHECK_EVENT_TYPE, ProjectRunStatus, ServiceConfig, StateStore,
	current_timestamp,
	harness_improvement::{
		HarnessImprovementCandidateSummary, harness_improvement_candidates_from_private_events,
	},
	operator_run_issue_identifier_from_fields, relative_worktree_path_for_path,
	rendered_recovery_worktrees, state,
	status_summary::operator_run_has_stale_execution_without_known_process,
};

mod capsules;
mod files;
mod private_readback;
mod snapshot;

use capsules::{
	agent_connector_backoff, agent_recovery_contract, agent_recovery_worktree,
	build_agent_blockers, build_run_capsules, run_capsule_ref,
};
use files::write_agent_evidence_files;
pub(in crate::orchestrator) use private_readback::{
	agent_private_evidence_ref, build_private_evidence_readback,
	private_evidence_ref_for_run_fields, render_private_evidence_readback,
	render_private_evidence_reference,
};
pub(in crate::orchestrator) use snapshot::{
	render_agent_evidence_write_result, write_agent_evidence_best_effort,
	write_agent_evidence_snapshot,
};

const AGENT_HANDOFF_INDEX_SCHEMA: &str = "decodex.agent_handoff_index/1";
const AGENT_BLOCKER_SNAPSHOT_SCHEMA: &str = "decodex.blocker_snapshot/1";
const AGENT_RUN_CAPSULE_SCHEMA: &str = "decodex.run_capsule/1";
const AGENT_EVIDENCE_EVENT_SCHEMA: &str = "decodex.agent_evidence_event/1";
const PRIVATE_EVIDENCE_READBACK_SCHEMA: &str = "decodex.private_execution_evidence_readback/1";
const PRIVATE_EVIDENCE_PAYLOAD_PREVIEW_LIMIT: usize = 160;
const REVIEW_CHECKPOINT_EVENT_TYPE: &str = "review_checkpoint";
const HANDOFF_INDEX_FILE_NAME: &str = "handoff-index.json";
const BLOCKERS_DIR_NAME: &str = "blockers";
const RUNS_DIR_NAME: &str = "runs";
const EVENTS_FILE_NAME: &str = "events.jsonl";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::orchestrator) enum AgentEvidenceSource {
	DiagnoseCommand,
	ServeTick,
}
impl AgentEvidenceSource {
	fn as_str(self) -> &'static str {
		match self {
			Self::DiagnoseCommand => "diagnose_command",
			Self::ServeTick => "serve_tick",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct AgentEvidenceWriteResult {
	pub(in crate::orchestrator) project_id: String,
	pub(in crate::orchestrator) handoff_index_path: String,
	pub(in crate::orchestrator) handoff_index: AgentHandoffIndex,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct AgentHandoffIndex {
	schema: &'static str,
	project_id: String,
	generated_at: String,
	source: String,
	evidence_root: String,
	handoff_index_path: String,
	blockers_dir: String,
	runs_dir: String,
	events_path: String,
	pub(in crate::orchestrator) summary: AgentEvidenceSummary,
	github_cli_authority: Option<OperatorGitHubCliAuthority>,
	warnings: Vec<String>,
	connector_backoffs: Vec<AgentConnectorBackoff>,
	blockers: Vec<AgentBlocker>,
	run_capsules: Vec<AgentRunCapsuleRef>,
	recovery_worktrees: Vec<AgentRecoveryWorktree>,
	recovery_contracts: Vec<AgentRecoveryContract>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct AgentEvidenceSummary {
	pub(in crate::orchestrator) project_count: usize,
	pub(in crate::orchestrator) current_lane_count: usize,
	pub(in crate::orchestrator) recent_run_count: usize,
	pub(in crate::orchestrator) history_lane_count: usize,
	pub(in crate::orchestrator) queued_candidate_count: usize,
	pub(in crate::orchestrator) post_review_lane_count: usize,
	pub(in crate::orchestrator) recovery_worktree_count: usize,
	pub(in crate::orchestrator) blocker_count: usize,
	pub(in crate::orchestrator) run_capsule_count: usize,
	pub(in crate::orchestrator) connector_backoff_count: usize,
	pub(in crate::orchestrator) warning_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentConnectorBackoff {
	evidence_ref: String,
	connector: String,
	sync_phase: String,
	quota_class: String,
	reset_at: String,
	reset_unix_epoch: i64,
	reset_source: String,
	retry_after_seconds: i64,
	warning: String,
	next_action: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct AgentPrivateEvidenceRef {
	pub(in crate::orchestrator) evidence_ref: String,
	pub(in crate::orchestrator) source: String,
	pub(in crate::orchestrator) default_view: String,
	pub(in crate::orchestrator) read_command: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentBlocker {
	evidence_ref: String,
	project_id: String,
	surface: String,
	issue_id: Option<String>,
	issue_identifier: Option<String>,
	run_id: Option<String>,
	attempt_number: Option<i64>,
	classification: String,
	reason_code: String,
	reason: String,
	next_action: String,
	blocker_snapshot_path: String,
	related_run_capsule_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentBlockerSnapshot {
	schema: &'static str,
	project_id: String,
	generated_at: String,
	issue_id: Option<String>,
	issue_identifier: Option<String>,
	blockers: Vec<AgentBlocker>,
	related_run_capsules: Vec<AgentRunCapsuleRef>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentRunCapsuleRef {
	evidence_ref: String,
	run_id: String,
	issue_id: String,
	issue_identifier: Option<String>,
	attempt_number: i64,
	status: String,
	phase: String,
	current_operation: String,
	path: String,
	private_evidence: AgentPrivateEvidenceRef,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentRunCapsule {
	schema: &'static str,
	evidence_ref: String,
	project_id: String,
	generated_at: String,
	path: String,
	run_id: String,
	issue_id: String,
	issue_identifier: Option<String>,
	title: Option<String>,
	attempt_number: i64,
	status: String,
	attempt_status: String,
	phase: String,
	wait_reason: Option<String>,
	current_operation: String,
	queue_lease_state: String,
	execution_liveness: String,
	ownership_state: String,
	liveness_state: String,
	policy_state: String,
	terminalization_state: String,
	lane_control_next_action: String,
	lane_control_conditions: Vec<String>,
	run_lease: bool,
	continuation_pending: bool,
	suspected_stall: bool,
	thread_id: Option<String>,
	turn_id: Option<String>,
	thread_status: Option<String>,
	thread_active_flags: Vec<String>,
	interactive_requested: bool,
	process_id: Option<u32>,
	process_alive: Option<bool>,
	process_liveness_reason: Option<String>,
	event_count: i64,
	last_event_type: Option<String>,
	last_event_at: Option<String>,
	last_run_activity_at: Option<String>,
	last_protocol_activity_at: Option<String>,
	last_progress_at: Option<String>,
	idle_for_seconds: Option<i64>,
	protocol_idle_for_seconds: Option<i64>,
	retry_kind: Option<String>,
	next_retry_at: Option<String>,
	effective_model: Option<String>,
	effective_model_provider: Option<String>,
	effective_cwd: Option<String>,
	effective_approval_policy: Option<String>,
	effective_approvals_reviewer: Option<String>,
	effective_sandbox_mode: Option<String>,
	branch_name: Option<String>,
	worktree_path: Option<String>,
	private_evidence: AgentPrivateEvidenceRef,
	ledger_outcome: Option<AgentRunLedgerOutcome>,
	diagnosis: AgentRunDiagnosis,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentRunLedgerOutcome {
	ledger_status: String,
	final_outcome: String,
	final_event_type: Option<String>,
	final_event_at: Option<String>,
	summary: Option<String>,
	pr_url: Option<String>,
	commit_sha: Option<String>,
	closeout_status: Option<String>,
	needs_attention_reason: Option<String>,
	record_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentRunDiagnosis {
	attention_required: bool,
	reason_code: Option<String>,
	next_action: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentRecoveryWorktree {
	issue_id: String,
	issue_identifier: Option<String>,
	issue_state: Option<String>,
	branch_name: String,
	worktree_path: String,
	role: String,
	ownership: String,
	ownership_reason: String,
	hygiene_classification: Option<String>,
	hygiene_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentRecoveryContract {
	evidence_ref: String,
	kind: String,
	issue_identifier: Option<String>,
	reason_code: String,
	command: Option<String>,
	next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentEvidenceEvent {
	schema: &'static str,
	project_id: String,
	generated_at: String,
	source: String,
	handoff_index_path: String,
	blocker_count: usize,
	run_capsule_count: usize,
	warning_count: usize,
	connector_backoff_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceReadback {
	pub(in crate::orchestrator) schema: &'static str,
	pub(in crate::orchestrator) project_id: String,
	pub(in crate::orchestrator) issue_selector: String,
	pub(in crate::orchestrator) issue_id: String,
	pub(in crate::orchestrator) issue_identifier: Option<String>,
	pub(in crate::orchestrator) run_id: String,
	pub(in crate::orchestrator) attempt_number: i64,
	pub(in crate::orchestrator) source: &'static str,
	pub(in crate::orchestrator) evidence_ref: String,
	pub(in crate::orchestrator) read_command: String,
	pub(in crate::orchestrator) payload_mode: &'static str,
	pub(in crate::orchestrator) event_count: usize,
	pub(in crate::orchestrator) latest_event_type: Option<String>,
	pub(in crate::orchestrator) latest_event_at: Option<String>,
	pub(in crate::orchestrator) review_checkpoints: Vec<PrivateEvidenceReviewCheckpointSummary>,
	pub(in crate::orchestrator) repo_gate_failures: Vec<PrivateEvidenceRepoGateFailureSummary>,
	pub(in crate::orchestrator) phase_acceptance_checks: Vec<PrivateEvidencePhaseAcceptanceSummary>,
	pub(in crate::orchestrator) boundary_checks: Vec<PrivateEvidenceBoundaryCheckSummary>,
	pub(in crate::orchestrator) decision_requests: Vec<PrivateEvidenceDecisionRequestSummary>,
	pub(in crate::orchestrator) architecture_recoveries:
		Vec<PrivateEvidenceArchitectureRecoverySummary>,
	pub(in crate::orchestrator) improvement_candidates: Vec<HarnessImprovementCandidateSummary>,
	pub(in crate::orchestrator) events: Vec<PrivateEvidenceReadbackEvent>,
	pub(in crate::orchestrator) warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceDecisionRequestSummary {
	pub(in crate::orchestrator) decision_request_id: String,
	pub(in crate::orchestrator) phase: String,
	pub(in crate::orchestrator) reason: String,
	pub(in crate::orchestrator) boundary: String,
	pub(in crate::orchestrator) next_action: String,
	pub(in crate::orchestrator) recommendation: Option<String>,
	pub(in crate::orchestrator) resume_condition: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceReviewCheckpointSummary {
	pub(in crate::orchestrator) phase: String,
	pub(in crate::orchestrator) status: String,
	pub(in crate::orchestrator) head_sha: Option<String>,
	pub(in crate::orchestrator) round: Option<u64>,
	pub(in crate::orchestrator) review_class: Option<String>,
	pub(in crate::orchestrator) risk_class: Option<String>,
	pub(in crate::orchestrator) compact_eligible: Option<bool>,
	pub(in crate::orchestrator) fallback_reason: Option<String>,
	pub(in crate::orchestrator) active_fingerprints: Vec<String>,
	pub(in crate::orchestrator) stop_fingerprint: Option<String>,
	pub(in crate::orchestrator) accepted_finding_count: usize,
	pub(in crate::orchestrator) rejected_finding_count: usize,
	pub(in crate::orchestrator) route_counts: Vec<PrivateEvidenceReviewRouteCount>,
	pub(in crate::orchestrator) route_next_action: Option<String>,
	pub(in crate::orchestrator) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceReviewRouteCount {
	pub(in crate::orchestrator) route: String,
	pub(in crate::orchestrator) count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceRepoGateFailureSummary {
	pub(in crate::orchestrator) record_id: i64,
	pub(in crate::orchestrator) phase: String,
	pub(in crate::orchestrator) error_class: String,
	pub(in crate::orchestrator) disposition: String,
	pub(in crate::orchestrator) stage: Option<String>,
	pub(in crate::orchestrator) failed_command: Option<String>,
	pub(in crate::orchestrator) exit_status: Option<i64>,
	pub(in crate::orchestrator) summary: Option<String>,
	pub(in crate::orchestrator) problem_lines: Vec<String>,
	pub(in crate::orchestrator) output_excerpt: Option<String>,
	pub(in crate::orchestrator) output_truncated: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidencePhaseAcceptanceSummary {
	pub(in crate::orchestrator) phase: String,
	pub(in crate::orchestrator) decision: String,
	pub(in crate::orchestrator) reason_code: String,
	pub(in crate::orchestrator) objective_covered: bool,
	pub(in crate::orchestrator) effective_delta_present: bool,
	pub(in crate::orchestrator) changed_surfaces: Vec<String>,
	pub(in crate::orchestrator) non_goal_passed: bool,
	pub(in crate::orchestrator) validation_passed: bool,
	pub(in crate::orchestrator) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceBoundaryCheckSummary {
	pub(in crate::orchestrator) disposition: String,
	pub(in crate::orchestrator) policy_decision: String,
	pub(in crate::orchestrator) reason: Option<String>,
	pub(in crate::orchestrator) attempted_recovery_reason: Option<String>,
	pub(in crate::orchestrator) decision_contract_count: usize,
	pub(in crate::orchestrator) changed_surface_count: usize,
	pub(in crate::orchestrator) improvement_signal_count: usize,
	pub(in crate::orchestrator) requires_enhanced_evidence: bool,
	pub(in crate::orchestrator) blocks_landing: bool,
	pub(in crate::orchestrator) next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceArchitectureRecoverySummary {
	pub(in crate::orchestrator) reason_code: String,
	pub(in crate::orchestrator) guardrail_reason: Option<String>,
	pub(in crate::orchestrator) boundary_disposition: Option<String>,
	pub(in crate::orchestrator) boundary_policy_decision: Option<String>,
	pub(in crate::orchestrator) requires_enhanced_evidence: bool,
	pub(in crate::orchestrator) blocks_landing: bool,
	pub(in crate::orchestrator) recovery_budget_attempt: Option<u64>,
	pub(in crate::orchestrator) recovery_budget_max_attempts: Option<u64>,
	pub(in crate::orchestrator) next_action: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidenceReadbackEvent {
	pub(in crate::orchestrator) record_id: i64,
	pub(in crate::orchestrator) event_type: String,
	pub(in crate::orchestrator) recorded_at: String,
	pub(in crate::orchestrator) payload_summary: PrivateEvidencePayloadSummary,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::orchestrator) payload: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(in crate::orchestrator) struct PrivateEvidencePayloadSummary {
	pub(in crate::orchestrator) kind: String,
	pub(in crate::orchestrator) byte_count: usize,
	pub(in crate::orchestrator) keys: Vec<String>,
	pub(in crate::orchestrator) preview: Vec<String>,
	pub(in crate::orchestrator) redacted_default_keys: Vec<String>,
}

struct PrivateEvidenceTarget {
	issue_id: String,
	issue_identifier: Option<String>,
	run_id: String,
	attempt_number: i64,
}

struct AgentEvidenceFileWriteContext<'a> {
	project_id: &'a str,
	generated_at: &'a str,
	source: AgentEvidenceSource,
	handoff_index_path: &'a Path,
	blockers_dir: &'a Path,
	events_path: &'a Path,
}

struct AgentEvidenceProjectView<'a> {
	project_id: &'a str,
	warnings: Vec<String>,
	projects: Vec<&'a OperatorProjectStatus>,
	connector_backoffs: Vec<&'a OperatorConnectorBackoffStatus>,
	current_lanes: Vec<&'a OperatorRunStatus>,
	recent_runs: Vec<&'a OperatorRunStatus>,
	history_lanes: Vec<&'a OperatorHistoryLaneStatus>,
	queued_candidates: Vec<&'a OperatorQueuedIssueStatus>,
	recovery_worktrees: Vec<(&'a str, &'a OperatorWorktreeStatus)>,
	post_review_lanes: Vec<&'a OperatorPostReviewLaneStatus>,
}
impl<'a> AgentEvidenceProjectView<'a> {
	fn from_snapshot(snapshot: &'a OperatorStatusSnapshot, project_id: &'a str) -> Self {
		let single_project_snapshot = snapshot.project_id == project_id;
		let projects = snapshot
			.projects
			.iter()
			.filter(|project| project.project_id == project_id)
			.collect::<Vec<_>>();
		let connector_backoffs = snapshot
			.connector_backoffs
			.iter()
			.filter(|backoff| backoff.project_id == project_id)
			.collect::<Vec<_>>();
		let current_lanes = snapshot
			.current_lanes
			.iter()
			.filter(|run| run.project_id == project_id)
			.collect::<Vec<_>>();
		let recent_runs = snapshot
			.recent_runs
			.iter()
			.filter(|run| run.project_id == project_id)
			.collect::<Vec<_>>();
		let history_lanes = snapshot
			.history_lanes
			.iter()
			.filter(|lane| lane.project_id == project_id)
			.collect::<Vec<_>>();
		let post_review_lanes = snapshot
			.post_review_lanes
			.iter()
			.filter(|lane| {
				lane_issue_belongs_to_project(lane.issue_id.as_str(), project_id, snapshot)
			})
			.collect::<Vec<_>>();
		let queued_candidates = snapshot
			.queued_candidates
			.iter()
			.filter(|candidate| {
				lane_issue_belongs_to_project(candidate.issue_id.as_str(), project_id, snapshot)
			})
			.collect::<Vec<_>>();
		let recovery_worktrees = if single_project_snapshot {
			rendered_recovery_worktrees(snapshot)
		} else {
			rendered_recovery_worktrees(snapshot)
				.into_iter()
				.filter(|(_, worktree)| {
					lane_issue_belongs_to_project(worktree.issue_id.as_str(), project_id, snapshot)
				})
				.collect()
		};

		Self {
			project_id,
			warnings: snapshot.warnings.clone(),
			projects,
			connector_backoffs,
			current_lanes,
			recent_runs,
			history_lanes,
			queued_candidates,
			recovery_worktrees,
			post_review_lanes,
		}
	}
}

fn lane_issue_belongs_to_project(
	issue_id: &str,
	project_id: &str,
	snapshot: &OperatorStatusSnapshot,
) -> bool {
	snapshot
		.current_lanes
		.iter()
		.chain(snapshot.recent_runs.iter())
		.any(|run| run.project_id == project_id && run.issue_id == issue_id)
		|| snapshot
			.history_lanes
			.iter()
			.any(|lane| lane.project_id == project_id && lane.issue_id == issue_id)
		|| snapshot.project_id == project_id
}

fn agent_evidence_project_ids(snapshot: &OperatorStatusSnapshot) -> Vec<String> {
	let mut project_ids = collections::BTreeSet::new();

	for project in &snapshot.projects {
		project_ids.insert(project.project_id.clone());
	}
	for run in snapshot.current_lanes.iter().chain(snapshot.recent_runs.iter()) {
		project_ids.insert(run.project_id.clone());
	}
	for lane in &snapshot.history_lanes {
		project_ids.insert(lane.project_id.clone());
	}
	for backoff in &snapshot.connector_backoffs {
		project_ids.insert(backoff.project_id.clone());
	}

	if project_ids.is_empty() && snapshot.project_id != "all" {
		project_ids.insert(snapshot.project_id.clone());
	}

	project_ids.into_iter().collect()
}

fn issue_key(issue_identifier: Option<&str>, issue_id: &str) -> String {
	issue_identifier.map_or_else(
		|| sanitize_evidence_path_component(issue_id),
		sanitize_evidence_path_component,
	)
}

fn blocker_snapshot_path(blockers_dir: &Path, issue_key: &str) -> PathBuf {
	blockers_dir.join(format!("{issue_key}.json"))
}

fn run_capsule_path(runs_dir: &Path, month_bucket: &str, run_id: &str) -> PathBuf {
	runs_dir.join(month_bucket).join(sanitize_evidence_path_component(run_id)).join("capsule.json")
}

fn run_evidence_ref(project_id: &str, run_id: &str) -> String {
	format!("run:{project_id}/{run_id}")
}

fn blocker_evidence_ref(project_id: &str, issue_key: &str, reason_code: &str) -> String {
	format!("blocker:{project_id}/{issue_key}/{reason_code}")
}

fn sanitize_evidence_path_component(raw: &str) -> String {
	let mut out = String::new();
	let mut previous_dash = false;

	for byte in raw.bytes() {
		let character = byte as char;

		if character.is_ascii_alphanumeric() {
			out.push(character.to_ascii_lowercase());

			previous_dash = false;
		} else if !previous_dash {
			out.push('-');

			previous_dash = true;
		}
	}

	let out = out.trim_matches('-').to_owned();

	if out.is_empty() { String::from("unknown") } else { out }
}

fn current_month_bucket() -> String {
	let now = OffsetDateTime::now_utc();

	format!("{:04}-{:02}", now.year(), u8::from(now.month()))
}
