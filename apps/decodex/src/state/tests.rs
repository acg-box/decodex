#[cfg(unix)] use std::os::fd::{AsRawFd, IntoRawFd};
use std::{
	fs,
	path::Path,
	process, slice,
	sync::{Arc, Barrier},
	thread,
};

#[cfg(unix)] use libc::{F_GETFD, FD_CLOEXEC};
use rusqlite::{self, Connection};
use serde_json::Value;
use tempfile::TempDir;
use time::OffsetDateTime;

use crate::{
	execution_program::{
		ExecutionLinearIssueMapping, ExecutionProgram, ExecutionProgramNode,
		ExecutionProgramNodeStage, ExecutionQueueIntent,
	},
	loop_contract::{
		DecisionContract, DecisionContractStatus, DecisionPromotion, DecisionPromotionActorKind,
	},
	state::{
		self, ChildAgentActivityBucket, ChildAgentActivitySummary, CodexAccountActivitySummary,
		CodexAccountMarker, ConnectorBackoffInput, DispatchSlotLimit, EffectiveRuntimeMarker,
		LoopGuardrailCheckpointInput, PreacquiredLeaseGuards, ProjectRegistration,
		ProtocolActivityMarker, ProtocolActivitySummary, RUN_ACTIVITY_MARKER_FILE,
		RUN_CONTROL_ACTION_COMPLETED, RUN_CONTROL_ACTION_FAILED, RUN_CONTROL_ACTION_FALLBACK,
		RUN_CONTROL_ACTION_TIMED_OUT, RUN_OPERATION_REPO_GATE, Result, ReviewHandoffMarker,
		ReviewOrchestrationMarker, ReviewPolicyCheckpointInput, RunControlActionRequest,
		StateStore,
	},
	tracker::records::{LinearExecutionEventIdentity, LinearExecutionEventRecord},
};

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
			status,
			head_sha,
			nonclean_rounds,
			details_json: "{}",
		})
		.expect("review policy checkpoint should persist");
}

#[test]
fn loop_guardrail_checkpoints_track_fingerprints_and_retarget_issue() {
	let store = StateStore::open_in_memory().expect("state store should open");
	let first = store
		.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			reason: "validation_repeat",
			fingerprint: "fp-a",
			run_id: "run-1",
			attempt_number: 1,
			details_json: "{}",
		})
		.expect("first loop guardrail observation should persist");

	assert_eq!(first.consecutive_count(), 1);
	assert_eq!(first.reason(), "validation_repeat");

	let second = store
		.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			reason: "validation_repeat",
			fingerprint: "fp-a",
			run_id: "run-2",
			attempt_number: 2,
			details_json: "{\"attempt\":2}",
		})
		.expect("same fingerprint should increment");

	assert_eq!(second.consecutive_count(), 2);
	assert_eq!(second.run_id(), "run-2");
	assert_eq!(second.attempt_number(), 2);
	assert!(second.updated_at_unix() > 0);

	let reset = store
		.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			reason: "validation_repeat",
			fingerprint: "fp-b",
			run_id: "run-3",
			attempt_number: 3,
			details_json: "{\"attempt\":3}",
		})
		.expect("new fingerprint should reset");

	assert_eq!(reset.consecutive_count(), 1);
	assert_eq!(reset.fingerprint(), "fp-b");
	assert_eq!(reset.details_json(), "{\"attempt\":3}");
	assert!(!reset.updated_at().is_empty());

	store
		.canonicalize_issue_identity("PUB-101", "linear-id-101")
		.expect("issue identity should retarget");

	assert!(
		store
			.loop_guardrail_checkpoint("pubfi", "PUB-101", "validation_repeat")
			.expect("old checkpoint should read")
			.is_none(),
		"legacy issue identity should be cleared after retarget"
	);

	let canonical = store
		.loop_guardrail_checkpoint("pubfi", "linear-id-101", "validation_repeat")
		.expect("canonical checkpoint should read")
		.expect("canonical checkpoint should exist");

	assert_eq!(canonical.project_id(), "pubfi");
	assert_eq!(canonical.issue_id(), "linear-id-101");
	assert_eq!(canonical.fingerprint(), "fp-b");
	assert_eq!(canonical.consecutive_count(), 1);

	store
		.clear_loop_guardrail_checkpoints_for_issue("pubfi", "linear-id-101")
		.expect("checkpoint should clear");

	assert!(
		store
			.loop_guardrail_checkpoint("pubfi", "linear-id-101", "validation_repeat")
			.expect("cleared checkpoint should read")
			.is_none()
	);
}

#[test]
fn review_lifecycle_record_roundtrip_preserves_required_fields_and_projection() {
	let store = StateStore::open_in_memory().expect("state store should open");
	let handoff = ReviewHandoffMarker::new(
		"run-1",
		2,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);

	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("review handoff projection should persist");

	let restored_handoff = store
		.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
		.expect("review handoff projection should read")
		.expect("review handoff projection should exist");

	assert_eq!(restored_handoff, handoff);

	let orchestration = ReviewOrchestrationMarker::new(
		"run-1",
		2,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"waiting_for_ack",
		Some(1_234),
		Some(1_775_200_000),
		Some(3),
		1,
		2,
		Some(1_775_200_900),
	);

	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &orchestration)
		.expect("review orchestration projection should persist");

	let lifecycle = store
		.review_lifecycle_record("pubfi", "PUB-101", "x/decodex-pub-101")
		.expect("review lifecycle record should read")
		.expect("review lifecycle record should exist");

	assert_eq!(lifecycle.project_id(), "pubfi");
	assert_eq!(lifecycle.issue_id(), "PUB-101");
	assert_eq!(lifecycle.branch_name(), "x/decodex-pub-101");
	assert_eq!(lifecycle.run_id(), "run-1");
	assert_eq!(lifecycle.attempt_number(), 2);
	assert_eq!(lifecycle.pr_url(), "https://github.com/hack-ink/decodex/pull/101");
	assert_eq!(lifecycle.target_base_ref_name(), Some("main"));
	assert_eq!(lifecycle.pr_head_ref_name(), "x/decodex-pub-101");
	assert_eq!(lifecycle.pr_head_oid(), "08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	assert_eq!(lifecycle.head_sha(), "08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	assert_eq!(lifecycle.phase(), "waiting_for_ack");
	assert_eq!(lifecycle.request_comment_database_id(), Some(1_234));
	assert_eq!(lifecycle.request_created_at_unix_epoch(), Some(1_775_200_000));
	assert_eq!(lifecycle.request_description_thumbs_up_count(), Some(3));
	assert_eq!(lifecycle.request_retry_count(), 1);
	assert_eq!(lifecycle.external_round_count(), 2);
	assert_eq!(lifecycle.auto_merge_enabled_at_unix_epoch(), Some(1_775_200_900));
	assert_eq!(lifecycle.landing_state(), "not_started");
	assert_eq!(lifecycle.closeout_state(), "not_started");
	assert_eq!(lifecycle.repair_attempt_count(), 0);
	assert_eq!(lifecycle.evidence_json(), "{}");
	assert_eq!(lifecycle.next_action(), "");
	assert!(!lifecycle.updated_at().is_empty());
	assert!(lifecycle.updated_at_unix() > 0);

	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("same handoff projection should persist without resetting lifecycle state");

	let lifecycle = store
		.review_lifecycle_record("pubfi", "PUB-101", "x/decodex-pub-101")
		.expect("review lifecycle record should read after same handoff")
		.expect("review lifecycle record should exist after same handoff");

	assert_eq!(lifecycle.phase(), "waiting_for_ack");
	assert_eq!(lifecycle.request_comment_database_id(), Some(1_234));

	let restored_orchestration = store
		.review_orchestration_marker("pubfi", "PUB-101", &handoff)
		.expect("review orchestration projection should read")
		.expect("review orchestration projection should exist");

	assert_eq!(restored_orchestration, orchestration);

	let snapshot = store
		.project_loop_evidence_snapshot("pubfi")
		.expect("project loop evidence snapshot should read");
	let snapshot_lifecycle = snapshot
		.review_lifecycle_record("PUB-101", "x/decodex-pub-101")
		.expect("snapshot review lifecycle should exist");

	assert_eq!(snapshot_lifecycle, &lifecycle);
}

#[test]
fn changed_review_handoff_projection_resets_lifecycle_phase_fields() {
	let store = StateStore::open_in_memory().expect("state store should open");
	let old_handoff = ReviewHandoffMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let old_orchestration = ReviewOrchestrationMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"19b20f7dfb9526e7421a5f095b1c6adec84e52d7",
		"waiting_for_ack",
		Some(1_234),
		Some(1_775_200_000),
		Some(3),
		2,
		4,
		Some(1_775_200_900),
	);
	let new_handoff = ReviewHandoffMarker::new(
		"run-2",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"28c20f7dfb9526e7421a5f095b1c6adec84e52d8",
	);

	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &old_handoff)
		.expect("old handoff projection should persist");
	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &old_orchestration)
		.expect("old orchestration projection should persist");
	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &new_handoff)
		.expect("changed handoff projection should persist");

	let lifecycle = store
		.review_lifecycle_record("pubfi", "PUB-101", "x/decodex-pub-101")
		.expect("review lifecycle record should read")
		.expect("review lifecycle record should exist");

	assert_eq!(lifecycle.run_id(), "run-2");
	assert_eq!(lifecycle.pr_head_oid(), "28c20f7dfb9526e7421a5f095b1c6adec84e52d8");
	assert_eq!(lifecycle.head_sha(), "28c20f7dfb9526e7421a5f095b1c6adec84e52d8");
	assert_eq!(lifecycle.phase(), "request_pending");
	assert_eq!(lifecycle.request_comment_database_id(), None);
	assert_eq!(lifecycle.request_created_at_unix_epoch(), None);
	assert_eq!(lifecycle.request_description_thumbs_up_count(), None);
	assert_eq!(lifecycle.request_retry_count(), 0);
	assert_eq!(lifecycle.external_round_count(), 0);
	assert_eq!(lifecycle.auto_merge_enabled_at_unix_epoch(), None);
	assert_eq!(lifecycle.landing_state(), "not_started");
	assert_eq!(lifecycle.closeout_state(), "not_started");
	assert_eq!(lifecycle.repair_attempt_count(), 0);
	assert_eq!(lifecycle.evidence_json(), "{}");
	assert_eq!(lifecycle.next_action(), "");

	let orchestration = store
		.review_orchestration_marker("pubfi", "PUB-101", &new_handoff)
		.expect("new orchestration projection should read")
		.expect("new orchestration projection should exist");

	assert_eq!(orchestration.run_id(), "run-2");
	assert_eq!(orchestration.head_sha(), "28c20f7dfb9526e7421a5f095b1c6adec84e52d8");
	assert_eq!(orchestration.phase(), "request_pending");
	assert_eq!(orchestration.request_retry_count(), 0);
	assert_eq!(orchestration.external_round_count(), 0);
}

#[test]
fn historical_review_marker_tables_drop_without_lifecycle_migration() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");

	seed_dropped_review_marker_tables(&state_path);

	let store = StateStore::open(&state_path).expect("state store should drop historical markers");

	assert!(
		store
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("handoff projection should read")
			.is_none(),
		"historical review_handoffs rows must not become lifecycle records"
	);
	assert!(
		store
			.review_lifecycle_record("pubfi", "PUB-202", "x/decodex-pub-202")
			.expect("orchestration-only lifecycle should read")
			.is_none(),
		"historical review_orchestrations rows must not become lifecycle records"
	);
	assert!(
		store
			.review_lifecycle_record("pubfi", "PUB-303", "x/decodex-pub-303")
			.expect("stale historical lifecycle should read")
			.is_none(),
		"historical mixed review rows must not become lifecycle records"
	);

	drop(store);

	let connection = Connection::open(&state_path).expect("bootstrapped db should open");
	let legacy_table_count: i64 = connection
		.query_row(
			"SELECT COUNT(*) FROM sqlite_master \
			 WHERE type = 'table' AND name IN ('review_handoffs', 'review_orchestrations')",
			[],
			|row| row.get(0),
		)
		.expect("legacy marker tables should query");

	assert_eq!(legacy_table_count, 0);

	let lifecycle_count: i64 = connection
		.query_row("SELECT COUNT(*) FROM review_lifecycle_records", [], |row| row.get(0))
		.expect("review lifecycle rows should query");

	assert_eq!(lifecycle_count, 0);
}

#[test]
fn connector_backoff_roundtrip_and_clear_from_runtime_store() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_connector_backoff(ConnectorBackoffInput {
			project_id: "pubfi",
			connector: "linear",
			sync_phase: "post_review_lane_status",
			quota_class: "linear_graphql_api",
			reset_unix_epoch: 1_777_392_000,
			reset_source: "linear",
			warning: "tracker_rate_limited",
		})
		.expect("connector backoff should persist");

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let backoff = reopened
		.connector_backoff("pubfi", "linear")
		.expect("connector backoff should read")
		.expect("connector backoff should exist");

	assert_eq!(backoff.project_id(), "pubfi");
	assert_eq!(backoff.connector(), "linear");
	assert_eq!(backoff.sync_phase(), "post_review_lane_status");
	assert_eq!(backoff.quota_class(), "linear_graphql_api");
	assert_eq!(backoff.reset_unix_epoch(), 1_777_392_000);
	assert_eq!(backoff.reset_source(), "linear");
	assert_eq!(backoff.warning(), "tracker_rate_limited");

	reopened.clear_connector_backoff("pubfi", "linear").expect("connector backoff should clear");

	let reopened = StateStore::open(&state_path).expect("state store should reopen again");

	assert!(
		reopened
			.connector_backoff("pubfi", "linear")
			.expect("connector backoff should read after clear")
			.is_none()
	);
}

