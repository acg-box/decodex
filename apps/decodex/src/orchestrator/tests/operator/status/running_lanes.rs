#[test]
fn failure_comments_use_repo_relative_worktree_paths() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let worktree = WorktreeSpec {
		branch_name: String::from("x/pubfi-pub-101"),
		issue_identifier: String::from("PUB-101"),
		path: config.repo_root().join(".worktrees/PUB-101"),
		reused_existing: true,
	};

	assert_eq!(orchestrator::relative_worktree_path(&config, &worktree), ".worktrees/PUB-101");
}

#[test]
fn operator_status_snapshot_includes_active_runs_and_repo_relative_paths() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store.update_run_thread("run-1", "thread-1").expect("thread id should attach");
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
		.append_event("run-1", 1, "turn/completed", "{\"turn\":\"1\"}")
		.expect("event should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	assert_eq!(snapshot.project_id, "pubfi");
	assert_eq!(snapshot.active_runs.len(), 1);
	assert_eq!(snapshot.recent_runs.len(), 1);
	assert_eq!(snapshot.active_runs[0].project_id, "pubfi");
	assert_eq!(snapshot.active_runs[0].run_id, "run-1");
	assert_eq!(snapshot.active_runs[0].phase, "executing");
	assert_eq!(snapshot.active_runs[0].current_operation, state::RUN_OPERATION_AGENT_RUN);
	assert_eq!(snapshot.active_runs[0].thread_id.as_deref(), Some("thread-1"));
	assert_eq!(snapshot.active_runs[0].branch_name.as_deref(), Some("x/pubfi-pub-101"));
	assert_eq!(snapshot.active_runs[0].worktree_path.as_deref(), Some(".worktrees/PUB-101"));
	assert!(snapshot.active_runs[0].last_run_activity_at.is_some());
	assert!(snapshot.active_runs[0].last_progress_at.is_some());
	assert!(!snapshot.active_runs[0].suspected_stall);
	assert_eq!(snapshot.active_runs[0].last_event_type.as_deref(), Some("turn/completed"));
	assert_eq!(snapshot.worktrees[0].worktree_path, ".worktrees/PUB-101");
	assert_eq!(snapshot.worktrees[0].ownership, "active_lane");
	assert!(snapshot.worktrees[0].ownership_reason.contains("Active lane `run-1`"));

	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(project.active_run_count, 1);
	assert_eq!(
		project.retained_worktree_count, 0,
		"active running lane worktrees must not inflate project recovery counts"
	);
	assert_eq!(project.connector_state, "ok");
	assert!(project.last_activity_at.is_some());
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
		worktree
			.ownership_reason
			.contains("already merged into `main`"),
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
fn live_operator_status_snapshot_hydrates_active_run_issue_display_metadata() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "xy-392-attempt-1-1777551056";
	let mut issue = sample_issue_with_sort_fields(
		"issue-active",
		"XY-392",
		"In Progress",
		&[],
		Some(3),
		"2026-04-30T03:01:00Z",
	);

	issue.title = String::from("Hydrate issue display metadata on run rows");

	let tracker = FakeTracker::new(vec![issue.clone()]);

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("active run should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "In Progress")
		.expect("active lease should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let active_run = snapshot.active_runs.first().expect("active run should exist");
	let recent_run = snapshot.recent_runs.first().expect("recent run should exist");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(active_run.project_id, config.service_id());
	assert_eq!(active_run.issue_identifier.as_deref(), Some("XY-392"));
	assert_eq!(active_run.title.as_deref(), Some("Hydrate issue display metadata on run rows"));
	assert_eq!(active_run.author.as_deref(), Some("Yvette"));
	assert_eq!(recent_run.issue_identifier.as_deref(), Some("XY-392"));
	assert_eq!(recent_run.title.as_deref(), Some("Hydrate issue display metadata on run rows"));
	assert_eq!(recent_run.author.as_deref(), Some("Yvette"));
	assert_eq!(snapshot_json["active_runs"][0]["project_id"], "pubfi");
	assert_eq!(snapshot_json["active_runs"][0]["issue_identifier"], "XY-392");
	assert_eq!(
		snapshot_json["active_runs"][0]["title"],
		"Hydrate issue display metadata on run rows"
	);
	assert_eq!(snapshot_json["active_runs"][0]["author"], "Yvette");
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
	assert!(snapshot.active_runs.is_empty(), "idle snapshot should have no active runs");
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
		"active_runs",
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
	assert!(rendered.contains("Active queue echoes: 0"));
	assert!(rendered.contains("Stale closed queue labels: 0"));
	assert!(rendered.contains("Recovery worktrees: 0"));
	assert!(rendered.contains("Post-review lanes: 0"));
	assert!(rendered.contains("\nRunning Lanes\n- none\n"));
	assert!(rendered.contains("\nRun Ledger\n- none\n"));
	assert!(rendered.contains("\nBacklog\n- none\n"));
	assert!(rendered.contains("\nActive Queue Echoes\n- none\n"));
	assert!(rendered.contains("\nStale Closed Queue Labels\n- none\n"));
	assert!(rendered.contains("\nRecovery Worktrees\n- none\n"));
	assert!(rendered.contains("\nPost-Review Lanes\n- none\n"));
	assert!(!rendered.contains("Warning details:"));
	assert!(!rendered.contains("run_id:"));
	assert!(!rendered.contains("active_lease: true"));
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
		r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":7,"limit_window_seconds":18000,"reset_at":1800018000},"secondary_window":{"used_percent":11,"limit_window_seconds":604800,"reset_at":1800604800}},"credits":{"has_credits":true,"unlimited":false,"balance":"12.34"}}"#,
		r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":22,"limit_window_seconds":18000,"reset_at":1800019000},"secondary_window":{"used_percent":33,"limit_window_seconds":604800,"reset_at":1800605800}},"credits":{"has_credits":false,"unlimited":false,"balance":"0"}}"#,
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
		base_config.codex().internal_review_mode(),
		base_config.codex().external_review_enabled(),
	);

	config_toml.push_str(&format!(
		"\n[codex.accounts]\nusage_endpoint = \"{}\"\n",
		usage_endpoint
	));

	write_service_config(base_config.repo_root(), &config_toml);

	let config = load_service_config(base_config.repo_root());
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let accounts =
		snapshot_json["accounts"].as_array().expect("snapshot should expose configured accounts");

	assert!(snapshot.active_runs.is_empty());
	assert_eq!(snapshot_json["account_control"]["mode"], "balanced");
	assert_eq!(
		snapshot_json["account_control"]["account_selector"],
		serde_json::Value::Null,
	);
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

fn start_codex_usage_fixture_server(responses: Vec<&'static str>) -> String {
	let listener = TcpListener::bind("127.0.0.1:0").expect("usage fixture server should bind");
	let address = listener.local_addr().expect("usage fixture address should resolve");

	thread::spawn(move || {
		for body in responses {
			let (mut stream, _peer) =
				listener.accept().expect("usage fixture request should arrive");
			let mut request = [0_u8; 4_096];
			let _ = stream.read(&mut request);
			let response = format!(
				"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
				body.len(),
				body
			);

			stream
				.write_all(response.as_bytes())
				.expect("usage fixture response should write");

			let _ = stream.shutdown(Shutdown::Both);
		}
	});

	format!("http://{address}/wham/usage")
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
	let run = snapshot.active_runs.first().expect("active run should exist");

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

	state::write_run_operation_marker(
		&worktree_path,
		"run-1",
		1,
		RUN_OPERATION_RECONCILIATION,
	)
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
	let suspected_age = (ACTIVE_RUN_IDLE_TIMEOUT.as_secs() / 2).saturating_add(1) as i64;
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
	let run = snapshot.active_runs.first().expect("active run should exist");

	assert_eq!(run.current_operation, state::RUN_OPERATION_AGENT_RUN);
	assert!(run.last_progress_at.is_some());
	assert!(run.suspected_stall);
}

#[test]
fn operator_status_snapshot_counts_stopped_active_process_as_attention_not_running() {
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
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, u32::MAX)
		.expect("stopped process marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.active_runs.first().expect("active run should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.phase, "executing");
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("process_stopped"));
	assert_eq!(project.active_run_count, 0);
	assert_eq!(project.attention_count, 1);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn operator_status_snapshot_counts_previous_boot_process_as_attention_not_running() {
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
		.expect("live process marker should write");

	rewrite_run_activity_marker_host_boot_id(&worktree_path, "previous-boot");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.active_runs.first().expect("active run should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.phase, "executing");
	assert_eq!(run.process_id, Some(process::id()));
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("host_boot_id_mismatch"));
	assert_eq!(project.active_run_count, 0);
	assert_eq!(project.attention_count, 1);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn operator_status_snapshot_counts_reused_pid_as_attention_not_running() {
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
		.expect("live process marker should write");

	rewrite_run_activity_marker_process_start_identity(&worktree_path, "previous-process-start");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.active_runs.first().expect("active run should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.phase, "executing");
	assert_eq!(run.process_id, Some(process::id()));
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(
		run.process_liveness_reason.as_deref(),
		Some("process_start_identity_mismatch")
	);
	assert_eq!(project.active_run_count, 0);
	assert_eq!(project.attention_count, 1);
}

#[test]
fn operator_status_snapshot_keeps_unleased_live_process_in_running_lanes() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
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
		.expect("live process marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.active_runs.first().expect("active run should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(snapshot.active_runs.len(), 1);
	assert_eq!(run.run_id, "run-1");
	assert_eq!(run.status, "running");
	assert_eq!(run.attempt_status, "running");
	assert!(!run.active_lease);
	assert_eq!(run.queue_lease_state, "not_held");
	assert_eq!(run.execution_liveness, "process_alive");
	assert_eq!(run.process_alive, Some(true));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("process_alive"));
	assert_eq!(project.active_run_count, 1);
	assert_eq!(project.retained_worktree_count, 0);
}

#[test]
fn operator_status_snapshot_promotes_starting_after_app_server_activity() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "starting")
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

	state::write_run_thread_status_marker(
		&worktree_path,
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[],
	)
	.expect("thread status should write");
	state::write_run_effective_runtime_marker(
		&worktree_path,
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
	.expect("effective runtime should write");
	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
				event_count: 2,
				last_event_type: "model/response",
				child_agent_activity: None,
				protocol_activity: None,
			},
		)
	.expect("protocol summary should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.active_runs.first().expect("active run should remain visible");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(run.status, "running");
	assert_eq!(run.attempt_status, "starting");
	assert_eq!(run.phase, "executing");
	assert_eq!(run.queue_lease_state, "held");
	assert_eq!(run.execution_liveness, "process_alive");
	assert_eq!(run.thread_status.as_deref(), Some("active"));
	assert_eq!(run.effective_model.as_deref(), Some("gpt-5.4"));
	assert!(rendered.contains("status: running"));
	assert!(rendered.contains("attempt_status: starting"));
}

