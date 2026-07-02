use super::*;
use crate::orchestrator::tests::recovery_terminal_support;

#[test]
fn operator_status_snapshot_surfaces_repeated_continuation_recovery_lineage() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue =
		sample_issue("In Progress", &[tracker::automation_active_label(TEST_SERVICE_ID).as_str()]);
	let worktree_path = config.worktree_root().join("PUB-101");

	for (run_id, attempt_number) in [("run-1", 1), ("run-2", 2)] {
		state_store
			.append_private_execution_event(
				TEST_SERVICE_ID,
				&issue.id,
				run_id,
				attempt_number,
				PHASE_GOAL_RECOVERY_EVENT_TYPE,
				serde_json::json!({
					"schema": "decodex.phase_goal_signal/1",
					"phase": "implement_to_validation_ready",
					"signal": "phase_goal_recovered",
					"payload": {
						"nextPhase": "repair_validation_failures",
						"sourceErrorClass": "app_server_preflight_timeout",
						"sourceErrorMessage": "Timed out while waiting for app-server output.",
					},
				}),
			)
			.expect("phase goal recovery event should record");
	}

	state_store
		.record_run_attempt("run-3", &issue.id, 3, "running")
		.expect("current run attempt should record");
	state_store
		.upsert_lease(TEST_SERVICE_ID, &issue.id, "run-3", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let recovery = run
		.continuation_recovery
		.as_ref()
		.expect("continuation recovery lineage should project onto current lane");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(recovery.state, "continuation_scheduled");
	assert_eq!(recovery.source_phase, "implement_to_validation_ready");
	assert_eq!(recovery.next_phase, "repair_validation_failures");
	assert_eq!(recovery.source_error_class, "app_server_preflight_timeout");
	assert_eq!(recovery.recovery_count, 2);
	assert_eq!(recovery.automatic_continuation_limit, 1);
	assert!(recovery.budget_exceeded);
	assert_eq!(run.policy_state, "continuation_recovery_churn_exceeded");
	assert!(
		run.lane_control_conditions
			.contains(&String::from("continuation_recovery_budget_exceeded"))
	);
	assert!(rendered.contains("continuation_recovery: state=continuation_scheduled"));
	assert!(rendered.contains("count=2/1 budget_exceeded=yes"));
	assert_eq!(snapshot_json["current_lanes"][0]["continuation_recovery"]["budget_exceeded"], true);
}

#[test]
fn operator_status_snapshot_surfaces_phase_acceptance_check() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue =
		sample_issue("In Progress", &[tracker::automation_active_label(TEST_SERVICE_ID).as_str()]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"run-1",
			1,
			PHASE_ACCEPTANCE_CHECK_EVENT_TYPE,
			serde_json::json!({
				"schema": "decodex.phase_acceptance_check/1",
				"phase": "implement_to_validation_ready",
				"decision": "fail",
				"reason_code": "no_effective_delta",
				"objective_coverage": { "covered": true },
				"effective_delta": {
					"present": false,
					"changed_surfaces": ["runtime"],
				},
				"non_goal_check": {
					"passed": true,
					"blocker_count": 0,
				},
				"validation_evidence": {
					"repo_gate_passed": true,
				},
				"next_action": "produce an issue-scoped effective delta before completing the phase goal again",
			}),
		)
		.expect("phase acceptance check should record");
	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("current run attempt should record");
	state_store
		.upsert_lease(TEST_SERVICE_ID, &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let acceptance =
		run.phase_acceptance.as_ref().expect("phase acceptance should project onto current lane");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(acceptance.decision, "fail");
	assert_eq!(acceptance.reason_code, "no_effective_delta");
	assert_eq!(acceptance.changed_surfaces, vec![String::from("runtime")]);
	assert!(rendered.contains("phase_acceptance: phase=implement_to_validation_ready"));
	assert!(rendered.contains("reason=no_effective_delta"));
}

#[test]
fn operator_status_snapshot_surfaces_merged_dirty_ad_hoc_worktree() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("accounts-column-format");

	git_status_success(
		config.repo_root(),
		&[
			"worktree",
			"add",
			"-b",
			"xy/accounts-column-format",
			worktree_path.to_str().expect("worktree path should be UTF-8"),
			"main",
		],
	);
	commit_worktree_change(&worktree_path, "README.md", "feature work\n", "feature work");
	git_status_success(
		config.repo_root(),
		&["merge", "--no-ff", "xy/accounts-column-format", "-m", "land feature"],
	);

	fs::write(worktree_path.join("README.md"), "dirty after land\n")
		.expect("worktree file should become dirty");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let worktree = snapshot
		.worktrees
		.iter()
		.find(|worktree| worktree.worktree_path == ".worktrees/accounts-column-format")
		.expect("ad-hoc merged dirty worktree should be surfaced");

	assert!(snapshot.warnings.contains(&String::from("merged_worktree_cleanup_pending")));
	assert!(snapshot.warnings.contains(&String::from("merged_dirty_worktree")));
	assert_eq!(worktree.branch_name, "xy/accounts-column-format");
	assert_eq!(worktree.ownership, "post_land_cleanup");
	assert!(
		worktree.ownership_reason.contains("already merged into `main`"),
		"ownership reason should explain why the worktree is no longer usable"
	);
	assert!(
		worktree.hygiene.as_ref().is_some_and(|hygiene| hygiene.dirty),
		"hygiene state should mark the local changes"
	);

	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(project.attention_count, 0);
	assert_eq!(project.cleanup_blocked_count, 1);
	assert_eq!(project.cleanup_pending_count, 0);

	let error = orchestrator::ensure_project_has_no_merged_worktree_cleanup_debt(&config)
		.expect_err("normal automation should stop while merged dirty worktrees remain");

	assert!(error.to_string().contains("Post-land worktree cleanup is pending"));
}

