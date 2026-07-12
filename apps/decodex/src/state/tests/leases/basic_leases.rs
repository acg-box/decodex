use tempfile::TempDir;

use crate::{
	lane_authority::{
		EffectClass, EffectCommand, EffectState, LaneCommand, LaneEffect, LaneEffectKind, LaneId,
		LanePhase, NoEffectiveDeltaCommand, NoEffectiveDeltaDecision, NoEffectiveDeltaFacts,
		ProjectBinding,
	},
	state::{ProjectRegistration, StateStore},
};

const LEASE_IN_PROGRESS_STATE: &str = "In Progress";

fn registered_project(temp_dir: &TempDir) -> ProjectRegistration {
	ProjectRegistration {
		service_id: String::from("pubfi"),
		config_path: temp_dir.path().join("project.toml"),
		repo_root: temp_dir.path().join("repo"),
		worktree_root: temp_dir.path().join("repo/.worktrees"),
		workflow_path: temp_dir.path().join("WORKFLOW.md"),
		tracker_api_key_env_var: String::from("LINEAR_API_KEY"),
		github_token_env_var: String::from("GITHUB_TOKEN"),
		enabled: true,
		config_fingerprint: String::from("binding-1"),
		binding: ProjectBinding::new(
			"pubfi",
			"helixbox",
			"pubfi-mono",
			"team-pubfi",
			"decodex:queued:pubfi",
			"binding-1",
		)
		.expect("binding"),
		updated_at: String::from("2026-07-12T00:00:00Z"),
		updated_at_unix: 1_783_814_400,
	}
}

#[test]
fn lane_effect_journal_survives_restart_and_revalidates_lane_epoch() {
	let temp_dir = TempDir::new().expect("tempdir");
	let path = temp_dir.path().join("state.sqlite");
	let store = StateStore::open(&path).expect("store");
	store.upsert_project(&registered_project(&temp_dir)).expect("register project");
	store
		.try_acquire_registered_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
		.expect("claim");
	let lane_id = LaneId::new("pubfi", "PUB-101").expect("lane id");
	let lane = store.lane(&lane_id).expect("lane read").expect("lane");
	let effect = LaneEffect::plan(
		"effect-1",
		"operation-1",
		0,
		lane_id.clone(),
		lane.binding_fingerprint(),
		"run-1",
		lane.epoch(),
		LaneEffectKind::LinearIssueCommentCreate,
		EffectClass::DurablePublication,
		"operation-1:comment",
		"sha256:request",
		"sha256:desired",
		"sha256:facts",
	)
	.expect("effect");
	store.plan_lane_effect(effect).expect("plan effect");
	let invoking = store
		.apply_lane_effect_command(
			"effect-1",
			0,
			EffectCommand::BeginInvocation {
				authority_epoch: lane.epoch(),
				facts_fingerprint: String::from("sha256:facts"),
			},
		)
		.expect("begin effect");
	assert_eq!(invoking.state(), EffectState::Invoking);
	drop(store);

	let reopened = StateStore::open(&path).expect("reopen");
	assert_eq!(
		reopened.lane_effect("effect-1").expect("effect read").expect("effect").state(),
		EffectState::Invoking,
	);
	reopened
		.apply_lane_command(lane_id, lane.binding_fingerprint(), LaneCommand::BeginRun)
		.expect("advance lane");
	let error = reopened
		.apply_lane_effect_command("effect-1", 1, EffectCommand::MarkOutcomeUnknown)
		.expect_err("stale lane epoch must reject journal mutation");
	assert!(error.to_string().contains("prerequisites drifted"));
}

#[test]
fn no_effective_delta_recovery_survives_restart_and_replays_exactly_once() {
	let temp_dir = TempDir::new().expect("tempdir");
	let path = temp_dir.path().join("state.sqlite");
	let store = StateStore::open(&path).expect("store");
	store.upsert_project(&registered_project(&temp_dir)).expect("register project");
	store
		.try_acquire_registered_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
		.expect("claim");
	let command = no_effective_delta_observation();
	let first =
		store.decide_no_effective_delta("operation-1", command.clone()).expect("first retry");
	assert!(matches!(first, NoEffectiveDeltaDecision::Retry(_)));
	drop(store);

	let reopened = StateStore::open(&path).expect("reopen");
	let replay = reopened.decide_no_effective_delta("operation-1", command).expect("replay");
	assert_eq!(replay, first);
	assert_eq!(
		reopened
			.no_effective_delta_recovery("operation-1")
			.expect("read")
			.expect("recovery")
			.ordinal(),
		1
	);
	let attention = reopened
		.decide_no_effective_delta("operation-1", no_effective_delta_retry_result())
		.expect("terminal attention");
	assert!(matches!(attention, NoEffectiveDeltaDecision::AttentionRequired { .. }));
	drop(reopened);

	let reopened = StateStore::open(&path).expect("reopen terminal recovery");
	let terminal_replay = reopened
		.decide_no_effective_delta("operation-1", no_effective_delta_observation())
		.expect("terminal replay");
	assert_eq!(terminal_replay, attention);
}