#[test]
fn clear_review_lifecycle_for_handoff_preserves_other_branches() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let removed_handoff = sample_pub_101_review_handoff();
	let removed_orchestration = sample_pub_101_review_orchestration();
	let kept_handoff = ReviewHandoffMarker::new(
		"run-2",
		1,
		"x/decodex-pub-101-review",
		"https://github.com/hack-ink/decodex/pull/102",
		"main",
		"x/decodex-pub-101-review",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let kept_orchestration = ReviewOrchestrationMarker::new(
		"run-2",
		1,
		"x/decodex-pub-101-review",
		"https://github.com/hack-ink/decodex/pull/102",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);

	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &removed_handoff)
		.expect("removed handoff projection should persist");
	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &removed_orchestration)
		.expect("removed orchestration projection should persist");

	upsert_handoff_review_policy_checkpoint(
		&store,
		"PUB-101",
		"run-1",
		"findings",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		2,
	);

	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &kept_handoff)
		.expect("kept handoff projection should persist");
	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &kept_orchestration)
		.expect("kept orchestration projection should persist");

	upsert_handoff_review_policy_checkpoint(
		&store,
		"PUB-101",
		"run-2",
		"clean",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		0,
	);

	store
		.clear_review_lifecycle_for_handoff(
			"pubfi",
			"PUB-101",
			&removed_handoff,
			&removed_orchestration,
		)
		.expect("exact review lifecycle should clear");

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");

	assert!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("removed handoff projection should read")
			.is_none()
	);
	assert_eq!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101-review")
			.expect("kept handoff projection should read"),
		Some(kept_handoff.clone())
	);
	assert_eq!(
		reopened
			.review_orchestration_marker("pubfi", "PUB-101", &kept_handoff)
			.expect("kept orchestration projection should read"),
		Some(kept_orchestration)
	);
	assert!(
		reopened
			.review_policy_checkpoint("pubfi", "PUB-101", "run-1", 1, "handoff")
			.expect("removed review policy checkpoint should read")
			.is_none()
	);

	let kept_checkpoint = reopened
		.review_policy_checkpoint("pubfi", "PUB-101", "run-2", 1, "handoff")
		.expect("kept review policy checkpoint should read")
		.expect("kept review policy checkpoint should exist");

	assert_eq!(kept_checkpoint.status(), "clean");
	assert_eq!(kept_checkpoint.head_sha(), "18a20f7dfb9526e7421a5f095b1c6adec84e52d6");
}

#[test]
fn missing_review_lifecycle_projections_return_absent() {
	let store = StateStore::open_in_memory().expect("state store should open");
	let handoff = ReviewHandoffMarker::new(
		"run-1",
		2,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);

	assert!(
		store
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("review handoff projection should read")
			.is_none()
	);
	assert!(
		store
			.review_orchestration_marker("pubfi", "PUB-101", &handoff)
			.expect("review orchestration projection should read")
			.is_none()
	);
}

#[test]
fn review_policy_checkpoints_persist_reload_and_clear_for_run_attempt() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let checkpoint = store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			run_id: "run-1",
			attempt_number: 2,
			phase: "handoff",
			status: "findings",
			head_sha: "abc123",
			nonclean_rounds: 2,
			details_json: r#"{"reviewer":"independent_fresh_context"}"#,
		})
		.expect("review policy checkpoint should persist");

	assert_eq!(checkpoint.project_id(), "pubfi");
	assert_eq!(checkpoint.issue_id(), "PUB-101");
	assert_eq!(checkpoint.run_id(), "run-1");
	assert_eq!(checkpoint.attempt_number(), 2);
	assert_eq!(checkpoint.phase(), "handoff");
	assert_eq!(checkpoint.status(), "findings");
	assert_eq!(checkpoint.head_sha(), "abc123");
	assert_eq!(checkpoint.nonclean_rounds(), 2);
	assert_eq!(checkpoint.details_json(), r#"{"reviewer":"independent_fresh_context"}"#);
	assert!(!checkpoint.updated_at().is_empty());
	assert!(checkpoint.updated_at_unix() > 0);

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");
	let reloaded = reopened
		.review_policy_checkpoint("pubfi", "PUB-101", "run-1", 2, "handoff")
		.expect("review policy checkpoint should read")
		.expect("review policy checkpoint should exist");

	assert_eq!(reloaded.status(), "findings");
	assert_eq!(reloaded.nonclean_rounds(), 2);
	assert_eq!(reloaded.details_json(), r#"{"reviewer":"independent_fresh_context"}"#);

	reopened
		.clear_review_policy_checkpoints_for_run_attempt("pubfi", "PUB-101", "run-1", 2)
		.expect("review policy checkpoint should clear");

	assert!(
		reopened
			.review_policy_checkpoint("pubfi", "PUB-101", "run-1", 2, "handoff")
			.expect("cleared review policy checkpoint should read")
			.is_none()
	);
}

#[test]
fn persistent_review_lifecycle_survives_stale_store_persist_and_is_visible() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
	let handoff = ReviewHandoffMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let orchestration = ReviewOrchestrationMarker::new(
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
	);

	writer
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("handoff projection should persist");
	writer
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &orchestration)
		.expect("orchestration projection should persist");

	let observed_handoff = observer
		.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
		.expect("observer should read handoff projection")
		.expect("observer should see lifecycle written by another store");

	assert_eq!(observed_handoff, handoff);

	observer
		.record_run_attempt("run-2", "PUB-202", 1, "running")
		.expect("stale observer should persist unrelated runtime state");

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");

	assert_eq!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("reopened store should read handoff projection"),
		Some(handoff.clone())
	);
	assert_eq!(
		reopened
			.review_orchestration_marker("pubfi", "PUB-101", &handoff)
			.expect("reopened store should read orchestration projection"),
		Some(orchestration)
	);
	assert!(
		reopened.run_attempt("run-2").expect("run attempt should read").is_some(),
		"unrelated stale-store persist should still keep its own update"
	);
}

#[test]
fn persistent_event_appenders_can_write_distinct_runs_concurrently() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let first = StateStore::open(&state_path).expect("first state store should open");
	let second = StateStore::open(&state_path).expect("second state store should open");

	first.record_run_attempt("run-a", "PUB-101", 1, "running").expect("first run should record");
	second.record_run_attempt("run-b", "PUB-102", 1, "running").expect("second run should record");

	let barrier = Arc::new(Barrier::new(2));
	let first_barrier = Arc::clone(&barrier);
	let first_writer = thread::spawn(move || {
		first_barrier.wait();

		for sequence_number in 1..=40 {
			first
				.append_event("run-a", sequence_number, "item/agentMessage/delta", "{}")
				.expect("first event writer should append");
		}
	});
	let second_writer = thread::spawn(move || {
		barrier.wait();

		for sequence_number in 1..=40 {
			second
				.append_event("run-b", sequence_number, "item/agentMessage/delta", "{}")
				.expect("second event writer should append");
		}
	});

	first_writer.join().expect("first event writer should finish");
	second_writer.join().expect("second event writer should finish");

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");

	assert_eq!(reopened.event_count("run-a").expect("first event count should load"), 40);
	assert_eq!(reopened.event_count("run-b").expect("second event count should load"), 40);
}

#[test]
fn persistent_append_event_does_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let first = StateStore::open(&state_path).expect("first state store should open");
	let second = StateStore::open(&state_path).expect("second state store should open");

	first.record_run_attempt("run-a", "PUB-101", 1, "running").expect("first run should record");
	second.record_run_attempt("run-b", "PUB-102", 1, "running").expect("second run should record");
	second
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("second store should append an unrelated event");
	first
		.append_event("run-a", 1, "item/agentMessage/delta", "{}")
		.expect("first store should append without full journal refresh");

	let state = first.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.events.contains_key("run-b"),
		"append_event should not refresh the full persistent event journal into the local cache"
	);

	drop(state);

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");

	assert_eq!(reopened.event_count("run-a").expect("first event count should load"), 1);
	assert_eq!(reopened.event_count("run-b").expect("second event count should load"), 1);
}

#[test]
fn persistent_run_attempt_update_does_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let first = StateStore::open(&state_path).expect("first state store should open");
	let second = StateStore::open(&state_path).expect("second state store should open");

	second.record_run_attempt("run-b", "PUB-102", 1, "running").expect("second run should record");
	second
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("second store should append an unrelated event");
	first.record_run_attempt("run-a", "PUB-101", 1, "running").expect("first run should record");
	first.update_run_thread("run-a", "thread-a").expect("first run thread should update");
	first.update_run_turn("run-a", "turn-a").expect("first run turn should update");
	first.update_run_status("run-a", "succeeded").expect("first run status should update");

	let state = first.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.events.contains_key("run-b"),
		"run attempt updates should not refresh the full persistent event journal into the local cache"
	);

	drop(state);

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");
	let attempt = reopened
		.run_attempt("run-a")
		.expect("run attempt lookup should succeed")
		.expect("run attempt should persist");

	assert_eq!(attempt.status(), "succeeded");
	assert_eq!(attempt.thread_id(), Some("thread-a"));
	assert_eq!(attempt.turn_id(), Some("turn-a"));
	assert_eq!(reopened.event_count("run-b").expect("second event count should load"), 1);
}

#[test]
fn persistent_project_run_listing_does_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");

	observer
		.record_run_attempt("run-a", "PUB-101", 1, "running")
		.expect("observer run should record");
	observer
		.upsert_lease("pubfi", "PUB-101", "run-a", IN_PROGRESS_STATE)
		.expect("observer lease should record");
	observer
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("observer worktree should record");
	observer.append_event("run-a", 1, "item/started", "{}").expect("observer event should append");
	writer.record_run_attempt("run-b", "PUB-102", 1, "running").expect("writer run should record");
	writer
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("writer event should append");
	writer
		.record_run_attempt("run-c", "PUB-103", 1, "succeeded")
		.expect("writer project run should record");
	writer
		.upsert_worktree("pubfi", "PUB-103", "x/pubfi-pub-103", "/tmp/worktrees/pub-103")
		.expect("writer project worktree should persist");
	writer
		.append_event("run-c", 1, "thread/archive", "{}")
		.expect("writer project event should append");

	let mut writer_record = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "PUB-102",
			issue_identifier: "PUB-102",
			run_id: "run-b",
			attempt_number: 1,
		},
		"closeout",
		String::from("2026-04-29T10:12:00Z"),
		"closeout",
	);

	writer_record.summary = Some(String::from("Writer closeout."));
	writer_record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/102"));
	writer_record.commit_sha = Some(String::from("2222222222222222222222222222222222222222"));

	writer
		.record_linear_execution_event(&writer_record)
		.expect("writer ledger event should persist");

	let runs = observer.list_leased_runs("pubfi").expect("leased runs should load");
	let recent_runs = observer.list_recent_runs("pubfi", 10).expect("recent runs should load");
	let leases = observer.list_active_shared_leases("pubfi").expect("shared leases should load");
	let worktrees = observer.list_worktrees("pubfi").expect("worktrees should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-a");
	assert_eq!(runs[0].event_count(), 1);
	assert_eq!(runs[0].last_event_type(), Some("item/started"));
	assert!(
		recent_runs.iter().any(|run| run.run_id() == "run-c"
			&& run.event_count() == 1
			&& run.last_event_type() == Some("thread/archive")),
		"project-scoped persistent event summaries should still load for matching runs"
	);
	assert_eq!(leases.len(), 1);
	assert_eq!(leases[0].issue_id(), "PUB-101");
	assert_eq!(worktrees.len(), 2);
	assert_eq!(worktrees[0].issue_id(), "PUB-101");

	let state = observer.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.events.contains_key("run-b"),
		"operator run listing should refresh event summaries without materializing unrelated event rows"
	);
	assert!(
		!state.events.contains_key("run-c"),
		"operator run listing should refresh project summaries without materializing project event rows"
	);
	assert!(
		!state.event_summaries.contains_key("run-b"),
		"operator run listing should not refresh summaries for runs outside the requested project"
	);
	assert!(
		!state.linear_execution_events.contains_key(&writer_record.idempotency_key),
		"operator run and worktree listing should not refresh the full persistent ledger into the local cache"
	);
}

#[test]
fn persistent_project_listing_does_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
	let registration = ProjectRegistration {
		service_id: String::from("pubfi"),
		config_path: temp_dir.path().join("project.toml"),
		repo_root: temp_dir.path().join("repo"),
		worktree_root: temp_dir.path().join("repo/.worktrees"),
		workflow_path: temp_dir.path().join("repo/WORKFLOW.md"),
		tracker_api_key_env_var: String::from("LINEAR_API_KEY_HACKINK"),
		github_token_env_var: String::from("GITHUB_PAT_Y"),
		enabled: true,
		config_fingerprint: String::from("abc123"),
		updated_at: String::from("2026-05-25T00:00:00Z"),
		updated_at_unix: 1_779_667_200,
	};

	observer.upsert_project(&registration).expect("project should persist");
	writer.record_run_attempt("run-b", "PUB-102", 1, "running").expect("writer run should record");
	writer
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("writer event should append");

	let projects = observer.list_projects().expect("projects should load");

	assert_eq!(projects.len(), 1);
	assert_eq!(projects[0].service_id(), "pubfi");

	let state = observer.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.events.contains_key("run-b"),
		"project listing should not refresh the full persistent event journal into the local cache"
	);
	assert!(
		!state.event_summaries.contains_key("run-b"),
		"project listing should not refresh protocol summaries unrelated to the registry"
	);
}

#[test]
fn persistent_retry_budget_queries_do_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");

	writer
		.record_run_attempt("run-a", "PUB-101", 1, "interrupted")
		.expect("writer retry attempt should record");
	writer.record_run_attempt("run-b", "PUB-102", 1, "running").expect("writer run should record");
	writer
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("writer event should append");

	assert_eq!(observer.retry_budget_attempt_count("PUB-101").expect("retry count should read"), 1);
	assert!(
		observer
			.issue_has_retry_budget_attempt_after("PUB-101", 0)
			.expect("retry after query should read")
	);
	assert!(
		!observer
			.issue_has_retry_budget_attempt_after("PUB-101", 1)
			.expect("retry after query should read")
	);

	let state = observer.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.events.contains_key("run-b"),
		"retry-budget queries should not refresh the full persistent event journal into the local cache"
	);
	assert!(
		!state.event_summaries.contains_key("run-b"),
		"retry-budget queries should not refresh protocol summaries unrelated to the issue"
	);
	assert!(
		!state.run_attempts.contains_key("run-a"),
		"retry-budget queries should use issue-scoped persistent reads instead of a full runtime refresh"
	);
}

