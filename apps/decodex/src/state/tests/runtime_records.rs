mod contracts_autonomy;
mod lane_runs;
mod persistence;
mod programs_registry;

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
fn read_only_project_run_listing_does_not_persist_marker_identities() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let worktree_path = temp_dir.path().join("worktrees/PUB-101");
	let store = StateStore::open(&state_path).expect("state store should open");

	fs::create_dir_all(&worktree_path).expect("worktree should exist");

	store.record_run_attempt("run-1", "PUB-101", 1, "running").expect("run attempt should persist");
	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should persist");
	store
		.upsert_worktree(
			"pubfi",
			"PUB-101",
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should persist");

	state::write_run_thread_marker(&worktree_path, "run-1", 1, "thread-marker")
		.expect("thread marker should write");
	state::write_run_turn_marker(&worktree_path, "run-1", 1, "turn-marker")
		.expect("turn marker should write");

	let (leased_runs, _) =
		store.list_project_runs_read_only("pubfi", 0).expect("read-only runs should load");

	assert_eq!(leased_runs.len(), 1);
	assert_eq!(leased_runs[0].thread_id(), None);
	assert_eq!(leased_runs[0].turn_id(), None);

	assert_sqlite_run_attempt_identity(&state_path, None, None);

	store.list_project_runs("pubfi", 0).expect("ordinary runs should load");

	assert_sqlite_run_attempt_identity(&state_path, Some("thread-marker"), Some("turn-marker"));
}

fn assert_sqlite_run_attempt_identity(
	state_path: &Path,
	expected_thread_id: Option<&str>,
	expected_turn_id: Option<&str>,
) {
	let connection = Connection::open(state_path).expect("sqlite should open");
	let (thread_id, turn_id): (Option<String>, Option<String>) = connection
		.query_row(
			"SELECT thread_id, turn_id FROM run_attempts WHERE run_id = 'run-1'",
			[],
			|row| Ok((row.get(0)?, row.get(1)?)),
		)
		.expect("run attempt row should exist");

	assert_eq!(thread_id.as_deref(), expected_thread_id);
	assert_eq!(turn_id.as_deref(), expected_turn_id);
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
			review_level: "standard",
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
			review_level: "standard",
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
fn autonomy_objective_draft_accept_current_history_and_supersession_persist() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let draft_v1 = store
		.upsert_autonomy_objective_draft("decodex", autonomy_objective_fixture(1))
		.expect("objective draft v1 should persist");

	assert_eq!(draft_v1.project_id(), "decodex");
	assert_eq!(draft_v1.objective_id(), "quality-autonomy");
	assert_eq!(draft_v1.version(), 1);
	assert_eq!(draft_v1.state(), AutonomyObjectiveState::Draft);
	assert_eq!(
		store
			.autonomy_objective("decodex", "quality-autonomy", 1)
			.expect("draft objective should read")
			.expect("draft objective should exist")
			.state(),
		AutonomyObjectiveState::Draft
	);

	let accepted_v1 = store
		.accept_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			1,
			sample_objective_acceptance(),
		)
		.expect("objective v1 should accept");

	assert_eq!(accepted_v1.state(), AutonomyObjectiveState::Accepted);
	assert_eq!(
		accepted_v1.objective().acceptance().expect("acceptance should be retained").accepted_by(),
		"operator"
	);
	assert_eq!(
		store
			.autonomy_objective("decodex", "quality-autonomy", 1)
			.expect("accepted objective should read")
			.expect("accepted objective should exist")
			.state(),
		AutonomyObjectiveState::Accepted
	);
	assert!(
		store.upsert_autonomy_objective_draft("decodex", autonomy_objective_fixture(1)).is_err(),
		"accepted objective versions must not be overwritten as drafts"
	);

	store
		.upsert_autonomy_objective_draft("decodex", autonomy_objective_fixture(2))
		.expect("objective draft v2 should persist");

	let accepted_v2 = store
		.accept_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			2,
			sample_objective_acceptance(),
		)
		.expect("objective v2 should accept and supersede v1");

	assert_eq!(accepted_v2.version(), 2);
	assert_eq!(accepted_v2.state(), AutonomyObjectiveState::Accepted);

	let current = store
		.current_accepted_autonomy_objective("decodex", "quality-autonomy")
		.expect("current accepted objective should read")
		.expect("current accepted objective should exist");

	assert_eq!(current.version(), 2);

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let history = reopened
		.list_autonomy_objective_history("decodex", "quality-autonomy")
		.expect("objective history should list");

	assert_eq!(history.len(), 2);
	assert_eq!(history[0].version(), 1);
	assert_eq!(history[0].state(), AutonomyObjectiveState::Superseded);
	assert_eq!(
		history[0]
			.objective()
			.supersession()
			.expect("supersession should be retained")
			.superseded_by_version(),
		2
	);
	assert_eq!(
		history[0].objective().summary(),
		"Improve Decodex autonomy quality version 1.",
		"superseding an accepted version must preserve its objective body"
	);
	assert_eq!(history[1].version(), 2);
	assert_eq!(history[1].state(), AutonomyObjectiveState::Accepted);
}

