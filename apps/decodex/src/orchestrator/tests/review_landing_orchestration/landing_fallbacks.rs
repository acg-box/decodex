mod admin_merge_without_external_review;
mod checkpoint_before_admin_merge;
mod checkpoint_failure_after_budget;
mod non_clean_landing_agent_fallback;
mod non_github_non_clean_agent_fallback;
mod repair_checkpoint_after_findings;
mod runtime_review_after_external_pass;
mod skips_review_while_gates_pending;
mod strict_failure_no_external_restart;
mod terminal_review_status_attention;
mod unknown_review_status_attention;
mod waits_for_checkpoint;

use std::{
	cell::{Cell, RefCell},
	fs,
	path::Path,
};

use crate::{
	agent::ReviewExecutionMode,
	orchestrator::{
		self, ReviewLevel, StateStore,
		runtime_standard_review::{RuntimeStandardReviewRunRequest, RuntimeStandardReviewRunner},
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker, review_landing_status_support,
		},
	},
	prelude::{Result, eyre},
	state::ReviewPolicyCheckpointInput,
};

struct FailingRuntimeReviewRunner {
	calls: Cell<usize>,
}

impl FailingRuntimeReviewRunner {
	fn new() -> Self {
		Self { calls: Cell::new(0) }
	}
}

struct CleanRuntimeReviewRunner {
	calls: Cell<usize>,
	last_review_mode: Cell<Option<ReviewExecutionMode>>,
	last_head_sha: RefCell<Option<String>>,
}

impl CleanRuntimeReviewRunner {
	fn new() -> Self {
		Self {
			calls: Cell::new(0),
			last_review_mode: Cell::new(None),
			last_head_sha: RefCell::new(None),
		}
	}
}

struct TerminalRuntimeReviewRunner {
	status: &'static str,
}

impl TerminalRuntimeReviewRunner {
	fn new(status: &'static str) -> Self {
		Self { status }
	}
}

impl RuntimeStandardReviewRunner for TerminalRuntimeReviewRunner {
	fn run_runtime_standard_review(
		&self,
		_request: RuntimeStandardReviewRunRequest<'_>,
	) -> Result<String> {
		let route =
			if self.status == "blocked" { "landing_blocker" } else { "architecture_signal" };

		Ok(serde_json::json!({
			"status": self.status,
			"checks": {
				"intended_behavior": "The retained PR head was inspected.",
				"regression_risk": "Runtime review cannot clear the lane automatically.",
				"missing_tests": "Manual follow-up is required before adding more tests.",
				"docs_config_drift": "Manual follow-up is required before landing.",
				"migration_fallout": "Manual follow-up is required before landing.",
				"operator_facing_fallout": "Manual follow-up is required before landing.",
				"loop_decision_contract": "The runtime must fail closed for operator attention."
			},
			"evidence": [
				"Independent runtime review found a terminal Standard review condition."
			],
			"accepted_findings": [],
			"rejected_findings": [],
			"finding_routes": [{
				"route": route,
				"severity": "high",
				"risk_tier": "high",
				"summary": "Standard review cannot be cleared automatically.",
				"evidence": ["The condition requires manual review before landing."],
				"resolver": "human",
				"next_action": "Inspect the retained lane and resolve the terminal review condition."
			}],
			"review_cost_control": {
				"review_class": "full_current_head_review",
				"risk_class": "high",
				"changed_surface_count": 1,
				"changed_surface_summary": ["retained.txt"],
				"high_risk_surfaces": ["retained.txt"],
				"current_head_evidence": true,
				"validation_backed": true,
				"validation_current": true,
				"evidence_sufficient": true,
				"reviewer_judgment": "Manual attention is required before landing.",
				"fallback_reason": "runtime_terminal_status"
			}
		})
		.to_string())
	}
}