#[test]
fn persistent_shared_claim_check_does_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let holder = StateStore::open(&state_path).expect("holder state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
	let slot_root = temp_dir.path().join("slots");

	observer
		.configure_dispatch_slot_root("pubfi", &slot_root, 2)
		.expect("observer slot root should configure");
	holder
		.configure_dispatch_slot_root("pubfi", &slot_root, 2)
		.expect("holder slot root should configure");
	writer.record_run_attempt("run-b", "PUB-102", 1, "running").expect("writer run should record");
	writer
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("writer event should append");

	assert!(
		holder
			.try_acquire_lease("pubfi", "PUB-101", "run-a", IN_PROGRESS_STATE)
			.expect("holder should acquire the shared issue claim")
	);
	assert!(
		observer
			.issue_has_active_shared_claim("pubfi", "PUB-101")
			.expect("shared claim check should read")
	);

	let state = observer.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.events.contains_key("run-b"),
		"shared claim checks should not refresh the full persistent event journal into the local cache"
	);
	assert!(
		!state.event_summaries.contains_key("run-b"),
		"shared claim checks should not refresh protocol summaries unrelated to the issue"
	);
}

#[test]
fn persistent_linear_execution_event_listing_does_not_refresh_full_ledger() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
	let mut writer_record = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "PUB-102",
			issue_identifier: "PUB-102",
			run_id: "run-b",
			attempt_number: 1,
		},
		"closeout",
		String::from("2026-04-29T10:12:00Z"),
		"closeout",
	);

	writer_record.summary = Some(String::from("Writer closeout."));
	writer_record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/102"));
	writer_record.commit_sha = Some(String::from("2222222222222222222222222222222222222222"));

	writer
		.record_linear_execution_event(&writer_record)
		.expect("writer ledger event should persist");

	let observed = observer
		.list_linear_execution_events("pubfi", "PUB-102")
		.expect("observer should read issue-scoped ledger events");

	assert_eq!(observed, vec![writer_record.clone()]);

	let state = observer.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.linear_execution_events.contains_key(&writer_record.idempotency_key),
		"issue-scoped ledger listing should not refresh the full persistent ledger into the local cache"
	);
}

#[test]
fn manages_issue_leases() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should be inserted");

	let lease = store
		.lease_for_issue("PUB-101")
		.expect("lease read should succeed")
		.expect("lease should exist");

	assert_eq!(lease.issue_id(), "PUB-101");
	assert_eq!(lease.run_id(), "run-1");
	assert_eq!(lease.project_id(), "pubfi");
	assert_eq!(lease.issue_state(), IN_PROGRESS_STATE);

	store.clear_lease("PUB-101").expect("lease should be deleted");

	assert!(store.lease_for_issue("PUB-101").expect("lease lookup should succeed").is_none());
}

#[test]
fn tracks_issue_specific_leases_without_project_limit() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	assert!(
		store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first lease acquisition should succeed")
	);
	assert!(
		store
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("second lease acquisition should succeed for another issue")
	);
	assert!(
		!store
			.try_acquire_lease("pubfi", "PUB-101", "run-3", IN_PROGRESS_STATE)
			.expect("duplicate issue acquisition should be rejected")
	);
	assert!(
		store
			.try_acquire_lease("other", "PUB-201", "run-4", IN_PROGRESS_STATE)
			.expect("other project should still acquire its own slot")
	);
}

#[test]
fn shared_dispatch_slots_honor_configured_limit_across_process_local_stores() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store_one = StateStore::open_in_memory().expect("first store should open");
	let store_two = StateStore::open_in_memory().expect("second store should open");
	let store_three = StateStore::open_in_memory().expect("third store should open");

	store_one
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("first store should configure dispatch slot root");
	store_two
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("second store should configure dispatch slot root");
	store_three
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("third store should configure dispatch slot root");

	assert!(
		store_one
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first shared lease acquisition should succeed")
	);
	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("second store should acquire the second shared slot")
	);
	assert!(
		!store_three
			.try_acquire_lease("pubfi", "PUB-103", "run-3", IN_PROGRESS_STATE)
			.expect("third store should observe the configured shared slots as busy")
	);

	store_one.clear_lease("PUB-101").expect("shared lease should clear");

	assert!(
		store_three
			.try_acquire_lease("pubfi", "PUB-103", "run-3", IN_PROGRESS_STATE)
			.expect("shared slot should reopen after one of the configured leases clears")
	);
}

#[test]
fn cleared_shared_lease_removes_lock_anchor_files() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let issue_claim_path = temp_dir.path().join(".decodex-issue-claim.PUB-101.lock");
	let dispatch_slot_path = temp_dir.path().join(".decodex-dispatch-slot.0.lock");
	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("store should configure dispatch slot root");

	assert!(
		store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("shared lease acquisition should succeed")
	);
	assert!(issue_claim_path.exists(), "active issue claim should create a lock anchor");
	assert!(dispatch_slot_path.exists(), "active dispatch slot should create a lock anchor");

	store.clear_lease("PUB-101").expect("shared lease should clear");

	assert!(
		!issue_claim_path.exists(),
		"clearing the shared lease should remove its issue-claim anchor"
	);
	assert!(
		!dispatch_slot_path.exists(),
		"clearing the shared lease should remove its dispatch-slot anchor"
	);
}

#[test]
fn configure_dispatch_slot_root_prunes_unlocked_shared_lock_files() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let stale_issue_claim_path = temp_dir.path().join(".decodex-issue-claim.PUB-999.lock");
	let stale_dispatch_slot_path = temp_dir.path().join(".decodex-dispatch-slot.0.lock");
	let store = StateStore::open_in_memory().expect("state store should open");

	fs::write(
		&stale_issue_claim_path,
		"project_id=pubfi\nissue_id=PUB-999\nrun_id=run-stale\nissue_state=In Progress\n",
	)
	.expect("stale issue-claim anchor should write");
	fs::write(&stale_dispatch_slot_path, "").expect("stale dispatch-slot anchor should write");

	store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("configuration should prune unlocked shared lock anchors");

	assert!(
		!stale_issue_claim_path.exists(),
		"configuration should remove unlocked stale issue-claim anchors"
	);
	assert!(
		!stale_dispatch_slot_path.exists(),
		"configuration should remove unlocked stale dispatch-slot anchors"
	);
}

#[test]
fn shared_dispatch_slots_support_unlimited_across_process_local_stores() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store_one = StateStore::open_in_memory().expect("first store should open");
	let store_two = StateStore::open_in_memory().expect("second store should open");
	let store_three = StateStore::open_in_memory().expect("third store should open");

	store_one
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), DispatchSlotLimit::Unlimited)
		.expect("first store should configure unlimited dispatch slots");
	store_two
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), DispatchSlotLimit::Unlimited)
		.expect("second store should configure unlimited dispatch slots");
	store_three
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), DispatchSlotLimit::Unlimited)
		.expect("third store should configure unlimited dispatch slots");

	assert!(
		store_one
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first shared lease acquisition should succeed")
	);
	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("second store should acquire another shared slot")
	);
	assert!(
		store_three
			.try_acquire_lease("pubfi", "PUB-103", "run-3", IN_PROGRESS_STATE)
			.expect("third store should acquire another shared slot")
	);
}

#[test]
fn failed_shared_slot_attempt_releases_issue_claim_before_retry() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store_one = StateStore::open_in_memory().expect("first store should open");
	let store_two = StateStore::open_in_memory().expect("second store should open");

	store_one
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("first store should configure dispatch slot root");
	store_two
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("second store should configure dispatch slot root");

	assert!(
		store_one
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first store should acquire the only shared slot")
	);
	assert!(
		!store_two
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("second store should fail while the only slot is busy")
	);
	assert!(
		!temp_dir.path().join(".decodex-issue-claim.PUB-102.lock").exists(),
		"failed slot acquisition should remove its temporary issue-claim anchor"
	);

	store_one.clear_lease("PUB-101").expect("shared lease should clear");

	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("retry should succeed after the failed contender releases its issue claim")
	);
}

#[test]
fn shared_issue_claim_blocks_duplicate_issue_across_process_local_stores() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store_one = StateStore::open_in_memory().expect("first store should open");
	let store_two = StateStore::open_in_memory().expect("second store should open");

	store_one
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("first store should configure dispatch slot root");
	store_two
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("second store should configure dispatch slot root");

	assert!(
		store_one
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first issue claim should succeed")
	);
	assert!(
		!store_two
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("duplicate issue claim should be rejected across processes")
	);
	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-102", "run-3", IN_PROGRESS_STATE)
			.expect("another issue should still be able to use the remaining slot")
	);
}

#[test]
fn shared_issue_claim_reopens_same_issue_after_clear_across_process_local_stores() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store_one = StateStore::open_in_memory().expect("first store should open");
	let store_two = StateStore::open_in_memory().expect("second store should open");

	store_one
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("first store should configure dispatch slot root");
	store_two
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("second store should configure dispatch slot root");

	assert!(
		store_one
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first issue claim should succeed")
	);
	assert!(
		!store_two
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("duplicate issue claim should be rejected while the first lease is active")
	);

	store_one.clear_lease("PUB-101").expect("shared issue claim should clear");

	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("same issue claim should reopen after the first lease clears")
	);
}

#[test]
fn shared_issue_claim_listing_reports_other_process_state() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let remote_store = StateStore::open_in_memory().expect("remote store should open");
	let observer_store = StateStore::open_in_memory().expect("observer store should open");

	remote_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("remote store should configure dispatch slot root");
	observer_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("observer store should configure dispatch slot root");

	assert!(
		remote_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("remote issue claim should succeed")
	);

	let leases = observer_store
		.list_active_shared_leases("pubfi")
		.expect("shared claim listing should succeed");

	assert_eq!(leases.len(), 1);
	assert_eq!(leases[0].issue_id(), "PUB-101");
	assert_eq!(leases[0].run_id(), "run-1");
	assert_eq!(leases[0].issue_state(), IN_PROGRESS_STATE);
}