#[test]
fn operator_status_snapshot_explains_unavailable_worktree_hygiene() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	fs::remove_dir_all(config.repo_root().join(".git"))
		.expect("repo metadata should be removable for the fixture");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should degrade instead of failing");
	let detail = snapshot
		.warning_details
		.iter()
		.find(|detail| detail.warning == "worktree_hygiene_unavailable")
		.expect("hygiene warning should include operator-facing detail");

	assert!(snapshot.warnings.contains(&String::from("worktree_hygiene_unavailable")));
	assert_eq!(detail.project_id.as_deref(), Some("pubfi"));

	let repo_root = config.repo_root().display().to_string();

	assert_eq!(detail.repo_root.as_deref(), Some(repo_root.as_str()));
	assert!(detail.reason.contains("not a git repository"));
	assert!(
		detail
			.next_action
			.as_deref()
			.is_some_and(|action| action.contains("Remove the stale project registration")),
		"detail should tell the operator how to clear a stale project registration"
	);

	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("project=pubfi"));
	assert!(rendered.contains("repo_root="));
	assert!(rendered.contains("Remove the stale project registration"));
}

#[test]
fn operator_status_snapshot_updates_owned_merged_worktree_hygiene_without_global_warning() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Done", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	git_status_success(
		config.repo_root(),
		&[
			"worktree",
			"add",
			"-b",
			"xy/pub-101-cleanup",
			worktree_path.to_str().expect("worktree path should be UTF-8"),
			"main",
		],
	);
	commit_worktree_change(&worktree_path, "README.md", "feature work\n", "feature work");
	git_status_success(
		config.repo_root(),
		&["merge", "--no-ff", "xy/pub-101-cleanup", "-m", "land feature"],
	);

	fs::write(worktree_path.join("README.md"), "dirty after land\n")
		.expect("worktree file should become dirty");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"xy/pub-101-cleanup",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let worktree = snapshot
		.worktrees
		.iter()
		.find(|worktree| worktree.worktree_path == ".worktrees/PUB-101")
		.expect("owned merged worktree should still be visible");

	assert!(!snapshot.warnings.contains(&String::from("merged_worktree_cleanup_pending")));
	assert!(!snapshot.warnings.contains(&String::from("merged_dirty_worktree")));
	assert!(
		worktree.hygiene.as_ref().is_some_and(|hygiene| hygiene.dirty),
		"hygiene should still surface on the owned worktree row"
	);

	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(project.attention_count, 0);
	assert_eq!(project.cleanup_blocked_count, 1);
	assert_eq!(project.cleanup_pending_count, 0);
}

