#[cfg(unix)] use std::os::fd::{AsRawFd, IntoRawFd};
use std::{
	collections::BTreeMap,
	fs,
	path::Path,
	process, slice,
	sync::{Arc, Barrier},
	thread,
};

#[cfg(unix)] use libc::{F_GETFD, FD_CLOEXEC};
use rusqlite::{self, Connection};
use serde_json::{self, Value};
use tempfile::TempDir;
use time::OffsetDateTime;

#[rustfmt::skip]
use crate::state;
#[rustfmt::skip]
use crate::{autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
		AutonomyObjectiveRejection, AutonomyObjectiveState, AutonomyObjectiveSupersession,
	}, autonomy_proposal::{AutonomyProposal, AutonomyProposalCompileInput}, autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
		AutonomySignalSourceType,
	}, execution_program::{
		ExecutionLinearIssueMapping, ExecutionProgram, ExecutionProgramNode,
		ExecutionProgramNodeStage, ExecutionQueueIntent,
	}, loop_contract::{
		DecisionContract, DecisionContractStatus, DecisionPromotion, DecisionPromotionActorKind,
	}, state::{ChildAgentActivityBucket, ChildAgentActivitySummary, CodexAccountActivitySummary, CodexAccountMarker, ConnectorBackoffInput, EffectiveRuntimeMarker, LoopGuardrailCheckpointInput, PreacquiredLeaseGuards, ProjectRegistration, ProtocolActivityMarker, ProtocolActivitySummary, RUN_ACTIVITY_MARKER_FILE, RUN_CONTROL_ACTION_COMPLETED, RUN_CONTROL_ACTION_FAILED, RUN_CONTROL_ACTION_FALLBACK, RUN_CONTROL_ACTION_TIMED_OUT, RUN_OPERATION_REPO_GATE, ReviewCheckpointArtifactLookup, ReviewHandoffMarker, ReviewOrchestrationMarker, ReviewPolicyCheckpointInput, RunControlActionRequest, StateStore}, tracker::records::{LinearExecutionEventIdentity, LinearExecutionEventRecord}};

include!("tests/review_lifecycle.rs");

include!("tests/persistent_events.rs");

include!("tests/leases.rs");

include!("tests/run_activity.rs");

include!("tests/runtime_records.rs");

include!("tests/run_control.rs");

const IN_PROGRESS_STATE: &str = "In Progress";
const DROPPED_REVIEW_MARKER_TABLES_FIXTURE: &str = r#"
CREATE TABLE review_handoffs (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	branch_name TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	pr_url TEXT NOT NULL,
	target_base_ref_name TEXT,
	pr_head_ref_name TEXT NOT NULL,
	pr_head_oid TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, branch_name)
);
CREATE TABLE review_orchestrations (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	branch_name TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	pr_url TEXT NOT NULL,
	head_sha TEXT NOT NULL,
	phase TEXT NOT NULL,
	request_comment_database_id INTEGER,
	request_created_at_unix_epoch INTEGER,
	request_description_thumbs_up_count INTEGER,
	request_retry_count INTEGER NOT NULL,
	external_round_count INTEGER NOT NULL,
	auto_merge_enabled_at_unix_epoch INTEGER,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, branch_name, run_id, attempt_number)
);
INSERT INTO review_handoffs (
	project_id, issue_id, branch_name, run_id, attempt_number, pr_url,
	target_base_ref_name, pr_head_ref_name, pr_head_oid, updated_at, updated_at_unix
) VALUES (
	'pubfi', 'PUB-101', 'x/decodex-pub-101', 'run-1', 2,
	'https://github.com/hack-ink/decodex/pull/101', 'main', 'x/decodex-pub-101',
	'08a20f7dfb9526e7421a5f095b1c6adec84e52d6', '2026-06-17T01:00:00Z',
	1771290000
);
INSERT INTO review_orchestrations (
	project_id, issue_id, branch_name, run_id, attempt_number, pr_url, head_sha,
	phase, request_comment_database_id, request_created_at_unix_epoch,
	request_description_thumbs_up_count, request_retry_count, external_round_count,
	auto_merge_enabled_at_unix_epoch, updated_at, updated_at_unix
) VALUES (
	'pubfi', 'PUB-101', 'x/decodex-pub-101', 'run-1', 2,
	'https://github.com/hack-ink/decodex/pull/101',
	'19b20f7dfb9526e7421a5f095b1c6adec84e52d7', 'waiting_for_ack', 1234,
	1771290030, 4, 1, 3, 1771290060, '2026-06-17T01:01:00Z', 1771290060
);
INSERT INTO review_orchestrations (
	project_id, issue_id, branch_name, run_id, attempt_number, pr_url, head_sha,
	phase, request_comment_database_id, request_created_at_unix_epoch,
	request_description_thumbs_up_count, request_retry_count, external_round_count,
	auto_merge_enabled_at_unix_epoch, updated_at, updated_at_unix
) VALUES (
	'pubfi', 'PUB-202', 'x/decodex-pub-202', 'run-2', 1,
	'https://github.com/hack-ink/decodex/pull/202',
	'28c20f7dfb9526e7421a5f095b1c6adec84e52d8', 'request_pending', NULL,
	NULL, NULL, 0, 1, NULL, '2026-06-17T01:02:00Z', 1771290120
);
INSERT INTO review_handoffs (
	project_id, issue_id, branch_name, run_id, attempt_number, pr_url,
	target_base_ref_name, pr_head_ref_name, pr_head_oid, updated_at, updated_at_unix
) VALUES (
	'pubfi', 'PUB-303', 'x/decodex-pub-303', 'run-2', 1,
	'https://github.com/hack-ink/decodex/pull/303', 'main', 'x/decodex-pub-303',
	'38c20f7dfb9526e7421a5f095b1c6adec84e52d8', '2026-06-17T01:03:00Z',
	1771290180
);
INSERT INTO review_orchestrations (
	project_id, issue_id, branch_name, run_id, attempt_number, pr_url, head_sha,
	phase, request_comment_database_id, request_created_at_unix_epoch,
	request_description_thumbs_up_count, request_retry_count, external_round_count,
	auto_merge_enabled_at_unix_epoch, updated_at, updated_at_unix
) VALUES (
	'pubfi', 'PUB-303', 'x/decodex-pub-303', 'run-1', 1,
	'https://github.com/hack-ink/decodex/pull/303',
	'39c20f7dfb9526e7421a5f095b1c6adec84e52d9', 'waiting_for_ack', 4321,
	1771290240, 5, 2, 4, 1771290300, '2026-06-17T01:04:00Z', 1771290240
);
"#;