#[cfg(unix)]
#[test]
fn adopted_dispatch_slot_blocks_after_parent_releases_local_guard() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let parent_store = StateStore::open_in_memory().expect("parent store should open");
	let child_store = StateStore::open_in_memory().expect("child store should open");
	let contender_store = StateStore::open_in_memory().expect("contender store should open");

	parent_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("parent store should configure dispatch slot root");
	child_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("child store should configure dispatch slot root");
	contender_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("contender store should configure dispatch slot root");

	assert!(
		parent_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("parent should acquire the shared slot")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child("PUB-101")
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child("PUB-101")
		.expect("child should inherit the shared dispatch-slot fd");

	child_store
		.adopt_preacquired_lease(
			"pubfi",
			"PUB-101",
			"run-1",
			IN_PROGRESS_STATE,
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");
	parent_store
		.release_dispatch_slot("PUB-101")
		.expect("parent should release its local guard after handoff");

	assert!(
		!contender_store
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("child-held guard should keep the slot busy")
	);

	child_store.clear_lease("PUB-101").expect("child lease should clear");
}

#[cfg(unix)]
#[test]
fn adopted_issue_claim_blocks_same_issue_after_parent_clears_local_guard() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let issue_claim_path = temp_dir.path().join(".decodex-issue-claim.PUB-101.lock");
	let dispatch_slot_path = temp_dir.path().join(".decodex-dispatch-slot.0.lock");
	let parent_store = StateStore::open_in_memory().expect("parent store should open");
	let child_store = StateStore::open_in_memory().expect("child store should open");
	let contender_store = StateStore::open_in_memory().expect("contender store should open");

	parent_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("parent store should configure dispatch slot root");
	child_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("child store should configure dispatch slot root");
	contender_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("contender store should configure dispatch slot root");

	assert!(
		parent_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("parent should acquire the shared issue claim")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child("PUB-101")
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child("PUB-101")
		.expect("child should inherit the shared dispatch-slot fd");

	child_store
		.adopt_preacquired_lease(
			"pubfi",
			"PUB-101",
			"run-1",
			IN_PROGRESS_STATE,
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");
	parent_store
		.clear_lease("PUB-101")
		.expect("parent should drop its local lease without unlocking the child handoff");

	assert!(
		issue_claim_path.exists(),
		"parent-side handoff cleanup must not remove the child-held issue-claim anchor"
	);
	assert!(
		dispatch_slot_path.exists(),
		"parent-side handoff cleanup must not remove the child-held dispatch-slot anchor"
	);
	assert!(
		!contender_store
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("same issue should stay claimed while the child still holds the handoff fd")
	);

	child_store.clear_lease("PUB-101").expect("child lease should clear");

	assert!(
		!issue_claim_path.exists(),
		"child terminal cleanup should remove the inherited issue-claim anchor"
	);
	assert!(
		!dispatch_slot_path.exists(),
		"child terminal cleanup should remove the inherited dispatch-slot anchor"
	);
}

#[cfg(unix)]
#[test]
fn parent_can_release_handed_off_guards_without_dropping_runtime_lease() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let parent_store = StateStore::open_in_memory().expect("parent store should open");
	let child_store = StateStore::open_in_memory().expect("child store should open");
	let contender_store = StateStore::open_in_memory().expect("contender store should open");

	parent_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("parent store should configure dispatch slot root");
	child_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("child store should configure dispatch slot root");
	contender_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("contender store should configure dispatch slot root");

	assert!(
		parent_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("parent should acquire the shared issue claim")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child("PUB-101")
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child("PUB-101")
		.expect("child should inherit the shared dispatch-slot fd");

	child_store
		.adopt_preacquired_lease(
			"pubfi",
			"PUB-101",
			"run-1",
			IN_PROGRESS_STATE,
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");
	parent_store
		.release_handed_off_guards("PUB-101")
		.expect("parent should release process-local guards after handoff");

	assert!(
		parent_store
			.lease_for_issue("PUB-101")
			.expect("parent lease lookup should succeed")
			.is_some(),
		"parent must keep the runtime lease visible after dropping local fd guards"
	);
	assert!(
		!contender_store
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("same issue should stay claimed by the child handoff")
	);
	assert!(
		contender_store
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("another issue should acquire the second dispatch slot")
	);

	child_store.clear_lease("PUB-101").expect("child lease should clear");
}

#[cfg(unix)]
#[test]
fn adopted_preacquired_lease_restores_close_on_exec_on_inherited_fds() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let parent_store = StateStore::open_in_memory().expect("parent store should open");
	let child_store = StateStore::open_in_memory().expect("child store should open");

	parent_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("parent store should configure dispatch slot root");
	child_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("child store should configure dispatch slot root");

	assert!(
		parent_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("parent should acquire the shared slot")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child("PUB-101")
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child("PUB-101")
		.expect("child should inherit the shared dispatch-slot fd");
	let issue_claim_fd = child_issue_claim.as_raw_fd();
	let dispatch_slot_fd = child_guard.as_raw_fd();

	assert!(
		!fd_has_close_on_exec(issue_claim_fd),
		"handoff issue-claim fd should clear close-on-exec before exec"
	);
	assert!(
		!fd_has_close_on_exec(dispatch_slot_fd),
		"handoff dispatch-slot fd should clear close-on-exec before exec"
	);

	child_store
		.adopt_preacquired_lease(
			"pubfi",
			"PUB-101",
			"run-1",
			IN_PROGRESS_STATE,
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");

	assert!(
		fd_has_close_on_exec(issue_claim_fd),
		"adopted issue-claim fd must restore close-on-exec before spawning grandchildren"
	);
	assert!(
		fd_has_close_on_exec(dispatch_slot_fd),
		"adopted dispatch-slot fd must restore close-on-exec before spawning grandchildren"
	);

	child_store.clear_lease("PUB-101").expect("child lease should clear");
	parent_store.clear_lease("PUB-101").expect("parent lease should clear");
}

#[cfg(unix)]
#[test]
fn adopted_child_clear_releases_lock_when_descendant_keeps_inherited_fds_open() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let parent_store = StateStore::open_in_memory().expect("parent store should open");
	let child_store = StateStore::open_in_memory().expect("child store should open");
	let contender_store = StateStore::open_in_memory().expect("contender store should open");

	parent_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("parent store should configure dispatch slot root");
	child_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("child store should configure dispatch slot root");
	contender_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("contender store should configure dispatch slot root");

	assert!(
		parent_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("parent should acquire the shared slot")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child("PUB-101")
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child("PUB-101")
		.expect("child should inherit the shared dispatch-slot fd");
	let _descendant_issue_claim =
		child_issue_claim.try_clone().expect("descendant should inherit the issue-claim fd");
	let _descendant_guard =
		child_guard.try_clone().expect("descendant should inherit the dispatch-slot fd");

	child_store
		.adopt_preacquired_lease(
			"pubfi",
			"PUB-101",
			"run-1",
			IN_PROGRESS_STATE,
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");
	parent_store.clear_lease("PUB-101").expect("parent should drop its local handoff guard");
	child_store.clear_lease("PUB-101").expect("child lease should clear");

	assert!(
		contender_store
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("descendant-held fds must not keep the cleared lease claimed"),
		"clearing an adopted child lease must release the shared claim and slot even if a descendant still holds inherited fds"
	);
}

#[test]
fn records_run_attempts_and_events() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.record_run_attempt("run-1", "PUB-101", 1, "running")
		.expect("run attempt should be recorded");
	store.update_run_thread("run-1", "thread-1").expect("thread id should be attached");
	store
		.append_event("run-1", 1, "turn/started", "{\"turn\":\"1\"}")
		.expect("event should be recorded");

	let run_attempt = store
		.run_attempt("run-1")
		.expect("run attempt query should succeed")
		.expect("run attempt should exist");

	assert_eq!(run_attempt.issue_id(), "PUB-101");
	assert_eq!(run_attempt.attempt_number(), 1);
	assert_eq!(run_attempt.status(), "running");
	assert_eq!(run_attempt.thread_id(), Some("thread-1"));
	assert_eq!(store.event_count("run-1").expect("event count should succeed"), 1);
	assert_eq!(store.next_attempt_number("PUB-101").expect("next attempt should load"), 2);
	assert_eq!(
		store.retry_budget_attempt_count("PUB-101").expect("retry budget count should load"),
		0
	);

	store.update_run_status("run-1", "interrupted").expect("status should update");

	let updated = store
		.run_attempt("run-1")
		.expect("run attempt query should succeed")
		.expect("run attempt should exist");

	assert_eq!(updated.status(), "interrupted");
	assert!(
		store
			.last_run_activity_unix_epoch("run-1")
			.expect("last activity lookup should succeed")
			.is_some()
	);
}

#[test]
fn records_run_activity_summary_for_recent_project_runs() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let child_activity = ChildAgentActivitySummary {
		buckets: vec![ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds: 12,
			event_count: 3,
			tool_call_count: 0,
			input_tokens: 1_200,
			output_tokens: 240,
			output_bytes: 0,
		}],
		current_bucket: Some(String::from("Model")),
		current_detail: Some(String::from("gpt-5")),
		current_started_unix_epoch: None,
		current_elapsed_seconds: Some(12),
		wall_seconds: 12,
		event_count: 3,
		tool_call_count: 2,
		input_tokens_current: Some(1_200),
		input_tokens_max: Some(1_200),
		input_tokens_cumulative: 1_200,
		output_tokens_cumulative: 240,
		largest_tool_output_bytes: Some(4_096),
		largest_tool_output_tool: Some(String::from("shell")),
		large_output_warnings: vec![String::from("shell output was truncated")],
	};
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		..ProtocolActivitySummary::default()
	};
	let persisted_child_activity = child_activity.clone().sealed_durable();

	{
		let store = StateStore::open(&state_path).expect("persistent state store should open");

		store
			.record_run_attempt("run-1", "PUB-101", 1, "succeeded")
			.expect("run attempt should be recorded");
		store
			.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
			.expect("project ownership should record");
		store
			.record_run_activity_summary(
				"run-1",
				1,
				Some(&child_activity),
				Some(&protocol_activity),
			)
			.expect("activity summary should persist");
	}

	let reopened = StateStore::open(&state_path).expect("persistent state store should reopen");
	let runs = reopened.list_recent_runs("pubfi", 10).expect("recent project runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-1");
	assert_eq!(runs[0].child_agent_activity(), Some(&persisted_child_activity));
	assert_eq!(runs[0].protocol_activity(), Some(&protocol_activity));
}

#[test]
fn opening_state_store_seals_durable_run_activity_summary_rows() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let child_activity = ChildAgentActivitySummary {
		buckets: vec![ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds: 12,
			event_count: 3,
			tool_call_count: 0,
			input_tokens: 1_200,
			output_tokens: 240,
			output_bytes: 0,
		}],
		current_bucket: Some(String::from("Model")),
		current_detail: Some(String::from("gpt-5")),
		current_started_unix_epoch: Some(10),
		current_elapsed_seconds: Some(8),
		wall_seconds: 12,
		event_count: 3,
		tool_call_count: 2,
		input_tokens_current: Some(1_200),
		input_tokens_max: Some(1_200),
		input_tokens_cumulative: 1_200,
		output_tokens_cumulative: 240,
		largest_tool_output_bytes: Some(4_096),
		largest_tool_output_tool: Some(String::from("shell")),
		large_output_warnings: vec![String::from("shell output was truncated")],
	};
	let unsealed_json =
		serde_json::to_string(&child_activity).expect("unsealed activity should serialize");

	StateStore::open(&state_path).expect("persistent state store should bootstrap");

	{
		let connection = Connection::open(&state_path).expect("sqlite connection should reopen");

		connection
			.execute(
				"INSERT INTO run_activity_summaries (
				 run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
				 updated_at, updated_at_unix
				 ) VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
				rusqlite::params!["run-old", 1_i64, unsealed_json, "2026-06-17T00:00:00Z", 1_i64],
			)
			.expect("unsealed activity row should insert");
	}

	StateStore::open(&state_path).expect("persistent state store should seal stored row");

	let sealed_json: String = Connection::open(&state_path)
		.expect("sqlite connection should reopen")
		.query_row(
			"SELECT child_agent_activity_json FROM run_activity_summaries WHERE run_id = ?1",
			["run-old"],
			|row| row.get(0),
		)
		.expect("sealed row should load");
	let sealed_value: Value =
		serde_json::from_str(&sealed_json).expect("sealed activity should remain json");
	let sealed_activity: ChildAgentActivitySummary =
		serde_json::from_str(&sealed_json).expect("sealed activity should deserialize");

	assert!(sealed_value["current_bucket"].is_null());
	assert!(sealed_value["current_detail"].is_null());
	assert!(sealed_value["current_started_unix_epoch"].is_null());
	assert!(sealed_value["current_elapsed_seconds"].is_null());
	assert_eq!(sealed_activity, child_activity.sealed_durable());
}

#[test]
fn lists_issue_attempts_and_protocol_event_presence() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.record_run_attempt("run-2", "PUB-101", 2, "succeeded")
		.expect("second run attempt should record");
	store
		.record_run_attempt("run-1", "PUB-101", 1, "failed")
		.expect("first run attempt should record");
	store
		.record_run_attempt("run-other", "PUB-102", 1, "succeeded")
		.expect("other issue run attempt should record");
	store.update_run_thread("run-1", "thread-1").expect("first thread should attach");
	store.update_run_thread("run-2", "thread-2").expect("second thread should attach");
	store.append_event("run-1", 1, "thread/archive", "{}").expect("archive event should record");

	let attempts =
		store.list_run_attempts_for_issue("PUB-101").expect("issue attempts should load");

	assert_eq!(attempts.len(), 2);
	assert_eq!(attempts[0].run_id(), "run-1");
	assert_eq!(attempts[0].thread_id(), Some("thread-1"));
	assert_eq!(attempts[1].run_id(), "run-2");
	assert!(store.run_has_protocol_event("run-1", "thread/archive").expect("event should load"));
	assert!(
		!store
			.run_has_protocol_event("run-2", "thread/archive")
			.expect("missing event should load")
	);
}

#[test]
fn sqlite_lists_project_attempts_and_protocol_event_presence() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
	let observer = StateStore::open(&state_path).expect("observer state store should open");

	writer
		.try_acquire_lease("decodex", "issue-1", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record project ownership");
	writer
		.record_run_attempt("run-1", "issue-1", 1, "succeeded")
		.expect("first run attempt should record");
	writer.update_run_thread("run-1", "thread-1").expect("first thread should attach");
	writer.append_event("run-1", 1, "thread/archive", "{}").expect("archive event should record");
	writer
		.try_acquire_lease("other", "issue-2", "run-2", IN_PROGRESS_STATE)
		.expect("other lease should record project ownership");
	writer
		.record_run_attempt("run-2", "issue-2", 1, "succeeded")
		.expect("other run attempt should record");

	let attempts = observer
		.list_run_attempts_for_project("decodex")
		.expect("project attempts should load from sqlite");

	assert_eq!(attempts.len(), 1);
	assert_eq!(attempts[0].run_id(), "run-1");
	assert_eq!(attempts[0].thread_id(), Some("thread-1"));
	assert!(
		observer
			.run_has_protocol_event("run-1", "thread/archive")
			.expect("sqlite event presence should load")
	);
	assert!(
		!observer
			.run_has_protocol_event("run-2", "thread/archive")
			.expect("sqlite missing event presence should load")
	);
}

#[test]
fn run_activity_marker_round_trips_marker_surfaces() {
	assert_run_activity_marker_round_trips_clearable_auxiliary_fields();
	assert_run_activity_marker_round_trips_thread_and_protocol_summary_fields();
	assert_run_activity_marker_round_trips_child_agent_activity_summary();
	assert_run_activity_marker_round_trips_account_summary();
	assert_run_activity_marker_preserves_account_summary_after_activity_refresh();
	assert_run_activity_marker_preserves_account_summary_after_stale_rewrite();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_host_boot_id_uses_boot_session_uuid() {
	let host_boot_id = state::current_host_boot_id().expect("macOS boot session UUID should read");

	assert!(
		host_boot_id.starts_with("macos_bootsessionuuid:"),
		"macOS host boot identity should use boot-session UUID, got {host_boot_id}"
	);
	assert!(
		!host_boot_id.contains("boottime") && !host_boot_id.contains("usec"),
		"macOS host boot identity should not depend on kern.boottime timeval output"
	);
}

fn assert_run_activity_marker_round_trips_clearable_auxiliary_fields() {
	let temp_dir = TempDir::new().expect("tempdir should create");

	state::write_run_activity_marker_for_process(temp_dir.path(), "run-1", 1, process::id())
		.expect("activity marker should write");
	state::write_run_retry_schedule(temp_dir.path(), "run-1", 1, "failure", 12_345)
		.expect("retry schedule should write");
	state::write_run_review_policy_state(
		temp_dir.path(),
		"run-1",
		1,
		"handoff",
		"findings",
		"abc123",
		2,
	)
	.expect("review policy state should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.run_id(), "run-1");
	assert_eq!(marker.attempt_number(), 1);

	if let Some(host_boot_id) = state::current_host_boot_id() {
		assert_eq!(marker.host_boot_id(), Some(host_boot_id.as_str()));
		assert!(
			fs::read_to_string(temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE))
				.expect("activity marker body should load")
				.contains(&format!("host_boot_id={host_boot_id}\n")),
			"activity markers should record the host boot identity for reboot-safe liveness"
		);
	}
	if let Some(process_start_identity) = state::current_process_start_identity() {
		assert_eq!(marker.process_start_identity(), Some(process_start_identity.as_str()));
		assert!(
			fs::read_to_string(temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE))
				.expect("activity marker body should load")
				.contains(&format!("process_start_identity={process_start_identity}\n")),
			"activity markers should record the process start identity for PID-reuse-safe liveness"
		);
	}

	assert_eq!(marker.retry_kind(), Some("failure"));
	assert_eq!(marker.retry_ready_at_unix_epoch(), Some(12_345));
	assert_eq!(marker.review_policy_phase(), Some("handoff"));
	assert_eq!(marker.review_policy_status(), Some("findings"));
	assert_eq!(marker.review_policy_head_sha(), Some("abc123"));
	assert_eq!(marker.review_policy_nonclean_rounds(), Some(2));

	state::clear_run_retry_schedule(temp_dir.path()).expect("retry schedule should clear");

	let retry_cleared = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should reload")
		.expect("marker snapshot should still exist");

	assert_eq!(retry_cleared.retry_kind(), None);
	assert_eq!(retry_cleared.retry_ready_at_unix_epoch(), None);
	assert_eq!(retry_cleared.review_policy_phase(), Some("handoff"));

	state::clear_run_review_policy_state(temp_dir.path())
		.expect("review policy state should clear");

	let policy_cleared = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should reload")
		.expect("marker snapshot should still exist");

	assert_eq!(policy_cleared.review_policy_phase(), None);
	assert_eq!(policy_cleared.review_policy_status(), None);
	assert_eq!(policy_cleared.review_policy_head_sha(), None);
	assert_eq!(policy_cleared.review_policy_nonclean_rounds(), None);
}