#[test]
fn live_operator_status_snapshot_hydrates_current_lane_issue_display_metadata() {
	let (temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "xy-392-attempt-1-1777551056";
	let channel_path = temp_dir.path().join("control.channel");
	let mut issue = sample_issue_with_sort_fields(
		"issue-active",
		"XY-392",
		"In Progress",
		&[],
		Some(3),
		"2026-04-30T03:01:00Z",
	);

	issue.title = String::from("Hydrate issue display metadata on run rows");

	git_status_success(
		config.repo_root(),
		&["remote", "add", "origin", "git@github.com:hack-ink/pubfi-mono-v2.git"],
	);

	let tracker = FakeTracker::new(vec![issue.clone()]);

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("current lane should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "In Progress")
		.expect("run lease should record");
	state_store.update_run_thread(run_id, "thread-1").expect("thread should record");
	state_store.update_run_turn(run_id, "turn-1").expect("turn should record");

	std::fs::write(&channel_path, "ready\n").expect("control channel should write");

	state_store
		.publish_run_control_channel_for_active_attempt(run_id, 1, &channel_path, "local_file")
		.expect("control channel should publish");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let current_lane = snapshot.current_lanes.first().expect("current lane should exist");
	let recent_run = snapshot.recent_runs.first().expect("recent run should exist");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(current_lane.project_id, config.service_id());
	assert_eq!(current_lane.project_display_name, "hack-ink/pubfi-mono-v2");
	assert_eq!(current_lane.issue_identifier.as_deref(), Some("XY-392"));
	assert_eq!(current_lane.title.as_deref(), Some("Hydrate issue display metadata on run rows"));
	assert_eq!(current_lane.author.as_deref(), Some("Yvette"));

	let expected_private_evidence_command = format!(
		"decodex evidence --config {} XY-392 --run-id {run_id} --attempt 1 --json",
		config.config_path().display()
	);

	assert_eq!(current_lane.private_evidence.read_command, expected_private_evidence_command);
	assert_eq!(recent_run.issue_identifier.as_deref(), Some("XY-392"));
	assert_eq!(recent_run.title.as_deref(), Some("Hydrate issue display metadata on run rows"));
	assert_eq!(recent_run.author.as_deref(), Some("Yvette"));
	assert_eq!(snapshot_json["current_lanes"][0]["project_id"], "pubfi");
	assert_eq!(snapshot_json["current_lanes"][0]["project_display_name"], "hack-ink/pubfi-mono-v2");
	assert_eq!(snapshot_json["current_lanes"][0]["issue_identifier"], "XY-392");
	assert_eq!(
		snapshot_json["current_lanes"][0]["title"],
		"Hydrate issue display metadata on run rows"
	);
	assert_eq!(snapshot_json["current_lanes"][0]["author"], "Yvette");
	assert_eq!(
		snapshot_json["current_lanes"][0]["private_evidence"]["read_command"],
		expected_private_evidence_command
	);
	assert_eq!(snapshot_json["current_lanes"][0]["control_capability"]["status"], "active");
	assert_eq!(snapshot_json["current_lanes"][0]["control_capability"]["thread_id"], "thread-1");
	assert_eq!(snapshot_json["current_lanes"][0]["control_capability"]["turn_id"], "turn-1");
}

#[test]
fn idle_operator_status_snapshot_has_no_runtime_or_recovery_noise() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("idle snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(snapshot.project_id, "pubfi");
	assert_eq!(snapshot.run_limit, 10);
	assert!(snapshot.warnings.is_empty(), "idle snapshot warnings: {:?}", snapshot.warnings);
	assert!(snapshot.current_lanes.is_empty(), "idle snapshot should have no current lanes");
	assert!(snapshot.recent_runs.is_empty(), "idle snapshot should have no run history");
	assert!(snapshot.history_lanes.is_empty(), "idle snapshot should have no run ledger lanes");
	assert!(
		snapshot.queued_candidates.is_empty(),
		"idle snapshot should have no queued candidates"
	);
	assert!(snapshot.worktrees.is_empty(), "idle snapshot should have no recovery worktrees");
	assert!(
		snapshot.post_review_lanes.is_empty(),
		"idle snapshot should have no retained post-review lanes"
	);

	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(project.retained_worktree_count, 0);
	assert_eq!(project.waiting_lane_count, 0);
	assert_eq!(project.attention_count, 0);
	assert_eq!(project.cleanup_blocked_count, 0);
	assert_eq!(project.cleanup_pending_count, 0);
	assert_eq!(project.connector_state, "ok");
	assert_eq!(project.last_activity_at, None);

	for field in [
		"warnings",
		"warning_details",
		"current_lanes",
		"recent_runs",
		"history_lanes",
		"queued_candidates",
		"worktrees",
		"post_review_lanes",
	] {
		assert_eq!(
			snapshot_json[field],
			serde_json::json!([]),
			"idle operator snapshot field {field} should serialize as an empty array",
		);
	}

	assert!(rendered.contains("Warnings: 0"));
	assert!(rendered.contains("Running lanes: 0"));
	assert!(rendered.contains("Run ledger shown: 0 issue lanes from 0 history attempts"));
	assert!(rendered.contains("Backlog: 0"));
	assert!(rendered.contains("Claimed queue echoes: 0"));
	assert!(rendered.contains("Stale closed queue labels: 0"));
	assert!(rendered.contains("Recovery worktrees: 0"));
	assert!(rendered.contains("Post-review lanes: 0"));
	assert!(rendered.contains("\nCurrent Lanes\n- none\n"));
	assert!(rendered.contains("\nRun Ledger\n- none\n"));
	assert!(rendered.contains("\nBacklog\n- none\n"));
	assert!(rendered.contains("\nClaimed Queue Echoes\n- none\n"));
	assert!(rendered.contains("\nStale Closed Queue Labels\n- none\n"));
	assert!(rendered.contains("\nRecovery Worktrees\n- none\n"));
	assert!(rendered.contains("\nPost-Review Lanes\n- none\n"));
	assert!(!rendered.contains("Warning details:"));
	assert!(!rendered.contains("run_id:"));
	assert!(!rendered.contains("run_lease: true"));
	assert!(!rendered.contains("role: post_review_lane"));
	assert!(!rendered.contains("role: cleanup_only"));
}

#[test]
fn idle_operator_status_snapshot_includes_configured_codex_accounts() {
	let (temp_dir, base_config, _workflow) = temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let accounts_path = temp_dir.path().join(".codex/decodex/accounts.jsonl");
	let usage_endpoint = start_codex_usage_fixture_server(vec![
		(
			"acct_default",
			r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":7,"limit_window_seconds":18000,"reset_at":1800018000},"secondary_window":{"used_percent":11,"limit_window_seconds":604800,"reset_at":1800604800}},"credits":{"has_credits":true,"unlimited":false,"balance":"12.34"}}"#,
		),
		(
			"acct_copy",
			r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":22,"limit_window_seconds":18000,"reset_at":1800019000},"secondary_window":{"used_percent":33,"limit_window_seconds":604800,"reset_at":1800605800}},"credits":{"has_credits":false,"unlimited":false,"balance":"0"}}"#,
		),
	]);

	std::fs::create_dir_all(accounts_path.parent().expect("accounts path should have parent"))
		.expect("accounts dir should exist");
	std::fs::write(
		&accounts_path,
		r#"{"email":"default@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-default","refresh_token":"refresh-default","account_id":"acct_default"}}
{"email":"copy@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-copy","refresh_token":"refresh-copy","account_id":"acct_copy"}}
"#,
	)
	.expect("accounts fixture should write");

	let mut config_toml = service_config_toml_for_config(
		&base_config,
		base_config.github().token_env_var(),
		base_config.codex().review_level(),
	);

	config_toml.push_str(&format!("\n[codex.accounts]\nusage_endpoint = \"{}\"\n", usage_endpoint));

	write_service_config(base_config.repo_root(), &config_toml);

	let config = load_service_config(base_config.repo_root());
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let accounts =
		snapshot_json["accounts"].as_array().expect("snapshot should expose configured accounts");

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(snapshot_json["account_control"]["mode"], "balanced");
	assert!(snapshot_json["account_control"]["account_selector"].is_null());
	assert_eq!(accounts.len(), 2);
	assert_eq!(accounts[0]["email"], "default@example.com");
	assert_eq!(accounts[0]["status"], "available");
	assert_eq!(accounts[0]["refresh_status"], "not_needed");
	assert_eq!(accounts[0]["plan_type"], "pro");
	assert_eq!(accounts[0]["primary_remaining_percent"], 93);
	assert_eq!(accounts[0]["credits_balance"], "12.34");
	assert_eq!(accounts[1]["email"], "copy@example.com");
	assert_eq!(accounts[1]["status"], "available");
	assert_eq!(accounts[1]["refresh_status"], "not_needed");
	assert_eq!(accounts[1]["plan_type"], "plus");
	assert_eq!(accounts[1]["primary_remaining_percent"], 78);
	assert_eq!(accounts[1]["credits_balance"], "0");
}

#[test]
fn status_command_snapshot_does_not_probe_configured_codex_accounts() {
	let (temp_dir, base_config, workflow) = temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let accounts_path = temp_dir.path().join(".codex/decodex/accounts.jsonl");

	std::fs::create_dir_all(accounts_path.parent().expect("accounts path should have parent"))
		.expect("accounts dir should exist");
	std::fs::write(
		&accounts_path,
		r#"{"email":"default@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-default","refresh_token":"refresh-default","account_id":"acct_default"}}
"#,
	)
	.expect("accounts fixture should write");

	let mut config_toml = service_config_toml_for_config(
		&base_config,
		base_config.github().token_env_var(),
		base_config.codex().review_level(),
	);

	config_toml
		.push_str("\n[codex.accounts]\nusage_endpoint = \"http://127.0.0.1:9/wham/usage\"\n");

	write_service_config(base_config.repo_root(), &config_toml);

	let config = load_service_config(base_config.repo_root());
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let snapshot = orchestrator::build_status_command_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status command snapshot should build without probing account usage");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let accounts =
		snapshot_json["accounts"].as_array().expect("snapshot should expose configured accounts");

	assert_eq!(accounts.len(), 1);
	assert_eq!(accounts[0]["email"], "default@example.com");
	assert_eq!(accounts[0]["status"], "available");
	assert_eq!(accounts[0]["refresh_status"], "not_checked");
	assert!(accounts[0]["primary_remaining_percent"].is_null());
	assert!(!snapshot.warnings.contains(&String::from("codex_accounts_unavailable")));
}

fn start_codex_usage_fixture_server(responses: Vec<(&'static str, &'static str)>) -> String {
	let listener = TcpListener::bind("127.0.0.1:0").expect("usage fixture server should bind");
	let address = listener.local_addr().expect("usage fixture address should resolve");

	thread::spawn(move || {
		let responses_by_account = responses.into_iter().collect::<HashMap<_, _>>();
		let request_count = responses_by_account.len();

		for _ in 0..request_count {
			let (mut stream, _peer) =
				listener.accept().expect("usage fixture request should arrive");
			let mut request = [0_u8; 4_096];
			let bytes_read = stream.read(&mut request).expect("usage request should read");
			let request = String::from_utf8_lossy(&request[..bytes_read]);
			let account_id = usage_fixture_account_id(&request);
			let (status, body) = match account_id
				.and_then(|account_id| responses_by_account.get(account_id).copied())
			{
				Some(body) => ("200 OK", body),
				None => ("404 Not Found", r#"{"error":"unknown account"}"#),
			};
			let response = format!(
				"HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
				body.len(),
				body
			);

			stream.write_all(response.as_bytes()).expect("usage fixture response should write");

			let _ = stream.shutdown(Shutdown::Both);
		}
	});

	format!("http://{address}/wham/usage")
}

fn usage_fixture_account_id(request: &str) -> Option<&str> {
	request.lines().find_map(|line| {
		let (name, value) = line.split_once(':')?;

		name.eq_ignore_ascii_case("ChatGPT-Account-Id").then_some(value.trim())
	})
}

#[test]
fn operator_status_snapshot_includes_local_recovery_worktree_directories() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-199");

	fs::create_dir_all(&worktree_path).expect("worktree directory should exist");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	assert_eq!(snapshot.worktrees.len(), 1);
	assert_eq!(snapshot.worktrees[0].issue_id, "PUB-199");
	assert!(!snapshot.worktrees[0].branch_name.is_empty());
	assert_eq!(snapshot.worktrees[0].worktree_path, ".worktrees/PUB-199");
	assert_eq!(snapshot.worktrees[0].ownership, "cleanup_only");
	assert!(snapshot.worktrees[0].ownership_reason.contains("local cleanup only"));
	assert_eq!(snapshot.projects[0].retained_worktree_count, 1);
}

#[test]
fn completed_retained_worktree_without_post_review_owner_is_cleanup_only() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-cleanup",
		"PUB-199",
		"Done",
		&[],
		Some(4),
		"2026-03-13T07:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-199",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert!(snapshot.post_review_lanes.is_empty());
	assert_eq!(snapshot.projects[0].retained_worktree_count, 1);
	assert_eq!(snapshot.worktrees[0].issue_identifier.as_deref(), Some("PUB-199"));
	assert_eq!(snapshot.worktrees[0].issue_state.as_deref(), Some("Done"));
	assert_eq!(snapshot.worktrees[0].ownership, "cleanup_only");
	assert!(snapshot.worktrees[0].ownership_reason.contains("Issue is Done"));
	assert_eq!(snapshot_json["worktrees"][0]["ownership"], "cleanup_only");
	assert_eq!(snapshot_json["worktrees"][0]["issue_state"], "Done");
	assert!(rendered.contains("role: cleanup_only"));
	assert!(rendered.contains("reason: Issue is Done"));
	assert!(!rendered.contains("role: post_review_lane"));
	assert!(!rendered.contains("classification: blocked"));
	assert!(!rendered.contains("review_handoff_missing"));
}