#[test]
fn autonomy_objective_rejection_and_explicit_supersession_keep_provenance() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_autonomy_objective_draft("decodex", autonomy_objective_fixture(1))
		.expect("objective draft v1 should persist");

	let rejected = store
		.reject_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			1,
			AutonomyObjectiveRejection::new(
				"operator",
				"2026-06-22T10:05:00Z",
				"conversation",
				"Objective version needs narrower surfaces.",
			)
			.expect("rejection should validate"),
		)
		.expect("objective draft should reject");

	assert_eq!(rejected.state(), AutonomyObjectiveState::Rejected);
	assert_eq!(
		rejected.objective().rejection().expect("rejection should exist").reason(),
		"Objective version needs narrower surfaces."
	);
	assert_eq!(
		store
			.autonomy_objective("decodex", "quality-autonomy", 1)
			.expect("rejected objective should read")
			.expect("rejected objective should exist")
			.state(),
		AutonomyObjectiveState::Rejected
	);
	assert!(
		store
			.accept_autonomy_objective_version(
				"decodex",
				"quality-autonomy",
				1,
				sample_objective_acceptance()
			)
			.is_err(),
		"rejected objective versions cannot later become accepted authority"
	);

	store
		.upsert_autonomy_objective_draft("decodex", autonomy_objective_fixture(2))
		.expect("objective draft v2 should persist");

	let superseded = store
		.supersede_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			2,
			AutonomyObjectiveSupersession::new(
				"quality-autonomy",
				3,
				"operator",
				"2026-06-22T10:10:00Z",
				"conversation",
				"Draft was replaced before acceptance.",
			)
			.expect("supersession should validate"),
		)
		.expect("objective draft should supersede");

	assert_eq!(superseded.state(), AutonomyObjectiveState::Superseded);
	assert_eq!(
		superseded
			.objective()
			.supersession()
			.expect("supersession should exist")
			.superseded_by_version(),
		3
	);
	assert_eq!(
		store
			.autonomy_objective("decodex", "quality-autonomy", 2)
			.expect("superseded objective should read")
			.expect("superseded objective should exist")
			.state(),
		AutonomyObjectiveState::Superseded
	);
	assert_eq!(
		store
			.upsert_autonomy_objective_draft("decodex", autonomy_objective_fixture(3))
			.expect("objective draft v3 should persist")
			.state(),
		AutonomyObjectiveState::Draft
	);
	assert!(
		store
			.supersede_autonomy_objective_version(
				"decodex",
				"quality-autonomy",
				3,
				AutonomyObjectiveSupersession::new(
					"quality-autonomy",
					3,
					"operator",
					"2026-06-22T10:11:00Z",
					"conversation",
					"Invalid self-supersession.",
				)
				.expect("self-supersession payload should build"),
			)
			.is_err(),
		"same objective version cannot supersede itself"
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
fn decision_contract_snapshot_load_quarantines_invalid_issue_dependency_rows() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_decision_contract("decodex", Some("XY-852"), latent_decision_contract_fixture())
		.expect("valid decision contract should persist");

	let mut invalid_payload = serde_json::to_value(latent_decision_contract_fixture())
		.expect("fixture should encode as JSON");

	invalid_payload["contract_id"] = serde_json::json!("invalid-dependency-contract");
	invalid_payload["execution_readiness"]["proposed_issues"][0]["dependencies"] =
		serde_json::json!(["XY-952"]);

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute(
			"INSERT INTO decision_contracts (
					project_id, contract_id, source_issue_id, status, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			rusqlite::params![
				"decodex",
				"invalid-dependency-contract",
				"XY-BROKEN",
				"draft_latent",
				serde_json::to_string(&invalid_payload)
					.expect("invalid dependency payload should serialize"),
				"2026-07-01T00:00:00Z",
				1_i64,
				"2026-07-01T00:00:00Z",
				1_i64,
			],
		)
		.expect("invalid dependency row should insert");

	let reopened =
		StateStore::open(&state_path).expect("invalid dependency contract should be quarantined");
	let valid_contract = reopened
		.decision_contract("decodex", "research-x-loop-contract")
		.expect("valid contract should remain readable")
		.expect("valid contract should exist");

	assert_eq!(valid_contract.contract_id(), "research-x-loop-contract");

	let project_contracts = reopened
		.list_decision_contracts_for_project("decodex")
		.expect("project contract list should skip invalid rows");

	assert_eq!(project_contracts.len(), 1);
	assert_eq!(project_contracts[0].contract_id(), "research-x-loop-contract");
	assert!(
		reopened
			.list_decision_contracts_for_issue("decodex", "XY-BROKEN")
			.expect("issue contract list should skip invalid rows")
			.is_empty()
	);
	assert!(
		reopened.decision_contract("decodex", "invalid-dependency-contract").is_err(),
		"direct reads of the invalid contract should still fail validation"
	);
}