#[test]
fn no_effective_delta_recovery_rejects_tampered_durable_diagnostics_on_open() {
	let temp_dir = TempDir::new().expect("tempdir");
	let path = temp_dir.path().join("state.sqlite");
	let store = StateStore::open(&path).expect("store");
	store.upsert_project(&registered_project(&temp_dir)).expect("register project");
	store
		.try_acquire_registered_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
		.expect("claim");
	store
		.decide_no_effective_delta("operation-1", no_effective_delta_observation())
		.expect("persist recovery");
	drop(store);

	let connection = rusqlite::Connection::open(&path).expect("open sqlite");
	let payload = connection
		.query_row(
			"SELECT payload_json FROM no_effective_delta_recoveries WHERE operation_id = 'operation-1'",
			[],
			|row| row.get::<_, String>(0),
		)
		.expect("read recovery");
	let mut payload = serde_json::from_str::<serde_json::Value>(&payload).expect("decode recovery");
	payload["facts"]["head_oid"] = serde_json::Value::String(String::from("tampered"));
	connection
		.execute(
			"UPDATE no_effective_delta_recoveries SET payload_json = ?1 WHERE operation_id = 'operation-1'",
			[serde_json::to_string(&payload).expect("encode recovery")],
		)
		.expect("tamper recovery");
	drop(connection);

	assert!(StateStore::open(&path).is_err());
}

fn no_effective_delta_observation() -> NoEffectiveDeltaCommand {
	NoEffectiveDeltaCommand::Observe {
		operation_id: String::from("operation-1"),
		lane_id: LaneId::new("pubfi", "PUB-101").expect("lane"),
		facts: NoEffectiveDeltaFacts::new(
			"base",
			"head",
			"merge-base",
			"patch",
			"names",
			"status",
			"surface",
			"acceptance",
			"checkpoints",
			"validation",
			false,
		)
		.expect("facts"),
	}
}

fn no_effective_delta_retry_result() -> NoEffectiveDeltaCommand {
	let NoEffectiveDeltaCommand::Observe { operation_id, lane_id, facts } =
		no_effective_delta_observation()
	else {
		unreachable!();
	};
	NoEffectiveDeltaCommand::ObserveRetryResult { operation_id, lane_id, facts }
}

#[test]
fn registered_lease_is_a_retryable_projection_of_canonical_lane_claim() {
	let temp_dir = TempDir::new().expect("tempdir");
	let store = StateStore::open(temp_dir.path().join("state.sqlite")).expect("store");
	store.upsert_project(&registered_project(&temp_dir)).expect("register project");
	let lane_id = LaneId::new("pubfi", "PUB-101").expect("lane id");

	assert!(
		store
			.try_acquire_registered_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE,)
			.expect("claim")
	);
	let claimed = store.lane(&lane_id).expect("lane read").expect("lane");
	assert_eq!(claimed.phase(), LanePhase::Claimed);
	assert_eq!(claimed.claim_run_id(), Some("run-1"));
	store.record_lane_run_attempt("pubfi", "run-1", "PUB-101", 3, "running").expect("lane attempt");
	assert_eq!(store.next_lane_attempt_number("pubfi", "PUB-101").expect("next attempt"), 4);
	store
		.record_lane_run_attempt("pubfi", "run-1", "PUB-101", 3, "failed")
		.expect("failed attempt");
	assert_eq!(
		store.retry_budget_attempt_count_for_lane("pubfi", "PUB-101").expect("retry count"),
		1
	);
	store
		.upsert_claimed_worktree("pubfi", "PUB-101", "xv/pub-101", "/tmp/pubfi/.worktrees/PUB-101")
		.expect("attach worktree");
	let attached = store.lane(&lane_id).expect("lane read").expect("lane");
	assert_eq!(attached.branch_name(), Some("xv/pub-101"));
	assert_eq!(
		attached.worktree_path().map(|path| path.as_path()),
		Some(std::path::Path::new("/tmp/pubfi/.worktrees/PUB-101")),
	);
	store
		.detach_claimed_worktree("pubfi", "PUB-101", "xv/pub-101", "/tmp/pubfi/.worktrees/PUB-101")
		.expect("detach worktree");
	let detached = store.lane(&lane_id).expect("lane read").expect("lane");
	assert_eq!(detached.branch_name(), None);
	assert_eq!(detached.worktree_path(), None);
	assert!(store.worktree_for_issue("PUB-101").expect("projection read").is_none());
	store
		.upsert_claimed_worktree("pubfi", "PUB-101", "xv/pub-101", "/tmp/pubfi/.worktrees/PUB-101")
		.expect("reattach worktree");

	store.clear_lease("PUB-101").expect("release claim");
	let released = store.lane(&lane_id).expect("lane read").expect("lane");
	assert_eq!(released.phase(), LanePhase::Unclaimed);
	assert_eq!(released.claim_run_id(), None);
	let mut alternate = registered_project(&temp_dir);
	alternate.service_id = String::from("pubfi-insight");
	alternate.config_fingerprint = String::from("binding-2");
	alternate.binding = ProjectBinding::new(
		"pubfi-insight",
		"helixbox",
		"pubfi-insight",
		"team-pubfi",
		"decodex:queued:pubfi-insight",
		"binding-2",
	)
	.expect("alternate binding");
	store.upsert_project(&alternate).expect("register alternate project");
	assert!(
		store
			.try_acquire_registered_lease(
				"pubfi-insight",
				"PUB-101",
				"alternate-run",
				LEASE_IN_PROGRESS_STATE,
			)
			.expect("released issue may move to another project")
	);
	assert_eq!(
		store.next_lane_attempt_number("pubfi-insight", "PUB-101").expect("next attempt"),
		1,
		"another project must not inherit the source lane attempt sequence",
	);
	assert!(
		store
			.latest_run_attempt_for_lane("pubfi-insight", "PUB-101")
			.expect("alternate latest attempt")
			.is_none(),
		"another project must not inherit source lane history",
	);
	assert_eq!(
		store
			.retry_budget_attempt_count_for_lane("pubfi-insight", "PUB-101")
			.expect("alternate retry count"),
		0,
	);
	store.clear_lease("PUB-101").expect("release alternate claim");
	assert!(
		store
			.try_acquire_registered_lease("pubfi", "PUB-101", "run-2", LEASE_IN_PROGRESS_STATE,)
			.expect("retry claim")
	);
}

