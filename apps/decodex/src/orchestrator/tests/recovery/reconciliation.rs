fn reconciliation_sample_service_owned_issue(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	sample_issue(state_name, &[active_label.as_str()])
}

#[test]
fn active_run_reconciliation_detects_terminal_nonactive_and_stalled_runs() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let terminal_issue = sample_issue_with_sort_fields(
		"issue-terminal",
		"PUB-201",
		"Done",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let nonactive_issue = sample_issue_with_sort_fields(
		"issue-nonactive",
		"PUB-202",
		"Blocked",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let stalled_issue = sample_issue_with_sort_fields(
		"issue-stalled",
		"PUB-203",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![
		terminal_issue.clone(),
		nonactive_issue.clone(),
		stalled_issue.clone(),
	]);

	for issue in [&terminal_issue, &nonactive_issue, &stalled_issue] {
		state_store
			.record_run_attempt(&format!("run-{}", issue.identifier), &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease("pubfi", &issue.id, &format!("run-{}", issue.identifier), "In Progress")
			.expect("lease should record");
	}

	state_store
		.append_event(
			&format!("run-{}", stalled_issue.identifier),
			1,
			"thread/status/changed",
			"{\"status\":\"active\"}",
		)
		.expect("stalled issue protocol event should record");

	let now =
		OffsetDateTime::now_utc().unix_timestamp() + ACTIVE_RUN_IDLE_TIMEOUT.as_secs() as i64 + 1;
	let actions = orchestrator::inspect_active_run_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		now,
	)
	.expect("active-run inspection should succeed");

	assert!(actions.iter().any(|action| {
		action.issue.id == terminal_issue.id
			&& matches!(action.disposition, orchestrator::ActiveRunDisposition::Terminal)
	}));
	assert!(actions.iter().any(|action| {
		action.issue.id == nonactive_issue.id
			&& matches!(action.disposition, orchestrator::ActiveRunDisposition::NonActive)
	}));
	assert!(actions.iter().any(|action| {
		action.issue.id == stalled_issue.id
			&& matches!(
			action.disposition,
			ActiveRunDisposition::Stalled{ idle_for }
				if idle_for >= ACTIVE_RUN_IDLE_TIMEOUT
			)
	}));
}

#[test]
fn active_run_reconciliation_detects_stalled_run_without_protocol_events() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let stalled_issue = sample_issue_with_sort_fields(
		"issue-stalled-no-events",
		"PUB-204",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![stalled_issue.clone()]);

	state_store
		.record_run_attempt(
			&format!("run-{}", stalled_issue.identifier),
			&stalled_issue.id,
			1,
			"running",
		)
		.expect("run attempt should record");
	state_store
		.upsert_lease(
			"pubfi",
			&stalled_issue.id,
			&format!("run-{}", stalled_issue.identifier),
			"In Progress",
		)
		.expect("lease should record");

	let now =
		OffsetDateTime::now_utc().unix_timestamp() + ACTIVE_RUN_IDLE_TIMEOUT.as_secs() as i64 + 1;
	let actions = orchestrator::inspect_active_run_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		now,
	)
	.expect("active-run inspection should succeed");

	assert!(actions.iter().any(|action| {
		action.issue.id == stalled_issue.id
			&& matches!(
				action.disposition,
				ActiveRunDisposition::Stalled{ idle_for }
					if idle_for >= ACTIVE_RUN_IDLE_TIMEOUT
			)
	}));
}