#[test]
fn legacy_cleanup_only_worktree_requires_audited_manual_closeout() {
	let (temp_dir, config, workflow) = temp_project_layout();
	let db_path = temp_dir.path().join("legacy-runtime.sqlite3");
	let issue = sample_issue_with_sort_fields(
		"issue-cleanup",
		"PUB-199",
		"Done",
		&[],
		Some(4),
		"2026-03-13T07:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	{
		let connection = Connection::open(&db_path).expect("legacy db should open");

		connection
			.execute_batch(&format!(
				"CREATE TABLE worktrees (
					issue_id TEXT PRIMARY KEY NOT NULL,
					project_id TEXT NOT NULL,
					branch_name TEXT NOT NULL,
					worktree_path TEXT NOT NULL
				);
				INSERT INTO worktrees (issue_id, project_id, branch_name, worktree_path)
				VALUES ('{}', 'pubfi', 'x/pubfi-pub-199', '{}');",
				issue.id,
				worktree_path.display()
			))
			.expect("legacy worktree row should write");
	}

	let tracker = FakeTracker::new(vec![issue]);
	let state_store = StateStore::open(&db_path).expect("state store should migrate");
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(snapshot.worktrees[0].ownership, "cleanup_only");
	assert_eq!(snapshot.worktrees[0].provenance.source, "legacy_unknown");
	assert!(snapshot.worktrees[0].provenance.audit_required);
	assert!(
		snapshot.worktrees[0]
			.recovery_next_action
			.as_deref()
			.is_some_and(|action| action.contains("decodex recover legacy-closeout PUB-199"))
	);
	assert_eq!(snapshot_json["worktrees"][0]["provenance"]["source"], "legacy_unknown");
	assert_eq!(snapshot_json["worktrees"][0]["provenance"]["audit_required"], true);
	assert!(rendered.contains("provenance_source: legacy_unknown"));
	assert!(rendered.contains("audit_required: true"));
	assert!(rendered.contains("recovery_next_action: verify tracker/PR terminal state"));
	assert!(rendered.contains("decodex recover legacy-closeout PUB-199"));
}