impl RuntimeStandardReviewRunner for CleanRuntimeReviewRunner {
	fn run_runtime_standard_review(
		&self,
		request: RuntimeStandardReviewRunRequest<'_>,
	) -> Result<String> {
		self.calls.set(self.calls.get() + 1);
		self.last_review_mode.set(Some(request.review_mode()));
		*self.last_head_sha.borrow_mut() = Some(request.head_sha().to_owned());

		Ok(serde_json::json!({
			"status": "clean",
			"checks": {
				"intended_behavior": "The retained PR head still satisfies the issue objective.",
				"regression_risk": "No current-head regression was found.",
				"missing_tests": "No additional required tests were identified.",
				"docs_config_drift": "No OpenWiki or config drift was found.",
				"migration_fallout": "No migration fallout was found.",
				"operator_facing_fallout": "No operator-facing fallout was found.",
				"loop_decision_contract": "The lifecycle can proceed after the runtime-owned checkpoint."
			},
			"evidence": [
				"Independent reviewer inspected the current retained HEAD and PR lineage."
			],
			"review_cost_control": {
				"review_class": "full_current_head_review",
				"risk_class": "localized",
				"changed_surface_count": 1,
				"changed_surface_summary": ["retained.txt"],
				"high_risk_surfaces": [],
				"current_head_evidence": true,
				"validation_backed": true,
				"validation_current": true,
				"evidence_sufficient": true,
				"reviewer_judgment": "The current retained HEAD has enough evidence for landing.",
				"fallback_reason": "runtime_full_review"
			}
		})
		.to_string())
	}
}

impl RuntimeStandardReviewRunner for FailingRuntimeReviewRunner {
	fn run_runtime_standard_review(
		&self,
		_request: RuntimeStandardReviewRunRequest<'_>,
	) -> Result<String> {
		self.calls.set(self.calls.get() + 1);

		eyre::bail!("fake runtime review runner keeps checkpoint pending")
	}
}

fn configure_cached_github_origin(repo_root: &Path) {
	tests::git_status_success(
		repo_root,
		&["remote", "add", "origin", "https://github.com/hack-ink/decodex.git"],
	);
	tests::git_status_success(repo_root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
	tests::git_status_success(
		repo_root,
		&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"],
	);
}

fn assert_admin_merge_without_external_review(review_level: ReviewLevel) {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&config,
			"PATH",
			&gh_command_path,
		),
		review_level,
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/2","change":"current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
	let landed_merge_subject = r#"{"schema":"decodex/commit/2","change":"Land current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
	let head_oid =
		tests::commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);

	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should succeed");

	let gh_invocation = fs::read_to_string(&invocation_log_path)
		.expect("fake gh invocation log should read")
		.lines()
		.map(str::to_owned)
		.collect::<Vec<_>>();

	assert_eq!(
		gh_invocation,
		vec![
			String::from("pr"),
			String::from("merge"),
			String::from("--admin"),
			String::from("--merge"),
			String::from("--match-head-commit"),
			head_oid,
			String::from("--subject"),
			String::from(landed_merge_subject),
			String::from("--body"),
			String::new(),
			String::from(pr_url),
			String::from("pr"),
			String::from("view"),
			String::from(pr_url),
			String::from("--json"),
			String::from("state,headRefOid,mergeCommit"),
		]
	);
	let lifecycle = state_store
		.review_lifecycle_record(config.service_id(), &issue.id, "main")
		.expect("lifecycle record should read")
		.expect("landing authority should record");
	assert_eq!(lifecycle.next_state(), "landed");
	assert_eq!(lifecycle.merge_commit(), Some("cafebabe"));
	assert!(
		tracker.comments.borrow().is_empty(),
		"runtime orchestration state should stay in StateStore rather than Linear comments",
	);
}

fn assert_waits_for_checkpoint() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&config,
			"PATH",
			&gh_command_path,
		),
		ReviewLevel::Standard,
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/2","change":"current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
	let head_oid =
		tests::commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);
	configure_cached_github_origin(&repo_root);

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);

	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	let runtime_review_runner = FailingRuntimeReviewRunner::new();
	orchestrator::retained_review_orchestration::reconcile_post_review_orchestration_with_runners(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
		&runtime_review_runner,
	)
	.expect("post-review orchestration should succeed");

	let marker = tests::persisted_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "request_pending");
	assert_eq!(marker.request_retry_count(), 1);
	assert!(
		!invocation_log_path.exists(),
		"standard retained landing must wait for runtime-owned review evidence",
	);
	assert_eq!(runtime_review_runner.calls.get(), 1);
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
}