#[cfg(unix)]
fn fd_has_close_on_exec(fd: i32) -> bool {
	let flags = unsafe { libc::fcntl(fd, F_GETFD) };

	assert_ne!(flags, -1, "fcntl(F_GETFD) should succeed for test fd {fd}");

	flags & FD_CLOEXEC != 0
}

fn sample_pub_101_review_handoff() -> ReviewHandoffMarker {
	ReviewHandoffMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	)
}

fn sample_pub_101_review_orchestration() -> ReviewOrchestrationMarker {
	ReviewOrchestrationMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	)
}

fn latent_decision_contract_fixture() -> DecisionContract {
	serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/decision_contract/research_x_latent_contract.json"
	)))
	.expect("research X latent contract fixture should deserialize")
}

fn sample_decision_promotion() -> DecisionPromotion {
	DecisionPromotion::new(
		"operator",
		DecisionPromotionActorKind::User,
		"2026-06-09T10:00:00Z",
		"conversation",
		Some(String::from("User asked Decodex to push this forward.")),
	)
	.expect("sample promotion should validate")
}

fn autonomy_objective_fixture(version: u64) -> AutonomyObjectiveContract {
	serde_json::from_value(serde_json::json!({
		"schema": "decodex.autonomy_objective/1",
		"record_version": 1,
		"project_id": "decodex",
		"id": "quality-autonomy",
		"version": version,
		"state": "draft",
		"summary": format!("Improve Decodex autonomy quality version {version}."),
		"goals": ["Reduce repeated validation and review churn."],
		"non_goals": ["Do not bypass Decision Contract authority."],
		"metrics": ["Validation retry count stays below objective tolerance."],
		"allowed_surfaces": ["apps/decodex/src", "docs/spec"],
		"allowed_signal_kinds": ["validation_regression", "review_feedback_cluster"],
		"validation_gates": ["cargo make check-docs"],
		"review_policy": "independent current-head review required",
		"memory_policy": "read-only source-linked memory only",
		"report_policy": "public-safe summaries only"
	}))
	.expect("autonomy objective fixture should deserialize")
}

fn sample_objective_acceptance() -> AutonomyObjectiveAcceptance {
	AutonomyObjectiveAcceptance::new(
		"operator",
		AutonomyObjectiveActorKind::User,
		"2026-06-22T10:00:00Z",
		"conversation",
	)
	.expect("sample objective acceptance should validate")
}