fn assert_run_activity_marker_round_trips_thread_and_protocol_summary_fields() {
	let temp_dir = TempDir::new().expect("tempdir should create");

	state::write_run_activity_marker_for_process(temp_dir.path(), "run-1", 1, process::id())
		.expect("activity marker should write");
	state::write_run_thread_marker(temp_dir.path(), "run-1", 1, "thread-1")
		.expect("thread marker should write");
	state::write_run_turn_marker(temp_dir.path(), "run-1", 1, "turn-1")
		.expect("turn marker should write");
	state::write_run_thread_status_marker(
		temp_dir.path(),
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[String::from("waitingOnApproval")],
	)
	.expect("thread status marker should write");
	state::write_run_effective_runtime_marker(
		temp_dir.path(),
		"run-1",
		1,
		&EffectiveRuntimeMarker {
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			effective_model: "gpt-5.4",
			effective_model_provider: "openai",
			effective_cwd: "/tmp/worktree",
			effective_approval_policy: "never",
			effective_approvals_reviewer: "human",
			effective_sandbox_mode: "workspaceWrite",
		},
	)
	.expect("effective runtime marker should write");

	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		waiting_reason: Some(String::from("model_execution")),
		rate_limit_status: Some(String::from("usageLimitExceeded")),
		recent_events: vec![
			state::ProtocolActivityEventSummary {
				event_type: String::from("turn/started"),
				category: String::from("turn"),
				detail: Some(String::from("running")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("turn/completed"),
				category: String::from("turn"),
				detail: Some(String::from("completed")),
			},
		],
	};

	state::write_run_protocol_activity_marker(
		temp_dir.path(),
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 3,
			last_event_type: "turn/completed",
			child_agent_activity: None,
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol summary should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.thread_id(), Some("thread-1"));
	assert_eq!(marker.turn_id(), Some("turn-1"));
	assert_eq!(marker.thread_status(), Some("active"));
	assert_eq!(marker.thread_active_flags(), &[String::from("waitingOnApproval")]);
	assert_eq!(marker.event_count(), 3);
	assert_eq!(marker.last_event_type(), Some("turn/completed"));
	assert_eq!(marker.effective_model(), Some("gpt-5.4"));
	assert_eq!(marker.effective_model_provider(), Some("openai"));
	assert_eq!(marker.effective_cwd(), Some("/tmp/worktree"));
	assert_eq!(marker.effective_approval_policy(), Some("never"));
	assert_eq!(marker.effective_approvals_reviewer(), Some("human"));
	assert_eq!(marker.effective_sandbox_mode(), Some("workspaceWrite"));
	assert_eq!(marker.protocol_activity(), Some(&protocol_activity));
	assert!(marker.last_protocol_activity_unix_epoch().is_some());
	assert_eq!(marker.current_operation(), Some(state::RUN_OPERATION_AGENT_RUN));
	assert!(marker.last_progress_unix_epoch().is_some());
}

fn assert_run_activity_marker_round_trips_child_agent_activity_summary() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let summary = ChildAgentActivitySummary {
		buckets: vec![
			state::ChildAgentActivityBucket {
				name: String::from("Model"),
				wall_seconds: 693,
				event_count: 12,
				tool_call_count: 0,
				input_tokens: 4_270_000,
				output_tokens: 12_000,
				output_bytes: 0,
			},
			state::ChildAgentActivityBucket {
				name: String::from("Browser/Image"),
				wall_seconds: 41,
				event_count: 6,
				tool_call_count: 3,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 180_000,
			},
		],
		current_bucket: Some(String::from("Model")),
		current_detail: Some(String::from("waiting after tool output")),
		current_started_unix_epoch: Some(1_800_000_000),
		current_elapsed_seconds: Some(9),
		wall_seconds: 734,
		event_count: 18,
		tool_call_count: 3,
		input_tokens_current: Some(105_000),
		input_tokens_max: Some(105_000),
		input_tokens_cumulative: 4_270_000,
		output_tokens_cumulative: 12_000,
		largest_tool_output_bytes: Some(180_000),
		largest_tool_output_tool: Some(String::from("view_image")),
		large_output_warnings: vec![String::from(
			"view_image repeated 3 large outputs; largest 180000 bytes",
		)],
	};

	state::write_run_protocol_activity_marker(
		temp_dir.path(),
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 18,
			last_event_type: "item/tool/call/response",
			child_agent_activity: Some(&summary),
			protocol_activity: None,
		},
	)
	.expect("protocol summary should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.child_agent_activity(), Some(&summary));
}

#[test]
fn run_protocol_non_work_events_do_not_refresh_progress_marker() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let stale_progress = OffsetDateTime::now_utc().unix_timestamp() - 3_600;

	fs::write(
		temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE),
		format!(
			"run_id=run-1\nattempt_number=1\nlast_activity_unix_epoch={stale_progress}\nlast_protocol_activity_unix_epoch={stale_progress}\nlast_progress_unix_epoch={stale_progress}\n"
		),
	)
	.expect("initial marker should write");

	let account_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model_execution")),
		recent_events: vec![state::ProtocolActivityEventSummary {
			event_type: String::from("account/rateLimits/updated"),
			category: String::from("rate_limit"),
			detail: Some(String::from("pro")),
		}],
		..ProtocolActivitySummary::default()
	};

	write_test_protocol_activity_marker(
		temp_dir.path(),
		1,
		"account/rateLimits/updated",
		Some(&account_activity),
	)
	.expect("account protocol activity should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.last_progress_unix_epoch(), Some(stale_progress));
	assert!(
		marker
			.last_protocol_activity_unix_epoch()
			.is_some_and(|last_protocol| last_protocol > stale_progress)
	);

	let first_protocol_activity = marker
		.last_protocol_activity_unix_epoch()
		.expect("account protocol activity should update protocol time");

	write_test_protocol_activity_marker(
		temp_dir.path(),
		2,
		"account/rateLimits/updated",
		Some(&account_activity),
	)
	.expect("second account protocol activity should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.last_progress_unix_epoch(), Some(stale_progress));
	assert!(
		marker
			.last_protocol_activity_unix_epoch()
			.is_some_and(|last_protocol| last_protocol >= first_protocol_activity)
	);

	let goal_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model_execution")),
		recent_events: vec![state::ProtocolActivityEventSummary {
			event_type: String::from("thread/goal/updated"),
			category: String::from("protocol"),
			detail: Some(String::from("active")),
		}],
		..ProtocolActivitySummary::default()
	};

	write_test_protocol_activity_marker(
		temp_dir.path(),
		3,
		"thread/goal/updated",
		Some(&goal_activity),
	)
	.expect("goal status protocol activity should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.last_progress_unix_epoch(), Some(stale_progress));

	write_test_protocol_activity_marker(temp_dir.path(), 4, "item/fileChange/patchUpdated", None)
		.expect("work protocol activity should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert!(
		marker
			.last_progress_unix_epoch()
			.is_some_and(|last_progress| last_progress > stale_progress)
	);
}

fn write_test_protocol_activity_marker(
	worktree_path: &Path,
	event_count: i64,
	last_event_type: &str,
	protocol_activity: Option<&ProtocolActivitySummary>,
) -> Result<()> {
	state::write_run_protocol_activity_marker(
		worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count,
			last_event_type,
			child_agent_activity: None,
			protocol_activity,
		},
	)
}

fn assert_run_activity_marker_round_trips_account_summary() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let summary = sample_codex_account_activity_summary();

	state::write_run_account_marker(
		temp_dir.path(),
		&CodexAccountMarker {
			run_id: "run-1",
			attempt_number: 1,
			account: &summary,
			accounts: slice::from_ref(&summary),
		},
	)
	.expect("account summary should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.account(), Some(&summary));
	assert_eq!(marker.accounts(), slice::from_ref(&summary));

	let body = fs::read_to_string(temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE))
		.expect("marker body should read");

	assert!(body.contains("account="));
	assert!(body.contains("accounts="));
	assert!(!body.contains("codex_account="));
	assert!(!body.contains("codex_accounts="));
}

fn assert_run_activity_marker_preserves_account_summary_after_activity_refresh() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let summary = sample_codex_account_activity_summary();

	state::write_run_account_marker(
		temp_dir.path(),
		&CodexAccountMarker {
			run_id: "run-1",
			attempt_number: 1,
			account: &summary,
			accounts: slice::from_ref(&summary),
		},
	)
	.expect("account summary should write");
	state::write_run_activity_marker_at(
		temp_dir.path(),
		"run-1",
		1,
		process::id(),
		1_800_000_020,
		Some(1_800_000_019),
	)
	.expect("activity refresh should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.account(), Some(&summary));
	assert_eq!(marker.accounts(), slice::from_ref(&summary));

	let leftover_temp_marker = fs::read_dir(temp_dir.path())
		.expect("tempdir should be readable")
		.filter_map(|entry| entry.ok())
		.any(|entry| entry.file_name().to_string_lossy().contains(".decodex-run-activity."));

	assert!(!leftover_temp_marker, "atomic marker rewrites should not leave temp files");
}

fn assert_run_activity_marker_preserves_account_summary_after_stale_rewrite() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let summary = sample_codex_account_activity_summary();

	state::write_run_activity_marker_for_process(temp_dir.path(), "run-1", 1, process::id())
		.expect("initial activity marker should write");

	let stale_activity_marker = state::read_run_activity_marker_record(temp_dir.path())
		.expect("activity marker should read")
		.expect("activity marker should exist");

	state::write_run_account_marker(
		temp_dir.path(),
		&CodexAccountMarker {
			run_id: "run-1",
			attempt_number: 1,
			account: &summary,
			accounts: slice::from_ref(&summary),
		},
	)
	.expect("account summary should write");
	state::write_run_activity_marker_record(temp_dir.path(), &stale_activity_marker)
		.expect("stale activity marker rewrite should preserve current account");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.account(), Some(&summary));
	assert_eq!(marker.accounts(), slice::from_ref(&summary));
}

fn sample_codex_account_activity_summary() -> CodexAccountActivitySummary {
	CodexAccountActivitySummary {
		account_fingerprint: String::from("acct_...cdef"),
		email: Some(String::from("account@example.com")),
		plan_type: Some(String::from("pro")),
		status: String::from("selected"),
		refresh_status: String::from("not_needed"),
		checked_at_unix_epoch: Some(1_800_000_010),
		selected_at_unix_epoch: Some(1_800_000_011),
		primary_window_seconds: Some(18_000),
		primary_remaining_percent: Some(72),
		primary_resets_at_unix_epoch: Some(1_800_018_000),
		secondary_window_seconds: Some(604_800),
		secondary_remaining_percent: Some(91),
		secondary_resets_at_unix_epoch: Some(1_800_604_800),
		credits_has_credits: Some(true),
		credits_unlimited: Some(false),
		credits_balance: Some(String::from("9.99")),
		rate_limit_reached_type: None,
		cooldown_until_unix_epoch: None,
		note: Some(String::from("usage probe ok")),
		..CodexAccountActivitySummary::default()
	}
}

#[test]
fn run_operation_marker_resets_stale_per_attempt_fields_on_new_attempt() {
	let temp_dir = TempDir::new().expect("tempdir should create");

	state::write_run_activity_marker_for_process(temp_dir.path(), "run-1", 1, process::id())
		.expect("first activity marker should write");
	state::write_run_thread_marker(temp_dir.path(), "run-1", 1, "thread-1")
		.expect("thread marker should write");
	state::write_run_turn_marker(temp_dir.path(), "run-1", 1, "turn-1")
		.expect("turn marker should write");
	state::write_run_thread_status_marker(
		temp_dir.path(),
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[String::from("waitingOnUserInput")],
	)
	.expect("thread status should write");
	state::write_run_effective_runtime_marker(
		temp_dir.path(),
		"run-1",
		1,
		&EffectiveRuntimeMarker {
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			effective_model: "gpt-5.4",
			effective_model_provider: "openai",
			effective_cwd: "/tmp/worktree",
			effective_approval_policy: "never",
			effective_approvals_reviewer: "human",
			effective_sandbox_mode: "dangerFullAccess",
		},
	)
	.expect("effective runtime should write");
	state::write_run_protocol_activity_marker(
		temp_dir.path(),
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 3,
			last_event_type: "turn/completed",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol summary should write");
	state::write_run_retry_schedule(temp_dir.path(), "run-1", 1, "failure", 123)
		.expect("retry schedule should write");
	state::write_run_retry_budget_attempt_count(temp_dir.path(), "run-1", 1, 2)
		.expect("retry budget should write");
	state::write_run_review_policy_state(
		temp_dir.path(),
		"run-1",
		1,
		"repair",
		"findings",
		"def456",
		2,
	)
	.expect("review policy should write");
	state::write_run_operation_marker(temp_dir.path(), "run-2", 2, RUN_OPERATION_REPO_GATE)
		.expect("next attempt operation marker should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.run_id(), "run-2");
	assert_eq!(marker.attempt_number(), 2);
	assert_eq!(marker.current_operation(), Some(state::RUN_OPERATION_REPO_GATE));
	assert!(marker.last_progress_unix_epoch().is_some());
	assert_eq!(marker.thread_id(), None);
	assert_eq!(marker.turn_id(), None);
	assert_eq!(marker.thread_status(), None);
	assert!(marker.thread_active_flags().is_empty());
	assert_eq!(marker.event_count(), 0);
	assert_eq!(marker.last_event_type(), None);
	assert_eq!(marker.protocol_activity(), None);
	assert_eq!(marker.effective_model(), None);
	assert_eq!(marker.effective_model_provider(), None);
	assert_eq!(marker.effective_cwd(), None);
	assert_eq!(marker.effective_approval_policy(), None);
	assert_eq!(marker.effective_approvals_reviewer(), None);
	assert_eq!(marker.effective_sandbox_mode(), None);
	assert_eq!(marker.last_protocol_activity_unix_epoch(), None);
	assert_eq!(marker.retry_kind(), None);
	assert_eq!(marker.retry_ready_at_unix_epoch(), None);
	assert_eq!(
		state::read_run_retry_budget_attempt_count(temp_dir.path())
			.expect("retry budget count should load"),
		Some(2)
	);
	assert_eq!(marker.review_policy_phase(), Some("repair"));
	assert_eq!(marker.review_policy_status(), Some("findings"));
	assert_eq!(marker.review_policy_head_sha(), Some("def456"));
	assert_eq!(marker.review_policy_nonclean_rounds(), Some(2));
}

#[test]
fn counts_retry_budget_attempts_per_issue() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store.record_run_attempt("run-1", "PUB-101", 1, "succeeded").expect("first run should record");
	store.record_run_attempt("run-2", "PUB-101", 2, "failed").expect("second run should record");
	store
		.record_run_attempt("run-3", "PUB-101", 3, "interrupted")
		.expect("third run should record");
	store
		.record_run_attempt("run-5", "PUB-101", 4, "terminal_guarded")
		.expect("guarded run should record");
	store
		.record_run_attempt("run-4", "PUB-102", 1, "failed")
		.expect("other issue run should record");

	assert_eq!(
		store.retry_budget_attempt_count("PUB-101").expect("retry budget count should load"),
		3
	);
	assert_eq!(
		store.retry_budget_attempt_count("PUB-102").expect("retry budget count should load"),
		1
	);
}

#[test]
fn loads_latest_run_attempt_for_issue() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store.record_run_attempt("run-1", "PUB-101", 1, "failed").expect("first run should record");
	store
		.record_run_attempt("run-2", "PUB-101", 2, "terminal_guarded")
		.expect("latest run should record");

	let attempt = store
		.latest_run_attempt_for_issue("PUB-101")
		.expect("latest run lookup should succeed")
		.expect("latest run should exist");

	assert_eq!(attempt.run_id(), "run-2");
	assert_eq!(attempt.attempt_number(), 2);
	assert_eq!(attempt.status(), "terminal_guarded");
}