#[test]
fn operator_status_snapshot_counts_stale_starting_run_as_attention_not_running() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let stale_activity =
		OffsetDateTime::now_utc().unix_timestamp() - ACTIVE_RUN_IDLE_TIMEOUT.as_secs() as i64 - 30;

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "starting")
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
	fs::write(
		worktree_path.join(RUN_ACTIVITY_MARKER_FILE),
		format!(
			"run_id=run-1\nattempt_number=1\nlast_activity_unix_epoch={stale_activity}\nlast_protocol_activity_unix_epoch={stale_activity}\nlast_progress_unix_epoch={stale_activity}\n"
		),
	)
	.expect("stale processless marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.active_runs.first().expect("active run should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.status, "starting");
	assert_eq!(run.phase, "executing");
	assert_eq!(run.process_alive, None);
	assert!(run.protocol_idle_for_seconds.is_some_and(|idle| {
		u64::try_from(idle).is_ok_and(|idle| idle >= ACTIVE_RUN_IDLE_TIMEOUT.as_secs())
	}));
	assert_eq!(project.active_run_count, 0);
	assert_eq!(project.attention_count, 1);
}

#[test]
fn operator_status_snapshot_excludes_completed_lingering_lease_from_active_runs() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let completed_issue = sample_issue_with_sort_fields(
		"issue-1",
		"XY-379",
		"Done",
		&[],
		Some(3),
		"2026-04-29T17:00:33.133Z",
	);
	let active_issue = sample_issue_with_sort_fields(
		"issue-2",
		"XY-378",
		"In Progress",
		&[],
		Some(3),
		"2026-04-29T17:01:33.133Z",
	);
	let completed_run_id = "xy-379-attempt-1-1777482033";
	let active_run_id = "xy-378-attempt-1-1777482000";

	state_store
		.record_run_attempt(completed_run_id, &completed_issue.id, 1, "running")
		.expect("completed run should record");
	state_store
		.upsert_lease("pubfi", &completed_issue.id, completed_run_id, "In Progress")
		.expect("stale active lease should remain in runtime db");
	state_store
		.append_event(completed_run_id, 1, "turn/completed", "{\"turn\":\"1\"}")
		.expect("terminal protocol evidence should record");
	state_store
		.update_run_status(completed_run_id, "succeeded")
		.expect("terminal status should update");
	state_store
		.record_run_attempt(active_run_id, &active_issue.id, 1, "running")
		.expect("active run should record");
	state_store
		.upsert_lease("pubfi", &active_issue.id, active_run_id, "In Progress")
		.expect("active lease should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 25)
		.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");
	let completed_run = snapshot
		.recent_runs
		.iter()
		.find(|run| run.run_id == completed_run_id)
		.expect("completed stale-lease run should remain in history");

	assert_eq!(snapshot.active_runs.len(), 1);
	assert_eq!(snapshot.active_runs[0].run_id, active_run_id);
	assert_eq!(snapshot.active_runs[0].phase, "executing");
	assert_eq!(project.active_run_count, 1);
	assert_eq!(snapshot.recent_runs.len(), 2);
	assert_eq!(completed_run.phase, "completed");
	assert!(
		completed_run.active_lease,
		"regression setup should keep the stale lease visible in history"
	);
}