fn assert_checkpoint_failure_after_budget() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&config,
			"PATH",
			&gh_command_path,
		),
		ReviewLevel::Standard,
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/2","change":"current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
	let head_oid =
		tests::commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);
	configure_cached_github_origin(&repo_root);

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");
	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);

	let review_state = || {
		tests::sample_pull_request_review_state(
			pr_url,
			"main",
			&head_oid,
			Some("APPROVED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		)
	};
	let runtime_review_runner = FailingRuntimeReviewRunner::new();

	for _ in 0..3 {
		orchestrator::retained_review_orchestration::reconcile_post_review_orchestration_with_runners(
			&tracker,
			&config,
			&workflow,
			&state_store,
			&FakePullRequestReviewStateInspector::new(vec![Ok(review_state())]),
			&runtime_review_runner,
		)
		.expect("post-review orchestration should tolerate bounded runtime review failures");
	}

	let marker = tests::persisted_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);
	let comments = tracker.comments.borrow();

	assert_eq!(marker.phase(), "manual_attention_required");
	assert_eq!(marker.request_retry_count(), 3);
	assert_eq!(runtime_review_runner.calls.get(), 3);
	assert!(
		comments
			.iter()
			.any(|comment| comment.contains("runtime_standard_review_checkpoint_producer_failed")),
		"producer failures past the budget must become durable manual attention"
	);
	assert!(
		!invocation_log_path.exists(),
		"bounded producer failures must not fall through to admin merge",
	);
	assert_standard_review_attention_lifecycle_authority(
		&state_store,
		config.service_id(),
		&issue.id,
		"main",
	);
}

fn assert_terminal_review_status_attention(status: &'static str, expected_reason: &str) {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&config,
			"PATH",
			&gh_command_path,
		),
		ReviewLevel::Standard,
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/2","change":"current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
	let head_oid =
		tests::commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);
	configure_cached_github_origin(&repo_root);

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");
	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);

	let review_state = || {
		tests::sample_pull_request_review_state(
			pr_url,
			"main",
			&head_oid,
			Some("APPROVED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		)
	};
	let runtime_review_runner = TerminalRuntimeReviewRunner::new(status);

	orchestrator::retained_review_orchestration::reconcile_post_review_orchestration_with_runners(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state())]),
		&runtime_review_runner,
	)
	.expect("first pass should record terminal runtime review checkpoint");

	let checkpoint = state_store
		.review_checkpoint_artifact(crate::state::ReviewCheckpointArtifactLookup {
			project_id: config.service_id(),
			issue_id: &issue.id,
			phase: "handoff",
			review_level: "standard",
			head_sha: &head_oid,
		})
		.expect("checkpoint lookup should succeed")
		.expect("terminal runtime review checkpoint should persist");
	assert_eq!(checkpoint.status(), status);
	assert!(tracker.comments.borrow().is_empty());

	orchestrator::retained_review_orchestration::reconcile_post_review_orchestration_with_runners(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state())]),
		&runtime_review_runner,
	)
	.expect("second pass should route terminal checkpoint to manual attention");

	assert!(
		tracker.comments.borrow().iter().any(|comment| comment.contains(expected_reason)),
		"terminal Standard review status must become durable manual attention"
	);
	assert!(
		!invocation_log_path.exists(),
		"terminal Standard review status must not fall through to admin merge",
	);
	assert_standard_review_attention_lifecycle_authority(
		&state_store,
		config.service_id(),
		&issue.id,
		"main",
	);
}

fn assert_unknown_review_status_attention() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&config,
			"PATH",
			&gh_command_path,
		),
		ReviewLevel::Standard,
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/2","change":"current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
	let head_oid =
		tests::commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);
	configure_cached_github_origin(&repo_root);

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");
	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: &issue.id,
			run_id: "run-1:runtime-review:repair:unknown",
			attempt_number: 1,
			phase: "repair",
			review_level: "standard",
			status: "unexpected_runtime_status",
			head_sha: &head_oid,
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("unknown checkpoint should persist");

	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);
	let runtime_review_runner = CleanRuntimeReviewRunner::new();

	orchestrator::retained_review_orchestration::reconcile_post_review_orchestration_with_runners(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
		&runtime_review_runner,
	)
	.expect("unknown runtime review status should route to manual attention");

	assert_eq!(runtime_review_runner.calls.get(), 0);
	assert!(
		tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("runtime_standard_review_unknown_checkpoint_status")),
		"unknown Standard review checkpoint status must become durable manual attention"
	);
	assert!(
		!invocation_log_path.exists(),
		"unknown Standard review status must not fall through to admin merge",
	);
	assert_standard_review_attention_lifecycle_authority(
		&state_store,
		config.service_id(),
		&issue.id,
		"main",
	);
}

fn assert_standard_review_attention_lifecycle_authority(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	branch_name: &str,
) {
	let lifecycle = state_store
		.review_lifecycle_record(project_id, issue_id, branch_name)
		.expect("lifecycle authority lookup should succeed")
		.expect("Standard review attention should persist lifecycle authority");

	assert_eq!(lifecycle.next_state(), "manual_attention_required");
	assert_eq!(lifecycle.transition(), "manual_attention_required");
	assert_eq!(lifecycle.next_action(), "request_manual_attention");
}