#[test]
fn active_run_reconciliation_supersedes_stale_lease_for_newer_attempt() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-superseded-lease",
		"PUB-207",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let stale_run_id = "run-superseded-lease-1";
	let newer_run_id = "run-superseded-lease-2";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	state_store
		.record_run_attempt(stale_run_id, &issue.id, 1, "running")
		.expect("stale run should record");
	state_store
		.record_run_attempt(newer_run_id, &issue.id, 2, "succeeded")
		.expect("newer run should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, stale_run_id, "In Progress")
		.expect("stale lease should record");

	let actions = orchestrator::inspect_active_run_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		OffsetDateTime::now_utc().unix_timestamp(),
	)
	.expect("active-run inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(
		&actions[0].disposition,
		ActiveRunDisposition::Superseded {
			newer_run_id: observed_run_id,
			newer_attempt_number: 2,
		} if observed_run_id == newer_run_id
	));

	orchestrator::apply_active_run_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		actions,
	)
	.expect("superseded reconciliation should succeed");

	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
	assert_eq!(
		state_store
			.run_attempt(stale_run_id)
			.expect("run attempt lookup should succeed")
			.expect("stale run should exist")
			.status(),
		"interrupted"
	);
	assert!(
		tracker.comments.borrow().is_empty(),
		"superseded stale lease must not write needs-attention comments"
	);
}

#[test]
fn active_run_reconciliation_keeps_completed_closeout_lane_with_fresh_activity() {
	let (_temp_dir, base_config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_ACTIVE_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
	let issue = reconciliation_sample_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-closeout-active";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/180";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store.upsert_lease("pubfi", &issue.id, run_id, "Done").expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	state::write_run_activity_marker(&worktree.path, run_id, 1)
		.expect("fresh activity marker should write");

	let actions = orchestrator::inspect_active_run_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		OffsetDateTime::now_utc().unix_timestamp(),
	)
	.expect("active-run inspection should succeed");

	assert!(
		actions.is_empty(),
		"completed retained closeout lanes with fresh activity must not be reconciled as terminal or non-active"
	);
}

#[test]
fn active_daemon_child_reconciliation_keeps_completed_closeout_lane_with_fresh_activity() {
	let (_temp_dir, base_config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_ACTIVE_DAEMON_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
	let issue = reconciliation_sample_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-daemon-closeout-active";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/181";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store.upsert_lease("pubfi", &issue.id, run_id, "Done").expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	state::write_run_activity_marker(&worktree.path, run_id, 1)
		.expect("fresh activity marker should write");

	let actions = orchestrator::inspect_active_daemon_child_reconciliation(
		&tracker,
		&config,
		&workflow,
		&state_store,
		ActiveChildRunContext {
			child: ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
			workflow: &workflow,
			dispatch_mode: IssueDispatchMode::Closeout,
		},
	)
	.expect("active daemon-child inspection should succeed");

	assert!(
		actions.is_empty(),
		"completed retained closeout daemon children with fresh activity must not be reconciled as terminal or non-active"
	);
}

#[test]
fn active_daemon_child_reconciliation_keeps_review_repair_lane_in_review() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = reconciliation_sample_service_owned_issue("In Review");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-review-repair-active";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained review-repair worktree should exist");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "In Review")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let actions = orchestrator::inspect_active_daemon_child_reconciliation(
		&tracker,
		&config,
		&workflow,
		&state_store,
		ActiveChildRunContext {
			child: ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
			workflow: &workflow,
			dispatch_mode: IssueDispatchMode::ReviewRepair,
		},
	)
	.expect("active review-repair daemon-child inspection should succeed");

	assert!(
		actions.is_empty(),
		"active review-repair lanes in In Review must stay active instead of being interrupted as non-active"
	);
}

#[test]
fn active_daemon_child_reconciliation_keeps_closeout_child_after_tracker_completion() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("Done", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-closeout-completed";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "Done")
		.expect("lease should record");

	let actions = orchestrator::inspect_active_daemon_child_reconciliation(
		&tracker,
		&config,
		&workflow,
		&state_store,
		ActiveChildRunContext {
			child: ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
			workflow: &workflow,
			dispatch_mode: IssueDispatchMode::Closeout,
		},
	)
	.expect("active closeout daemon-child inspection should succeed");

	assert!(
		actions.is_empty(),
		"closeout children may legitimately observe a completed tracker issue while they finish local cleanup"
	);
}