#[test]
fn runtime_recovery_preserves_legacy_cleanup_only_provenance_without_recoverable_owner() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let db_path = temp_dir.path().join("runtime.sqlite3");
	let (_layout_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue_with_sort_fields(
		"issue-legacy",
		"PUB-199",
		"Done",
		&[],
		Some(4),
		"2026-03-13T07:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("legacy worktree path should exist");

	{
		let connection = Connection::open(&db_path).expect("legacy db should open");

		connection
			.execute_batch(&format!(
				"CREATE TABLE worktrees (
					issue_id TEXT PRIMARY KEY NOT NULL,
					project_id TEXT NOT NULL,
					branch_name TEXT NOT NULL,
					worktree_path TEXT NOT NULL
				);
				INSERT INTO worktrees (issue_id, project_id, branch_name, worktree_path)
				VALUES ('{}', 'pubfi', 'x/pubfi-pub-199', '{}');",
				issue.id,
				worktree_path.display()
			))
			.expect("legacy worktree row should write");
	}

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open(&db_path).expect("state store should migrate");
	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should succeed");
	let mapping = state_store
		.worktree_for_issue(&issue.id)
		.expect("mapping lookup should succeed")
		.expect("legacy mapping should remain");

	assert!(
		recovered_state.recoverable_issues.is_empty(),
		"terminal cleanup-only worktree should not become a retry lane"
	);
	assert_eq!(mapping.provenance().source(), "legacy_unknown");
	assert_eq!(mapping.provenance().created_at_unix(), None);
	assert_eq!(mapping.provenance().updated_at_unix(), None);
}