#[test]
fn autonomy_proposal_snapshot_load_quarantines_fingerprint_mismatch_rows() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let _store = StateStore::open(&state_path).expect("state store should open");
	let proposal = autonomy_proposal_fixture();
	let mut invalid_payload =
		serde_json::to_value(&proposal).expect("proposal should encode as JSON");

	invalid_payload["affected_identifiers"] = serde_json::json!(["OperatorLoopStatus"]);

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute(
			"INSERT INTO autonomy_proposals (
					project_id, proposal_id, objective_id, objective_version, state, fingerprint,
					source_family, intended_surface, payload_json, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
			rusqlite::params![
				"decodex",
				proposal.id(),
				proposal.objective_id(),
				1_i64,
				proposal.state().as_str(),
				proposal.fingerprint(),
				proposal.source_family(),
				proposal.intended_surface(),
				serde_json::to_string(&invalid_payload)
					.expect("invalid proposal payload should serialize"),
				"2026-07-01T00:00:00Z",
				1_i64,
				"2026-07-01T00:00:00Z",
				1_i64,
			],
		)
		.expect("invalid proposal row should insert");

	let reopened =
		StateStore::open(&state_path).expect("invalid proposal should be quarantined on open");

	assert!(
		reopened
			.recent_autonomy_proposals_for_project("decodex", 10)
			.expect("recent proposal list should skip invalid rows")
			.is_empty()
	);
	assert!(
		reopened.autonomy_proposal("decodex", proposal.id()).is_err(),
		"direct reads of the invalid proposal should still fail validation"
	);
}

#[test]
fn decision_contract_reload_migrates_removed_flat_issue_summary_rows() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");

	store
		.upsert_decision_contract("decodex", Some("XY-852"), latent_decision_contract_fixture())
		.expect("current decision contract should persist");

	let mut removed_field_payload = serde_json::to_value(latent_decision_contract_fixture())
		.expect("fixture should encode as JSON");

	removed_field_payload["contract_id"] = serde_json::json!("removed-flat-issue-contract");

	let readiness = removed_field_payload
		.get_mut("execution_readiness")
		.expect("readiness should exist")
		.as_object_mut()
		.expect("readiness should be an object");

	readiness.remove("proposed_issues");
	readiness.insert(
		String::from("proposed_issue_summaries"),
		serde_json::json!(["Flat summary that must be migrated."]),
	);
	readiness.insert(
		String::from("queue_intent"),
		serde_json::json!(["Removed queue intent that must not be re-admitted."]),
	);

	let connection = Connection::open(&state_path).expect("sqlite should open");

	connection
		.execute(
			"INSERT INTO decision_contracts (
					project_id, contract_id, source_issue_id, status, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			rusqlite::params![
				"decodex",
				"removed-flat-issue-contract",
				"XY-OLD",
				"draft_latent",
				serde_json::to_string(&removed_field_payload)
					.expect("removed-field payload should serialize"),
				"2026-06-17T00:00:00Z",
				1_i64,
				"2026-06-17T00:00:00Z",
				1_i64,
			],
		)
		.expect("removed-field decision contract row should insert");
	connection
		.execute("UPDATE schema_meta SET value = '11' WHERE key = 'schema_version'", [])
		.expect("schema version should mark removed-field state");

	let reopened =
		StateStore::open(&state_path).expect("removed flat issue summary row should migrate");
	let project_contracts = reopened
		.list_decision_contracts_for_project("decodex")
		.expect("project contracts should list");
	let contract_ids =
		project_contracts.iter().map(|record| record.contract_id()).collect::<Vec<_>>();

	assert_eq!(project_contracts.len(), 2);
	assert!(contract_ids.contains(&"research-x-loop-contract"));
	assert!(contract_ids.contains(&"removed-flat-issue-contract"));

	let migrated_contract = reopened
		.decision_contract("decodex", "removed-flat-issue-contract")
		.expect("migrated contract read should succeed")
		.expect("migrated contract should exist");

	assert_eq!(migrated_contract.source_issue_id(), Some("XY-OLD"));
	assert_eq!(migrated_contract.contract().execution_readiness().proposed_issues().len(), 1);
	assert_eq!(
		migrated_contract.contract().execution_readiness().proposed_issues()[0].objective(),
		"Flat summary that must be migrated."
	);

	let migrated_payload: String = connection
		.query_row(
			"SELECT payload_json FROM decision_contracts WHERE contract_id = 'removed-flat-issue-contract'",
			[],
			|row| row.get(0),
		)
		.expect("migrated payload should read");
	let migrated_value: Value =
		serde_json::from_str(&migrated_payload).expect("migrated payload should parse");

	assert!(
		migrated_value.pointer("/execution_readiness/proposed_issue_summaries").is_none(),
		"removed field should be absent after migration"
	);
	assert!(
		migrated_value.pointer("/execution_readiness/queue_intent").is_none(),
		"removed queue intent should be absent after migration"
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