#[test]
fn active_run_reconciliation_treats_completed_retained_handoff_as_success() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue_with_sort_fields(
		"issue-handoff-complete",
		"PUB-205",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained review worktree should exist");
	let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/205";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.append_event(run_id, 1, "thread/status/changed", "{\"status\":\"active\"}")
		.expect("stale run activity should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let now =
		OffsetDateTime::now_utc().unix_timestamp() + ACTIVE_RUN_IDLE_TIMEOUT.as_secs() as i64 + 1;
	let actions = orchestrator::inspect_active_run_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		now,
	)
	.expect("active-run inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(actions[0].disposition, ActiveRunDisposition::RetainedReviewComplete));

	orchestrator::apply_active_run_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		actions,
	)
	.expect("completed retained handoff reconciliation should succeed");

	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"retained post-review worktree must stay available for merge/closeout"
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"succeeded"
	);
	assert!(
		tracker.comments.borrow().iter().all(|comment| !comment.contains("stalled_run_detected")),
		"completed retained handoff must not be routed through needs-attention"
	);
}

#[test]
fn active_run_reconciliation_ignores_stale_retained_handoff_marker() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue_with_sort_fields(
		"issue-stale-handoff",
		"PUB-205B",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-current";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained review worktree should exist");
	let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.append_event(run_id, 1, "thread/status/changed", "{\"status\":\"active\"}")
		.expect("stale run activity should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&ReviewHandoffMarker::new(
			"run-previous",
			1,
			&worktree.branch_name,
			"https://github.com/hack-ink/decodex/pull/205",
			"main",
			&worktree.branch_name,
			&head_oid,
		),
	);

	let now =
		OffsetDateTime::now_utc().unix_timestamp() + ACTIVE_RUN_IDLE_TIMEOUT.as_secs() as i64 + 1;
	let actions = orchestrator::inspect_active_run_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		now,
	)
	.expect("active-run inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(
		actions[0].disposition,
		ActiveRunDisposition::Stalled { idle_for }
			if idle_for >= ACTIVE_RUN_IDLE_TIMEOUT
	));
}

#[test]
fn active_daemon_child_reconciliation_treats_completed_retained_handoff_as_success() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue_with_sort_fields(
		"issue-daemon-handoff-complete",
		"PUB-206",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained review worktree should exist");
	let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/206";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.append_event(run_id, 1, "thread/status/changed", "{\"status\":\"active\"}")
		.expect("stale run activity should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let now =
		OffsetDateTime::now_utc().unix_timestamp() + ACTIVE_RUN_IDLE_TIMEOUT.as_secs() as i64 + 1;
	let actions = orchestrator::inspect_active_daemon_child_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		ActiveChildRunContext {
			child: ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
			workflow: &workflow,
			dispatch_mode: IssueDispatchMode::Normal,
		},
		now,
	)
	.expect("active daemon-child inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(actions[0].disposition, ActiveRunDisposition::RetainedReviewComplete));
}

#[test]
fn stalled_idle_duration_ignores_future_last_activity() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let run_id = "run-future-activity";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");

	let last_activity = state_store
		.last_run_activity_unix_epoch(run_id)
		.expect("last activity lookup should succeed")
		.expect("run activity should exist");

	assert_eq!(
		orchestrator::stalled_idle_duration(
			&state_store,
			&state_store
				.run_attempt(run_id)
				.expect("run lookup should succeed")
				.expect("run attempt should exist"),
			None,
			last_activity - 1
		)
		.expect("idle duration should evaluate"),
		None
	);
}

#[test]
fn active_run_reconciliation_uses_worktree_activity_marker_from_child_process() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-shared-activity";
	let worktree_path = config.worktree_root().join("PUB-101");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

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
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let last_activity = state_store
		.last_run_activity_unix_epoch(run_id)
		.expect("last activity lookup should succeed")
		.expect("run activity should exist");
	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);

	fs::write(
		&marker_path,
		format!(
			"run_id={run_id}\nattempt_number=1\nlast_activity_unix_epoch={}\n",
			last_activity + ACTIVE_RUN_IDLE_TIMEOUT.as_secs() as i64
		),
	)
	.expect("activity marker should write");

	let actions = orchestrator::inspect_active_run_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		last_activity + ACTIVE_RUN_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("active run inspection should succeed");

	assert!(
		actions.is_empty(),
		"fresh child activity marker should prevent daemon stall reconciliation"
	);
}

