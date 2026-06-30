#[cfg(unix)] use std::os::unix::fs::PermissionsExt;

struct CloseoutIdentityFixture {
	_temp_dir: TempDir,
	_path_guard: TestEnvVarGuard,
	config: ServiceConfig,
	workflow: WorkflowDocument,
	tracker: FakeTracker,
	state_store: StateStore,
	issue: TrackerIssue,
	worktree: WorktreeSpec,
	pr_url: String,
	head_oid: String,
	completed_run_id: String,
}

fn sample_active_issue(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	sample_issue(state_name, &[active_label.as_str()])
}

fn sample_active_issue_without_needs_attention_team_label(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	sample_issue_without_needs_attention_team_label(state_name, &[active_label.as_str()])
}

fn install_fake_open_pr_gh_response(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
) -> TestEnvVarGuard {
	let fake_gh_dir = temp_dir.path().join("fake-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let fake_gh_response = serde_json::json!({
		"data": {
			"repository": {
				"mergeCommitAllowed": true,
				"pullRequest": {
					"url": pr_url,
					"state": "OPEN",
					"isDraft": false,
					"reviewDecision": "APPROVED",
					"baseRefName": "main",
					"mergeable": "MERGEABLE",
					"mergeStateStatus": "CLEAN",
					"headRefName": worktree.branch_name.clone(),
					"headRefOid": head_oid,
					"headRepository": { "name": "decodex" },
					"headRepositoryOwner": { "login": "hack-ink" },
					"reactionGroups": [],
					"comments": {
						"nodes": [],
						"pageInfo": { "hasNextPage": false, "endCursor": null }
					},
					"reviews": { "nodes": [] },
					"reviewRequests": { "totalCount": 0 },
					"reviewThreads": {
						"nodes": [],
						"pageInfo": { "hasNextPage": false, "endCursor": null }
					},
					"commits": {
						"nodes": [
							{ "commit": { "statusCheckRollup": { "state": "SUCCESS" } } }
						]
					}
				}
			}
		}
	})
	.to_string();

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(&fake_gh_path, format!("#!/bin/sh\nprintf '%s' '{fake_gh_response}'\n"))
		.expect("fake gh script should write");

	let mut permissions =
		fs::metadata(&fake_gh_path).expect("fake gh metadata should read").permissions();

	#[cfg(unix)]
	PermissionsExt::set_mode(&mut permissions, 0o755);
	fs::set_permissions(&fake_gh_path, permissions)
		.expect("fake gh script should become executable");

	let path_env = env::var("PATH").unwrap_or_default();

	TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_gh_dir.display()))
}

fn install_fake_conflicting_pr_gh_response(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
) -> TestEnvVarGuard {
	let fake_gh_dir = temp_dir.path().join("fake-conflict-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let fake_gh_response = serde_json::json!({
		"data": {
			"repository": {
				"mergeCommitAllowed": true,
				"pullRequest": {
					"url": pr_url,
					"state": "OPEN",
					"isDraft": false,
					"reviewDecision": "REVIEW_REQUIRED",
					"baseRefName": "main",
					"mergeable": "CONFLICTING",
					"mergeStateStatus": "DIRTY",
					"headRefName": worktree.branch_name.clone(),
					"headRefOid": head_oid,
					"headRepository": { "name": "decodex" },
					"headRepositoryOwner": { "login": "hack-ink" },
					"reactionGroups": [],
					"comments": {
						"nodes": [],
						"pageInfo": { "hasNextPage": false, "endCursor": null }
					},
					"reviews": { "nodes": [] },
					"reviewRequests": { "totalCount": 0 },
					"reviewThreads": {
						"nodes": [],
						"pageInfo": { "hasNextPage": false, "endCursor": null }
					},
					"commits": {
						"nodes": [
							{ "commit": { "statusCheckRollup": { "state": "SUCCESS" } } }
						]
					}
				}
			}
		}
	})
	.to_string();

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(&fake_gh_path, format!("#!/bin/sh\nprintf '%s' '{fake_gh_response}'\n"))
		.expect("fake gh script should write");

	let mut permissions =
		fs::metadata(&fake_gh_path).expect("fake gh metadata should read").permissions();

	#[cfg(unix)]
	PermissionsExt::set_mode(&mut permissions, 0o755);
	fs::set_permissions(&fake_gh_path, permissions)
		.expect("fake gh script should become executable");

	let path_env = env::var("PATH").unwrap_or_default();

	TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_gh_dir.display()))
}