#[test]
fn runtime_recovery_records_recovered_provenance_for_fresh_active_worktree() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("active worktree path should exist");
	state::write_run_activity_marker(&worktree_path, "run-1", 1)
		.expect("activity marker should write");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("activity marker should load")
		.expect("activity marker should exist");
	let observed_at_unix =
		marker.last_activity_unix_epoch().expect("activity marker should have a stable timestamp");
	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should succeed");
	let mapping = state_store
		.worktree_for_issue(&issue.id)
		.expect("mapping lookup should succeed")
		.expect("recovered mapping should exist");
	let lease = state_store
		.lease_for_issue(&issue.id)
		.expect("lease lookup should succeed")
		.expect("fresh active marker should recover the lease");

	assert!(
		recovered_state.recoverable_issues.is_empty(),
		"fresh marker should recover as the run lease instead of a retry queue item"
	);
	assert_eq!(mapping.provenance().source(), "runtime_recovered");
	assert_eq!(mapping.provenance().created_at_unix(), Some(observed_at_unix));
	assert_eq!(mapping.provenance().updated_at_unix(), Some(observed_at_unix));
	assert_eq!(lease.run_id(), "run-1");
}

#[test]
fn runtime_recovery_splits_invalid_local_id_batch_without_losing_valid_issue() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let mut issue = recovery_terminal_support::sample_active_issue("In Progress");

	issue.id = String::from("00000000-0000-0000-0000-000000000101");

	let tracker = FakeTracker::with_refresh_error(
		vec![issue.clone()],
		"Linear GraphQL request failed: Argument Validation Error",
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("active worktree path should exist");
	state::write_run_activity_marker(&worktree_path, "run-101", 1)
		.expect("activity marker should write");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("valid worktree mapping should record");
	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("invalid local run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("invalid local lease should record");

	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should split invalid local ids from valid server ids");
	let recovered_mapping = state_store
		.worktree_for_issue(&issue.id)
		.expect("mapping lookup should succeed")
		.expect("valid issue mapping should remain");
	let lease = state_store
		.lease_for_issue(&issue.id)
		.expect("lease lookup should succeed")
		.expect("valid issue lease should recover");

	assert!(
		recovered_state.recoverable_issues.is_empty(),
		"fresh valid issue should recover as active lease rather than disappear"
	);
	assert_eq!(recovered_mapping.issue_id(), issue.id);
	assert_eq!(lease.issue_id(), issue.id);
	assert_eq!(lease.run_id(), "run-101");
}

#[test]
fn post_review_worktree_refresh_splits_invalid_local_id_batch_without_losing_valid_issue() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let mut issue = recovery_terminal_support::sample_active_issue("In Review");

	issue.id = String::from("00000000-0000-0000-0000-000000000101");

	let tracker = FakeTracker::with_refresh_error(
		vec![issue.clone()],
		"Linear GraphQL request failed: Argument Validation Error",
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let valid_worktree_path = config.worktree_root().join(&issue.identifier);
	let missing_ghost_path = config.worktree_root().join("PUB-012");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&valid_worktree_path.display().to_string(),
		)
		.expect("valid worktree mapping should record");
	state_store
		.upsert_worktree(
			"pubfi",
			"PUB-012",
			"x/pubfi-pub-012",
			&missing_ghost_path.display().to_string(),
		)
		.expect("stale local-id worktree mapping should record");

	let worktree_issues =
		orchestrator::load_post_review_worktree_issues(&tracker, &config, &state_store)
			.expect("post-review refresh should split invalid local ids from valid server ids");
	let (worktree, refreshed_issue) =
		worktree_issues.first().expect("valid post-review worktree issue should remain");

	assert_eq!(worktree_issues.len(), 1);
	assert_eq!(worktree.issue_id(), issue.id);
	assert_eq!(refreshed_issue.id, issue.id);
	assert!(
		tracker
			.refresh_queries
			.borrow()
			.iter()
			.any(|query| query == &vec![String::from("PUB-012")]),
		"stale local issue id should be retried in isolation"
	);
}