#[test]
fn manages_worktree_mappings() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_worktree("pubfi", "PUB-101", "x/pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree mapping should be recorded");

	let mapping = store
		.worktree_for_issue("PUB-101")
		.expect("mapping lookup should succeed")
		.expect("mapping should exist");

	assert_eq!(mapping.issue_id(), "PUB-101");
	assert_eq!(mapping.branch_name(), "x/pub-101");
	assert_eq!(mapping.worktree_path(), Path::new("/tmp/worktrees/pub-101"));
	assert_eq!(mapping.project_id(), "pubfi");
	assert_eq!(mapping.provenance().source(), "runtime_recorded");
	assert!(mapping.provenance().created_at_unix().is_some());
	assert!(mapping.provenance().updated_at_unix().is_some());
	assert_eq!(store.list_worktrees("pubfi").expect("list should succeed").len(), 1);

	store.clear_worktree("PUB-101").expect("mapping should be deleted");

	assert!(store.worktree_for_issue("PUB-101").expect("lookup should succeed").is_none());
}

#[test]
fn opens_legacy_worktree_rows_with_unknown_provenance() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let db_path = temp_dir.path().join("runtime.sqlite3");

	{
		let connection = Connection::open(&db_path).expect("legacy db should open");

		connection
			.execute_batch(
				"CREATE TABLE worktrees (
					issue_id TEXT PRIMARY KEY NOT NULL,
					project_id TEXT NOT NULL,
					branch_name TEXT NOT NULL,
					worktree_path TEXT NOT NULL
				);
				INSERT INTO worktrees (issue_id, project_id, branch_name, worktree_path)
				VALUES ('issue-legacy', 'pubfi', 'x/pubfi-pub-101', '/tmp/worktrees/pub-101');",
			)
			.expect("legacy worktree row should write");
	}

	let store = StateStore::open(&db_path).expect("state store should migrate");
	let mapping = store
		.worktree_for_issue("issue-legacy")
		.expect("mapping lookup should succeed")
		.expect("legacy mapping should exist");

	assert_eq!(mapping.provenance().source(), "legacy_unknown");
	assert_eq!(mapping.provenance().created_at_unix(), None);
	assert_eq!(mapping.provenance().updated_at_unix(), None);
}

#[test]
fn persistent_clear_worktree_deletes_review_lifecycle() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let handoff = ReviewHandoffMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let orchestration = ReviewOrchestrationMarker::new(
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
	);

	store
		.upsert_worktree("pubfi", "PUB-101", "x/decodex-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree mapping should be recorded");
	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("handoff projection should persist");
	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &orchestration)
		.expect("orchestration projection should persist");
	store.clear_worktree("PUB-101").expect("worktree cleanup should persist");

	let reopened = StateStore::open(&state_path).expect("reopened store should open");

	assert!(
		reopened.worktree_for_issue("PUB-101").expect("worktree lookup should succeed").is_none()
	);
	assert!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("handoff lookup should succeed")
			.is_none()
	);
	assert!(
		reopened
			.review_orchestration_marker("pubfi", "PUB-101", &handoff)
			.expect("orchestration lookup should succeed")
			.is_none()
	);
}

#[test]
fn canonicalize_issue_identity_retargets_persistent_rows_without_cache_refresh() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let stale_store = StateStore::open(&state_path).expect("stale state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
	let handoff = sample_pub_101_review_handoff();
	let orchestration = sample_pub_101_review_orchestration();

	writer
		.record_run_attempt("run-1", "PUB-101", 1, "running")
		.expect("run attempt should persist");
	writer
		.upsert_worktree("pubfi", "PUB-101", "x/decodex-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree mapping should persist");
	writer
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should persist");
	writer
		.append_private_execution_event(
			"pubfi",
			"PUB-101",
			"run-1",
			1,
			"progress_checkpoint",
			serde_json::json!({ "summary": "cached on visible tracker key" }),
		)
		.expect("private evidence should persist");
	writer
		.upsert_decision_contract("pubfi", Some("PUB-101"), latent_decision_contract_fixture())
		.expect("decision contract should persist");
	writer
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("handoff projection should persist");
	writer
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &orchestration)
		.expect("orchestration projection should persist");

	upsert_handoff_review_policy_checkpoint(
		&writer,
		"PUB-101",
		"run-1",
		"findings",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		2,
	);

	stale_store
		.canonicalize_issue_identity("PUB-101", "linear-id-101")
		.expect("identity should canonicalize from SQLite rows");

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let run = reopened
		.run_attempt("run-1")
		.expect("run attempt should read")
		.expect("run attempt should exist");

	assert_eq!(run.issue_id(), "linear-id-101");
	assert!(reopened.lease_for_issue("PUB-101").expect("old lease lookup should read").is_none());
	assert!(
		reopened.worktree_for_issue("PUB-101").expect("old worktree lookup should read").is_none()
	);
	assert_eq!(
		reopened
			.lease_for_issue("linear-id-101")
			.expect("canonical lease lookup should read")
			.expect("canonical lease should exist")
			.run_id(),
		"run-1"
	);
	assert_eq!(
		reopened
			.worktree_for_issue("linear-id-101")
			.expect("canonical worktree lookup should read")
			.expect("canonical worktree should exist")
			.branch_name(),
		"x/decodex-pub-101"
	);
	assert_eq!(
		reopened
			.list_private_execution_events("pubfi", "linear-id-101", "run-1", 1)
			.expect("canonical private evidence should read")
			.len(),
		1
	);

	assert_decision_contract_retargeted(&reopened);

	assert_eq!(
		reopened
			.review_handoff_marker("pubfi", "linear-id-101", "x/decodex-pub-101")
			.expect("canonical handoff should read"),
		Some(handoff.clone())
	);
	assert_eq!(
		reopened
			.review_orchestration_marker("pubfi", "linear-id-101", &handoff)
			.expect("canonical orchestration should read"),
		Some(orchestration)
	);
	assert!(
		reopened
			.review_policy_checkpoint("pubfi", "PUB-101", "run-1", 1, "handoff")
			.expect("old review policy checkpoint should read")
			.is_none()
	);

	let canonical_checkpoint = reopened
		.review_policy_checkpoint("pubfi", "linear-id-101", "run-1", 1, "handoff")
		.expect("canonical review policy checkpoint should read")
		.expect("canonical review policy checkpoint should exist");

	assert_eq!(canonical_checkpoint.status(), "findings");
	assert_eq!(canonical_checkpoint.nonclean_rounds(), 2);
}

#[test]
fn lists_issue_leases() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("first lease should be inserted");
	store
		.upsert_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
		.expect("second lease should be inserted");

	let leases = store.list_leases("pubfi").expect("lease listing should succeed");

	assert_eq!(leases.len(), 2);
	assert_eq!(leases[0].project_id(), "pubfi");
	assert_eq!(leases[0].issue_id(), "PUB-101");
	assert_eq!(leases[1].issue_id(), "PUB-102");
}

#[test]
fn lists_recent_project_runs_with_protocol_summary() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.record_run_attempt("run-2", "PUB-102", 2, "failed")
		.expect("older run attempt should be recorded");
	store
		.record_run_attempt("run-1", "PUB-101", 1, "running")
		.expect("running run attempt should be recorded");
	store.update_run_thread("run-1", "thread-1").expect("thread id should attach");
	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("active worktree should record");
	store
		.upsert_worktree("pubfi", "PUB-102", "x/pubfi-pub-102", "/tmp/worktrees/pub-102")
		.expect("retained worktree should record");
	store
		.append_event("run-1", 1, "turn/started", "{\"turn\":\"1\"}")
		.expect("event should record");
	store
		.append_event("run-1", 2, "turn/completed", "{\"turn\":\"1\"}")
		.expect("second event should record");

	let runs = store.list_recent_runs("pubfi", 10).expect("recent project runs should load");

	assert_eq!(runs.len(), 2);
	assert_eq!(runs[0].run_id(), "run-1");
	assert!(runs[0].run_lease());
	assert_eq!(runs[0].thread_id(), Some("thread-1"));
	assert_eq!(runs[0].event_count(), 2);
	assert_eq!(runs[0].last_event_type(), Some("turn/completed"));
	assert_eq!(runs[0].branch_name(), Some("x/pubfi-pub-101"));
	assert_eq!(runs[0].worktree_path(), Some(Path::new("/tmp/worktrees/pub-101")));
	assert_eq!(runs[1].run_id(), "run-2");
	assert!(!runs[1].run_lease());
	assert_eq!(runs[1].event_count(), 0);
}

#[test]
fn lists_project_issue_runs_recovered_from_local_evidence() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");
	let activity = ChildAgentActivitySummary {
		buckets: vec![ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds: 120,
			event_count: 2,
			tool_call_count: 1,
			input_tokens: 400,
			output_tokens: 80,
			..ChildAgentActivityBucket::default()
		}],
		wall_seconds: 120,
		event_count: 2,
		tool_call_count: 1,
		input_tokens_cumulative: 400,
		output_tokens_cumulative: 80,
		..ChildAgentActivitySummary::default()
	};

	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree should record");
	store
		.record_run_activity_summary("run-recovered", 1, Some(&activity), None)
		.expect("activity summary should record");
	store
		.append_event("run-recovered", 1, "turn/completed", "{}")
		.expect("protocol event should record");
	store
		.append_private_execution_event(
			"pubfi",
			"PUB-101",
			"run-recovered",
			1,
			"issue_progress_checkpoint",
			serde_json::json!({ "source": "test" }),
		)
		.expect("private execution evidence should record");

	let runs = store.list_project_issue_runs("pubfi", "PUB-101").expect("issue runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-recovered");
	assert_eq!(runs[0].attempt_number(), 1);
	assert_eq!(runs[0].status(), "recovered");
	assert_eq!(runs[0].recovery_source(), "recovered");
	assert!(
		runs[0]
			.recovery_evidence()
			.iter()
			.any(|evidence| evidence == "private_execution_event:issue_progress_checkpoint")
	);
	assert!(runs[0].recovery_evidence().iter().any(|evidence| evidence == "run_activity_summary"));
	assert!(runs[0].recovery_evidence().iter().any(|evidence| evidence == "protocol_events:1"));
	assert!(runs[0].recovery_gaps().is_empty());
	assert_eq!(runs[0].event_count(), 1);
	assert_eq!(runs[0].child_agent_activity().expect("activity should recover").event_count, 2);
}

#[test]
fn lists_recent_project_runs_after_terminal_lane_cleanup() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.record_run_attempt("run-1", "PUB-101", 1, "running")
		.expect("run attempt should record before project ownership is known");
	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record project ownership");
	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree should record project ownership");
	store.update_run_status("run-1", "succeeded").expect("terminal status should update");
	store.clear_lease("PUB-101").expect("terminal cleanup should clear run lease");
	store.clear_worktree("PUB-101").expect("terminal cleanup should clear worktree mapping");

	let runs = store.list_recent_runs("pubfi", 10).expect("recent project runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-1");
	assert_eq!(runs[0].status(), "succeeded");
	assert!(!runs[0].run_lease());
	assert_eq!(runs[0].branch_name(), None);
	assert_eq!(runs[0].worktree_path(), None);
	assert!(
		store.list_recent_runs("other", 10).expect("other project lookup should load").is_empty(),
		"remembered run ownership must stay scoped to the original project"
	);
}

#[test]
fn lists_active_project_runs_only() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store.record_run_attempt("run-1", "PUB-101", 1, "running").expect("first run should record");
	store.record_run_attempt("run-2", "PUB-102", 1, "running").expect("second run should record");
	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store
		.upsert_lease("other", "PUB-102", "run-2", IN_PROGRESS_STATE)
		.expect("other-project lease should record");
	store
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("first worktree should record");
	store
		.upsert_worktree("other", "PUB-102", "x/other-pub-102", "/tmp/worktrees/pub-102")
		.expect("second worktree should record");

	let runs = store.list_leased_runs("pubfi").expect("active project runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-1");
	assert!(runs[0].run_lease());
}

#[test]
fn state_store_open_persists_runtime_history_across_instances() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.db");
	let first = StateStore::open(&state_path).expect("first state store should open");

	first
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should persist");
	first.record_run_attempt("run-1", "PUB-101", 1, "running").expect("run attempt should record");
	first.update_run_thread("run-1", "thread-1").expect("thread should persist");
	first.append_event("run-1", 1, "thread/run/created", "{}").expect("event should persist");
	first
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree should persist");

	let mut ledger_record = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "PUB-101",
			issue_identifier: "PUB-101",
			run_id: "run-1",
			attempt_number: 1,
		},
		"closeout",
		String::from("2026-04-29T10:10:00Z"),
		"closeout",
	);

	ledger_record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/101"));
	ledger_record.commit_sha = Some(String::from("1111111111111111111111111111111111111111"));
	ledger_record.summary = Some(String::from("Completed retained closeout."));

	first
		.record_linear_execution_event(&ledger_record)
		.expect("linear execution event should persist");

	assert!(state_path.exists(), "persistent runtime DB should be created");

	let second = StateStore::open(&state_path).expect("second state store should open");
	let latest = second
		.latest_run_attempt_for_issue("PUB-101")
		.expect("latest run lookup should succeed")
		.expect("persistent store should recover run history");

	assert_eq!(latest.run_id(), "run-1");
	assert_eq!(latest.thread_id(), Some("thread-1"));
	assert_eq!(second.event_count("run-1").expect("event count should load"), 1);
	assert!(
		second.lease_for_issue("PUB-101").expect("lease lookup should succeed").is_some(),
		"persistent store should recover run leases"
	);
	assert!(
		second.worktree_for_issue("PUB-101").expect("worktree lookup should succeed").is_some(),
		"persistent store should recover retained worktree mappings"
	);

	let ledger_records = second
		.list_linear_execution_events("pubfi", "PUB-101")
		.expect("linear execution events should load");

	assert_eq!(ledger_records, vec![ledger_record]);
}