fn install_fake_ready_to_land_admin_merge_gh_response(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
) -> (TestEnvVarGuard, PathBuf) {
	let fake_gh_dir = temp_dir.path().join("fake-ready-to-land-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let invocation_log_path = temp_dir.path().join("ready-to-land-gh-invocation.log");
	let fake_graphql_response = serde_json::json!({
		"data": {
			"repository": {
				"mergeCommitAllowed": true,
				"pullRequest": {
					"url": pr_url,
					"state": "OPEN",
					"isDraft": false,
					"reviewDecision": "APPROVED",
					"mergeable": "MERGEABLE",
					"mergeStateStatus": "CLEAN",
					"headRefName": worktree.branch_name.clone(),
					"headRefOid": head_oid,
					"headRepository": { "name": "decodex" },
					"headRepositoryOwner": { "login": "hack-ink" },
					"reactionGroups": [],
					"comments": {
						"nodes": [],
						"pageInfo": { "hasNextPage": false, "endCursor": null }
					},
					"reviews": { "nodes": [] },
					"reviewRequests": { "totalCount": 0 },
					"reviewThreads": {
						"nodes": [],
						"pageInfo": { "hasNextPage": false, "endCursor": null }
					},
					"commits": {
						"nodes": [
							{ "commit": { "statusCheckRollup": { "state": "SUCCESS" } } }
						]
					}
				}
			}
		}
	})
	.to_string();
	let fake_pr_view_response = serde_json::json!({
		"state": "MERGED",
		"headRefOid": head_oid,
		"mergeCommit": { "oid": "cafebabe" },
	})
	.to_string();

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(
		&fake_gh_path,
		format!(
			"#!/bin/sh\n\
if [ \"$1\" = \"api\" ] && [ \"$2\" = \"graphql\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"merge\" ]; then\n\
  printf '%s\\n' \"$@\" >> '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
			fake_graphql_response,
			invocation_log_path.display(),
			fake_pr_view_response
		),
	)
	.expect("fake gh script should write");

	let mut permissions =
		fs::metadata(&fake_gh_path).expect("fake gh metadata should read").permissions();

	#[cfg(unix)]
	PermissionsExt::set_mode(&mut permissions, 0o755);
	fs::set_permissions(&fake_gh_path, permissions)
		.expect("fake gh script should become executable");

	let path_env = env::var("PATH").unwrap_or_default();

	(
		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_gh_dir.display())),
		invocation_log_path,
	)
}

fn install_fake_merged_pr_gh_response(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
) -> TestEnvVarGuard {
	install_fake_merged_pr_gh_response_with_base_ref_and_delete_exit_code(
		temp_dir, worktree, pr_url, head_oid, "main", 0,
	)
}

fn install_fake_merged_pr_gh_response_with_base_ref(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
	base_ref_name: &str,
) -> TestEnvVarGuard {
	install_fake_merged_pr_gh_response_with_base_ref_and_delete_exit_code(
		temp_dir,
		worktree,
		pr_url,
		head_oid,
		base_ref_name,
		0,
	)
}

fn install_fake_merged_pr_gh_response_with_delete_exit_code(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
	delete_exit_code: i32,
) -> TestEnvVarGuard {
	install_fake_merged_pr_gh_response_with_base_ref_and_delete_exit_code(
		temp_dir,
		worktree,
		pr_url,
		head_oid,
		"main",
		delete_exit_code,
	)
}

fn install_fake_merged_pr_gh_response_with_base_ref_and_delete_exit_code(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
	base_ref_name: &str,
	delete_exit_code: i32,
) -> TestEnvVarGuard {
	let fake_gh_dir = temp_dir.path().join("fake-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let fake_pr_view_response = serde_json::json!({
		"state": "MERGED",
		"headRefOid": head_oid,
		"mergeCommit": { "oid": "cafebabe" }
	})
	.to_string();
	let fake_gh_response = serde_json::json!({
		"data": {
			"repository": {
				"mergeCommitAllowed": true,
				"pullRequest": {
					"url": pr_url,
					"state": "MERGED",
					"isDraft": false,
					"reviewDecision": "APPROVED",
					"baseRefName": base_ref_name,
					"mergeable": "MERGEABLE",
					"mergeStateStatus": "CLEAN",
					"headRefName": worktree.branch_name.clone(),
					"headRefOid": head_oid,
					"headRepository": { "name": "decodex" },
					"headRepositoryOwner": { "login": "hack-ink" },
					"reactionGroups": [],
					"comments": {
						"nodes": [],
						"pageInfo": { "hasNextPage": false, "endCursor": null }
					},
					"reviews": { "nodes": [] },
					"reviewRequests": { "totalCount": 0 },
					"reviewThreads": {
						"nodes": [],
						"pageInfo": { "hasNextPage": false, "endCursor": null }
					},
					"commits": {
						"nodes": [
							{ "commit": { "statusCheckRollup": { "state": "SUCCESS" } } }
						]
					}
				}
			}
		}
	})
	.to_string();

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(
		&fake_gh_path,
		format!(
			"#!/bin/sh\n\
if [ \"$1\" = \"api\" ] && [ \"$2\" = \"graphql\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"api\" ] && [ \"$2\" = \"--method\" ] && [ \"$3\" = \"DELETE\" ]; then\n\
  if [ {delete_exit_code} -eq 0 ]; then\n\
    exit 0\n\
  fi\n\
  echo 'delete denied by fake gh' >&2\n\
  exit {delete_exit_code}\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
			fake_gh_response, fake_pr_view_response
		),
	)
	.expect("fake gh script should write");

	let mut permissions =
		fs::metadata(&fake_gh_path).expect("fake gh metadata should read").permissions();

	#[cfg(unix)]
	PermissionsExt::set_mode(&mut permissions, 0o755);
	fs::set_permissions(&fake_gh_path, permissions)
		.expect("fake gh script should become executable");

	let path_env = env::var("PATH").unwrap_or_default();

	TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_gh_dir.display()))
}

