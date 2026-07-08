// Workflow reload, intake eligibility, prompting, and candidate selection.
mod fake_github;
mod fake_tracker;
mod git_helpers;
mod intake_candidate_selection;
mod intake_eligibility;
mod intake_prepare_issue_run;
mod intake_run_and_prompting;
mod intake_workflow_reload;
mod issue_builders;
mod project_layout;
mod recovery_closeout_cleanup;
mod recovery_closeout_dispatch;
mod recovery_closeout_identity;
mod recovery_reconciliation;
mod recovery_runtime_reentry;
mod recovery_terminal_failures;
mod recovery_terminal_support;
mod retry_scheduling;
mod retry_selection;
mod review_landing_classification_checks;
mod review_landing_classification_review;
mod review_landing_orchestration;
mod review_landing_review_state;
mod review_landing_status_markers;
mod review_landing_status_rows;
mod review_landing_status_support;
mod review_markers;
mod review_state;
mod runtime_failure;
mod runtime_loop_scenarios;
mod runtime_program_intake_dogfood;
mod runtime_program_reconciler;
mod runtime_repo_gate;
mod runtime_thread_archive;
// Operator status plus retained post-review review/landing behavior.
mod operator;

#[cfg(unix)] use std::os::unix::fs::PermissionsExt;
use std::{
	cell::RefCell,
	collections::{BTreeSet, HashMap},
	env, fs, iter,
	path::{Path, PathBuf},
	process,
	time::Duration,
};

use color_eyre::Report;
use serde_json::{self, Value};
use tempfile::TempDir;

use self::{
	fake_github::{
		install_fake_admin_merge_gh_response,
		install_fake_admin_merge_gh_response_with_merge_exit_code,
		install_fake_post_issue_comment_gh_response, rewrite_run_activity_marker_host_boot_id,
		rewrite_run_activity_marker_process_start_identity,
	},
	fake_tracker::FakeTracker,
	git_helpers::{
		add_origin_remote, checkout_new_branch, commit_worktree_change, git_output, git_remote_url,
		git_status_success, try_git_local_config_value,
	},
	issue_builders::{
		sample_blocker, sample_issue, sample_issue_with_project_slug_and_sort_fields,
		sample_issue_with_sort_fields, sample_issue_without_needs_attention_team_label,
	},
	project_layout::{
		load_service_config, profile_scoped_workflow_markdown, sample_service_config_toml,
		sample_service_config_toml_with_github_command_path, sample_workflow,
		sample_workflow_markdown, service_config_dir, service_config_path,
		service_config_toml_for_config, service_config_toml_for_config_with_github_command_path,
		service_config_with_github_token_env_var,
		service_config_with_github_token_env_var_and_command_path,
		service_config_with_review_level, service_workflow_path, temp_project_layout,
		temp_project_layout_with_max_turns, temp_project_layout_with_read_first,
		temp_project_layout_with_tracker_project_slug,
		temp_project_layout_with_tracker_project_slug_and_read_first,
		temp_project_layout_with_tracker_project_slug_max_turns_and_read_first,
		temp_project_layout_with_workflow_markdown, write_service_config,
	},
	review_markers::{
		persisted_review_lifecycle_handoff_fixture, persisted_review_lifecycle_transition_fixture,
		persisted_review_lifecycle_transition_fixture_for_path,
		sample_review_lifecycle_handoff_fixture, sample_review_lifecycle_record,
		sample_review_lifecycle_transition_fixture, seed_review_lifecycle_handoff_fixture,
		seed_review_lifecycle_handoff_fixture_for_path,
		seed_review_lifecycle_handoff_fixture_value, seed_review_lifecycle_transition_fixture,
		seed_review_lifecycle_transition_fixture_for_path, worktree_mapping_for_path,
	},
	review_state::{
		FakePullRequestReviewStateInspector, add_external_review_ack, add_external_review_findings,
		add_external_review_pass, add_external_review_pass_from_actor, add_external_review_summary,
		add_review_request_ack_from_actor, add_review_summary_from_actor,
		sample_pull_request_review_state, sample_pull_request_review_state_page,
		sample_pull_request_review_state_repository,
		sample_pull_request_review_state_with_pending_requests,
	},
	runtime_failure::loop_guardrail_issue_run,
};
use crate::{
	agent::{
		AppServerCapabilityPreflightFailure, AppServerDynamicToolFailure,
		AppServerHomePreflightFailure, AppServerPhaseGoalFailure, AppServerTransportFailure,
		AppServerTurnFailure, PhaseGoalKind, RUN_LEASE_IDLE_TIMEOUT, ReviewPolicyStopReason,
		ReviewPolicyStopRequested,
	},
	config::{ReviewLevel, ServiceConfig},
	github,
	orchestrator::{
		AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput, AuthorityBoundaryDisposition,
		AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface, ChildRunRef,
		EXTERNAL_REVIEW_ACTOR_LOGIN, EXTERNAL_REVIEW_PASS_PHRASE, EXTERNAL_REVIEW_REQUEST_BODY,
		IssueDispatchMode, IssueRunPlan, ManualAttentionRequested, PrepareIssueRunContext,
		PullRequestCommitConnection, PullRequestCommitNode, PullRequestCommitPayload,
		PullRequestIssueCommentConnection, PullRequestIssueCommentState, PullRequestPageInfo,
		PullRequestRepository, PullRequestRepositoryOwner, PullRequestReviewConnection,
		PullRequestReviewRequestConnection, PullRequestReviewState,
		PullRequestReviewStateInspector, PullRequestReviewStateNode,
		PullRequestReviewStateRepository, PullRequestReviewSummaryState,
		PullRequestReviewThreadConnection, PullRequestReviewThreadNode,
		PullRequestStatusCheckRollup, RetainedPartialProgress, RetainedReviewRepairPushFailed,
		RetryComment, ReviewLifecycleHandoffFixture, RunCompletionDisposition,
	},
	prelude::Result,
	state::{
		RUN_ACTIVITY_MARKER_FILE, RUN_OPERATION_RECONCILIATION, RUN_OPERATION_REPO_GATE,
		ReviewPolicyCheckpointInput, StateStore, WorktreeMapping,
	},
	test_support::TestEnvVarGuard,
	tracker::{
		IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBlocker, TrackerIssueCreate,
		TrackerLabel, TrackerState, TrackerTeam,
	},
	workflow::WorkflowDocument,
	worktree::{WorktreeManager, WorktreeSpec},
};

const TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID: i64 = 991;
const TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT: i64 = 1_763_600_000;
const TEST_EXTERNAL_REVIEW_AUTO_MERGE_ENABLED_AT: i64 = 1_763_600_120;
const TEST_NON_EXTERNAL_REVIEW_ACTOR_LOGIN: &str = "someone-else";
const TEST_SERVICE_ID: &str = "pubfi";
const TEST_PROJECT_CONFIG_FILE: &str = "project.toml";