#[test]
fn operator_status_snapshot_rolls_current_child_bucket_elapsed_time_into_bucket() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let started_at = OffsetDateTime::now_utc().unix_timestamp() - 90;

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

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 1,
			last_event_type: "item/tool/call",
			child_agent_activity: Some(&ChildAgentActivitySummary {
				buckets: vec![state::ChildAgentActivityBucket {
					name: String::from("Tracker"),
					event_count: 1,
					tool_call_count: 1,
					..state::ChildAgentActivityBucket::default()
				}],
				current_bucket: Some(String::from("Tracker")),
				current_detail: Some(String::from("issue_progress_checkpoint")),
				current_started_unix_epoch: Some(started_at),
				current_elapsed_seconds: Some(0),
				event_count: 1,
				tool_call_count: 1,
				..ChildAgentActivitySummary::default()
				}),
				protocol_activity: None,
			},
		)
	.expect("protocol activity marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.active_runs.first().expect("active run should exist");
	let activity = run.child_agent_activity.as_ref().expect("activity should render");
	let protocol_activity =
		run.protocol_activity.as_ref().expect("protocol fallback should render");
	let tracker_bucket =
		activity.buckets.iter().find(|bucket| bucket.name == "Tracker").expect("tracker bucket");

	assert_eq!(run.wait_reason.as_deref(), Some("tool_execution"));
	assert_eq!(protocol_activity.waiting_reason.as_deref(), Some("tool_execution"));
	assert!(activity.current_elapsed_seconds.is_some_and(|elapsed| elapsed >= 90));
	assert!(
		tracker_bucket.wall_seconds >= 90,
		"current tool-call elapsed time should contribute to tracker bucket wall time"
	);
}