fn assert_skips_review_while_gates_pending() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&config,
			"PATH",
			&gh_command_path,
		),
		ReviewLevel::Standard,
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/2","change":"current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
	let head_oid =
		tests::commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);
	configure_cached_github_origin(&repo_root);

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");
	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);

	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("PENDING"),
		0,
	);
	let runtime_review_runner = CleanRuntimeReviewRunner::new();

	orchestrator::retained_review_orchestration::reconcile_post_review_orchestration_with_runners(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
		&runtime_review_runner,
	)
	.expect("pending landing gates should not invoke runtime review");

	assert_eq!(runtime_review_runner.calls.get(), 0);
	assert!(!invocation_log_path.exists(), "pending checks must not land",);
	assert!(tracker.comments.borrow().is_empty());
}

fn assert_runtime_review_after_external_pass() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&config,
			"PATH",
			&gh_command_path,
		),
		ReviewLevel::Strict,
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/2","change":"current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
	let head_oid =
		tests::commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);
	configure_cached_github_origin(&repo_root);

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");
	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);
	tests::seed_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_transition_fixture(
			"main",
			pr_url,
			&head_oid,
			"waiting_for_result",
			1,
		),
	);

	let review_state = || {
		let mut review_state = tests::sample_pull_request_review_state(
			pr_url,
			"main",
			&head_oid,
			Some("APPROVED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		);
		tests::add_external_review_ack(&mut review_state);
		tests::add_external_review_pass(&mut review_state);
		review_state
	};
	let runtime_review_runner = CleanRuntimeReviewRunner::new();

	orchestrator::retained_review_orchestration::reconcile_post_review_orchestration_with_runners(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state())]),
		&runtime_review_runner,
	)
	.expect("strict pass should record runtime review before landing");

	assert_eq!(runtime_review_runner.calls.get(), 1);
	assert_eq!(runtime_review_runner.last_review_mode.get(), Some(ReviewExecutionMode::Repair));
	assert!(
		!invocation_log_path.exists(),
		"strict review must not land in the same tick that records runtime review evidence",
	);

	orchestrator::retained_review_orchestration::reconcile_post_review_orchestration_with_runners(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state())]),
		&runtime_review_runner,
	)
	.expect("strict pass should reuse runtime review checkpoint and land");

	assert_eq!(
		runtime_review_runner.calls.get(),
		1,
		"strict review must reuse the current-head runtime checkpoint"
	);
	assert!(
		invocation_log_path.exists(),
		"strict review should land after GitHub pass plus clean runtime checkpoint",
	);
}

fn assert_strict_failure_no_external_restart() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&config,
			"PATH",
			&gh_command_path,
		),
		ReviewLevel::Strict,
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/2","change":"current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
	let head_oid =
		tests::commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);
	configure_cached_github_origin(&repo_root);

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");
	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);
	tests::seed_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_transition_fixture(
			"main",
			pr_url,
			&head_oid,
			"waiting_for_result",
			1,
		),
	);

	let review_state = || {
		let mut review_state = tests::sample_pull_request_review_state(
			pr_url,
			"main",
			&head_oid,
			Some("APPROVED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		);
		tests::add_external_review_ack(&mut review_state);
		tests::add_external_review_pass(&mut review_state);
		review_state
	};
	let runtime_review_runner = FailingRuntimeReviewRunner::new();

	for _ in 0..3 {
		orchestrator::retained_review_orchestration::reconcile_post_review_orchestration_with_runners(
			&tracker,
			&config,
			&workflow,
			&state_store,
			&FakePullRequestReviewStateInspector::new(vec![Ok(review_state())]),
			&runtime_review_runner,
		)
		.expect("strict runtime review failures should remain in external result phase");
	}

	let marker = tests::persisted_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "manual_attention_required");
	assert_eq!(marker.request_retry_count(), 3);
	assert_eq!(runtime_review_runner.calls.get(), 3);
	assert!(
		tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("runtime_standard_review_checkpoint_producer_failed")),
		"strict runtime review failures past the budget must become durable manual attention"
	);
	assert!(
		!invocation_log_path.exists(),
		"strict runtime review failures must not fall through to admin merge",
	);
}