#[test]
fn private_execution_events_persist_reload_and_keep_append_order() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let first = store
		.append_private_execution_event(
			"decodex",
			"XY-520",
			"run-1",
			2,
			"evidence_snapshot",
			serde_json::json!({
				"summary": "first private snapshot",
				"evidence": ["runtime-db", "local-only"],
			}),
		)
		.expect("first private event should append");
	let second = store
		.append_private_execution_event(
			"decodex",
			"XY-520",
			"run-1",
			2,
			"review_pass",
			serde_json::json!({
				"summary": "second private snapshot",
				"outcome": "clean",
			}),
		)
		.expect("second private event should append");

	assert!(
		first.record_id() < second.record_id(),
		"private event row ids should preserve append order"
	);

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let events = reopened
		.list_private_execution_events("decodex", "XY-520", "run-1", 2)
		.expect("private events should reload");

	assert_eq!(events.len(), 2);
	assert_eq!(events[0].record_id(), first.record_id());
	assert_eq!(events[0].project_id(), "decodex");
	assert_eq!(events[0].issue_id(), "XY-520");
	assert_eq!(events[0].run_id(), "run-1");
	assert_eq!(events[0].attempt_number(), 2);
	assert_eq!(events[0].event_type(), "evidence_snapshot");
	assert_eq!(events[0].payload()["evidence"], serde_json::json!(["runtime-db", "local-only"]));
	assert_eq!(events[1].record_id(), second.record_id());
	assert_eq!(events[1].event_type(), "review_pass");
	assert_eq!(events[1].payload()["outcome"], serde_json::json!("clean"));
	assert!(events[0].recorded_at_unix() <= events[1].recorded_at_unix());
	assert!(!events[0].recorded_at().is_empty());
}

#[test]
fn project_loop_evidence_snapshot_filters_project_evidence_once() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let first = store
		.append_private_execution_event(
			"decodex",
			"XY-520",
			"run-1",
			2,
			"evidence_snapshot",
			serde_json::json!({"match": true}),
		)
		.expect("first private event should append");
	let second = store
		.append_private_execution_event(
			"decodex",
			"XY-520",
			"run-1",
			2,
			"terminal_finalize",
			serde_json::json!({"path": "review_handoff"}),
		)
		.expect("second private event should append");

	store
		.append_private_execution_event(
			"other",
			"XY-520",
			"run-1",
			2,
			"other_project",
			serde_json::json!({"match": false}),
		)
		.expect("other project private event should append");
	store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "decodex",
			issue_id: "XY-520",
			run_id: "run-1",
			attempt_number: 2,
			phase: "handoff",
			status: "clean",
			head_sha: "abc123",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review policy checkpoint should persist");
	store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "other",
			issue_id: "XY-520",
			run_id: "run-1",
			attempt_number: 2,
			phase: "handoff",
			status: "findings",
			head_sha: "def456",
			nonclean_rounds: 1,
			details_json: "{}",
		})
		.expect("other project checkpoint should persist");

	let snapshot = StateStore::open(&state_path)
		.expect("state store should reopen")
		.project_loop_evidence_snapshot("decodex")
		.expect("project loop evidence should load");
	let events = snapshot.private_events("XY-520", "run-1", 2);
	let checkpoint = snapshot
		.review_policy_checkpoint("XY-520", "run-1", 2, "handoff")
		.expect("matching checkpoint should exist");

	assert_eq!(
		events.iter().map(|event| event.record_id()).collect::<Vec<_>>(),
		vec![first.record_id(), second.record_id()],
		"snapshot should preserve append order and exclude other projects"
	);
	assert_eq!(events[1].event_type(), "terminal_finalize");
	assert_eq!(checkpoint.status(), "clean");
	assert!(snapshot.private_events("XY-521", "run-1", 2).is_empty());
}

#[test]
fn private_execution_events_filter_issue_run_attempt_and_stay_out_of_linear_cache() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.append_private_execution_event(
			"decodex",
			"XY-520",
			"run-1",
			1,
			"kept",
			serde_json::json!({"match": true}),
		)
		.expect("matching private event should append");
	store
		.append_private_execution_event(
			"decodex",
			"XY-521",
			"run-1",
			1,
			"other_issue",
			serde_json::json!({"match": false}),
		)
		.expect("other issue private event should append");
	store
		.append_private_execution_event(
			"decodex",
			"XY-520",
			"run-2",
			1,
			"other_run",
			serde_json::json!({"match": false}),
		)
		.expect("other run private event should append");
	store
		.append_private_execution_event(
			"decodex",
			"XY-520",
			"run-1",
			2,
			"other_attempt",
			serde_json::json!({"match": false}),
		)
		.expect("other attempt private event should append");
	store
		.append_private_execution_event(
			"pubfi",
			"XY-520",
			"run-1",
			1,
			"other_project",
			serde_json::json!({"match": false}),
		)
		.expect("other project private event should append");

	let events = store
		.list_private_execution_events("decodex", "XY-520", "run-1", 1)
		.expect("private events should list");

	assert_eq!(events.len(), 1);
	assert_eq!(events[0].event_type(), "kept");
	assert_eq!(events[0].payload()["match"], serde_json::json!(true));
	assert!(
		store
			.list_linear_execution_events("decodex", "XY-520")
			.expect("linear event cache should read")
			.is_empty(),
		"private execution events must not populate the public Linear mirror cache"
	);
}

#[test]
fn decision_contracts_persist_reload_and_promote_without_linear_mirror() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let latent = latent_decision_contract_fixture();
	let record = store
		.upsert_decision_contract("decodex", Some("XY-852"), latent)
		.expect("latent decision contract should persist");

	assert_eq!(record.project_id(), "decodex");
	assert_eq!(record.source_issue_id(), Some("XY-852"));
	assert_eq!(record.contract_id(), "research-x-loop-contract");
	assert_eq!(record.status(), DecisionContractStatus::DraftLatent);
	assert!(record.created_at_unix() > 0);
	assert!(record.updated_at_unix() >= record.created_at_unix());

	let promoted = store
		.promote_decision_contract(
			"decodex",
			"research-x-loop-contract",
			sample_decision_promotion(),
		)
		.expect("latent contract should promote");

	assert_eq!(promoted.status(), DecisionContractStatus::AcceptedPromoted);
	assert_eq!(
		promoted.contract().promotion().expect("promotion metadata should persist").accepted_by(),
		"operator"
	);
	assert!(
		store
			.list_linear_execution_events("decodex", "XY-852")
			.expect("linear mirror should read")
			.is_empty(),
		"decision contracts stay in runtime SQLite and do not populate Linear cache"
	);

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let reloaded = reopened
		.decision_contract("decodex", "research-x-loop-contract")
		.expect("decision contract should read")
		.expect("decision contract should exist");

	assert_eq!(reloaded.status(), DecisionContractStatus::AcceptedPromoted);
	assert_eq!(reloaded.source_issue_id(), Some("XY-852"));
	assert_eq!(reloaded.created_at(), record.created_at());
	assert!(reloaded.updated_at_unix() >= record.updated_at_unix());
	assert_eq!(reloaded.contract().accepted_authority().accepted_objectives().len(), 2);

	let issue_contracts = reopened
		.list_decision_contracts_for_issue("decodex", "XY-852")
		.expect("source issue contracts should list");

	assert_eq!(issue_contracts.len(), 1);
	assert_eq!(issue_contracts[0].contract_id(), "research-x-loop-contract");

	let project_contracts = reopened
		.list_decision_contracts_for_project("decodex")
		.expect("project contracts should list");

	assert_eq!(project_contracts.len(), 1);
	assert_eq!(project_contracts[0].contract_id(), "research-x-loop-contract");
}

#[test]
fn decision_contracts_record_human_decision_and_rejection_transitions() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_decision_contract("decodex", Some("XY-852"), latent_decision_contract_fixture())
		.expect("latent decision contract should persist");

	let waiting = store
		.mark_decision_contract_needs_human_decision(
			"decodex",
			"research-x-loop-contract",
			"Choose which generated issue should run first.",
		)
		.expect("contract should record human decision need");

	assert_eq!(waiting.status(), DecisionContractStatus::NeedsHumanDecision);
	assert!(
		waiting
			.contract()
			.execution_readiness()
			.missing_decisions()
			.iter()
			.any(|decision| decision == "Choose which generated issue should run first.")
	);

	let rejected = store
		.reject_decision_contract(
			"decodex",
			"research-x-loop-contract",
			Some(String::from("research-x-loop-contract-v2")),
		)
		.expect("contract should reject");

	assert_eq!(rejected.status(), DecisionContractStatus::RejectedSuperseded);
	assert_eq!(
		rejected.contract().links().superseded_by_contract_id(),
		Some("research-x-loop-contract-v2")
	);
	assert!(
		store
			.promote_decision_contract(
				"decodex",
				"research-x-loop-contract",
				sample_decision_promotion()
			)
			.is_err(),
		"rejected contracts cannot later become execution authority"
	);
}

#[test]
fn execution_programs_persist_reload_and_list_by_contract() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let mut contract = latent_decision_contract_fixture();

	contract.promote(sample_decision_promotion()).expect("contract should promote");

	let program = sample_execution_program(&contract);
	let record = store
		.upsert_execution_program("decodex", program)
		.expect("execution program should persist");

	assert_eq!(record.project_id(), "decodex");
	assert_eq!(record.program_id(), "program-853");
	assert_eq!(record.source_contract_id(), Some("research-x-loop-contract"));
	assert_eq!(record.program().nodes().len(), 1);
	assert!(record.created_at_unix() > 0);
	assert!(record.updated_at_unix() >= record.created_at_unix());

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let reloaded = reopened
		.execution_program("decodex", "program-853")
		.expect("execution program should read")
		.expect("execution program should exist");

	assert_eq!(reloaded.created_at(), record.created_at());
	assert_eq!(reloaded.program().source_contract_id(), Some("research-x-loop-contract"));

	let contract_programs = reopened
		.list_execution_programs_for_contract("decodex", "research-x-loop-contract")
		.expect("contract programs should list");

	assert_eq!(contract_programs.len(), 1);
	assert_eq!(contract_programs[0].program_id(), "program-853");

	let project_programs =
		reopened.list_execution_programs("decodex").expect("project programs should list");

	assert_eq!(project_programs.len(), 1);
	assert_eq!(project_programs[0].program_id(), "program-853");

	let intake_plans =
		reopened.list_program_intake_plans("decodex").expect("program intake plans should list");

	assert_eq!(intake_plans.len(), 1);
	assert_eq!(intake_plans[0].program_id(), "program-853");
	assert_eq!(intake_plans[0].intake_kind(), "goal_intake");
	assert_eq!(intake_plans[0].source_contract_id(), Some("research-x-loop-contract"));

	let issue_mappings = reopened
		.list_program_issue_mappings("decodex", "program-853")
		.expect("program issue mappings should list");

	assert_eq!(issue_mappings.len(), 1);
	assert_eq!(issue_mappings[0].node_id(), "runtime-readiness");
	assert_eq!(issue_mappings[0].issue_identifier(), "XY-853");
	assert_eq!(issue_mappings[0].queue_intent(), "ready_to_queue");
	assert!(!issue_mappings[0].has_active_label());
}

#[test]
fn execution_program_reload_rejects_row_key_payload_mismatch() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let mut contract = latent_decision_contract_fixture();

	contract.promote(sample_decision_promotion()).expect("contract should promote");
	store
		.upsert_execution_program("decodex", sample_execution_program(&contract))
		.expect("execution program should persist");

	let connection = Connection::open(&state_path).expect("sqlite should open");
	let mut payload: Value = serde_json::from_str(
		&connection
			.query_row(
				"SELECT payload_json FROM execution_programs WHERE program_id = ?1",
				["program-853"],
				|row| row.get::<_, String>(0),
			)
			.expect("payload should load"),
	)
	.expect("payload should parse");

	payload["program_id"] = serde_json::json!("program-mismatch");

	connection
		.execute(
			"UPDATE execution_programs SET payload_json = ?1 WHERE program_id = ?2",
			[
				serde_json::to_string(&payload).expect("payload should serialize"),
				String::from("program-853"),
			],
		)
		.expect("payload should corrupt");

	assert!(
		StateStore::open(&state_path).is_err(),
		"execution program row key must match the versioned payload program_id"
	);
}

#[test]
fn decision_contract_reload_rejects_row_key_payload_mismatch() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_decision_contract("decodex", Some("XY-852"), latent_decision_contract_fixture())
		.expect("latent decision contract should persist");

	let mut payload = serde_json::from_str::<Value>(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/decision_contract/research_x_latent_contract.json"
	)))
	.expect("fixture should parse as JSON");

	payload["contract_id"] = serde_json::json!("mismatched-contract-id");

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute(
			"UPDATE decision_contracts SET payload_json = ?1 WHERE contract_id = ?2",
			rusqlite::params![
				serde_json::to_string(&payload).expect("payload should serialize"),
				"research-x-loop-contract",
			],
		)
		.expect("decision contract row should corrupt for test");

	assert!(
		StateStore::open(&state_path).is_err(),
		"decision contract row key must match the versioned payload contract_id"
	);
}