fn sample_execution_program(contract: &DecisionContract) -> ExecutionProgram {
	let node = ExecutionProgramNode::new(
		"runtime-readiness",
		ExecutionProgramNodeStage::Runtime,
		"Implement runtime readiness evaluation.",
		ExecutionQueueIntent::ReadyToQueue,
	)
	.expect("program node should validate")
	.with_acceptance_expectations([String::from("Readiness can explain startability.")])
	.expect("acceptance expectations should attach")
	.with_validation_expectations([String::from("Run the registered repo gate.")])
	.expect("validation expectations should attach")
	.with_linear_issue(
		ExecutionLinearIssueMapping::new("issue-853", "XY-853", "Todo")
			.expect("issue mapping should validate"),
	)
	.expect("issue mapping should attach");

	ExecutionProgram::from_accepted_contract("program-853", "decodex", contract, vec![node])
		.expect("execution program should derive from accepted contract")
}

fn assert_decision_contract_retargeted(reopened: &StateStore) {
	assert_eq!(
		reopened
			.list_decision_contracts_for_issue("pubfi", "linear-id-101")
			.expect("canonical decision contracts should list")
			.len(),
		1
	);
	assert!(
		reopened
			.list_decision_contracts_for_issue("pubfi", "PUB-101")
			.expect("old decision contracts should list")
			.is_empty()
	);
}

fn seed_dropped_review_marker_tables(state_path: &Path) {
	let connection = Connection::open(state_path).expect("fixture db should open");

	connection
		.execute_batch(DROPPED_REVIEW_MARKER_TABLES_FIXTURE)
		.expect("dropped review marker tables should seed");
}

fn upsert_handoff_review_policy_checkpoint(
	store: &StateStore,
	issue_id: &str,
	run_id: &str,
	status: &str,
	head_sha: &str,
	nonclean_rounds: i64,
) {
	store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "pubfi",
			issue_id,
			run_id,
			attempt_number: 1,
			phase: "handoff",
			review_level: "standard",
			status,
			head_sha,
			nonclean_rounds,
			details_json: "{}",
		})
		.expect("review policy checkpoint should persist");
}

fn accepted_autonomy_objective_fixture() -> AutonomyObjectiveContract {
	let mut objective = autonomy_objective_fixture(1);

	objective.accept(sample_objective_acceptance()).expect("objective should accept");

	objective
}

fn autonomy_signal_fixture() -> AutonomySignal {
	AutonomySignal::validation_regression(AutonomySignalInput {
		project_id: String::from("decodex"),
		objective_id: String::from("quality-autonomy"),
		objective_version: 1,
		source_type: AutonomySignalSourceType::Runtime,
		source_refs: vec![String::from("status:runtime-health")],
		primary_source_refs: Vec::new(),
		issue_id: Some(String::from("XY-1086")),
		run_id: Some(String::from("xy-1086-attempt-1")),
		attempt_id: Some(String::from("1")),
		head_sha: Some(String::from("3cd19609c44cb18bff9e7a34a2f4853754afcee0")),
		captured_at: String::from("2026-06-22T00:00:00Z"),
		freshness: AutonomySignalFreshness::Fresh,
		summary: String::from("Runtime status readback showed repeated friction."),
		evidence: vec![String::from("status readback retained the repeated friction signal")],
		evidence_class: AutonomySignalEvidenceClass::LiveReadback,
		contradictions: Vec::new(),
		gaps: vec![String::from("No dashboard comparison included.")],
		confidence: AutonomySignalConfidence::Medium,
		privacy: AutonomySignalPrivacy::Team,
		observed_counts: BTreeMap::new(),
		review_evidence: None,
		proposal_only: true,
		created_at: String::from("2026-06-22T00:00:05Z"),
	})
	.expect("runtime signal should validate")
}

fn autonomy_proposal_fixture() -> AutonomyProposal {
	AutonomyProposal::compile_dry_run(
		Some(&accepted_autonomy_objective_fixture()),
		&[autonomy_signal_fixture()],
		AutonomyProposalCompileInput {
			project_id: String::from("decodex"),
			objective_id: String::from("quality-autonomy"),
			objective_version: 1,
			source_family: String::from("runtime_status"),
			intended_surface: String::from("apps/decodex/src/orchestrator/status.rs"),
			affected_identifiers: vec![
				String::from("OperatorLoopStatus"),
				String::from("operator_status"),
			],
			summary: String::from("Compile a bounded proposal from runtime friction evidence."),
			challenge_requirements: vec![String::from(
				"Subagent or inline skeptic objections are evidence only.",
			)],
			rejected_alternatives: vec![String::from("Direct Decision Contract promotion.")],
			rollback_path: String::from("Discard the dry-run proposal record."),
			weakened_validation_or_review: Vec::new(),
			issue_candidates: Vec::new(),
			created_at: String::from("2026-06-22T00:01:00Z"),
		},
	)
	.expect("autonomy proposal should compile")
}