#[test]
fn active_run_reconciliation_allows_running_model_execution_until_model_timeout() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-model-execution-idle";
	let worktree_path = config.worktree_root().join("PUB-101-model");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

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
			"x/pubfi-pub-101-model",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let last_activity = state_store
		.last_run_activity_unix_epoch(run_id)
		.expect("last activity lookup should succeed")
		.expect("run activity should exist");
	let protocol_activity =
		r#"{"turn_status":"running","waiting_reason":"model_execution","rate_limit_status":null,"recent_events":[]}"#;

	fs::write(
		worktree_path.join(RUN_ACTIVITY_MARKER_FILE),
		format!(
			"run_id={run_id}\nattempt_number=1\nlast_activity_unix_epoch={last_activity}\nlast_protocol_activity_unix_epoch={last_activity}\nlast_progress_unix_epoch={last_activity}\nprotocol_activity={protocol_activity}\n"
		),
	)
	.expect("activity marker should write");

	let actions = orchestrator::inspect_active_run_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		last_activity + ACTIVE_RUN_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("active run inspection should succeed");

	assert!(
		actions.is_empty(),
		"running model execution should not stall on the generic active idle timeout"
	);

	let actions = orchestrator::inspect_active_run_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		last_activity + MODEL_EXECUTION_IDLE_TIMEOUT.as_secs() as i64 + 1,
	)
	.expect("active run inspection should succeed");

	assert!(actions.iter().any(|action| {
		action.issue.id == issue.id
			&& matches!(
				action.disposition,
				ActiveRunDisposition::Stalled{ idle_for }
					if idle_for >= MODEL_EXECUTION_IDLE_TIMEOUT
			)
	}));
}

#[test]
fn stalled_protocol_idle_duration_ignores_future_protocol_activity() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-protocol-future-activity";

	state_store
		.record_run_attempt(run_id, "issue-1", 1, "running")
		.expect("run attempt should record");
	state_store
		.append_event(run_id, 1, "thread/status/changed", "{\"status\":\"active\"}")
		.expect("protocol event should record");

	let run_attempt = state_store
		.run_attempt(run_id)
		.expect("run attempt lookup should succeed")
		.expect("run attempt should exist");
	let last_activity = state_store
		.last_protocol_activity_unix_epoch(run_id)
		.expect("protocol activity lookup should succeed")
		.expect("protocol activity should exist");

	assert_eq!(
		orchestrator::stalled_protocol_idle_duration(
			&state_store,
			&run_attempt,
			None,
			last_activity - 1,
		)
		.expect("protocol idle duration should evaluate"),
		None
	);
}

#[test]
fn active_run_reconciliation_ignores_startable_preclaim_states() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-startable",
		"PUB-204",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	state_store
		.record_run_attempt("run-startable", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-startable", "In Progress")
		.expect("lease should record");

	let now = OffsetDateTime::now_utc().unix_timestamp() + 1;
	let actions = orchestrator::inspect_active_run_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		now,
	)
	.expect("active-run inspection should succeed");

	assert!(actions.is_empty(), "startable pre-claim states should not be interrupted");
}

#[test]
fn active_run_reconciliation_clears_terminal_lane_labels() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = reconciliation_sample_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let run_id = "run-terminal";
	let worktree_path = config.worktree_root().join("PUB-101");

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
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let action = ActiveRunReconciliation {
		issue: issue.clone(),
		run_attempt: state_store
			.run_attempt(run_id)
			.expect("run attempt query should succeed")
			.expect("run attempt should exist"),
		worktree_mapping: state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree query should succeed"),
		disposition: ActiveRunDisposition::Terminal,
		workflow: workflow.clone(),
	};

	orchestrator::apply_active_run_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		vec![action],
	)
	.expect("terminal reconciliation should succeed");

	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[
			(issue.id.clone(), vec![String::from("label-active")]),
			(issue.id.clone(), vec![String::from("label-queued")]),
		]
	);
}