fn install_fake_closeout_gh_responses(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
) -> TestEnvVarGuard {
	install_fake_closeout_gh_responses_with_state(temp_dir, worktree, pr_url, head_oid, "MERGED")
}

fn install_fake_closeout_gh_responses_with_state(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
	pr_state: &str,
) -> TestEnvVarGuard {
	install_fake_closeout_gh_responses_with_states(
		temp_dir, worktree, pr_url, head_oid, pr_state, pr_state,
	)
}

fn install_fake_closeout_gh_responses_with_states(
	temp_dir: &TempDir,
	worktree: &WorktreeSpec,
	pr_url: &str,
	head_oid: &str,
	pr_view_state: &str,
	graphql_state: &str,
) -> TestEnvVarGuard {
	let fake_gh_dir = temp_dir.path().join("fake-closeout-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let fake_pr_view_response = serde_json::json!({
		"url": pr_url,
		"state": pr_view_state,
		"isDraft": false,
		"baseRefName": "main",
		"headRefName": worktree.branch_name.clone(),
		"headRefOid": head_oid,
		"headRepository": { "name": "decodex" },
		"headRepositoryOwner": { "login": "hack-ink" }
	})
	.to_string();
	let fake_graphql_response = serde_json::json!({
		"data": {
			"repository": {
				"mergeCommitAllowed": true,
				"pullRequest": {
					"url": pr_url,
					"state": graphql_state,
					"isDraft": false,
					"reviewDecision": "APPROVED",
					"baseRefName": "main",
					"mergeable": "MERGEABLE",
					"mergeStateStatus": "CLEAN",
					"headRefName": worktree.branch_name.clone(),
					"headRefOid": head_oid,
					"headRepository": { "name": "decodex" },
					"headRepositoryOwner": { "login": "hack-ink" },
					"reactionGroups": [],
					"comments": {
						"nodes": [],
						"pageInfo": { "hasNextPage": false, "endCursor": null }
					},
					"reviews": { "nodes": [] },
					"reviewRequests": { "totalCount": 0 },
					"reviewThreads": {
						"nodes": [],
						"pageInfo": { "hasNextPage": false, "endCursor": null }
					},
					"commits": {
						"nodes": [
							{ "commit": { "statusCheckRollup": { "state": "SUCCESS" } } }
						]
					}
				}
			}
		}
	})
	.to_string();

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(
		&fake_gh_path,
		format!(
			"#!/bin/sh\n\
if [ \"$1\" = \"--version\" ]; then\n\
  printf '%s\\n' 'gh version 2.0.0'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"api\" ] && [ \"$2\" = \"graphql\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"api\" ] && [ \"$2\" = \"--method\" ] && [ \"$3\" = \"DELETE\" ]; then\n\
  exit 0\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
			fake_pr_view_response, fake_graphql_response
		),
	)
	.expect("fake gh script should write");

	let mut permissions =
		fs::metadata(&fake_gh_path).expect("fake gh metadata should read").permissions();

	#[cfg(unix)]
	PermissionsExt::set_mode(&mut permissions, 0o755);
	fs::set_permissions(&fake_gh_path, permissions)
		.expect("fake gh script should become executable");

	let path_env = env::var("PATH").unwrap_or_default();

	TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_gh_dir.display()))
}

fn initialize_closeout_cleanup_origin(repo_root: &Path, remote_root: &Path) {
	git_status_success(
		remote_root.parent().expect("remote root should have parent"),
		&[
			"init",
			"--bare",
			"--initial-branch",
			"main",
			remote_root.to_str().expect("remote path should be utf-8"),
		],
	);
	git_status_success(
		repo_root,
		&["remote", "add", "origin", remote_root.to_string_lossy().as_ref()],
	);
	git_status_success(repo_root, &["push", "-u", "origin", "main"]);
}