#[test]
fn operator_status_snapshot_uses_structured_protocol_activity_summary() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("approval_or_user_input")),
		rate_limit_status: Some(String::from("primary")),
		recent_events: vec![
			state::ProtocolActivityEventSummary {
				event_type: String::from("plan/update"),
				category: String::from("plan"),
				detail: Some(String::from("verify")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("item/tool/requestUserInput"),
				category: String::from("item"),
				detail: None,
			},
		],
	};

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

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 2,
			last_event_type: "item/tool/requestUserInput",
			child_agent_activity: None,
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol activity marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.active_runs.first().expect("active run should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(run.wait_reason.as_deref(), Some("approval_or_user_input"));
	assert_eq!(run.protocol_activity.as_ref(), Some(&protocol_activity));
	assert!(rendered.contains("protocol_activity: turn=running; waiting=approval_or_user_input; rate_limit=primary; recent=item/tool/requestUserInput, plan/update:verify"));
}

#[test]
fn operator_status_snapshot_ignores_marker_from_newer_attempt_for_stored_run() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "failed")
		.expect("stored run should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-2", 2, process::id())
		.expect("newer attempt marker should write");
	state::write_run_retry_schedule(
		&worktree_path,
		"run-2",
		2,
		"failure",
		OffsetDateTime::now_utc().unix_timestamp() + 60,
	)
	.expect("retry schedule marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.recent_runs.first().expect("recent run should exist");

	assert_eq!(run.run_id, "run-1");
	assert_eq!(run.phase, "failed");
	assert_eq!(run.wait_reason, None);
	assert_eq!(run.process_id, None);
	assert_eq!(run.process_alive, None);
	assert_eq!(run.retry_kind, None);
	assert_eq!(run.next_retry_at, None);
}

#[test]
fn operator_status_snapshot_keeps_all_active_runs_when_recent_runs_are_limited() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let first_issue = sample_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let second_issue = sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:17:17.133Z",
	);

	for (run_id, issue, branch_suffix) in
		[("run-1", &first_issue, "101"), ("run-2", &second_issue, "102")]
	{
		state_store
			.record_run_attempt(run_id, &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease("pubfi", &issue.id, run_id, "In Progress")
			.expect("lease should record");
		state_store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				&format!("x/pubfi-pub-{branch_suffix}"),
				&config.worktree_root().join(&issue.identifier).display().to_string(),
			)
			.expect("worktree should record");
	}

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 1)
		.expect("snapshot should build");

	assert_eq!(snapshot.run_limit, 1);
	assert_eq!(snapshot.recent_runs.len(), 2);
	assert_eq!(snapshot.active_runs.len(), 2);
	assert!(snapshot.active_runs.iter().all(|run| run.active_lease));
}