#[test]
fn operator_status_snapshot_reports_retry_backoff_from_worktree_marker() {
	for (retry_kind, expected_wait_reason) in
		[("failure", "failure_retry"), ("git_lock_contention", "git_lock_contention")]
	{
		let (_temp_dir, config, _workflow) = temp_project_layout();
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = sample_issue("Todo", &[]);
		let worktree_path = config.worktree_root().join("PUB-101");

		state_store
			.record_run_attempt("run-1", &issue.id, 1, "failed")
			.expect("run attempt should record");
		state_store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-101",
				&worktree_path.display().to_string(),
			)
			.expect("worktree should record");

		state::write_run_retry_schedule(
			&worktree_path,
			"run-1",
			1,
			retry_kind,
			OffsetDateTime::now_utc().unix_timestamp() + 60,
		)
		.expect("retry schedule marker should write");

		let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
			.expect("snapshot should build");
		let run = snapshot.recent_runs.first().expect("recent run should exist");

		assert_eq!(run.phase, "retry_backoff");
		assert_eq!(run.wait_reason.as_deref(), Some(expected_wait_reason));
		assert_eq!(run.retry_kind.as_deref(), Some(retry_kind));
		assert!(run.next_retry_at.is_some());
		assert_eq!(snapshot.projects[0].waiting_lane_count, 1);
		assert_eq!(snapshot.projects[0].connector_state, "backoff");
	}
}

#[test]
fn operator_status_snapshot_keeps_continuation_retry_from_orphaning_live_marker_worktree() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "continuation_pending")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, process::id())
		.expect("live marker should write");
	state::write_run_retry_schedule(
		&worktree_path,
		"run-1",
		1,
		"continuation",
		OffsetDateTime::now_utc().unix_timestamp() + 60,
	)
	.expect("continuation retry schedule marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.recent_runs.first().expect("recent run should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(run.status, "continuation_pending");
	assert_eq!(run.phase, "retry_backoff");
	assert_eq!(run.wait_reason.as_deref(), Some("continuation_retry"));
	assert_eq!(run.retry_kind.as_deref(), Some("continuation"));
	assert_eq!(run.ownership_state, "continuation_pending");
	assert_eq!(run.liveness_state, "process_alive");
	assert_eq!(run.lane_control_next_action, "wait_for_continuation_reentry");
	assert_eq!(snapshot.worktrees[0].ownership, "continuation_pending");
	assert_eq!(snapshot.projects[0].retained_worktree_count, 0);
	assert_eq!(snapshot.projects[0].waiting_lane_count, 1);
	assert!(rendered.contains("Recovery worktrees: 0"));
	assert!(rendered.contains("- none (owned worktrees are shown in their lane sections above)"));
	assert!(!rendered.contains("role: orphaned_live_thread"));
}

#[test]
fn operator_status_snapshot_ignores_retry_schedule_on_running_attempt() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, process::id())
		.expect("run marker should write");
	state::write_run_retry_schedule(
		&worktree_path,
		"run-1",
		1,
		"failure",
		OffsetDateTime::now_utc().unix_timestamp() + 60,
	)
	.expect("stale retry schedule marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");

	assert_eq!(run.phase, "executing");
	assert_eq!(run.wait_reason, None);
	assert_eq!(run.retry_kind, None);
	assert_eq!(run.next_retry_at, None);
	assert_eq!(snapshot.projects[0].waiting_lane_count, 0);
	assert_eq!(snapshot.projects[0].connector_state, "ok");
}

#[test]
fn operator_status_snapshot_reports_stalled_runs_explicitly() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "stalled")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.recent_runs.first().expect("recent run should exist");

	assert_eq!(run.phase, "stalled");
	assert_eq!(run.wait_reason.as_deref(), Some("app_server_idle_timeout"));
	assert_eq!(run.current_operation, state::RUN_OPERATION_IDLE);
	assert!(!run.suspected_stall);
}

#[test]
fn operator_status_snapshot_surfaces_reconciliation_operation_for_stalled_runs() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "stalled")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_operation_marker(&worktree_path, "run-1", 1, RUN_OPERATION_RECONCILIATION)
		.expect("reconciliation marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.recent_runs.first().expect("recent run should exist");

	assert_eq!(run.phase, "stalled");
	assert_eq!(run.current_operation, state::RUN_OPERATION_RECONCILIATION);
}