#[test]
fn state_store_open_refreshes_pubfi_project_registry_across_instances() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.db");
	let initial_config_path = temp_dir.path().join("stale/project.toml");
	let initial_repo_root = temp_dir.path().join("stale/repo");
	let initial_worktree_root = temp_dir.path().join("stale/repo/.worktrees");
	let initial_workflow_path = temp_dir.path().join("stale/repo/WORKFLOW.md");
	let refreshed_config_path = temp_dir.path().join("current/project.toml");
	let refreshed_repo_root = temp_dir.path().join("current/repo");
	let refreshed_worktree_root = temp_dir.path().join("current/repo/.worktrees");
	let refreshed_workflow_path = temp_dir.path().join("current/repo/WORKFLOW.md");
	let store = StateStore::open(&state_path).expect("state store should open");
	let registration = ProjectRegistration {
		service_id: String::from("pubfi"),
		config_path: initial_config_path,
		repo_root: initial_repo_root,
		worktree_root: initial_worktree_root,
		workflow_path: initial_workflow_path,
		tracker_api_key_env_var: String::from("LINEAR_API_KEY_HACKINK"),
		github_token_env_var: String::from("GITHUB_PAT_Y"),
		enabled: true,
		config_fingerprint: String::from("abc123"),
		updated_at: String::from("2026-04-29T00:00:00Z"),
		updated_at_unix: 1_777_392_000,
	};
	let refreshed_registration = ProjectRegistration {
		service_id: String::from("pubfi"),
		config_path: refreshed_config_path.clone(),
		repo_root: refreshed_repo_root.clone(),
		worktree_root: refreshed_worktree_root.clone(),
		workflow_path: refreshed_workflow_path.clone(),
		tracker_api_key_env_var: String::from("LINEAR_API_KEY_HACKINK"),
		github_token_env_var: String::from("GITHUB_PAT_Y"),
		enabled: true,
		config_fingerprint: String::from("def456"),
		updated_at: String::from("2026-04-30T00:00:00Z"),
		updated_at_unix: 1_777_478_400,
	};

	store.upsert_project(&registration).expect("project should persist");
	store.set_project_enabled("pubfi", false).expect("project should disable");
	store.upsert_project(&refreshed_registration).expect("project should refresh");

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let projects = reopened.list_projects().expect("project registry should load");

	assert_eq!(projects.len(), 1, "pubfi refresh should keep one scoped registry row");

	let project = &projects[0];

	assert_eq!(
		project.service_id(),
		"pubfi",
		"pubfi refresh should stay scoped to the same service id"
	);
	assert!(!project.enabled(), "pubfi refresh should preserve the existing disabled state");
	assert_eq!(
		project.config_fingerprint(),
		"def456",
		"pubfi refresh should replace the stale config fingerprint"
	);
	assert_eq!(
		project.config_path(),
		refreshed_config_path.as_path(),
		"pubfi refresh should replace the stale config path"
	);
	assert_eq!(
		project.repo_root(),
		refreshed_repo_root.as_path(),
		"pubfi refresh should replace the stale repo root"
	);
	assert_eq!(
		project.worktree_root(),
		refreshed_worktree_root.as_path(),
		"pubfi refresh should replace the stale worktree root"
	);
	assert_eq!(
		project.workflow_path(),
		refreshed_workflow_path.as_path(),
		"pubfi refresh should replace the stale workflow path"
	);
}

#[test]
fn lazy_project_registry_refresh_preserves_runtime_rows() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.db");
	let full_store = StateStore::open(&state_path).expect("state store should open");
	let registration = ProjectRegistration {
		service_id: String::from("pubfi"),
		config_path: temp_dir.path().join("project.toml"),
		repo_root: temp_dir.path().join("repo"),
		worktree_root: temp_dir.path().join("repo/.worktrees"),
		workflow_path: temp_dir.path().join("repo/WORKFLOW.md"),
		tracker_api_key_env_var: String::from("LINEAR_API_KEY_HACKINK"),
		github_token_env_var: String::from("GITHUB_PAT_Y"),
		enabled: true,
		config_fingerprint: String::from("abc123"),
		updated_at: String::from("2026-04-29T00:00:00Z"),
		updated_at_unix: 1_777_392_000,
	};
	let refreshed_registration = ProjectRegistration {
		config_fingerprint: String::from("def456"),
		updated_at: String::from("2026-04-30T00:00:00Z"),
		updated_at_unix: 1_777_478_400,
		..registration.clone()
	};

	full_store.upsert_project(&registration).expect("project should persist");
	full_store.record_run_attempt("run-1", "PUB-101", 1, "running").expect("run should record");
	full_store
		.append_event("run-1", 1, "item/agentMessage/delta", "{}")
		.expect("event should append");
	full_store
		.upsert_worktree(
			"pubfi",
			"PUB-101",
			"x/pub-101",
			temp_dir.path().join("repo/.worktrees/PUB-101").to_string_lossy().as_ref(),
		)
		.expect("worktree should persist");

	let lazy_store = StateStore::open_lazy(&state_path).expect("lazy state store should open");

	lazy_store.upsert_project(&refreshed_registration).expect("project should refresh");

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let attempt = reopened
		.latest_run_attempt_for_issue("PUB-101")
		.expect("attempt lookup should succeed")
		.expect("attempt should survive lazy project refresh");
	let mapping = reopened
		.worktree_for_issue("PUB-101")
		.expect("worktree lookup should succeed")
		.expect("worktree should survive lazy project refresh");

	assert_eq!(attempt.run_id(), "run-1");
	assert_eq!(reopened.event_count("run-1").expect("event count should survive"), 1);
	assert_eq!(mapping.project_id(), "pubfi");
	assert_eq!(
		reopened.list_projects().expect("project registry should load")[0].config_fingerprint(),
		"def456"
	);
}

#[test]
fn remove_project_deletes_persistent_registry_row() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.db");
	let store = StateStore::open(&state_path).expect("state store should open");
	let registration = ProjectRegistration {
		service_id: String::from("vibe-mono"),
		config_path: temp_dir.path().join("project.toml"),
		repo_root: temp_dir.path().join("repo"),
		worktree_root: temp_dir.path().join("repo/.worktrees"),
		workflow_path: temp_dir.path().join("repo/WORKFLOW.md"),
		tracker_api_key_env_var: String::from("LINEAR_API_KEY_HACKINK"),
		github_token_env_var: String::from("GITHUB_PAT_Y"),
		enabled: true,
		config_fingerprint: String::from("abc123"),
		updated_at: String::from("2026-05-25T00:00:00Z"),
		updated_at_unix: 1_779_667_200,
	};

	store.upsert_project(&registration).expect("project should persist");

	let removed = store.remove_project("vibe-mono").expect("project should remove");

	assert_eq!(removed.service_id(), "vibe-mono");
	assert!(store.list_projects().expect("projects should list").is_empty());

	let reopened = StateStore::open(&state_path).expect("state store should reopen");

	assert!(
		reopened.list_projects().expect("project registry should load").is_empty(),
		"removed project must not remain in SQLite registry"
	);
}

#[test]
fn run_control_accepts_active_attempt_and_persists_audit() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let channel_path = temp_dir.path().join("control.channel");

	fs::write(&channel_path, "ready\n").expect("control channel should write");

	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_lease("pubfi", "issue-1", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store.record_run_attempt("run-1", "issue-1", 1, "running").expect("attempt should record");
	store.update_run_thread("run-1", "thread-1").expect("thread should record");
	store.update_run_turn("run-1", "turn-1").expect("turn should record");
	store
		.publish_run_control_channel_for_active_attempt("run-1", 1, &channel_path, "local_file")
		.expect("control channel should publish")
		.expect("active control channel should exist");

	let receipt = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			source: "test_hook",
			action: "noop",
			timeout_ms: Some(500),
			metadata: None,
			context: None,
		})
		.expect("control request should resolve");

	assert_eq!(receipt.outcome(), "accepted");
	assert_eq!(receipt.reason(), "run_lease_control_channel_resolved");
	assert!(receipt.channel().is_some());

	for (outcome, reason) in [
		(RUN_CONTROL_ACTION_COMPLETED, "noop_completed"),
		(RUN_CONTROL_ACTION_FAILED, "noop_failed"),
		(RUN_CONTROL_ACTION_TIMED_OUT, "noop_timed_out"),
		(RUN_CONTROL_ACTION_FALLBACK, "noop_fallback"),
	] {
		store
			.record_run_control_action_outcome(&receipt, outcome, reason)
			.expect("follow-up control audit should record");
	}

	drop(store);

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let events = reopened
		.list_private_execution_events("pubfi", "issue-1", "run-1", 1)
		.expect("private control audit should read");
	let outcomes = events
		.iter()
		.filter(|event| event.event_type() == "control_action")
		.filter_map(|event| event.payload().get("outcome").and_then(|value| value.as_str()))
		.collect::<Vec<_>>();

	assert_eq!(outcomes, vec!["accepted", "completed", "failed", "timed_out", "fallback"]);
}

#[test]
fn run_control_rejects_stale_turn_and_run_mismatch() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let channel_path = temp_dir.path().join("control.channel");

	fs::write(&channel_path, "ready\n").expect("control channel should write");

	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.upsert_lease("pubfi", "issue-1", "run-current", IN_PROGRESS_STATE)
		.expect("lease should record");
	store
		.record_run_attempt("run-current", "issue-1", 1, "running")
		.expect("attempt should record");
	store.update_run_thread("run-current", "thread-1").expect("thread should record");
	store.update_run_turn("run-current", "turn-current").expect("turn should record");
	store
		.publish_run_control_channel_for_active_attempt(
			"run-current",
			1,
			&channel_path,
			"local_file",
		)
		.expect("control channel should publish");

	let stale_turn = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-current",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-old"),
			source: "test_hook",
			action: "steer",
			timeout_ms: None,
			metadata: None,
			context: None,
		})
		.expect("stale turn should be audited");
	let stale_run = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-old",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-current"),
			source: "test_hook",
			action: "noop",
			timeout_ms: None,
			metadata: None,
			context: None,
		})
		.expect("stale run should be audited");

	assert_eq!(stale_turn.outcome(), "rejected");
	assert_eq!(stale_turn.reason(), "turn_mismatch");
	assert_eq!(stale_turn.current_turn_id(), Some("turn-current"));
	assert_eq!(stale_run.outcome(), "rejected");
	assert_eq!(stale_run.reason(), "run_not_found");

	let events = store
		.list_private_execution_events("pubfi", "issue-1", "run-current", 1)
		.expect("private control audit should read");
	let stale_turn_event = events
		.iter()
		.find(|event| event.record_id() == stale_turn.audit_record_id())
		.expect("stale turn audit event should exist");

	assert_eq!(
		stale_turn_event.payload()["failure_class"].as_str(),
		Some("stale_expected_turn_id")
	);
	assert_eq!(stale_turn_event.payload()["observed"]["turn_id"].as_str(), Some("turn-current"));
}

#[test]
fn run_control_rejects_missing_channel_file() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let channel_path = temp_dir.path().join("control.channel");

	fs::write(&channel_path, "ready\n").expect("control channel should write");

	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.upsert_lease("pubfi", "issue-1", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store.record_run_attempt("run-1", "issue-1", 1, "running").expect("attempt should record");
	store
		.publish_run_control_channel_for_active_attempt("run-1", 1, &channel_path, "local_file")
		.expect("control channel should publish");

	fs::remove_file(&channel_path).expect("control channel should be removable");

	let receipt = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-1",
			attempt_number: 1,
			thread_id: None,
			turn_id: None,
			source: "test_hook",
			action: "noop",
			timeout_ms: None,
			metadata: None,
			context: None,
		})
		.expect("missing channel should be audited");

	assert_eq!(receipt.outcome(), "rejected");
	assert_eq!(receipt.reason(), "control_channel_missing");
}

#[test]
fn run_control_requires_run_lease_ownership() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let channel_path = temp_dir.path().join("control.channel");
	let worktree_path = temp_dir.path().join("PUB-101");

	fs::write(&channel_path, "ready\n").expect("control channel should write");

	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.upsert_lease("pubfi", "issue-1", "run-1", IN_PROGRESS_STATE)
		.expect("lease should record");
	store.record_run_attempt("run-1", "issue-1", 1, "running").expect("attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			"issue-1",
			"x/pubfi-issue-1",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.publish_run_control_channel_for_active_attempt("run-1", 1, &channel_path, "local_file")
		.expect("control channel should publish");
	store.clear_lease("issue-1").expect("lease should clear");

	let no_lease = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-1",
			attempt_number: 1,
			thread_id: None,
			turn_id: None,
			source: "test_hook",
			action: "noop",
			timeout_ms: None,
			metadata: None,
			context: None,
		})
		.expect("missing lease should be audited");

	store
		.upsert_lease("pubfi", "issue-1", "run-other", IN_PROGRESS_STATE)
		.expect("other lease should record");

	let wrong_run = store
		.resolve_run_control_action(RunControlActionRequest {
			project_id: "pubfi",
			issue_id: "issue-1",
			run_id: "run-1",
			attempt_number: 1,
			thread_id: None,
			turn_id: None,
			source: "test_hook",
			action: "noop",
			timeout_ms: None,
			metadata: None,
			context: None,
		})
		.expect("wrong run lease should be audited");

	assert_eq!(no_lease.outcome(), "rejected");
	assert_eq!(no_lease.reason(), "run_lease_missing");
	assert_eq!(wrong_run.outcome(), "rejected");
	assert_eq!(wrong_run.reason(), "run_lease_mismatch");

	let events = store
		.list_private_execution_events("pubfi", "issue-1", "run-1", 1)
		.expect("private control audit should read");
	let no_lease_event = events
		.iter()
		.find(|event| event.record_id() == no_lease.audit_record_id())
		.expect("missing lease audit event should exist");
	let expected_worktree_path = worktree_path.display().to_string();
	let expected_channel_path = channel_path.display().to_string();

	assert_eq!(no_lease_event.payload()["lane"]["run_lease"].as_bool(), Some(false));
	assert_eq!(no_lease_event.payload()["lane"]["attempt_status"].as_str(), Some("running"));
	assert_eq!(no_lease_event.payload()["lane"]["branch"].as_str(), Some("x/pubfi-issue-1"));
	assert_eq!(
		no_lease_event.payload()["lane"]["worktree_path"].as_str(),
		Some(expected_worktree_path.as_str())
	);
	assert_eq!(no_lease_event.payload()["channel"]["status"].as_str(), Some("active"));
	assert_eq!(no_lease_event.payload()["channel"]["path_exists"].as_bool(), Some(true));
	assert_eq!(
		no_lease_event.payload()["channel"]["channel_path"].as_str(),
		Some(expected_channel_path.as_str())
	);
}