fn route_origin_github_url_to_local_bare_repo(repo_root: &Path, remote_root: &Path) {
	let github_remote = "https://github.com/hack-ink/decodex.git";
	let local_remote = format!("file://{}", remote_root.display());

	git_status_success(
		repo_root,
		&["config", &format!("url.{local_remote}.insteadOf"), github_remote],
	);
	git_status_success(repo_root, &["remote", "set-url", "origin", github_remote]);
}

fn issue_with_completed_state(mut issue: TrackerIssue) -> TrackerIssue {
	if !issue.team.states.iter().any(|state| state.name == "Done") {
		issue
			.team
			.states
			.push(TrackerState { id: String::from("state-done"), name: String::from("Done") });
	}

	issue
}

fn sample_closeout_issue_run(
	issue: &TrackerIssue,
	worktree: &WorktreeSpec,
	run_id: &str,
) -> orchestrator::IssueRunPlan {
	orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("In Review"),
		worktree: WorktreeSpec {
			branch_name: worktree.branch_name.clone(),
			issue_identifier: issue.identifier.clone(),
			path: worktree.path.clone(),
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Closeout,
		attempt_number: 1,
		run_id: String::from(run_id),
		retry_budget_base: 0,
	}
}

fn closeout_identity_fixture() -> CloseoutIdentityFixture {
	let (temp_dir, base_config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(&base_config, "HOME");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = issue_with_completed_state(sample_issue("In Review", &[active_label.as_str()]));
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]; 8]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = String::from("https://github.com/hack-ink/decodex/pull/703");
	let _path_guard = install_fake_closeout_gh_responses(&temp_dir, &worktree, &pr_url, &head_oid);
	let remote_root =
		config.repo_root().parent().expect("repo root should have a parent").join("origin.git");
	let completed_run_id = String::from("pub-703-attempt-1-111");

	initialize_closeout_cleanup_origin(config.repo_root(), &remote_root);
	route_origin_github_url_to_local_bare_repo(config.repo_root(), &remote_root);

	assert!(
		crate::test_support::hermetic_git_command()
			.arg("-C")
			.arg(config.repo_root())
			.args(["push", "origin", &format!("HEAD:{}", worktree.branch_name)])
			.status()
			.expect("git push lane branch should run")
			.success()
	);

	state_store
		.upsert_review_handoff_marker(
			config.service_id(),
			&issue.id,
			&ReviewHandoffMarker::new(
				&completed_run_id,
				1,
				&worktree.branch_name,
				&pr_url,
				"main",
				&worktree.branch_name,
				&head_oid,
			),
		)
		.expect("review handoff marker should persist");
	state_store
		.record_run_attempt(&completed_run_id, &issue.id, 1, "succeeded")
		.expect("completed handoff attempt should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("retained closeout worktree should record");

	CloseoutIdentityFixture {
		_temp_dir: temp_dir,
		_path_guard,
		config,
		workflow,
		tracker,
		state_store,
		issue,
		worktree,
		pr_url,
		head_oid,
		completed_run_id,
	}
}

fn assert_closeout_lane_ready(fixture: &CloseoutIdentityFixture) {
	let mut merged_review_state = sample_pull_request_review_state(
		&fixture.pr_url,
		&fixture.worktree.branch_name,
		&fixture.head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	merged_review_state.state = String::from("MERGED");

	let lanes = orchestrator::build_post_review_lane_statuses(
		&fixture.tracker,
		&fixture.config,
		&fixture.workflow,
		&fixture.state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(merged_review_state)]),
	)
	.expect("post-review lane status should build");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "continue");
	assert_eq!(lanes[0].reason, "pull_request_merged_closeout_pending");
	assert!(
		orchestrator::issue_passes_closeout_dispatch_policy(
			&fixture.tracker,
			&fixture.issue,
			&fixture.config,
			&fixture.workflow,
			&fixture.state_store,
		)
		.expect("closeout policy should evaluate"),
		"closeout dispatch policy should accept the merged retained lane: {:?}",
		orchestrator::closeout_dispatch_block_reason(
			&fixture.tracker,
			&fixture.issue,
			&fixture.config,
			&fixture.workflow,
			&fixture.state_store,
		)
		.expect("closeout block reason should evaluate")
	);
}

fn assert_app_server_failure_requires_attention(
	error: Report,
	error_class: &str,
	next_action_fragment: &str,
) {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let issue_run = orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("app-server failure handling should succeed");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains(error_class)
			&& comment.contains(next_action_fragment)
			&& comment.contains("clear label `decodex:needs-attention`")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| { comment.contains("retryable_execution_failure") })
	);
}