fn assert_checkpoint_before_admin_merge() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&config,
			"PATH",
			&gh_command_path,
		),
		ReviewLevel::Standard,
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/2","change":"current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
	let head_oid =
		tests::commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);
	configure_cached_github_origin(&repo_root);

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &head_oid),
	);

	let review_state = || {
		tests::sample_pull_request_review_state(
			pr_url,
			"main",
			&head_oid,
			Some("APPROVED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		)
	};
	let runtime_review_runner = CleanRuntimeReviewRunner::new();

	orchestrator::retained_review_orchestration::reconcile_post_review_orchestration_with_runners(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state())]),
		&runtime_review_runner,
	)
	.expect("first post-review pass should record runtime review checkpoint");

	assert_eq!(runtime_review_runner.calls.get(), 1);
	assert_eq!(runtime_review_runner.last_review_mode.get(), Some(ReviewExecutionMode::Handoff),);
	assert_eq!(runtime_review_runner.last_head_sha.borrow().as_deref(), Some(head_oid.as_str()));
	assert!(
		!invocation_log_path.exists(),
		"first pass records review evidence but does not land in the same tick",
	);

	let checkpoint = state_store
		.review_checkpoint_artifact(crate::state::ReviewCheckpointArtifactLookup {
			project_id: config.service_id(),
			issue_id: &issue.id,
			phase: "handoff",
			review_level: "standard",
			head_sha: &head_oid,
		})
		.expect("checkpoint lookup should succeed")
		.expect("runtime-owned review checkpoint should persist");
	assert_eq!(checkpoint.status(), "clean");

	orchestrator::retained_review_orchestration::reconcile_post_review_orchestration_with_runners(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state())]),
		&runtime_review_runner,
	)
	.expect("second post-review pass should reuse checkpoint and land");

	assert_eq!(
		runtime_review_runner.calls.get(),
		1,
		"existing current-head checkpoint must prevent duplicate runtime review"
	);
	let lifecycle = state_store
		.review_lifecycle_record(config.service_id(), &issue.id, "main")
		.expect("lifecycle record should read")
		.expect("landing authority should record");

	assert_eq!(lifecycle.next_state(), "landed");
	assert!(
		invocation_log_path.exists(),
		"clean runtime review checkpoint should allow admin merge on the next pass",
	);
}

fn assert_repair_checkpoint_after_findings() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&config,
			"PATH",
			&gh_command_path,
		),
		ReviewLevel::Standard,
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = review_landing_status_support::post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let old_head_oid = tests::commit_worktree_change(
		&repo_root,
		"retained.txt",
		"findings\n",
		r#"{"schema":"decodex/commit/2","change":"retained handoff with findings","authority":"PUB-101","impact":"compatible"}"#,
	);
	let repaired_head_oid = tests::commit_worktree_change(
		&repo_root,
		"retained.txt",
		"repaired\n",
		r#"{"schema":"decodex/commit/2","change":"repair retained findings","authority":"PUB-101","impact":"compatible"}"#,
	);
	configure_cached_github_origin(&repo_root);

	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: &issue.id,
			run_id: "run-1:runtime-review:handoff:old",
			attempt_number: 1,
			phase: "handoff",
			review_level: "standard",
			status: "findings",
			head_sha: &old_head_oid,
			nonclean_rounds: 1,
			details_json: r#"{"finding_policy":{"schema_version":"decodex/review-finding-policy/1","phase":"handoff","status":"findings","head_sha":"old","nonclean_rounds":1,"active_fingerprints":[],"findings":[],"stop_fingerprint":null}}"#,
		})
		.expect("prior non-clean checkpoint should persist");
	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");
	tests::seed_review_lifecycle_handoff_fixture_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_lifecycle_handoff_fixture("main", pr_url, &repaired_head_oid),
	);

	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		"main",
		&repaired_head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);
	let runtime_review_runner = CleanRuntimeReviewRunner::new();

	orchestrator::retained_review_orchestration::reconcile_post_review_orchestration_with_runners(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
		&runtime_review_runner,
	)
	.expect("post-findings pass should record runtime repair checkpoint");

	assert_eq!(runtime_review_runner.calls.get(), 1);
	assert_eq!(runtime_review_runner.last_review_mode.get(), Some(ReviewExecutionMode::Repair),);
	assert_eq!(
		runtime_review_runner.last_head_sha.borrow().as_deref(),
		Some(repaired_head_oid.as_str()),
	);
	let checkpoint = state_store
		.review_checkpoint_artifact(crate::state::ReviewCheckpointArtifactLookup {
			project_id: config.service_id(),
			issue_id: &issue.id,
			phase: "repair",
			review_level: "standard",
			head_sha: &repaired_head_oid,
		})
		.expect("repair checkpoint lookup should succeed")
		.expect("runtime-owned repair checkpoint should persist");

	assert_eq!(checkpoint.status(), "clean");
	assert!(
		!invocation_log_path.exists(),
		"repair checkpoint producer must not land in the same tick",
	);
}