#[test]
fn registered_lease_rejects_same_active_tracker_issue_in_another_project() {
	let temp_dir = TempDir::new().expect("tempdir");
	let store = StateStore::open(temp_dir.path().join("state.sqlite")).expect("store");
	let first = registered_project(&temp_dir);
	let mut second = first.clone();
	second.service_id = String::from("pubfi-insight");
	second.config_fingerprint = String::from("binding-2");
	second.binding = ProjectBinding::new(
		"pubfi-insight",
		"helixbox",
		"pubfi-insight",
		"team-pubfi",
		"decodex:queued:pubfi-insight",
		"binding-2",
	)
	.expect("second binding");
	store.upsert_project(&first).expect("register first");
	store.upsert_project(&second).expect("register second");
	assert!(
		store
			.try_acquire_registered_lease("pubfi", "PUB-1711", "run-1", LEASE_IN_PROGRESS_STATE,)
			.expect("first claim")
	);
	let error = store
		.try_acquire_registered_lease("pubfi-insight", "PUB-1711", "run-2", LEASE_IN_PROGRESS_STATE)
		.expect_err("second project claim must fail");
	assert!(error.to_string().contains("TrackerIssueAlreadyActive"));
	assert!(
		store.list_leases("pubfi-insight").expect("leases").is_empty(),
		"rejected project must not persist a lease projection"
	);
}

#[test]
fn manages_issue_leases() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
		.expect("lease should be inserted");

	let lease = store
		.lease_for_issue("PUB-101")
		.expect("lease read should succeed")
		.expect("lease should exist");

	assert_eq!(lease.issue_id(), "PUB-101");
	assert_eq!(lease.run_id(), "run-1");
	assert_eq!(lease.project_id(), "pubfi");
	assert_eq!(lease.issue_state(), LEASE_IN_PROGRESS_STATE);

	store.clear_lease("PUB-101").expect("lease should be deleted");

	assert!(store.lease_for_issue("PUB-101").expect("lease lookup should succeed").is_none());
}

#[test]
fn tracks_issue_specific_leases_without_project_limit() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	assert!(
		store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
			.expect("first lease acquisition should succeed")
	);
	assert!(
		store
			.try_acquire_lease("pubfi", "PUB-102", "run-2", LEASE_IN_PROGRESS_STATE)
			.expect("second lease acquisition should succeed for another issue")
	);
	assert!(
		!store
			.try_acquire_lease("pubfi", "PUB-101", "run-3", LEASE_IN_PROGRESS_STATE)
			.expect("duplicate issue acquisition should be rejected")
	);
	assert!(
		store
			.try_acquire_lease("other", "PUB-201", "run-4", LEASE_IN_PROGRESS_STATE)
			.expect("other project should still acquire its own slot")
	);
}

#[test]
fn cleared_shared_lease_removes_lock_anchor_files() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let issue_claim_path = temp_dir.path().join(".decodex-issue-claim.PUB-101.lock");
	let dispatch_slot_path = temp_dir.path().join(".decodex-dispatch-slot.0.lock");
	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("store should configure dispatch slot root");

	assert!(
		store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", LEASE_IN_PROGRESS_STATE)
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