#[test]
fn active_run_reconciliation_keeps_nonterminal_nonactive_worktrees() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let issue = sample_issue("Todo", &[]);
	let run_id = "run-nonactive";
	let worktree_path = config.worktree_root().join("PUB-101");

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
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let action = ActiveRunReconciliation {
		issue: issue.clone(),
		run_attempt: state_store
			.run_attempt(run_id)
			.expect("run attempt query should succeed")
			.expect("run attempt should exist"),
		worktree_mapping: state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree query should succeed"),
		disposition: ActiveRunDisposition::NonActive,
		workflow: workflow.clone(),
	};

	orchestrator::apply_active_run_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		vec![action],
	)
	.expect("reconciliation should succeed");

	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some()
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"interrupted"
	);
}

#[test]
fn stalled_run_reconciliation_routes_to_needs_attention_without_cleanup() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let issue = sample_issue("In Progress", &[]);
	let run_id = "run-stalled";
	let worktree_path = config.worktree_root().join("PUB-101");

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
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let action = ActiveRunReconciliation {
		issue: issue.clone(),
		run_attempt: state_store
			.run_attempt(run_id)
			.expect("run attempt query should succeed")
			.expect("run attempt should exist"),
		worktree_mapping: state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree query should succeed"),
		disposition: ActiveRunDisposition::Stalled {
			idle_for: ACTIVE_RUN_IDLE_TIMEOUT + Duration::from_secs(1),
		},
		workflow: workflow.clone(),
	};

	orchestrator::apply_active_run_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		vec![action],
	)
	.expect("reconciliation should succeed");

	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some()
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"stalled"
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("stalled_run_detected")
			&& comment.contains("needs attention")
			&& comment.contains("clear label `decodex:needs-attention`")
	}));
	assert!(
		tracker
			.comments
			.borrow()
			.iter()
			.all(|comment| !comment.contains("retained partial progress"))
	);

	let ledger_event = tracker
		.comments
		.borrow()
		.iter()
		.find_map(|comment| records::parse_linear_execution_event_record(comment))
		.expect("stalled no-progress run should write a Linear execution event");

	assert_eq!(ledger_event.event_type, "terminal_failure");
	assert_eq!(ledger_event.error_class.as_deref(), Some("stalled_run_detected"));
	assert_eq!(ledger_event.terminal_path.as_deref(), None);
	assert_eq!(
		ledger_event.summary.as_deref(),
		Some("Decodex run failed and needs attention.")
	);
}

#[test]
fn stalled_run_reconciliation_reports_retained_partial_progress_for_dirty_worktree() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let issue = sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let run_id = "run-stalled-dirty";
	let worktree_path = config.worktree_root().join("PUB-102");

	git_status_success(
		config.repo_root(),
		&["worktree", "add", "-b", "x/pubfi-pub-102", ".worktrees/PUB-102", "main"],
	);

	fs::write(worktree_path.join("README.md"), "retained partial work\n")
		.expect("tracked worktree file should change");

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
			"x/pubfi-pub-102",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.append_event(run_id, 1, "turn/diff/updated", "{\"changes\":1}")
		.expect("stalled dirty issue protocol event should record");

	let now =
		OffsetDateTime::now_utc().unix_timestamp() + ACTIVE_RUN_IDLE_TIMEOUT.as_secs() as i64 + 1;
	let actions = orchestrator::inspect_active_run_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		now,
	)
	.expect("dirty stalled-run inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(
		actions[0].disposition,
		ActiveRunDisposition::StalledRetainedPartialProgress { idle_for }
			if idle_for >= ACTIVE_RUN_IDLE_TIMEOUT
	));

	orchestrator::apply_active_run_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		actions,
	)
	.expect("reconciliation should succeed");

	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some()
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"stalled"
	);

	let comments = tracker.comments.borrow();

	assert!(comments.iter().any(|comment| {
		comment.contains("decodex retained partial progress and needs attention")
			&& comment.contains("partial_progress_retained")
			&& comment.contains("finish validation and PR handoff or reset the patch manually")
			&& comment.contains(".worktrees/PUB-102")
	}));
	assert!(
		comments
			.iter()
			.all(|comment| !comment.contains("- error_class: `stalled_run_detected`"))
	);
	assert!(comments.iter().all(|comment| !comment.contains("decodex run failed and needs attention")));

	let ledger_event = comments
		.iter()
		.find_map(|comment| records::parse_linear_execution_event_record(comment))
		.expect("retained partial progress should write a Linear execution event");

	assert_eq!(ledger_event.event_type, "needs_attention");
	assert_eq!(ledger_event.error_class.as_deref(), Some("partial_progress_retained"));
	assert_eq!(ledger_event.terminal_path.as_deref(), Some("retained_partial_progress"));
	assert_eq!(
		ledger_event.summary.as_deref(),
		Some("Decodex retained partial progress and needs attention.")
	);
	assert_eq!(
		ledger_event.blockers.as_deref(),
		Some([String::from(
			"Retained tracked worktree changes require operator recovery."
		)]
		.as_slice())
	);
	assert!(
		ledger_event
			.evidence
			.as_deref()
			.is_some_and(|evidence| evidence
				.iter()
				.any(|item| item.contains("tracked worktree changes retained"))),
		"retained partial progress evidence should mention retained tracked changes"
	);
	assert!(
		ledger_event
			.evidence
			.as_deref()
			.is_some_and(|evidence| evidence
				.iter()
				.any(|item| item.contains("Source failure class `stalled_run_detected`"))),
		"retained partial progress evidence should preserve the stalled source class"
	);
}