#[test]
fn operator_status_snapshot_keeps_terminal_run_after_lane_cleanup() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"Done",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);

	state_store.record_run_attempt("run-done", &issue.id, 1, "running").expect("run should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-done", "In Progress")
		.expect("active lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should record");
	state_store.update_run_status("run-done", "succeeded").expect("terminal status should update");
	state_store.clear_lease(&issue.id).expect("terminal cleanup should clear active lease");
	state_store.clear_worktree(&issue.id).expect("terminal cleanup should clear worktree");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 25)
		.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(snapshot.active_runs.is_empty());
	assert_eq!(snapshot.recent_runs.len(), 1);
	assert_eq!(snapshot.recent_runs[0].run_id, "run-done");
	assert_eq!(snapshot.recent_runs[0].phase, "completed");
	assert!(!snapshot.recent_runs[0].active_lease);
	assert_eq!(snapshot.recent_runs[0].branch_name, None);
	assert_eq!(snapshot.recent_runs[0].worktree_path, None);
	assert_eq!(snapshot.history_lanes.len(), 1);
	assert_eq!(snapshot.history_lanes[0].latest_run.run_id, "run-done");
	assert!(rendered.contains("Run ledger shown: 1 issue lanes from 1 history attempts"));
	assert!(rendered.contains("run_id: run-done"));
}

#[test]
fn status_hydration_does_not_fabricate_active_leases_for_recovered_candidates() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	orchestrator::hydrate_status_snapshot_state(
		&config,
		&state_store,
		RecoveredRuntimeState { active_issues: vec![issue.clone()] },
	)
	.expect("status hydration should succeed");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	assert!(
		snapshot.active_runs.is_empty(),
		"recovered retry candidates should not appear as active leased runs"
	);
	assert!(
		snapshot.recent_runs.is_empty(),
		"status hydration should not persist synthetic recovered runs"
	);
}

#[test]
fn live_operator_status_snapshot_hydrates_active_run_thread_and_event_metadata_from_marker() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker(&worktree_path, "run-1", 1)
		.expect("activity marker should write");
	state::write_run_thread_marker(&worktree_path, "run-1", 1, "thread-1")
		.expect("thread marker should write");
	state::write_run_turn_marker(&worktree_path, "run-1", 1, "turn-1")
		.expect("turn marker should write");
	state::write_run_thread_status_marker(
		&worktree_path,
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[String::from("waitingOnApproval")],
	)
	.expect("thread status should write");
	state::write_run_effective_runtime_marker(
		&worktree_path,
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
	.expect("effective runtime should write");
	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
				event_count: 2,
				last_event_type: "turn/completed",
				child_agent_activity: None,
				protocol_activity: None,
			},
		)
	.expect("protocol summary should write");

	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should succeed");

	orchestrator::hydrate_status_snapshot_state(&config, &state_store, recovered_state)
		.expect("status hydration should succeed");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");

	assert_eq!(snapshot.active_runs.len(), 1);
	assert_eq!(snapshot.active_runs[0].thread_id.as_deref(), Some("thread-1"));
	assert_eq!(snapshot.active_runs[0].turn_id.as_deref(), Some("turn-1"));
	assert_eq!(snapshot.active_runs[0].thread_status.as_deref(), Some("active"));
	assert_eq!(
		snapshot.active_runs[0].thread_active_flags,
		vec![String::from("waitingOnApproval")]
	);
	assert!(snapshot.active_runs[0].interactive_requested);
	assert_eq!(snapshot.active_runs[0].event_count, 2);
	assert_eq!(snapshot.active_runs[0].last_event_type.as_deref(), Some("turn/completed"));
	assert_eq!(snapshot.active_runs[0].effective_model.as_deref(), Some("gpt-5.4"));
	assert_eq!(snapshot.active_runs[0].effective_model_provider.as_deref(), Some("openai"));
	assert_eq!(snapshot.active_runs[0].effective_approval_policy.as_deref(), Some("never"));
	assert_eq!(snapshot.active_runs[0].effective_sandbox_mode.as_deref(), Some("workspaceWrite"));
	assert!(snapshot.active_runs[0].last_event_at.is_some());
}