#[test]
fn operator_status_snapshot_preserves_stalled_run_activity_when_tagging_reconciliation() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "stalled")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, 42)
		.expect("initial activity marker should write");

	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = fs::read_to_string(&marker_path).expect("marker body should load");
	let stale_activity = OffsetDateTime::now_utc().unix_timestamp() - 600;
	let rewritten = marker_body
		.lines()
		.map(|line| {
			if line.starts_with("last_activity_unix_epoch=") {
				format!("last_activity_unix_epoch={stale_activity}")
			} else if line.starts_with("last_progress_unix_epoch=") {
				format!("last_progress_unix_epoch={stale_activity}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
		+ "\n";

	fs::write(&marker_path, rewritten).expect("marker body should rewrite");
	state::write_run_operation_marker_preserving_activity(
		&worktree_path,
		"run-1",
		1,
		RUN_OPERATION_RECONCILIATION,
	)
	.expect("reconciliation marker should preserve existing activity");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.recent_runs.first().expect("recent run should exist");

	assert_eq!(marker.process_id(), Some(42));
	assert_eq!(marker.last_activity_unix_epoch(), Some(stale_activity));
	assert_eq!(run.current_operation, state::RUN_OPERATION_RECONCILIATION);
	assert_eq!(run.process_id, Some(42));
}

#[test]
fn operator_status_snapshot_marks_soft_stalls_before_hard_timeout() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_operation_marker(&worktree_path, "run-1", 1, RUN_OPERATION_AGENT_RUN)
		.expect("operation marker should write");

	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = fs::read_to_string(&marker_path).expect("marker body should load");
	let suspected_age = (RUN_LEASE_IDLE_TIMEOUT.as_secs() / 2).saturating_add(1) as i64;
	let stale_progress = OffsetDateTime::now_utc().unix_timestamp() - suspected_age;
	let rewritten = marker_body
		.lines()
		.map(|line| {
			if line.starts_with("last_progress_unix_epoch=") {
				format!("last_progress_unix_epoch={stale_progress}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
		+ "\n";

	fs::write(&marker_path, rewritten).expect("marker body should rewrite");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");

	assert_eq!(run.current_operation, state::RUN_OPERATION_AGENT_RUN);
	assert!(run.last_progress_at.is_some());
	assert!(run.suspected_stall);
}

#[test]
fn operator_status_snapshot_diagnoses_protocol_only_model_execution() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_operation_marker(&worktree_path, "run-1", 1, RUN_OPERATION_AGENT_RUN)
		.expect("operation marker should write");

	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = fs::read_to_string(&marker_path).expect("marker body should load");
	let suspected_age = (MODEL_EXECUTION_IDLE_TIMEOUT.as_secs() / 2).saturating_add(1) as i64;
	let stale_progress = OffsetDateTime::now_utc().unix_timestamp() - suspected_age;
	let rewritten = marker_body
		.lines()
		.map(|line| {
			if line.starts_with("last_progress_unix_epoch=") {
				format!("last_progress_unix_epoch={stale_progress}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
		+ "\n";

	fs::write(&marker_path, rewritten).expect("marker body should rewrite");

	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model_execution")),
		recent_events: vec![
			state::ProtocolActivityEventSummary {
				event_type: String::from("thread/status/changed"),
				category: String::from("thread"),
				detail: Some(String::from("active")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("thread/goal/updated"),
				category: String::from("protocol"),
				detail: Some(String::from("active")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("account/rateLimits/updated"),
				category: String::from("rate_limit"),
				detail: Some(String::from("pro")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("account/rateLimits/updated"),
				category: String::from("rate_limit"),
				detail: Some(String::from("pro")),
			},
		],
		..ProtocolActivitySummary::default()
	};

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 2,
			last_event_type: "account/rateLimits/updated",
			child_agent_activity: None,
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol-only marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(run.wait_reason.as_deref(), Some("model_execution"));
	assert_eq!(run.progress_diagnostic.as_deref(), Some("protocol_only_activity"));
	assert_eq!(run.execution_liveness, "process_alive");
	assert!(run.suspected_stall);
	assert_ne!(run.last_progress_at, run.last_protocol_activity_at);
	assert!(rendered.contains("progress_diagnostic: protocol_only_activity"));
}

#[test]
fn operator_status_snapshot_prioritizes_repo_gate_progress_diagnostic() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);

	seed_running_repo_gate_status_lane(&state_store, &config, &issue);

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(
		run.progress_diagnostic.as_deref(),
		Some("repo_gate_failure:repo_gate_canonicalize_failed; failed_command:cargo make lint-fix")
	);
	assert!(
		rendered.contains(
			"progress_diagnostic: repo_gate_failure:repo_gate_canonicalize_failed; failed_command:cargo make lint-fix"
		)
	);
}

#[test]
fn operator_status_snapshot_clears_repo_gate_progress_after_later_transition() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);

	seed_running_repo_gate_status_lane(&state_store, &config, &issue);

	state_store
		.append_private_execution_event(
			"pubfi",
			&issue.id,
			"run-1",
			1,
			"phase_goal_transition",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "repair_validation_failures",
				"signal": "validation_pass",
				"payload": {
					"nextPhase": "handoff_evidence"
				}
			}),
		)
		.expect("validation pass event should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(run.progress_diagnostic, None);
	assert!(!rendered.contains("repo_gate_failure:repo_gate_canonicalize_failed"));
}

fn seed_running_repo_gate_status_lane(
	state_store: &StateStore,
	config: &ServiceConfig,
	issue: &TrackerIssue,
) {
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.append_private_execution_event(
			"pubfi",
			&issue.id,
			"run-1",
			1,
			"phase_goal_transition",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
				"signal": "validation_fail",
				"payload": {
					"errorClass": "repo_gate_canonicalize_failed",
					"disposition": "continue_repair",
					"repoGateFailure": {
						"schema": "decodex.repo_gate_failure_diagnostic/1",
						"stage": "canonicalize",
						"failed_command": "cargo make lint-fix",
						"exit_status": 101,
						"summary": "repo gate canonicalize command failed",
						"problem_lines": ["error: function has too many lines"],
						"output_excerpt": "error: function has too many lines",
						"output_truncated": false
					}
				}
			}),
		)
		.expect("repo gate failure event should record");

	state::write_run_operation_marker(&worktree_path, "run-1", 1, RUN_OPERATION_AGENT_RUN)
		.expect("operation marker should write");
}