#[test]
fn project_reconciliation_routes_orphaned_active_worktree_run_to_needs_attention() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = reconciliation_sample_service_owned_issue("In Progress");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let run_id = "run-orphaned-active";
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	state::write_run_activity_marker_for_process(&worktree_path, run_id, 1, u32::MAX)
		.expect("stopped process marker should write");
	orchestrator::reconcile_project_state(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&worktree_manager,
	)
	.expect("project reconciliation should succeed");

	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"orphaned active worktree must stay available for operator recovery"
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"stalled"
	);
	assert_eq!(
		tracker.label_additions.borrow().as_slice(),
		[(issue.id.clone(), vec![String::from("label-needs-attention")])]
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		[(issue.id.clone(), vec![String::from("label-active")])]
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("stalled_run_detected")
			&& comment.contains("needs attention")
			&& comment.contains("run-orphaned-active")
	}));
}

#[test]
fn project_reconciliation_marks_orphaned_attention_worktree_run_stalled_without_tracker_writes() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("Todo", &["decodex:needs-attention"]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let run_id = "run-attention-orphan";
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	state::write_run_activity_marker_for_process(&worktree_path, run_id, 1, u32::MAX)
		.expect("stopped process marker should write");
	orchestrator::reconcile_project_state(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&worktree_manager,
	)
	.expect("project reconciliation should succeed");

	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"stalled"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"attention worktree must stay available for operator recovery"
	);
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
	assert!(tracker.comments.borrow().is_empty());
}

#[test]
fn stalled_run_reconciliation_preserves_retry_budget_marker_from_retained_worktree() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let issue = sample_issue("In Progress", &[]);
	let run_id = "run-stalled-budget";
	let worktree_path = config.worktree_root().join("PUB-101");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_retry_budget_attempt_count(&worktree_path, "older-run", 2, 2)
		.expect("retry budget marker should write");

	state_store
		.record_run_attempt(run_id, &issue.id, 3, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, run_id, "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let action = ActiveRunReconciliation {
		issue: issue.clone(),
		run_attempt: state_store
			.run_attempt(run_id)
			.expect("run attempt query should succeed")
			.expect("run attempt should exist"),
		worktree_mapping: state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree query should succeed"),
		disposition: ActiveRunDisposition::Stalled {
			idle_for: ACTIVE_RUN_IDLE_TIMEOUT + Duration::from_secs(1),
		},
		workflow: workflow.clone(),
	};

	orchestrator::apply_active_run_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		vec![action],
	)
	.expect("reconciliation should succeed");

	assert_eq!(
		state::read_run_retry_budget_attempt_count(&worktree_path)
			.expect("retry budget marker should read")
			.expect("retry budget marker should remain present"),
		2,
		"stalled reconciliation should preserve the retained retry-budget base"
	);
}
