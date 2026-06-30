//! Explicit operator recovery surfaces for retained Decodex lanes.

use std::collections::BTreeSet;

#[cfg(test)] use crate::state::RUN_CONTROL_CHANNEL_STATUS_FAILED;
use crate::{
	tracker::{
		privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
		records::LinearExecutionEventRecord,
	},
	workflow::WorkflowTracker,
};

mod closeout;
mod context;
mod events;
mod evidence;
mod ghost_lane;
mod ghost_lane_cleanup;
mod ghost_lane_diagnosis;
mod git_worktree;
mod identifiers;
mod process_liveness;
mod pull_request_inspection;
mod reports;
mod requests;
mod review_handoff;
mod review_handoff_apply;
mod review_handoff_diagnosis;
mod review_handoff_policy;
mod stale_active;
mod stale_active_authority;
mod stale_active_diagnosis;
mod stale_active_labels;
mod stale_active_reentry;
mod stale_active_release;
mod stale_active_runtime;
mod stale_active_worktree;

pub(crate) use closeout::{run_legacy_closeout, run_merged_closeout};
#[cfg(test)] use context::LINEAR_RATE_LIMIT_BACKOFF_WARNING;
use context::{
	RecoveryContext, RecoveryRuntimeMutationPolicy, active_recovery_tracker_backoff_message,
	load_recovery_context_for_dry_run, load_recovery_context_read_only,
	remember_recovery_tracker_backoff_message,
};
use events::{
	append_review_handoff_adopt_private_event, append_review_handoff_rebind_private_event,
	manual_adopt_run_id, review_handoff_adopt_event, review_handoff_rebind_event,
};
#[cfg(test)] use events::{current_timestamp, timestamp_after_seconds};
pub(crate) use ghost_lane::{run_ghost_lane_cleanup, run_ghost_lane_diagnose};
use ghost_lane_cleanup::{
	apply_ghost_lane_cleanup, apply_ghost_lane_live_status_blockers,
	ensure_ghost_lane_live_status_allows_cleanup,
};
#[cfg(test)]
use ghost_lane_cleanup::{
	apply_ghost_lane_live_status_blockers_with_tracker,
	ensure_ghost_lane_live_status_allows_cleanup_with_tracker,
};
use ghost_lane_diagnosis::{diagnose_ghost_lanes, diagnose_ghost_lanes_read_only};
#[cfg(test)] use git_worktree::worktree_blocking_status_lines;
use git_worktree::{
	git_toplevel_path, repository_relative_path, worktree_checkout_branch_name, worktree_head_oid,
	worktree_is_clean,
};
use pull_request_inspection::{
	inspect_project_pull_request, inspect_project_pull_request_merge_commit,
	inspect_rebind_pull_request, landing_url,
};
#[cfg(test)] use reports::GhostLaneDiagnostic;
use reports::{
	GhostLaneRecoveryReport, ReviewHandoffRecoveryReport, StaleActiveRecoveryReport,
	render_ghost_lane_issue, render_ghost_lane_recovery_report,
	render_review_handoff_recovery_report, render_stale_active_recovery_report,
};
pub(crate) use requests::{
	GhostLaneCleanupRequest, GhostLaneDiagnoseRequest, LegacyCloseoutRecoveryRequest,
	MergedCloseoutRecoveryRequest, ReviewHandoffAdoptRequest, ReviewHandoffDiagnoseRequest,
	ReviewHandoffRebindRequest, StaleActiveDiagnoseRequest, StaleActiveReleaseRequest,
};
use review_handoff::{
	AdoptValidation, RebindValidation, load_issue_by_identifier,
	relative_worktree_path_for_recovery, validate_retained_pr_worktree,
};
pub(crate) use review_handoff::{
	run_review_handoff_adopt, run_review_handoff_diagnose, run_review_handoff_rebind,
};
#[cfg(test)]
use review_handoff::{validate_adopt_existing_worktree_mapping, validate_existing_handoff_refresh};
#[cfg(test)] use review_handoff_apply::write_review_lifecycle_markers_with_rollback;
use review_handoff_apply::{apply_review_handoff_adopt, apply_review_handoff_rebind};
#[cfg(test)]
use review_handoff_diagnosis::{
	HandoffDiagnosticRequest, diagnose_all_retained_review_worktrees_with_tracker,
	diagnose_issue_with_tracker, diagnostic_binding,
};
use review_handoff_diagnosis::{diagnose_all_retained_review_worktrees, diagnose_issue};
use review_handoff_policy::{
	RebindMode, RebindSuccessStateTransition, validate_adopt_issue_state_for_policy,
	validate_adopt_landing_state, validate_rebind_issue_state_for_policy,
};
pub(crate) use stale_active::{run_stale_active_diagnose, run_stale_active_release};
use stale_active_diagnosis::diagnose_stale_active_issues;
use stale_active_release::{apply_stale_active_release, preflight_stale_active_worktree_cleanup};
#[cfg(test)]
use stale_active_release::{
	apply_stale_active_release_with_tracker, ensure_stale_active_run_claim_guard,
};

const MISSING_HANDOFF_REASON: &str = "missing_review_handoff_record";
const ORPHANED_REVIEW_HANDOFF_CLASSIFICATION: &str = "orphaned_review_handoff";
const REVIEW_HANDOFF_BOUND_CLASSIFICATION: &str = "review_handoff_bound";
const REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION: &str = "review_handoff_ownership_drift";
const REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION: &str = "review_handoff_rebind_required";
const REVIEW_HANDOFF_UNVERIFIED_CLASSIFICATION: &str = "review_handoff_unverified";
const REVIEW_HANDOFF_MISMATCH_CLASSIFICATION: &str = "review_handoff_mismatch";
const REVIEW_HANDOFF_STALE_TERMINAL_RESIDUE_CLASSIFICATION: &str = "stale_terminal_local_residue";
const REVIEW_HANDOFF_REBIND_EVENT: &str = "review_handoff_rebind";
const REVIEW_HANDOFF_ADOPT_EVENT: &str = "review_handoff_adopt";
const LEGACY_MANUAL_CLOSEOUT_EVENT: &str = "closeout";
const LEGACY_MANUAL_CLOSEOUT_ANCHOR: &str = "legacy_manual_closeout";
const MERGED_CLOSEOUT_CLOSEOUT_ANCHOR: &str = "merged_closeout";
const MERGED_CLOSEOUT_CLEANUP_ANCHOR: &str = "merged_closeout_cleanup";
const GHOST_LANE_CLASSIFICATION: &str = "missing_issue_ghost_lane";
const MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION: &str = "mcp_test_fixture_ghost_lane";
const GHOST_LANE_BLOCKED_CLASSIFICATION: &str = "ghost_lane_recovery_blocked";
const GHOST_LANE_CLEANUP_EVENT: &str = "ghost_lane_cleanup";
const GHOST_LANE_TERMINAL_STATUS: &str = "terminal_guarded";
const STALE_ACTIVE_CLASSIFICATION: &str = "stale_active_ownership";
const STALE_ACTIVE_BLOCKED_CLASSIFICATION: &str = "stale_active_recovery_blocked";
const STALE_ACTIVE_STATE_RESTORE_CLASSIFICATION: &str = "stale_active_state_restore_pending";
const STALE_ACTIVE_RELEASE_EVENT: &str = "stale_active_release";
const STALE_ACTIVE_RECOVERY_SCHEMA: &str = "decodex.stale_active_recovery_private_event/1";
const MCP_TEST_FIXTURE_SOURCE: &str = "mcp-test";
const MCP_TEST_FIXTURE_PROJECT_ID: &str = "pubfi";
const MCP_TEST_FIXTURE_ISSUE_ID: &str = "PUB-012";
const MCP_TEST_FIXTURE_ALT_ISSUE_IDENTIFIER: &str = "PUBFI-012";
const MCP_TEST_FIXTURE_RUN_ID: &str = "run-12";
const MCP_TEST_FIXTURE_THREAD_ID: &str = "thread-12";
const MCP_TEST_FIXTURE_TURN_ID: &str = "turn-12";
const REBOUND_ORCHESTRATION_PHASE: &str = "request_pending";

fn sorted_unique(values: Vec<String>) -> Vec<String> {
	let mut set = BTreeSet::new();

	for value in values {
		set.insert(value);
	}

	set.into_iter().collect()
}

#[cfg(test)]
mod tests {
	use std::{cell::RefCell, fs, path::Path, process::Command};

	use tempfile::TempDir;

	use crate::{
		config::ServiceConfig,
		pull_request::PullRequestLandingState,
		recovery::{
			GHOST_LANE_BLOCKED_CLASSIFICATION, GHOST_LANE_CLASSIFICATION, GHOST_LANE_CLEANUP_EVENT,
			GHOST_LANE_TERMINAL_STATUS, MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION,
			REVIEW_HANDOFF_ADOPT_EVENT, REVIEW_HANDOFF_BOUND_CLASSIFICATION,
			REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION, REVIEW_HANDOFF_REBIND_EVENT,
			REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION, RUN_CONTROL_CHANNEL_STATUS_FAILED,
			STALE_ACTIVE_CLASSIFICATION, STALE_ACTIVE_RECOVERY_SCHEMA, STALE_ACTIVE_RELEASE_EVENT,
		},
		state::{
			self, ChildAgentActivitySummary, ConnectorBackoffInput, ProtocolActivityMarker,
			ProtocolActivitySummary, ReviewHandoffMarker, ReviewOrchestrationMarker,
			ReviewPolicyCheckpointInput, StateStore, WorktreeMapping,
		},
		tracker::{
			self, IssueTracker, TrackerComment, TrackerIssue, TrackerLabel, TrackerState,
			TrackerTeam,
			linear::LinearClient,
			records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
		},
		workflow::WorkflowDocument,
	};

	struct GhostLaneTestTracker {
		issues: Vec<TrackerIssue>,
		refresh_error: Option<String>,
		identifier_error: Option<String>,
		remove_error: Option<String>,
		comments: Vec<TrackerComment>,
		refresh_queries: RefCell<Vec<Vec<String>>>,
		label_removals: RefCell<Vec<(String, Vec<String>)>>,
		state_updates: RefCell<Vec<(String, String)>>,
	}
	impl GhostLaneTestTracker {
		fn missing() -> Self {
			Self {
				issues: Vec::new(),
				refresh_error: None,
				identifier_error: None,
				remove_error: None,
				comments: Vec::new(),
				refresh_queries: RefCell::new(Vec::new()),
				label_removals: RefCell::new(Vec::new()),
				state_updates: RefCell::new(Vec::new()),
			}
		}

		fn with_issues(issues: Vec<TrackerIssue>) -> Self {
			Self {
				issues,
				refresh_error: None,
				identifier_error: None,
				remove_error: None,
				comments: Vec::new(),
				refresh_queries: RefCell::new(Vec::new()),
				label_removals: RefCell::new(Vec::new()),
				state_updates: RefCell::new(Vec::new()),
			}
		}

		fn with_comments(mut self, comments: Vec<TrackerComment>) -> Self {
			self.comments = comments;
			self
		}

		fn remove_error(mut self, message: &str) -> Self {
			self.remove_error = Some(message.to_owned());
			self
		}

		fn refresh_error(message: &str) -> Self {
			Self {
				issues: Vec::new(),
				refresh_error: Some(message.to_owned()),
				identifier_error: None,
				remove_error: None,
				comments: Vec::new(),
				refresh_queries: RefCell::new(Vec::new()),
				label_removals: RefCell::new(Vec::new()),
				state_updates: RefCell::new(Vec::new()),
			}
		}

		fn identifier_error(message: &str) -> Self {
			Self {
				issues: Vec::new(),
				refresh_error: None,
				identifier_error: Some(message.to_owned()),
				remove_error: None,
				comments: Vec::new(),
				refresh_queries: RefCell::new(Vec::new()),
				label_removals: RefCell::new(Vec::new()),
				state_updates: RefCell::new(Vec::new()),
			}
		}
	}
	impl IssueTracker for GhostLaneTestTracker {
		fn list_issues_with_label(
			&self,
			label_name: &str,
		) -> crate::prelude::Result<Vec<TrackerIssue>> {
			Ok(self.issues.iter().filter(|issue| issue.has_label(label_name)).cloned().collect())
		}

		fn find_team_label_id(
			&self,
			team_id: &str,
			label_name: &str,
		) -> crate::prelude::Result<Option<String>> {
			Ok(self
				.issues
				.iter()
				.find(|issue| issue.team.id == team_id)
				.and_then(|issue| issue.label_id_for_name(label_name).map(ToOwned::to_owned)))
		}

		fn get_issue_by_identifier(
			&self,
			issue_identifier: &str,
		) -> crate::prelude::Result<Option<TrackerIssue>> {
			if let Some(message) = &self.identifier_error {
				return Err(crate::prelude::eyre::eyre!(message.clone()));
			}

			Ok(self
				.issues
				.iter()
				.find(|issue| issue.identifier.eq_ignore_ascii_case(issue_identifier))
				.cloned())
		}

		fn refresh_issues(
			&self,
			issue_ids: &[String],
		) -> crate::prelude::Result<Vec<TrackerIssue>> {
			self.refresh_queries.borrow_mut().push(issue_ids.to_vec());

			if let Some(message) = &self.refresh_error {
				return Err(crate::prelude::eyre::eyre!(message.clone()));
			}

			Ok(self
				.issues
				.iter()
				.filter(|issue| issue_ids.iter().any(|issue_id| issue_id == &issue.id))
				.cloned()
				.collect())
		}

		fn list_comments(&self, _issue_id: &str) -> crate::prelude::Result<Vec<TrackerComment>> {
			Ok(self.comments.clone())
		}

		fn update_issue_state(&self, issue_id: &str, state_id: &str) -> crate::prelude::Result<()> {
			self.state_updates.borrow_mut().push((issue_id.to_owned(), state_id.to_owned()));
			Ok(())
		}

		fn add_issue_labels(
			&self,
			_issue_id: &str,
			_label_ids: &[String],
		) -> crate::prelude::Result<()> {
			Ok(())
		}

		fn remove_issue_labels(
			&self,
			issue_id: &str,
			label_ids: &[String],
		) -> crate::prelude::Result<()> {
			self.label_removals.borrow_mut().push((issue_id.to_owned(), label_ids.to_vec()));
			if let Some(message) = &self.remove_error {
				return Err(crate::prelude::eyre::eyre!(message.clone()));
			}

			Ok(())
		}

		fn create_comment(&self, _issue_id: &str, _body: &str) -> crate::prelude::Result<()> {
			Ok(())
		}
	}

	struct FinalNeedsAttentionTracker {
		issue: TrackerIssue,
		needs_attention_label: String,
		get_issue_calls: RefCell<usize>,
		label_removals: RefCell<Vec<(String, Vec<String>)>>,
	}
	impl FinalNeedsAttentionTracker {
		fn new(issue: TrackerIssue, needs_attention_label: String) -> Self {
			Self {
				issue,
				needs_attention_label,
				get_issue_calls: RefCell::new(0),
				label_removals: RefCell::new(Vec::new()),
			}
		}

		fn issue_for_call(&self, call_count: usize) -> TrackerIssue {
			let mut issue = self.issue.clone();

			if call_count >= 3 {
				let label = TrackerLabel {
					id: format!("label-{}", self.needs_attention_label.replace(':', "-")),
					name: self.needs_attention_label.clone(),
				};
				if !issue.team.labels.iter().any(|candidate| candidate.name == label.name) {
					issue.team.labels.push(label.clone());
				}
				if !issue.labels.iter().any(|candidate| candidate.name == label.name) {
					issue.labels.push(label);
				}
			}

			issue
		}
	}
	impl IssueTracker for FinalNeedsAttentionTracker {
		fn list_issues_with_label(
			&self,
			label_name: &str,
		) -> crate::prelude::Result<Vec<TrackerIssue>> {
			let issue = self.issue_for_call(*self.get_issue_calls.borrow());

			Ok(issue.has_label(label_name).then_some(issue).into_iter().collect())
		}

		fn find_team_label_id(
			&self,
			_team_id: &str,
			label_name: &str,
		) -> crate::prelude::Result<Option<String>> {
			Ok(Some(format!("label-{}", label_name.replace(':', "-"))))
		}

		fn get_issue_by_identifier(
			&self,
			issue_identifier: &str,
		) -> crate::prelude::Result<Option<TrackerIssue>> {
			let mut calls = self.get_issue_calls.borrow_mut();

			*calls += 1;
			let issue = self.issue_for_call(*calls);

			Ok((issue.identifier == issue_identifier).then_some(issue))
		}

		fn refresh_issues(
			&self,
			issue_ids: &[String],
		) -> crate::prelude::Result<Vec<TrackerIssue>> {
			let issue = self.issue_for_call(*self.get_issue_calls.borrow());

			Ok(issue_ids
				.iter()
				.any(|issue_id| issue_id == &issue.id)
				.then_some(issue)
				.into_iter()
				.collect())
		}

		fn list_comments(&self, _issue_id: &str) -> crate::prelude::Result<Vec<TrackerComment>> {
			Ok(Vec::new())
		}

		fn update_issue_state(
			&self,
			_issue_id: &str,
			_state_id: &str,
		) -> crate::prelude::Result<()> {
			Ok(())
		}

		fn add_issue_labels(
			&self,
			_issue_id: &str,
			_label_ids: &[String],
		) -> crate::prelude::Result<()> {
			Ok(())
		}

		fn remove_issue_labels(
			&self,
			issue_id: &str,
			label_ids: &[String],
		) -> crate::prelude::Result<()> {
			self.label_removals.borrow_mut().push((issue_id.to_owned(), label_ids.to_vec()));

			Ok(())
		}

		fn create_comment(&self, _issue_id: &str, _body: &str) -> crate::prelude::Result<()> {
			Ok(())
		}
	}

	fn sample_worktree(branch_name: &str) -> WorktreeMapping {
		sample_worktree_at(branch_name, Path::new("/tmp/PUB-718"))
	}

	fn sample_worktree_at(branch_name: &str, worktree_path: &Path) -> WorktreeMapping {
		let store = StateStore::open_in_memory().expect("state store should open");
		let worktree_path = worktree_path.to_string_lossy();

		store
			.upsert_worktree("pubfi", "issue-id", branch_name, &worktree_path)
			.expect("worktree should persist");

		store
			.worktree_for_issue("issue-id")
			.expect("worktree should read")
			.expect("worktree should exist")
	}

	fn sample_landing_state(
		pr_url: &str,
		branch_name: &str,
		head_oid: &str,
	) -> PullRequestLandingState {
		PullRequestLandingState {
			url: pr_url.to_owned(),
			state: String::from("OPEN"),
			is_draft: false,
			review_decision: Some(String::from("APPROVED")),
			base_ref_name: String::from("main"),
			pending_review_requests: 0,
			mergeable: String::from("MERGEABLE"),
			merge_state_status: String::from("CLEAN"),
			head_ref_name: branch_name.to_owned(),
			head_ref_oid: head_oid.to_owned(),
			status_check_rollup_state: Some(String::from("SUCCESS")),
			unresolved_review_threads: 0,
		}
	}

	fn sample_workflow() -> WorkflowDocument {
		WorkflowDocument::parse_markdown(
			r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 8
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Test workflow.
"#,
		)
		.expect("sample workflow should parse")
	}

	fn sample_recovery_context(
		temp_dir: &TempDir,
		runtime_mutation_policy: super::RecoveryRuntimeMutationPolicy,
	) -> super::RecoveryContext {
		let repo_root = temp_dir.path().join("repo");
		let config_path = temp_dir.path().join("project.toml");

		fs::create_dir_all(&repo_root).expect("repo root should exist");
		fs::write(
			&config_path,
			r#"
service_id = "pubfi"

[paths]
repo_root = "repo"

[tracker]
api_key_env_var = "HOME"

[github]
token_env_var = "HOME"
"#,
		)
		.expect("config should write");

		super::RecoveryContext {
			config: ServiceConfig::from_path(&config_path).expect("config should load"),
			workflow: sample_workflow(),
			state_store: StateStore::open_in_memory().expect("state store should open"),
			tracker: LinearClient::new(String::from("test-token"))
				.expect("linear client should build"),
			runtime_mutation_policy,
		}
	}

	fn seed_mcp_test_fixture_ghost_lane(store: &StateStore, worktree_root: &Path) {
		let channel_path = worktree_root.join("missing-run-control.channel");
		let protocol_activity = ProtocolActivitySummary {
			turn_status: Some(String::from("completed")),
			waiting_reason: Some(String::from("turn_completed")),
			rate_limit_status: None,
			recent_events: Vec::new(),
		};

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		store.update_run_thread("run-12", "thread-12").expect("thread should record");
		store.update_run_turn("run-12", "turn-12").expect("turn should record");
		store
			.publish_run_control_channel_for_active_attempt(
				"run-12",
				1,
				&channel_path,
				"local_file",
			)
			.expect("control channel row should publish");
		store
			.append_event("run-12", 1, "turn/completed", r#"{"status":"completed"}"#)
			.expect("protocol event should record");
		store
			.record_run_activity_summary("run-12", 1, None, Some(&protocol_activity))
			.expect("protocol activity should record");

		append_mcp_test_control_private_events(store);
	}

	fn append_mcp_test_control_private_events(store: &StateStore) {
		for (event_type, payload) in [
			(
				"control_action",
				serde_json::json!({
					"schema": "decodex.run_control_action/v1",
					"source": "mcp-test",
					"action": "steer"
				}),
			),
			(
				"control_action",
				serde_json::json!({
					"schema": "decodex.run_control_action/v1",
					"source": "cli",
					"action": "interrupt",
					"requested": {
						"project_id": "pubfi",
						"issue_id": "PUB-012",
						"run_id": "run-12",
						"attempt_number": 1,
						"thread_id": "thread-12",
						"turn_id": "turn-12"
					}
				}),
			),
			(
				"lane_control/steer/requested",
				serde_json::json!({
					"source": "mcp-test",
					"method": "turn/steer"
				}),
			),
			(
				"lane_control/interrupt/requested",
				serde_json::json!({
					"source": "mcp-test",
					"method": "turn/interrupt"
				}),
			),
		] {
			store
				.append_private_execution_event(
					"pubfi", "PUB-012", "run-12", 1, event_type, payload,
				)
				.expect("mcp-test private evidence should record");
		}
	}

	fn append_mcp_test_fixture_ghost_lane_cleanup_audit(store: &StateStore) {
		store
			.append_private_execution_event(
				"pubfi",
				"PUB-012",
				"run-12",
				1,
				GHOST_LANE_CLEANUP_EVENT,
				serde_json::json!({
					"schema": "decodex.ghost_lane_recovery_private_event/1",
					"event": GHOST_LANE_CLEANUP_EVENT,
					"classification": MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION,
					"reason": "tracker_issue_missing_and_only_mcp_test_control_fixture_evidence",
					"issue_identifier": "PUBFI-012",
					"terminal_status": GHOST_LANE_TERMINAL_STATUS,
					"cleared_run_lease": true,
					"evidence": [
						"tracker_issue_missing",
						"worktree_mapping_path_missing",
						"worktree_missing",
						"control_channel_file_missing",
						"mcp_test_fixture_control_channel_row_present",
						"mcp_test_fixture_private_control_evidence_present",
						"review_lineage_missing"
					],
					"blockers": [],
					"next_action": "ordinary automation may continue after status readback confirms no current attention lane"
				}),
			)
			.expect("cleanup audit should record");
	}

	fn sample_issue(state_name: &str) -> TrackerIssue {
		let states = vec![
			TrackerState { id: String::from("state-todo"), name: String::from("Todo") },
			TrackerState { id: String::from("state-progress"), name: String::from("In Progress") },
			TrackerState { id: String::from("state-review"), name: String::from("In Review") },
			TrackerState { id: String::from("state-done"), name: String::from("Done") },
		];
		let state = states
			.iter()
			.find(|state| state.name == state_name)
			.expect("sample state should exist")
			.clone();

		TrackerIssue {
			id: String::from("issue-id"),
			identifier: String::from("PUB-718"),
			#[cfg(test)]
			project_slug: None,
			title: String::from("Sample issue"),
			author: None,
			description: String::new(),
			priority: None,
			created_at: String::from("2026-06-09T00:00:00Z"),
			updated_at: String::from("2026-06-09T00:00:00Z"),
			state,
			team: TrackerTeam {
				id: String::from("team-id"),
				name: String::from("XY"),
				states,
				labels: Vec::new(),
			},
			labels_complete: true,
			labels: Vec::new(),
			blockers: Vec::new(),
		}
	}

	fn sample_issue_with_labels(state_name: &str, labels: &[String]) -> TrackerIssue {
		let mut issue = sample_issue(state_name);

		for label in labels {
			let tracker_label = TrackerLabel {
				id: format!("label-{}", label.replace(':', "-")),
				name: label.clone(),
			};

			issue.team.labels.push(tracker_label.clone());
			issue.labels.push(tracker_label);
		}

		issue
	}

	fn init_git_repo(path: &Path) {
		fs::create_dir_all(path).expect("git repo path should create");
		let status = Command::new("git")
			.arg("-C")
			.arg(path)
			.arg("init")
			.status()
			.expect("git init should run");

		assert!(status.success(), "git init should succeed");
	}

	fn commit_test_file(path: &Path, file_name: &str, body: &str, message: &str) {
		fs::write(path.join(file_name), body).expect("test file should write");
		run_git(path, &["add", file_name]);
		run_git(
			path,
			&[
				"-c",
				"user.name=Decodex Test",
				"-c",
				"user.email=decodex-test@example.invalid",
				"-c",
				"commit.gpgsign=false",
				"commit",
				"-m",
				message,
			],
		);
	}

	fn init_clean_git_repo_with_remote_default(path: &Path, branch_name: &str) {
		init_git_repo(path);
		run_git(path, &["checkout", "-B", "main"]);
		commit_test_file(path, "README.md", "base\n", "base");
		run_git(path, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
		run_git(path, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
		run_git(path, &["checkout", "-B", branch_name]);
	}

	#[test]
	fn stale_active_diagnose_classifies_tracker_present_active_without_lease() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let queue_label = tracker::automation_queue_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store.update_run_thread("run-1626", "thread-stale").expect("thread should record");
		store.update_run_turn("run-1626", "turn-stale").expect("turn should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
		assert_eq!(
			diagnostic.reason,
			"tracker_issue_has_stale_active_label_without_live_or_retained_progress"
		);
		assert!(diagnostic.active_label_present);
		assert!(diagnostic.queue_label_present);
		assert!(!diagnostic.run_lease);
		assert_eq!(diagnostic.latest_run_id.as_deref(), Some("run-1626"));
		assert!(diagnostic.blockers.is_empty(), "unexpected blockers: {:?}", diagnostic.blockers);
		assert!(diagnostic.evidence.contains(&String::from("tracker_issue_present")));
		assert!(diagnostic.evidence.contains(&String::from("run_lease_missing")));
		assert!(diagnostic.evidence.contains(&String::from("private_evidence_missing")));
		assert!(diagnostic.evidence.contains(&String::from("stale_thread_reference_present")));
		assert!(diagnostic.next_action.contains("recover stale-active release PUB-1626 --dry-run"));
	}

	#[test]
	fn stale_active_diagnose_blocks_shared_claim_lock_file() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let owner_store = StateStore::open_in_memory().expect("owner store should open");
		let store = StateStore::open_in_memory().expect("reader store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.identifier = String::from("PUB-1626");
		owner_store
			.configure_dispatch_slot_root("pubfi", temp_dir.path())
			.expect("owner store should configure dispatch root");
		assert!(
			owner_store
				.try_acquire_lease("pubfi", &issue.id, "run-live", "In Progress")
				.expect("owner should acquire shared claim")
		);
		store
			.observe_dispatch_slot_root("pubfi", temp_dir.path())
			.expect("reader store should observe dispatch root");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.active_shared_claim);
		assert!(diagnostic.blockers.contains(&String::from("active_shared_claim_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_identifier_keyed_run_lease() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.id = String::from("linear-issue-1626");
		issue.identifier = String::from("PUB-1626");
		store
			.upsert_lease("pubfi", &issue.identifier, "run-identifier", "In Progress")
			.expect("identifier-keyed lease should record");
		store
			.record_run_attempt("run-identifier", &issue.identifier, 1, "running")
			.expect("identifier-keyed run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.identifier,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("identifier-keyed worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.latest_run_id.as_deref(), Some("run-identifier"));
		assert!(diagnostic.run_lease);
		assert!(diagnostic.blockers.contains(&String::from("run_lease_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_identifier_keyed_private_progress() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.id = String::from("linear-issue-1626");
		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.append_private_execution_event(
				"pubfi",
				&issue.identifier,
				"run-identifier",
				1,
				"source_progress",
				serde_json::json!({"phase": "implementation"}),
			)
			.expect("identifier-keyed private progress should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_identifier_keyed_worktree_progress() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("identifier-worktree");

		issue.id = String::from("linear-issue-1626");
		issue.identifier = String::from("PUB-1626");
		fs::create_dir_all(&worktree_path).expect("identifier worktree should create");
		fs::write(worktree_path.join("source.rs"), "fn progress() {}\n")
			.expect("ordinary worktree file should write");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.identifier,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("identifier-keyed worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.worktree_state, "non_git_files_present");
		assert!(diagnostic.blockers.contains(&String::from("non_git_worktree_files_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_active_thread_marker() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		fs::create_dir_all(&worktree_path).expect("worktree path should create");
		state::write_run_thread_status_marker(
			&worktree_path,
			"run-1626",
			1,
			Some("thread-1626"),
			Some("turn-1626"),
			"active",
			&[String::from("waitingOnApproval")],
		)
		.expect("active thread marker should write");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("activity_marker_thread_active")));
		assert!(!diagnostic.recoverable());
	}

	fn dead_orphan_activity_summaries() -> (ChildAgentActivitySummary, ProtocolActivitySummary) {
		(
			ChildAgentActivitySummary {
				event_count: 531,
				current_bucket: Some(String::from("Model")),
				..ChildAgentActivitySummary::default()
			},
			ProtocolActivitySummary {
				turn_status: Some(String::from("running")),
				waiting_reason: Some(String::from("model_execution")),
				..ProtocolActivitySummary::default()
			},
		)
	}

	fn seed_dead_orphan_runtime_telemetry(
		store: &StateStore,
		issue: &TrackerIssue,
		worktree_path: &Path,
	) {
		let control_channel_path = worktree_path.join(".decodex-run-control/run-1626-1.channel");
		let (child_activity, protocol_activity) = dead_orphan_activity_summaries();

		init_clean_git_repo_with_remote_default(worktree_path, "x/pubfi-pub-1626");
		state::write_run_activity_marker_for_process(worktree_path, "run-1626", 1, u32::MAX)
			.expect("stale process marker should write");
		state::write_run_protocol_activity_marker(
			worktree_path,
			&ProtocolActivityMarker {
				run_id: "run-1626",
				attempt_number: 1,
				thread_id: Some("thread-stale"),
				turn_id: Some("turn-stale"),
				event_count: 531,
				last_event_type: "item/started",
				child_agent_activity: Some(&child_activity),
				protocol_activity: Some(&protocol_activity),
			},
		)
		.expect("stale protocol marker should write");
		state::write_run_thread_status_marker(
			worktree_path,
			"run-1626",
			1,
			Some("thread-stale"),
			Some("turn-stale"),
			"active",
			&[],
		)
		.expect("stale thread marker should write");
		fs::create_dir_all(control_channel_path.parent().expect("channel parent"))
			.expect("control directory should create");
		fs::write(
			&control_channel_path,
			"schema=decodex.run_control_channel/v1\nrun_id=run-1626\nattempt_number=1\n",
		)
		.expect("control channel file should write");
		store.record_run_attempt("run-1626", &issue.id, 1, "running").expect("run attempt");
		store
			.upsert_lease("pubfi", &issue.id, "run-1626", "In Progress")
			.expect("temporary lease should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.publish_run_control_channel_for_active_attempt(
				"run-1626",
				1,
				&control_channel_path,
				"local_file",
			)
			.expect("control channel should publish");
		store.clear_lease(&issue.id).expect("stale lane lease should clear");
		store
			.append_event("run-1626", 1, "item/started", r#"{"kind":"model"}"#)
			.expect("protocol event should record");
		store
			.record_run_activity_summary(
				"run-1626",
				1,
				Some(&child_activity),
				Some(&protocol_activity),
			)
			.expect("activity summary should record");
		append_dead_orphan_private_telemetry(store, &issue.id);
	}

	fn seed_dead_orphan_runtime_telemetry_without_control_channel(
		store: &StateStore,
		issue: &TrackerIssue,
		worktree_path: &Path,
	) {
		let (child_activity, protocol_activity) = dead_orphan_activity_summaries();

		state::write_run_activity_marker_for_process(worktree_path, "run-1626", 1, u32::MAX)
			.expect("stale process marker should write");
		state::write_run_protocol_activity_marker(
			worktree_path,
			&ProtocolActivityMarker {
				run_id: "run-1626",
				attempt_number: 1,
				thread_id: Some("thread-stale"),
				turn_id: Some("turn-stale"),
				event_count: 531,
				last_event_type: "item/started",
				child_agent_activity: Some(&child_activity),
				protocol_activity: Some(&protocol_activity),
			},
		)
		.expect("stale protocol marker should write");
		state::write_run_thread_status_marker(
			worktree_path,
			"run-1626",
			1,
			Some("thread-stale"),
			Some("turn-stale"),
			"active",
			&[],
		)
		.expect("stale thread marker should write");
		store
			.upsert_lease("pubfi", &issue.id, "run-1626", "In Progress")
			.expect("temporary lease should record");
		store.record_run_attempt("run-1626", &issue.id, 1, "running").expect("run attempt");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");
		store.clear_lease(&issue.id).expect("stale lane lease should clear");
		store
			.append_event("run-1626", 1, "item/started", r#"{"kind":"model"}"#)
			.expect("protocol event should record");
		store
			.record_run_activity_summary(
				"run-1626",
				1,
				Some(&child_activity),
				Some(&protocol_activity),
			)
			.expect("activity summary should record");
		append_dead_orphan_private_telemetry_without_control_channel_marker(store, &issue.id);
	}

	fn append_dead_orphan_private_telemetry(store: &StateStore, issue_id: &str) {
		append_dead_orphan_private_telemetry_events(store, issue_id, true);
	}

	fn append_dead_orphan_private_telemetry_without_control_channel_marker(
		store: &StateStore,
		issue_id: &str,
	) {
		append_dead_orphan_private_telemetry_events(store, issue_id, false);
	}

	fn append_dead_orphan_private_telemetry_events(
		store: &StateStore,
		issue_id: &str,
		include_control_channel_marker: bool,
	) {
		let mut events = vec![
			(
				"phase_goal_set",
				serde_json::json!({
					"schema": "decodex.phase_goal_signal/1",
					"phase": "implement_to_validation_ready",
				}),
			),
			(
				"progress_checkpoint",
				serde_json::json!({
					"phase": "probing",
					"pr_url": null,
					"verification": [],
					"head_sha": "1111111111111111111111111111111111111111",
				}),
			),
			(
				"control_action",
				serde_json::json!({
					"action": "interrupt",
					"reason": "run_lease_missing",
				}),
			),
			(
				"lane_control/interrupt",
				serde_json::json!({
					"classification": "hard_interrupt_fallback",
					"processAliveAfter": false,
					"signals": [],
					"status": "sent",
				}),
			),
		];

		if include_control_channel_marker {
			events.insert(
				0,
				(
					"control_channel_published",
					serde_json::json!({
						"schema": "decodex.run_control_channel/v1",
						"status": "active",
					}),
				),
			);
		}

		for (event_type, payload) in events {
			store
				.append_private_execution_event(
					"pubfi", issue_id, "run-1626", 1, event_type, payload,
				)
				.expect("private stale telemetry should record");
		}
	}

	fn append_app_server_no_progress_failure_evidence(store: &StateStore, issue_id: &str) {
		for (event_type, payload) in [
			(
				"loop_guardrail_checkpoint",
				serde_json::json!({
					"checkpoint_attempt_number": 1,
					"checkpoint_run_id": "run-1626",
					"consecutive_count": 1,
					"details": serde_json::json!({
						"branch_delta_present": false,
						"effective_delta_present": false,
						"reason": "no_effective_diff",
						"source_error_class": "app_server_turn_failed",
					})
					.to_string(),
					"fingerprint": "empty:empty",
					"reason": "no_effective_diff",
					"schema": "decodex.loop_guardrail_checkpoint/1",
					"source_error_class": "app_server_turn_failed",
					"threshold": 3,
				}),
			),
			(
				"harness_outcome",
				serde_json::json!({
					"authority_boundary": {
						"dispositions": [],
						"failed_check_count": 0,
						"improvement_signal_count": 0,
					},
					"contracts": [],
					"execution_programs": [],
					"linear_projection": {
						"event_types": ["run_started"],
						"final_error_class": null,
						"final_event_type": "run_started",
						"final_terminal_path": null,
					},
					"manual_attention": null,
					"phase_goal_outcomes": [{
						"event_type": "phase_goal_set",
						"phase": "implement_to_validation_ready",
						"status": "active",
					}],
					"pr_lifecycle": {
						"outcome": "retryable_failure",
						"pr_urls": [],
					},
					"record_version": 1,
					"repair": {
						"attempt_number": 1,
						"repair_attempt_observed": false,
						"repair_phase_events": 0,
					},
					"review": {
						"accepted_finding_count": 0,
						"nonclean_rounds": 0,
						"rejected_finding_count": 0,
						"statuses": [],
					},
					"schema": "decodex.harness_outcome/1",
					"source": {
						"attempt_number": 1,
						"issue_id": issue_id,
						"issue_identifier": "PUB-1626",
						"outcome": "retryable_failure",
						"project_id": "pubfi",
						"run_id": "run-1626",
						"source_intents": [],
					},
					"validation": {
						"failure_classes": [],
						"failure_count": 0,
						"result": "not_recorded",
					},
				}),
			),
		] {
			store
				.append_private_execution_event(
					"pubfi", issue_id, "run-1626", 1, event_type, payload,
				)
				.expect("private no-progress failure evidence should record");
		}
	}

	fn append_no_diff_guardrail_event(
		store: &StateStore,
		issue_id: &str,
		branch_delta_present: bool,
		effective_delta_present: bool,
	) {
		store
			.append_private_execution_event(
				"pubfi",
				issue_id,
				"run-1626",
				1,
				"loop_guardrail_checkpoint",
				serde_json::json!({
					"details": serde_json::json!({
						"branch_delta_present": branch_delta_present,
						"effective_delta_present": effective_delta_present,
					})
					.to_string(),
					"reason": "no_effective_diff",
					"schema": "decodex.loop_guardrail_checkpoint/1",
					"source_error_class": "app_server_turn_failed",
				}),
			)
			.expect("private guardrail evidence should record");
	}

	fn append_harness_outcome_with_pr_progress(store: &StateStore, issue_id: &str) {
		store
			.append_private_execution_event(
				"pubfi",
				issue_id,
				"run-1626",
				1,
				"harness_outcome",
				serde_json::json!({
					"manual_attention": null,
					"pr_lifecycle": {
						"outcome": "retryable_failure",
						"pr_urls": ["https://github.com/hack-ink/pubfi/pull/1631"],
					},
					"review": {
						"accepted_finding_count": 0,
						"nonclean_rounds": 0,
						"rejected_finding_count": 0,
						"statuses": [],
					},
					"schema": "decodex.harness_outcome/1",
					"source": {
						"outcome": "retryable_failure",
					},
					"validation": {
						"failure_classes": [],
						"failure_count": 0,
						"result": "not_recorded",
					},
				}),
			)
			.expect("private harness progress evidence should record");
	}

	fn append_harness_outcome_with_review_progress(store: &StateStore, issue_id: &str) {
		store
			.append_private_execution_event(
				"pubfi",
				issue_id,
				"run-1626",
				1,
				"harness_outcome",
				serde_json::json!({
					"contracts": [],
					"execution_programs": [],
					"manual_attention": null,
					"pr_lifecycle": {
						"outcome": "retryable_failure",
						"pr_urls": [],
					},
					"review": {
						"accepted_finding_count": 1,
						"nonclean_rounds": 0,
						"rejected_finding_count": 0,
						"statuses": [],
					},
					"schema": "decodex.harness_outcome/1",
					"source": {
						"outcome": "retryable_failure",
					},
					"validation": {
						"failure_classes": [],
						"failure_count": 0,
						"result": "not_recorded",
					},
				}),
			)
			.expect("private harness review progress evidence should record");
	}

	fn append_harness_outcome_with_validation_progress(store: &StateStore, issue_id: &str) {
		store
			.append_private_execution_event(
				"pubfi",
				issue_id,
				"run-1626",
				1,
				"harness_outcome",
				serde_json::json!({
					"contracts": [],
					"execution_programs": [],
					"manual_attention": null,
					"pr_lifecycle": {
						"outcome": "retryable_failure",
						"pr_urls": [],
					},
					"review": {
						"accepted_finding_count": 0,
						"nonclean_rounds": 0,
						"rejected_finding_count": 0,
						"statuses": [],
					},
					"schema": "decodex.harness_outcome/1",
					"source": {
						"outcome": "retryable_failure",
					},
					"validation": {
						"failure_classes": ["repo_gate_verify_failed"],
						"failure_count": 1,
						"result": "failed",
					},
				}),
			)
			.expect("private harness validation progress evidence should record");
	}

	fn append_phase_goal_recovery_event(
		store: &StateStore,
		issue_id: &str,
		phase: &str,
		source_error_class: &str,
	) {
		store
			.append_private_execution_event(
				"pubfi",
				issue_id,
				"run-1626",
				1,
				"phase_goal_recovery",
				serde_json::json!({
						"schema": "decodex.phase_goal_signal/1",
						"phase": phase,
						"signal": "phase_goal_recovered",
						"payload": {
							"nextPhase": "handoff_evidence",
						"sourceErrorClass": source_error_class,
						"sourceErrorMessage": "runtime failure",
					},
				}),
			)
			.expect("private phase goal recovery evidence should record");
	}

	fn append_stale_active_release_audit(store: &StateStore, issue_id: &str) {
		append_stale_active_release_audit_for_run(store, issue_id, "run-1626", 1);
	}

	fn append_stale_active_release_audit_for_run(
		store: &StateStore,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
	) {
		store
			.append_private_execution_event(
				"pubfi",
				issue_id,
				run_id,
				attempt_number,
				STALE_ACTIVE_RELEASE_EVENT,
				serde_json::json!({
					"schema": STALE_ACTIVE_RECOVERY_SCHEMA,
					"event": STALE_ACTIVE_RELEASE_EVENT,
					"phase": "local_cleanup_complete_before_active_label_release",
				}),
			)
			.expect("stale active release audit should record");
	}

	#[test]
	fn stale_active_diagnose_allows_dead_orphan_thread_runtime_telemetry() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let queue_label = tracker::automation_queue_label("pubfi");
		let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
		assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
		assert!(diagnostic.evidence.contains(&String::from("process_not_alive")));
		assert!(
			diagnostic.evidence.contains(&String::from("stale_active_control_channel_present"))
		);
		assert!(
			diagnostic
				.evidence
				.contains(&String::from("only_stale_active_or_failed_control_evidence_present"))
		);
	}

	#[test]
	fn stale_active_diagnose_allows_app_server_no_progress_failure_evidence() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let queue_label = tracker::automation_queue_label("pubfi");
		let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
		append_app_server_no_progress_failure_evidence(&store, &issue.id);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
		assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
		assert!(
			diagnostic
				.evidence
				.contains(&String::from("only_stale_active_or_failed_control_evidence_present"))
		);
	}

	#[test]
	fn stale_active_diagnose_blocks_harness_outcome_with_pr_progress() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let queue_label = tracker::automation_queue_label("pubfi");
		let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
		append_harness_outcome_with_pr_progress(&store, &issue.id);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_harness_outcome_with_review_progress() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let queue_label = tracker::automation_queue_label("pubfi");
		let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
		append_harness_outcome_with_review_progress(&store, &issue.id);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_harness_outcome_with_validation_progress() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let queue_label = tracker::automation_queue_label("pubfi");
		let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
		append_harness_outcome_with_validation_progress(&store, &issue.id);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_no_diff_guardrail_with_delta() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let queue_label = tracker::automation_queue_label("pubfi");
		let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
		append_no_diff_guardrail_event(&store, &issue.id, true, false);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_allows_app_server_phase_goal_recovery_telemetry() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let queue_label = tracker::automation_queue_label("pubfi");
		let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
		append_phase_goal_recovery_event(
			&store,
			&issue.id,
			"implement_to_validation_ready",
			"app_server_dynamic_tool_protocol_failure",
		);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
		assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
	}

	#[test]
	fn stale_active_diagnose_blocks_repo_gate_phase_goal_recovery_telemetry() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let queue_label = tracker::automation_queue_label("pubfi");
		let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
		append_phase_goal_recovery_event(
			&store,
			&issue.id,
			"implement_to_validation_ready",
			"repo_gate_verify_failed",
		);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_repair_phase_goal_recovery_telemetry() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let queue_label = tracker::automation_queue_label("pubfi");
		let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
		append_phase_goal_recovery_event(
			&store,
			&issue.id,
			"repair_accepted_review_findings",
			"app_server_dynamic_tool_protocol_failure",
		);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_clean_worktree_with_unmerged_commits() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		init_git_repo(&worktree_path);
		run_git(&worktree_path, &["checkout", "-B", "main"]);
		commit_test_file(&worktree_path, "README.md", "base\n", "base");
		run_git(&worktree_path, &["checkout", "-b", "x/pubfi-pub-1626"]);
		commit_test_file(&worktree_path, "source.rs", "fn retained_progress() {}\n", "progress");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.worktree_state, "unmerged_commits_present");
		assert!(diagnostic.blockers.contains(&String::from("worktree_unmerged_commits_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_clean_git_worktree_without_default_branch() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		init_git_repo(&worktree_path);
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.worktree_state, "default_branch_unavailable");
		assert!(diagnostic.blockers.contains(&String::from("worktree_default_branch_unavailable")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_release_allows_reentry_after_local_cleanup_audit() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue =
			sample_issue_with_labels("In Progress", &[active_label.clone(), queue_label]);
		let worktree_path = context.config.worktree_root().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&context.state_store, &issue, &worktree_path);
		context
			.state_store
			.update_run_status("run-1626", GHOST_LANE_TERMINAL_STATUS)
			.expect("run should terminalize");
		context
			.state_store
			.retire_run_control_channel_for_attempt(
				"run-1626",
				1,
				RUN_CONTROL_CHANNEL_STATUS_FAILED,
			)
			.expect("control channel should retire");
		fs::remove_dir_all(&worktree_path).expect("worktree should be removed");
		context
			.state_store
			.clear_worktree_mapping(&issue.id)
			.expect("issue-id worktree mapping should clear");
		append_stale_active_release_audit(&context.state_store, &issue.id);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
		assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
		assert!(diagnostic.evidence.contains(&String::from("stale_active_local_cleanup_complete")));
		super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect("reentry release should remove active label");
		assert_eq!(
			tracker.label_removals.borrow().as_slice(),
			&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
		);
		assert_eq!(
			tracker.state_updates.borrow().as_slice(),
			&[(issue.id.clone(), String::from("state-todo"))]
		);
	}

	#[test]
	fn stale_active_release_reentry_blocks_active_status_after_local_cleanup_audit() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
		let worktree_path = context.config.worktree_root().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&context.state_store, &issue, &worktree_path);
		context
			.state_store
			.update_run_status("run-1626", "running")
			.expect("run should carry active status");
		context
			.state_store
			.retire_run_control_channel_for_attempt(
				"run-1626",
				1,
				RUN_CONTROL_CHANNEL_STATUS_FAILED,
			)
			.expect("control channel should retire");
		fs::remove_dir_all(&worktree_path).expect("worktree should be removed");
		context
			.state_store
			.clear_worktree_mapping(&issue.id)
			.expect("issue-id worktree mapping should clear");
		append_stale_active_release_audit(&context.state_store, &issue.id);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, "stale_active_recovery_blocked");
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("protocol_event_evidence_present")));
		assert!(diagnostic.blockers.contains(&String::from("child_agent_activity_present")));
		assert!(diagnostic.blockers.contains(&String::from("protocol_activity_present")));
	}

	#[test]
	fn stale_active_release_reentry_terminal_guards_terminal_looking_audited_run() {
		for status in ["failed", "interrupted"] {
			let temp_dir = TempDir::new().expect("tempdir should create");
			let context = sample_recovery_context(
				&temp_dir,
				super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
			);
			let active_label = tracker::automation_active_label(context.config.service_id());
			let queue_label = tracker::automation_queue_label(context.config.service_id());
			let mut issue =
				sample_issue_with_labels("In Progress", &[active_label.clone(), queue_label]);
			let worktree_path = context.config.worktree_root().join("PUB-1626");

			issue.identifier = String::from("PUB-1626");
			seed_dead_orphan_runtime_telemetry(&context.state_store, &issue, &worktree_path);
			context
				.state_store
				.update_run_status("run-1626", status)
				.expect("run should carry terminal-looking app-server status");
			context
				.state_store
				.retire_run_control_channel_for_attempt(
					"run-1626",
					1,
					RUN_CONTROL_CHANNEL_STATUS_FAILED,
				)
				.expect("control channel should retire");
			fs::remove_dir_all(&worktree_path).expect("worktree should be removed");
			context
				.state_store
				.clear_worktree_mapping(&issue.id)
				.expect("issue-id worktree mapping should clear");
			append_stale_active_release_audit(&context.state_store, &issue.id);

			let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
			let mut diagnostics = super::diagnose_stale_active_issues(
				context.config.service_id(),
				&context.workflow,
				context.config.worktree_root(),
				&context.state_store,
				&tracker,
				Some("PUB-1626"),
				super::RecoveryRuntimeMutationPolicy::ReadOnly,
			)
			.expect("stale active diagnosis should run");
			let diagnostic = diagnostics.pop().expect("diagnostic should exist");

			assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
			assert!(diagnostic.recoverable(), "{status} blockers: {:?}", diagnostic.blockers);
			assert_eq!(diagnostic.latest_attempt_status.as_deref(), Some(status));
			assert!(
				diagnostic.evidence.contains(&String::from("stale_active_local_cleanup_complete"))
			);

			super::apply_stale_active_release_with_tracker(
				&tracker,
				&context.config,
				&context.workflow,
				&context.state_store,
				&diagnostic,
			)
			.expect("reentry release should terminal-guard and remove active label");

			let run = context
				.state_store
				.run_attempt("run-1626")
				.expect("run attempt should read")
				.expect("run should exist");

			assert_eq!(run.status(), GHOST_LANE_TERMINAL_STATUS);
			assert_eq!(
				tracker.label_removals.borrow().as_slice(),
				&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
			);
			assert_eq!(
				tracker.state_updates.borrow().as_slice(),
				&[(issue.id.clone(), String::from("state-todo"))]
			);
		}
	}

	#[test]
	fn stale_active_release_allows_reentry_after_local_cleanup_without_control_channel() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue =
			sample_issue_with_labels("In Progress", &[active_label.clone(), queue_label]);
		let worktree_path = context.config.worktree_root().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		init_clean_git_repo_with_remote_default(&worktree_path, "x/pubfi-pub-1626");
		seed_dead_orphan_runtime_telemetry_without_control_channel(
			&context.state_store,
			&issue,
			&worktree_path,
		);
		context
			.state_store
			.update_run_status("run-1626", GHOST_LANE_TERMINAL_STATUS)
			.expect("run should terminalize");
		fs::remove_dir_all(&worktree_path).expect("worktree should be removed");
		context
			.state_store
			.clear_worktree_mapping(&issue.id)
			.expect("issue-id worktree mapping should clear");
		append_stale_active_release_audit(&context.state_store, &issue.id);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
		assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
		assert!(diagnostic.evidence.contains(&String::from("control_channel_missing")));
		assert!(diagnostic.evidence.contains(&String::from("stale_active_local_cleanup_complete")));

		super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect("reentry release should remove active label");
		assert_eq!(
			tracker.label_removals.borrow().as_slice(),
			&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
		);
	}

	#[test]
	fn stale_active_release_reentry_without_control_channel_blocks_private_progress() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
		let worktree_path = context.config.worktree_root().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		init_clean_git_repo_with_remote_default(&worktree_path, "x/pubfi-pub-1626");
		seed_dead_orphan_runtime_telemetry_without_control_channel(
			&context.state_store,
			&issue,
			&worktree_path,
		);
		context
			.state_store
			.update_run_status("run-1626", GHOST_LANE_TERMINAL_STATUS)
			.expect("run should terminalize");
		fs::remove_dir_all(&worktree_path).expect("worktree should be removed");
		context
			.state_store
			.clear_worktree_mapping(&issue.id)
			.expect("issue-id worktree mapping should clear");
		append_stale_active_release_audit(&context.state_store, &issue.id);
		append_harness_outcome_with_pr_progress(&context.state_store, &issue.id);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.evidence.contains(&String::from("control_channel_missing")));
		assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_release_reentry_restores_startable_state_after_active_label_release() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("In Progress", &[queue_label]);
		let worktree_path = context.config.worktree_root().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&context.state_store, &issue, &worktree_path);
		context
			.state_store
			.update_run_status("run-1626", GHOST_LANE_TERMINAL_STATUS)
			.expect("run should terminalize");
		context
			.state_store
			.retire_run_control_channel_for_attempt(
				"run-1626",
				1,
				RUN_CONTROL_CHANNEL_STATUS_FAILED,
			)
			.expect("control channel should retire");
		fs::remove_dir_all(&worktree_path).expect("worktree should be removed");
		context
			.state_store
			.clear_worktree_mapping(&issue.id)
			.expect("issue-id worktree mapping should clear");
		append_stale_active_release_audit(&context.state_store, &issue.id);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_STATE_RESTORE_CLASSIFICATION);
		assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
		assert!(
			diagnostic
				.evidence
				.contains(&String::from("stale_active_startable_state_restore_pending"))
		);
		super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect("state-restore reentry should complete");
		assert!(tracker.label_removals.borrow().is_empty());
		assert_eq!(
			tracker.state_updates.borrow().as_slice(),
			&[(issue.id.clone(), String::from("state-todo"))]
		);
	}

	#[test]
	fn stale_active_release_reentry_rejects_release_audit_from_other_run() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
		let worktree_path = context.config.worktree_root().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&context.state_store, &issue, &worktree_path);
		context
			.state_store
			.update_run_status("run-1626", GHOST_LANE_TERMINAL_STATUS)
			.expect("run should terminalize");
		context
			.state_store
			.retire_run_control_channel_for_attempt(
				"run-1626",
				1,
				RUN_CONTROL_CHANNEL_STATUS_FAILED,
			)
			.expect("control channel should retire");
		fs::remove_dir_all(&worktree_path).expect("worktree should be removed");
		context
			.state_store
			.clear_worktree_mapping(&issue.id)
			.expect("issue-id worktree mapping should clear");
		append_stale_active_release_audit_for_run(&context.state_store, &issue.id, "run-older", 1);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(
			!diagnostic.evidence.contains(&String::from("stale_active_local_cleanup_complete"))
		);
		assert!(diagnostic.blockers.contains(&String::from("protocol_activity_present")));
	}

	#[test]
	fn stale_active_diagnose_blocks_private_progress_from_older_attempt() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-old", &issue.id, 1, "running")
			.expect("old run attempt should record");
		store
			.record_run_attempt("run-new", &issue.id, 2, "running")
			.expect("new run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.append_private_execution_event(
				"pubfi",
				&issue.id,
				"run-old",
				1,
				"source_progress",
				serde_json::json!({"phase": "implementation"}),
			)
			.expect("private progress should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.latest_run_id.as_deref(), Some("run-new"));
		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_protocol_event_evidence() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.append_event("run-1626", 1, "turn/item", r#"{"kind":"progress"}"#)
			.expect("protocol event should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("protocol_event_evidence_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_marker_protocol_activity_evidence() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");
		state::write_run_protocol_activity_marker(
			&worktree_path,
			&ProtocolActivityMarker {
				run_id: "run-1626",
				attempt_number: 1,
				thread_id: Some("thread-stale"),
				turn_id: Some("turn-stale"),
				event_count: 1,
				last_event_type: "turn/completed",
				child_agent_activity: None,
				protocol_activity: None,
			},
		)
		.expect("protocol marker should write");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("protocol_event_marker_present")));
		assert!(
			diagnostic
				.blockers
				.contains(&String::from("activity_marker_protocol_activity_present"))
		);
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_untracked_worktree_progress() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		init_git_repo(&worktree_path);
		fs::write(worktree_path.join("new_source.rs"), "fn retained_progress() {}\n")
			.expect("untracked source should write");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("worktree_tracked_changes_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_non_git_retained_files() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		fs::create_dir_all(&worktree_path).expect("retained path should create");
		fs::write(worktree_path.join("retained.txt"), "retained work\n")
			.expect("retained file should write");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.worktree_state, "non_git_files_present");
		assert!(diagnostic.blockers.contains(&String::from("non_git_worktree_files_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_child_agent_activity_summary() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let activity = ChildAgentActivitySummary { event_count: 1, ..Default::default() };

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.record_run_activity_summary("run-1626", 1, Some(&activity), None)
			.expect("child activity should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("child_agent_activity_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_when_worktree_status_unknown() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let worktree_path = temp_dir.path().join("PUB-1626");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		fs::create_dir_all(&worktree_path).expect("worktree path should create");
		fs::write(worktree_path.join(".git"), "gitdir: /does/not/exist\n")
			.expect("invalid gitdir should write");
		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.worktree_state, "tracked_changes_unknown");
		assert!(diagnostic.blockers.contains(&String::from("worktree_tracked_changes_unknown")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_needs_attention_label() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let needs_attention_label = String::from("decodex:needs-attention");
		let mut issue = sample_issue_with_labels("Todo", &[active_label, needs_attention_label]);

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("needs_attention_label_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_review_policy_checkpoint() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
				project_id: "pubfi",
				issue_id: &issue.id,
				run_id: "run-1626",
				attempt_number: 1,
				phase: "handoff",
				review_level: "normal",
				status: "clean",
				head_sha: "2222222222222222222222222222222222222222",
				nonclean_rounds: 0,
				details_json: "{}",
			})
			.expect("review checkpoint should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("review_policy_checkpoint_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_identifier_keyed_review_policy_checkpoint() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.id = String::from("linear-issue-1626");
		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
				project_id: "pubfi",
				issue_id: "PUB-1626",
				run_id: "run-1626",
				attempt_number: 1,
				phase: "handoff",
				review_level: "normal",
				status: "clean",
				head_sha: "2222222222222222222222222222222222222222",
				nonclean_rounds: 0,
				details_json: "{}",
			})
			.expect("identifier-keyed review checkpoint should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("review_policy_checkpoint_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_identifier_keyed_pr_lineage() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let mut event = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "pubfi",
				issue_id: "PUB-1626",
				issue_identifier: "PUB-1626",
				run_id: "run-1626",
				attempt_number: 1,
			},
			"review_handoff",
			String::from("2026-06-28T00:00:00Z"),
			"review_handoff",
		);

		issue.id = String::from("linear-issue-1626");
		issue.identifier = String::from("PUB-1626");
		event.branch = Some(String::from("x/pubfi-pub-1626"));
		event.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/1626"));
		event.pr_head_sha = Some(String::from("2222222222222222222222222222222222222222"));
		event.pr_base_ref = Some(String::from("main"));
		event.commit_sha = Some(String::from("3333333333333333333333333333333333333333"));
		event.validation_result = Some(String::from("passed"));
		event.summary = Some(String::from("Recorded review handoff lineage."));
		event.terminal_path = Some(String::from("review_handoff"));
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store.record_linear_execution_event(&event).expect("linear event should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("pr_or_review_lineage_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_tracker_comment_pr_lineage() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let mut event = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "pubfi",
				issue_id: "linear-issue-1626",
				issue_identifier: "PUB-1626",
				run_id: "run-1626",
				attempt_number: 1,
			},
			"review_handoff",
			String::from("2026-06-28T00:00:00Z"),
			"review_handoff",
		);

		issue.id = String::from("linear-issue-1626");
		issue.identifier = String::from("PUB-1626");
		event.branch = Some(String::from("x/pubfi-pub-1626"));
		event.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/1626"));
		event.pr_head_sha = Some(String::from("2222222222222222222222222222222222222222"));
		event.pr_base_ref = Some(String::from("main"));
		event.commit_sha = Some(String::from("3333333333333333333333333333333333333333"));
		event.validation_result = Some(String::from("passed"));
		event.summary = Some(String::from("Recorded review handoff lineage."));
		event.terminal_path = Some(String::from("review_handoff"));

		let comment = TrackerComment {
			body: records::append_structured_comment_record(
				&records::render_linear_execution_event_comment_body(&event, None),
				&event,
			)
			.expect("structured comment should serialize"),
			created_at: String::from("2026-06-28T00:00:00Z"),
		};

		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]).with_comments(vec![comment]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("pr_or_review_lineage_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_identifier_keyed_review_lifecycle() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let marker = ReviewHandoffMarker::new(
			"run-1626",
			1,
			"x/pubfi-pub-1626",
			"https://github.com/hack-ink/decodex/pull/1626",
			"main",
			"x/pubfi-pub-1626",
			"2222222222222222222222222222222222222222",
		);

		issue.id = String::from("linear-issue-1626");
		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.upsert_review_handoff_marker("pubfi", "PUB-1626", &marker)
			.expect("identifier-keyed review lifecycle should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("review_lifecycle_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_unreadable_activity_marker() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		fs::create_dir_all(worktree_path.join(state::RUN_ACTIVITY_MARKER_FILE))
			.expect("directory marker should create");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("worktree_tracked_changes_unknown")));
		assert!(
			diagnostic.evidence.iter().any(|entry| entry.starts_with("worktree_status_error:")),
			"diagnostic should include marker read error evidence: {:?}",
			diagnostic.evidence
		);
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_release_removes_active_label_and_terminalizes_stale_run() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

		issue.identifier = String::from("PUB-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"x/pubfi-pub-1626",
				&context.config.worktree_root().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect("stale active release should apply");

		let run = context
			.state_store
			.run_attempt("run-1626")
			.expect("run attempt should read")
			.expect("run should exist");
		let events = context
			.state_store
			.list_private_execution_events("pubfi", &issue.id, "run-1626", 1)
			.expect("private events should read");

		assert_eq!(run.status(), GHOST_LANE_TERMINAL_STATUS);
		assert_eq!(
			tracker.label_removals.borrow().as_slice(),
			&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
		);
		assert!(events.iter().any(|event| {
			event.event_type() == STALE_ACTIVE_RELEASE_EVENT
				&& event.payload()["schema"] == super::STALE_ACTIVE_RECOVERY_SCHEMA
				&& event.payload()["active_label_release"] == "pending_final_mutation"
				&& event.payload()["phase"] == "local_cleanup_complete_before_active_label_release"
		}));
	}

	#[test]
	fn stale_active_release_allows_final_reentry_when_control_channel_was_never_published() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);
		let worktree_path = context.config.worktree_root().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		init_git_repo(context.config.repo_root());
		run_git(context.config.repo_root(), &["checkout", "-B", "main"]);
		commit_test_file(context.config.repo_root(), "README.md", "base\n", "base");
		run_git(context.config.repo_root(), &["update-ref", "refs/remotes/origin/main", "HEAD"]);
		run_git(
			context.config.repo_root(),
			&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"],
		);
		run_git(
			context.config.repo_root(),
			&[
				"worktree",
				"add",
				"-b",
				"x/pubfi-pub-1626",
				worktree_path.to_str().expect("worktree path should be utf-8"),
				"main",
			],
		);
		seed_dead_orphan_runtime_telemetry_without_control_channel(
			&context.state_store,
			&issue,
			&worktree_path,
		);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
		assert!(diagnostic.evidence.contains(&String::from("control_channel_missing")));

		super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect("stale active release should treat missing control channel as inactive reentry");

		assert_eq!(
			tracker.label_removals.borrow().as_slice(),
			&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
		);
	}

	#[test]
	fn stale_active_release_terminal_guards_terminal_looking_run_before_final_safety_check() {
		for status in ["failed", "interrupted"] {
			let temp_dir = TempDir::new().expect("tempdir should create");
			let context = sample_recovery_context(
				&temp_dir,
				super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
			);
			let active_label = tracker::automation_active_label(context.config.service_id());
			let queue_label = tracker::automation_queue_label(context.config.service_id());
			let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);
			let worktree_path = context.config.worktree_root().join("PUB-1626");

			issue.identifier = String::from("PUB-1626");
			init_git_repo(context.config.repo_root());
			run_git(context.config.repo_root(), &["checkout", "-B", "main"]);
			commit_test_file(context.config.repo_root(), "README.md", "base\n", "base");
			run_git(
				context.config.repo_root(),
				&["update-ref", "refs/remotes/origin/main", "HEAD"],
			);
			run_git(
				context.config.repo_root(),
				&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"],
			);
			run_git(
				context.config.repo_root(),
				&[
					"worktree",
					"add",
					"-b",
					"x/pubfi-pub-1626",
					worktree_path.to_str().expect("worktree path should be utf-8"),
					"main",
				],
			);
			seed_dead_orphan_runtime_telemetry_without_control_channel(
				&context.state_store,
				&issue,
				&worktree_path,
			);
			context
				.state_store
				.update_run_status("run-1626", status)
				.expect("run should carry terminal-looking app-server status");

			let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
			let mut diagnostics = super::diagnose_stale_active_issues(
				context.config.service_id(),
				&context.workflow,
				context.config.worktree_root(),
				&context.state_store,
				&tracker,
				Some("PUB-1626"),
				super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
			)
			.expect("stale active diagnosis should run");
			let diagnostic = diagnostics.pop().expect("diagnostic should exist");

			assert!(diagnostic.recoverable(), "{status} blockers: {:?}", diagnostic.blockers);
			assert_eq!(diagnostic.latest_attempt_status.as_deref(), Some(status));

			super::apply_stale_active_release_with_tracker(
				&tracker,
				&context.config,
				&context.workflow,
				&context.state_store,
				&diagnostic,
			)
			.expect("terminal-looking stale-active run should release after terminal guard");

			let run = context
				.state_store
				.run_attempt("run-1626")
				.expect("run attempt should read")
				.expect("run should exist");

			assert_eq!(run.status(), GHOST_LANE_TERMINAL_STATUS);
			assert_eq!(
				tracker.label_removals.borrow().as_slice(),
				&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
			);
		}
	}

	#[test]
	fn stale_active_release_removes_run_control_marker_only_directory() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);
		let worktree_path = context.config.worktree_root().join("PUB-1626");
		let control_dir = worktree_path.join(state::RUN_CONTROL_CHANNEL_DIR);

		issue.identifier = String::from("PUB-1626");
		fs::create_dir_all(&control_dir).expect("run-control marker directory should create");
		fs::write(control_dir.join("run-1626-1.channel"), "channel\n")
			.expect("run-control marker should write");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect("stale active release should apply");

		assert!(!worktree_path.exists(), "marker-only directory should be removed");
		assert_eq!(
			tracker.label_removals.borrow().as_slice(),
			&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
		);
	}

	#[test]
	fn stale_active_release_keeps_active_label_gate_when_tracker_label_removal_fails() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

		issue.identifier = String::from("PUB-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"x/pubfi-pub-1626",
				&context.config.worktree_root().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()])
			.remove_error("Linear label removal failed");
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		let error = super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect_err("tracker removal failure should abort release");
		let run = context
			.state_store
			.run_attempt("run-1626")
			.expect("run attempt should read")
			.expect("run should exist");
		let events = context
			.state_store
			.list_private_execution_events("pubfi", &issue.id, "run-1626", 1)
			.expect("private events should read");
		let mapping = context
			.state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree mapping should read");

		assert!(error.to_string().contains("Linear label removal failed"));
		assert_eq!(run.status(), GHOST_LANE_TERMINAL_STATUS);
		assert!(events.iter().any(|event| event.event_type() == STALE_ACTIVE_RELEASE_EVENT));
		assert!(mapping.is_none());
		assert_eq!(
			tracker.label_removals.borrow().as_slice(),
			&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
		);
	}

	#[test]
	fn stale_active_release_revalidates_needs_attention_before_final_label_removal() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let needs_attention_label =
			context.workflow.frontmatter().tracker().needs_attention_label().to_owned();
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.identifier = String::from("PUB-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"x/pubfi-pub-1626",
				&context.config.worktree_root().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = FinalNeedsAttentionTracker::new(issue, needs_attention_label);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("initial stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		let error = super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect_err("late needs-attention should block active-label release");
		let message = error.to_string();

		assert!(message.contains("safety inspection changed before apply"));
		assert!(message.contains("needs_attention_label_present"));
		assert!(
			tracker.label_removals.borrow().is_empty(),
			"active label should not be removed after late needs-attention appears"
		);
	}

	#[test]
	fn stale_active_release_preflight_rejects_worktree_progress_after_diagnosis() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);
		let worktree_path = context.config.worktree_root().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		init_clean_git_repo_with_remote_default(&worktree_path, "x/pubfi-pub-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		fs::write(worktree_path.join("late_progress.rs"), "fn late_progress() {}\n")
			.expect("late untracked progress should write");
		let error =
			super::preflight_stale_active_worktree_cleanup(&context.state_store, &diagnostic)
				.expect_err("preflight should reject late retained progress");

		assert!(
			error.to_string().contains("retained worktree changes appeared before cleanup"),
			"unexpected preflight error: {error:?}"
		);
	}

	#[test]
	fn stale_active_release_revalidates_late_default_worktree_progress_without_mapping() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);
		let default_worktree_path = context.config.worktree_root().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		init_git_repo(&default_worktree_path);
		fs::write(default_worktree_path.join("late_default_progress.rs"), "fn late() {}\n")
			.expect("late default progress should write");
		let error = super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect_err("late default worktree progress should block release");
		let run = context
			.state_store
			.run_attempt("run-1626")
			.expect("run attempt should read")
			.expect("run should exist");

		assert!(
			error.to_string().contains("safety inspection changed before apply"),
			"unexpected release error: {error:?}"
		);
		assert_eq!(run.status(), "running");
		assert!(tracker.label_removals.borrow().is_empty());
	}

	#[test]
	fn stale_active_release_revalidates_late_run_lease_before_mutation() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

		issue.identifier = String::from("PUB-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"x/pubfi-pub-1626",
				&context.config.worktree_root().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		context
			.state_store
			.upsert_lease(context.config.service_id(), &issue.id, "run-1626", "In Progress")
			.expect("late lease should record");

		let error = super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect_err("late run lease should block release");
		let run = context
			.state_store
			.run_attempt("run-1626")
			.expect("run attempt should read")
			.expect("run should exist");

		assert!(
			error.to_string().contains("safety inspection changed before apply"),
			"unexpected release error: {error:?}"
		);
		assert_eq!(run.status(), "running");
		assert!(tracker.label_removals.borrow().is_empty());
	}

	#[test]
	fn stale_active_release_revalidates_late_review_policy_before_mutation() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);

		issue.identifier = String::from("PUB-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"x/pubfi-pub-1626",
				&context.config.worktree_root().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		context
			.state_store
			.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
				project_id: context.config.service_id(),
				issue_id: &issue.id,
				run_id: "run-1626",
				attempt_number: 1,
				phase: "handoff",
				review_level: "normal",
				status: "clean",
				head_sha: "2222222222222222222222222222222222222222",
				nonclean_rounds: 0,
				details_json: "{}",
			})
			.expect("late review checkpoint should record");

		let error = super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect_err("late review checkpoint should block release");
		let run = context
			.state_store
			.run_attempt("run-1626")
			.expect("run attempt should read")
			.expect("run should exist");

		assert!(
			error.to_string().contains("safety inspection changed before apply")
				|| error.to_string().contains("review authority appeared"),
			"unexpected release error: {error:?}"
		);
		assert_eq!(run.status(), "running");
		assert!(tracker.label_removals.borrow().is_empty());
	}

	#[test]
	fn stale_active_final_label_guard_rejects_late_run_lease() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);

		issue.identifier = String::from("PUB-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		context
			.state_store
			.upsert_lease(context.config.service_id(), &issue.id, "run-1626", "In Progress")
			.expect("late lease should record");

		let error = super::ensure_stale_active_run_claim_guard(
			&context.config,
			&context.state_store,
			&diagnostic,
		)
		.expect_err("final guard should reject late lease");

		assert!(
			error.to_string().contains("appeared before active-label release"),
			"unexpected final guard error: {error:?}"
		);
	}

	#[test]
	fn stale_active_diagnose_blocks_when_run_lease_is_present() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", &issue.id, "run-1626", "In Progress")
			.expect("lease should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("run_lease_present")));
		assert!(diagnostic.blockers.contains(&String::from("active_shared_claim_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn recovery_read_only_backoff_observer_does_not_clear_expired_backoff() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let expired_unix_epoch = time::OffsetDateTime::now_utc().unix_timestamp() - 1;

		context
			.state_store
			.upsert_connector_backoff(ConnectorBackoffInput {
				project_id: context.config.service_id(),
				connector: "linear",
				sync_phase: "ghost_lane_recovery",
				quota_class: "linear_graphql_rate_limit",
				reset_unix_epoch: expired_unix_epoch,
				reset_source: "test",
				warning: super::LINEAR_RATE_LIMIT_BACKOFF_WARNING,
			})
			.expect("backoff should persist");

		let message = super::active_recovery_tracker_backoff_message(&context)
			.expect("backoff observer should run");

		assert_eq!(message, None);
		assert!(
			context
				.state_store
				.connector_backoff(context.config.service_id(), "linear")
				.expect("backoff should read")
				.is_some(),
			"read-only recovery diagnostics must not clear stored connector backoff"
		);
	}

	#[test]
	fn recovery_read_only_backoff_recorder_does_not_persist_new_backoff() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let error = crate::prelude::eyre::eyre!("Linear connector timed out while testing");
		let message = super::remember_recovery_tracker_backoff_message(
			&context,
			&error,
			"ghost_lane_recovery",
		)
		.expect("timeout should produce backoff message");

		assert!(message.contains("Linear connector is in backoff"));
		assert!(
			context
				.state_store
				.connector_backoff(context.config.service_id(), "linear")
				.expect("backoff should read")
				.is_none(),
			"read-only recovery diagnostics must not persist new connector backoff"
		);
	}

	#[test]
	fn review_handoff_diagnose_skips_terminal_identifier_worktree_before_tracker_refresh() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::missing();
		let stale_issue_id = "PUB-001";
		let stale_worktree_path = context.config.worktree_root().join(stale_issue_id);

		context
			.state_store
			.record_run_attempt("run-01", stale_issue_id, 1, super::GHOST_LANE_TERMINAL_STATUS)
			.expect("terminal attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				stale_issue_id,
				"x/pubfi-pub-001",
				&stale_worktree_path.display().to_string(),
			)
			.expect("stale worktree mapping should record");

		let diagnostics =
			super::diagnose_all_retained_review_worktrees_with_tracker(&context, &tracker)
				.expect("retained review diagnostics should build");
		let diagnostic = diagnostics.first().expect("local residue diagnostic should render");

		assert_eq!(diagnostics.len(), 1);
		assert_eq!(
			diagnostic.classification,
			super::REVIEW_HANDOFF_STALE_TERMINAL_RESIDUE_CLASSIFICATION
		);
		assert_eq!(diagnostic.issue_id, stale_issue_id);
		assert_eq!(diagnostic.issue_state, "local_terminal_residue");
		assert!(
			tracker.refresh_queries.borrow().is_empty(),
			"terminal local identifier residue must not be sent to tracker refresh"
		);
	}

	#[test]
	fn review_handoff_diagnose_targeted_terminal_identifier_worktree_before_tracker_lookup() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::identifier_error(
			"Linear GraphQL request failed: Argument Validation Error",
		);
		let stale_issue_id = "PUB-001";
		let stale_worktree_path = context.config.worktree_root().join(stale_issue_id);

		context
			.state_store
			.record_run_attempt("run-01", stale_issue_id, 1, super::GHOST_LANE_TERMINAL_STATUS)
			.expect("terminal attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				stale_issue_id,
				"x/pubfi-pub-001",
				&stale_worktree_path.display().to_string(),
			)
			.expect("stale worktree mapping should record");

		let diagnostic = super::diagnose_issue_with_tracker(&context, &tracker, stale_issue_id)
			.expect("targeted retained review diagnostic should classify local residue");

		assert_eq!(
			diagnostic.classification,
			super::REVIEW_HANDOFF_STALE_TERMINAL_RESIDUE_CLASSIFICATION
		);
		assert_eq!(diagnostic.issue_id, stale_issue_id);
		assert_eq!(diagnostic.issue_state, "local_terminal_residue");
		assert!(
			tracker.refresh_queries.borrow().is_empty(),
			"targeted terminal local residue must not be sent to tracker refresh"
		);
	}

	#[test]
	fn ghost_lane_live_status_overlay_tracker_backoff_stays_read_only() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let missing_tracker = GhostLaneTestTracker::missing();
		let error_tracker =
			GhostLaneTestTracker::refresh_error("Linear connector timed out while testing");

		context
			.state_store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let mut diagnostics = super::diagnose_ghost_lanes_read_only(
			context.config.service_id(),
			context.config.worktree_root(),
			&context.state_store,
			&missing_tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let error = super::apply_ghost_lane_live_status_blockers_with_tracker(
			&error_tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&mut diagnostics,
		)
		.expect_err("overlay tracker error should surface for recovery backoff wrapping");
		let message = super::remember_recovery_tracker_backoff_message(
			&context,
			&error,
			"ghost_lane_recovery",
		)
		.expect("timeout should become a recovery backoff message");

		assert!(message.contains("ghost_lane_recovery"));
		assert!(
			context
				.state_store
				.connector_backoff(context.config.service_id(), "linear")
				.expect("backoff should read")
				.is_none(),
			"read-only live-status overlay must not persist connector backoff"
		);
	}

	#[test]
	fn ghost_lane_diagnose_live_status_overlay_blocks_active_thread_marker() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::missing();
		let worktree_path = context.config.worktree_root().join("PUB-012");
		let mut diagnostics = vec![super::GhostLaneDiagnostic {
			project_id: String::from("pubfi"),
			issue_id: String::from("PUB-012"),
			issue_identifier: Some(String::from("PUBFI-012")),
			run_id: String::from("run-12"),
			attempt_number: 1,
			attempt_status: String::from("running"),
			classification: String::from(GHOST_LANE_CLASSIFICATION),
			reason: String::from("test"),
			run_lease: true,
			control_channel: String::from("missing"),
			evidence: Vec::new(),
			blockers: Vec::new(),
			next_action: String::from("test"),
		}];

		fs::create_dir_all(&worktree_path).expect("worktree path should exist");

		context
			.state_store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		context
			.state_store
			.upsert_worktree(
				"pubfi",
				"PUB-012",
				"x/pubfi-pub-012",
				&worktree_path.display().to_string(),
			)
			.expect("worktree should record");

		state::write_run_thread_status_marker(
			&worktree_path,
			"run-12",
			1,
			Some("thread-12"),
			Some("turn-12"),
			"active",
			&[String::from("waitingOnApproval")],
		)
		.expect("active thread marker should write");
		super::apply_ghost_lane_live_status_blockers_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&mut diagnostics,
		)
		.expect("status overlay should run");

		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("status:thread_active")));
		assert!(diagnostic.blockers.contains(&String::from("status:retained_worktree_present")));
	}

	#[test]
	fn ghost_lane_cleanup_live_status_gate_rejects_active_thread_marker() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::missing();
		let worktree_path = context.config.worktree_root().join("PUB-012");
		let diagnostic = super::GhostLaneDiagnostic {
			project_id: String::from("pubfi"),
			issue_id: String::from("PUB-012"),
			issue_identifier: Some(String::from("PUBFI-012")),
			run_id: String::from("run-12"),
			attempt_number: 1,
			attempt_status: String::from("running"),
			classification: String::from(GHOST_LANE_CLASSIFICATION),
			reason: String::from("test"),
			run_lease: true,
			control_channel: String::from("missing"),
			evidence: Vec::new(),
			blockers: Vec::new(),
			next_action: String::from("test"),
		};

		fs::create_dir_all(&worktree_path).expect("worktree path should exist");

		context
			.state_store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		context
			.state_store
			.upsert_worktree(
				"pubfi",
				"PUB-012",
				"x/pubfi-pub-012",
				&worktree_path.display().to_string(),
			)
			.expect("worktree should record");

		state::write_run_thread_status_marker(
			&worktree_path,
			"run-12",
			1,
			Some("thread-12"),
			Some("turn-12"),
			"active",
			&[String::from("waitingOnApproval")],
		)
		.expect("active thread marker should write");

		let error = super::ensure_ghost_lane_live_status_allows_cleanup_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect_err("live status should reject cleanup");
		let message = format!("{error:#}");

		assert!(message.contains("live status reported blockers"));
		assert!(message.contains("thread_active"));
		assert!(message.contains("retained_worktree_present"));
	}

	#[test]
	fn ghost_lane_cleanup_terminalizes_missing_issue_lease_and_records_private_audit() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let mut diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUBFI-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_CLASSIFICATION);
		assert!(diagnostic.recoverable());
		assert_eq!(diagnostic.issue_id, "PUB-012");
		assert_eq!(diagnostic.issue_identifier.as_deref(), Some("PUBFI-012"));
		assert!(diagnostic.evidence.contains(&String::from("tracker_issue_missing")));
		assert!(diagnostic.evidence.contains(&String::from("worktree_missing")));
		assert!(diagnostic.evidence.contains(&String::from("control_channel_missing")));
		assert!(diagnostic.evidence.contains(&String::from("private_evidence_missing")));
		assert!(diagnostic.evidence.contains(&String::from("review_lineage_missing")));

		super::apply_ghost_lane_cleanup(&store, &diagnostic).expect("cleanup should apply");

		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").is_empty(),
			"cleanup should clear the local run lease"
		);

		let runs =
			store.list_project_issue_runs("pubfi", "PUB-012").expect("issue runs should load");

		assert_eq!(runs.len(), 1);
		assert_eq!(runs[0].status(), GHOST_LANE_TERMINAL_STATUS);

		let events = store
			.list_private_execution_events("pubfi", "PUB-012", "run-12", 1)
			.expect("private events should load");

		assert_eq!(events.len(), 1);
		assert_eq!(events[0].event_type(), GHOST_LANE_CLEANUP_EVENT);
		assert_eq!(
			events[0].payload()["schema"].as_str(),
			Some("decodex.ghost_lane_recovery_private_event/1")
		);
		assert_eq!(events[0].payload()["cleared_run_lease"].as_bool(), Some(true));
	}

	#[test]
	fn ghost_lane_cleanup_dry_run_validation_keeps_runtime_state_untouched() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::missing();

		context
			.state_store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let diagnostics = super::diagnose_ghost_lanes_read_only(
			context.config.service_id(),
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		super::ensure_ghost_lane_live_status_allows_cleanup_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			diagnostic,
		)
		.expect("dry-run validation should allow cleanup");

		let runs = context
			.state_store
			.list_project_issue_runs("pubfi", "PUB-012")
			.expect("issue runs should load");
		let events = context
			.state_store
			.list_private_execution_events("pubfi", "PUB-012", "run-12", 1)
			.expect("private events should load");

		assert_eq!(runs.len(), 1);
		assert_eq!(runs[0].status(), "running");
		assert!(
			!context
				.state_store
				.list_leased_runs("pubfi")
				.expect("leased runs should load")
				.is_empty(),
			"dry-run validation must not clear the run lease"
		);
		assert!(events.is_empty(), "dry-run validation must not write cleanup audit events");
	}

	#[test]
	fn ghost_lane_diagnostic_allows_mcp_test_fixture_control_evidence() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::missing();

		seed_mcp_test_fixture_ghost_lane(&context.state_store, context.config.worktree_root());

		let diagnostics = super::diagnose_ghost_lanes_read_only(
			context.config.service_id(),
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("mcp-test fixture ghost lane should diagnose");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION);
		assert!(diagnostic.recoverable());
		assert!(diagnostic.blockers.is_empty());
		assert!(
			diagnostic
				.evidence
				.contains(&String::from("mcp_test_fixture_private_control_evidence_present"))
		);
		assert!(
			diagnostic
				.evidence
				.contains(&String::from("mcp_test_fixture_protocol_or_thread_evidence_present"))
		);
	}

	#[test]
	fn ghost_lane_diagnostic_allows_prior_mcp_test_fixture_cleanup_audit() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::missing();

		seed_mcp_test_fixture_ghost_lane(&context.state_store, context.config.worktree_root());
		append_mcp_test_fixture_ghost_lane_cleanup_audit(&context.state_store);

		let diagnostics = super::diagnose_ghost_lanes_read_only(
			context.config.service_id(),
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUBFI-012"),
		)
		.expect("prior cleanup audit should not block an idempotent diagnosis");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION);
		assert!(diagnostic.recoverable());
		assert!(diagnostic.blockers.is_empty());
		assert!(
			diagnostic
				.evidence
				.contains(&String::from("mcp_test_fixture_private_control_evidence_present"))
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_mcp_fixture_has_mixed_private_evidence() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::missing();

		seed_mcp_test_fixture_ghost_lane(&context.state_store, context.config.worktree_root());

		context
			.state_store
			.append_private_execution_event(
				"pubfi",
				"PUB-012",
				"run-12",
				1,
				"progress_checkpoint",
				serde_json::json!({"source": "runtime", "phase": "implementing"}),
			)
			.expect("real private evidence should record");

		let diagnostics = super::diagnose_ghost_lanes_read_only(
			context.config.service_id(),
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("private_evidence_present")));
	}

	#[test]
	fn ghost_lane_diagnostic_treats_invalid_local_issue_id_refresh_as_missing_issue() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::refresh_error(
			"Linear GraphQL request failed: Argument Validation Error",
		);

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let diagnostics = super::diagnose_ghost_lanes_read_only(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("missing local issue id should not abort ghost-lane diagnosis");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_CLASSIFICATION);
		assert!(diagnostic.recoverable());
		assert!(diagnostic.evidence.contains(&String::from("tracker_issue_missing")));
	}

	#[test]
	fn ghost_lane_diagnostic_treats_missing_identifier_lookup_as_missing_issue() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::identifier_error(
			"Linear GraphQL request failed: Entity not found: Issue",
		);

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let diagnostics = super::diagnose_ghost_lanes_read_only(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("missing issue identifier should not abort ghost-lane diagnosis");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_CLASSIFICATION);
		assert!(diagnostic.recoverable());
		assert!(diagnostic.evidence.contains(&String::from("tracker_issue_missing")));
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_requested_issue_identifier_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let mut issue = sample_issue("In Progress");

		issue.id = String::from("linear-pubfi-012");
		issue.identifier = String::from("PUBFI-012");

		let tracker = GhostLaneTestTracker {
			issues: vec![issue],
			refresh_error: None,
			identifier_error: None,
			remove_error: None,
			comments: Vec::new(),
			refresh_queries: RefCell::new(Vec::new()),
			label_removals: RefCell::new(Vec::new()),
			state_updates: RefCell::new(Vec::new()),
		};

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUBFI-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert_eq!(diagnostic.issue_identifier.as_deref(), Some("PUBFI-012"));
		assert!(diagnostic.blockers.contains(&String::from("tracker_issue_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_rejects_unrelated_requested_identifier() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();

		store
			.record_run_attempt("run-12", "ABC-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "ABC-012", "run-12", "In Progress")
			.expect("lease should record");

		let error = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUBFI-012"),
		)
		.expect_err("unrelated issue prefixes should not match by numeric suffix alone");

		assert!(
			format!("{error:#}").contains("No leased lane matched"),
			"unexpected error: {error:#}"
		);
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"failed selector matches must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_requested_worktree_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();
		let worktree_path = temp_dir.path().join("PUBFI-012");

		fs::create_dir_all(&worktree_path).expect("retained worktree should exist");

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUBFI-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("retained_worktree_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_control_channel_row_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();
		let channel_path = temp_dir.path().join("missing-control-channel.json");

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		store
			.publish_run_control_channel_for_active_attempt(
				"run-12",
				1,
				&channel_path,
				"local_file",
			)
			.expect("control channel row should publish");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.evidence.contains(&String::from("control_channel_file_missing")));
		assert!(diagnostic.blockers.contains(&String::from("control_channel_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_private_evidence_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		store
			.append_private_execution_event(
				"pubfi",
				"PUB-012",
				"run-12",
				1,
				"diagnostic",
				serde_json::json!({"schema": "test.private/1"}),
			)
			.expect("private evidence should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("private_evidence_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_review_lifecycle_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();
		let marker = ReviewHandoffMarker::new(
			"run-12",
			1,
			"x/pubfi-pub-012",
			"https://github.com/hack-ink/decodex/pull/12",
			"main",
			"x/pubfi-pub-012",
			"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		);

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		store
			.upsert_review_handoff_marker("pubfi", "PUB-012", &marker)
			.expect("review lifecycle should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("review_lifecycle_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_review_checkpoint_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		store
			.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
				project_id: "pubfi",
				issue_id: "PUB-012",
				run_id: "run-12",
				attempt_number: 1,
				phase: "handoff",
				review_level: "standard",
				status: "clean",
				head_sha: "2222222222222222222222222222222222222222",
				nonclean_rounds: 0,
				details_json: "{}",
			})
			.expect("review checkpoint should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("review_policy_checkpoint_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_pr_lineage_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();
		let mut event = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "pubfi",
				issue_id: "PUB-012",
				issue_identifier: "PUB-012",
				run_id: "run-12",
				attempt_number: 1,
			},
			"closeout",
			String::from("2026-06-18T00:00:00Z"),
			"closeout",
		);

		event.branch = Some(String::from("x/pubfi-pub-012"));
		event.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/12"));
		event.pr_head_sha = Some(String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"));
		event.pr_base_ref = Some(String::from("main"));
		event.commit_sha = Some(String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d7"));
		event.summary = Some(String::from("Recorded retained closeout."));

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		store.record_linear_execution_event(&event).expect("linear event should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("pr_or_review_lineage_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_retained_worktree_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();
		let worktree_path = temp_dir.path().join("PUB-012");

		fs::create_dir_all(&worktree_path).expect("retained worktree should exist");

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("retained_worktree_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_activity_summary_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();
		let activity = ChildAgentActivitySummary { event_count: 1, ..Default::default() };

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.record_run_activity_summary("run-12", 1, Some(&activity), None)
			.expect("activity summary should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("child_agent_activity_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	fn run_git(repo: &Path, args: &[&str]) -> String {
		let output = std::process::Command::new("git")
			.arg("-C")
			.arg(repo)
			.args(args)
			.output()
			.expect("git command should run");

		assert!(
			output.status.success(),
			"git {:?} failed: {}",
			args,
			String::from_utf8_lossy(&output.stderr)
		);

		String::from_utf8(output.stdout).expect("git stdout should be utf8").trim().to_owned()
	}

	fn commit_file(repo: &Path, contents: &str) -> String {
		fs::write(repo.join("tracked.txt"), contents).expect("tracked file should write");

		run_git(repo, &["add", "tracked.txt"]);
		run_git(repo, &["commit", "-m", "test commit"]);

		run_git(repo, &["rev-parse", "HEAD"])
	}

	fn temp_git_worktree(branch_name: &str) -> (TempDir, String, String) {
		let temp_dir = TempDir::new().expect("temp git repo should exist");
		let repo = temp_dir.path();

		run_git(repo, &["init"]);
		run_git(repo, &["config", "user.email", "decodex@example.invalid"]);
		run_git(repo, &["config", "user.name", "Decodex Test"]);
		run_git(repo, &["checkout", "-b", branch_name]);

		let first_head = commit_file(repo, "first\n");
		let second_head = commit_file(repo, "second\n");

		(temp_dir, first_head, second_head)
	}

	fn temp_rebased_git_worktree(branch_name: &str) -> (TempDir, String, String) {
		let (temp_dir, first_head, _) = temp_git_worktree(branch_name);
		let repo = temp_dir.path();

		run_git(repo, &["checkout", "--orphan", "rebased"]);
		run_git(repo, &["rm", "-rf", "."]);

		let rebased_head = commit_file(repo, "rebased\n");

		run_git(repo, &["branch", "-D", branch_name]);
		run_git(repo, &["branch", "-m", branch_name]);

		(temp_dir, first_head, rebased_head)
	}

	#[test]
	fn worktree_blocking_status_lines_ignores_untracked_decodex_runtime_artifacts() {
		let (temp_dir, _, _) = temp_git_worktree("x/pubfi-pub-718");
		let repo = temp_dir.path();

		fs::write(repo.join(crate::state::RUN_ACTIVITY_MARKER_FILE), "agent_run\n")
			.expect("activity marker should write");

		let control_dir = repo.join(crate::state::RUN_CONTROL_CHANNEL_DIR);

		fs::create_dir_all(&control_dir).expect("run-control directory should create");
		fs::write(control_dir.join("run-1-1.channel"), "channel\n")
			.expect("run-control channel should write");

		let blocking = super::worktree_blocking_status_lines(repo)
			.expect("worktree status should be readable");

		assert!(blocking.is_empty(), "runtime artifacts should not block rebind: {blocking:?}");
	}

	#[test]
	fn rebind_state_allows_missing_marker_partial_in_progress_handoff() {
		let workflow = sample_workflow();
		let issue = sample_issue("In Progress");
		let transition = super::validate_rebind_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&issue,
			super::RebindMode::RestoreMissingHandoff,
		)
		.expect("missing-marker rebind should recover partial in-progress handoff")
		.expect("partial in-progress handoff should transition to success state");

		assert_eq!(transition.state_name, "In Review");
		assert_eq!(transition.state_id, "state-review");
	}

	#[test]
	fn rebind_state_allows_current_marker_partial_in_progress_handoff() {
		let workflow = sample_workflow();
		let issue = sample_issue("In Progress");
		let transition = super::validate_rebind_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&issue,
			super::RebindMode::CompleteExistingHandoffState,
		)
		.expect("current-marker state completion should recover partial in-progress handoff")
		.expect("partial in-progress handoff should transition to success state");

		assert_eq!(transition.state_name, "In Review");
		assert_eq!(transition.state_id, "state-review");
	}

	#[test]
	fn rebind_state_allows_current_marker_failure_state_drift_recovery() {
		let workflow = sample_workflow();
		let issue = sample_issue("Todo");
		let transition = super::validate_rebind_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&issue,
			super::RebindMode::CompleteExistingHandoffState,
		)
		.expect("current-marker state completion should recover failure-state drift")
		.expect("failure-state drift should transition to success state");

		assert_eq!(transition.state_name, "In Review");
		assert_eq!(transition.state_id, "state-review");
	}

	#[test]
	fn rebind_state_rejects_failure_state_without_current_marker_repair_mode() {
		let workflow = sample_workflow();
		let issue = sample_issue("Todo");

		for mode in
			[super::RebindMode::RestoreMissingHandoff, super::RebindMode::RefreshExistingHandoff]
		{
			let error = super::validate_rebind_issue_state_for_policy(
				workflow.frontmatter().tracker(),
				&issue,
				mode,
			)
			.expect_err("failure-state repair requires current-marker completion mode");

			assert!(
				error.to_string().contains("review handoff rebind requires"),
				"unexpected error for {mode:?}: {error}"
			);
		}
	}

	#[test]
	fn rebind_state_requires_success_state_for_existing_marker_refresh() {
		let workflow = sample_workflow();
		let issue = sample_issue("In Progress");
		let error = super::validate_rebind_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&issue,
			super::RebindMode::RefreshExistingHandoff,
		)
		.expect_err("existing-marker refresh should still require success state");

		assert!(error.to_string().contains("requires `In Review`"));
		assert!(!error.to_string().contains("partial missing-marker"));
	}

	#[test]
	fn adopt_state_allows_in_progress_or_review_only() {
		let workflow = sample_workflow();
		let in_progress = sample_issue("In Progress");
		let transition = super::validate_adopt_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&in_progress,
		)
		.expect("in-progress issue should be adoptable")
		.expect("in-progress issue should transition to review");

		assert_eq!(transition.state_name, "In Review");
		assert_eq!(transition.state_id, "state-review");

		let in_review = sample_issue("In Review");
		let no_transition = super::validate_adopt_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&in_review,
		)
		.expect("in-review issue should remain adoptable");

		assert!(no_transition.is_none());

		let todo = sample_issue("Todo");
		let error =
			super::validate_adopt_issue_state_for_policy(workflow.frontmatter().tracker(), &todo)
				.expect_err("manual takeover should not bypass failure/start states");

		assert!(error.to_string().contains("manual takeover adopt requires"));
	}

	#[test]
	fn adopt_landing_state_rejects_pending_checks() {
		let mut landing_state = sample_landing_state(
			"https://github.com/hack-ink/decodex/pull/344",
			"xy/xy-944-manual-takeover-adopt",
			"1123456789abcdef0123456789abcdef01234567",
		);

		landing_state.status_check_rollup_state = Some(String::from("PENDING"));

		let error = super::validate_adopt_landing_state(&landing_state)
			.expect_err("manual takeover must not adopt pending checks");

		assert!(error.to_string().contains("still waiting on checks"));
	}

	#[test]
	fn adopt_landing_state_rejects_blocked_merge_state_after_green_gates() {
		let mut landing_state = sample_landing_state(
			"https://github.com/hack-ink/decodex/pull/344",
			"xy/xy-944-manual-takeover-adopt",
			"1123456789abcdef0123456789abcdef01234567",
		);

		landing_state.merge_state_status = String::from("BLOCKED");

		let error = super::validate_adopt_landing_state(&landing_state)
			.expect_err("manual takeover should not bypass blocked merge state");

		assert!(error.to_string().contains("not ready to adopt"));
		assert!(error.to_string().contains("mergeStateStatus=`BLOCKED`"));
	}

	#[test]
	fn adopt_landing_state_rejects_closed_or_draft_prs() {
		let mut closed = sample_landing_state(
			"https://github.com/hack-ink/decodex/pull/344",
			"xy/xy-944-manual-takeover-adopt",
			"1123456789abcdef0123456789abcdef01234567",
		);

		closed.state = String::from("CLOSED");

		let error = super::validate_adopt_landing_state(&closed)
			.expect_err("manual takeover must reject closed PRs");

		assert!(error.to_string().contains("adopt requires `OPEN`"));

		let mut draft = sample_landing_state(
			"https://github.com/hack-ink/decodex/pull/344",
			"xy/xy-944-manual-takeover-adopt",
			"1123456789abcdef0123456789abcdef01234567",
		);

		draft.is_draft = true;

		let error = super::validate_adopt_landing_state(&draft)
			.expect_err("manual takeover must reject draft PRs");

		assert!(error.to_string().contains("is still draft"));
	}

	#[test]
	fn adopt_landing_state_rejects_failed_required_checks() {
		let mut landing_state = sample_landing_state(
			"https://github.com/hack-ink/decodex/pull/344",
			"xy/xy-944-manual-takeover-adopt",
			"1123456789abcdef0123456789abcdef01234567",
		);

		landing_state.status_check_rollup_state = Some(String::from("FAILURE"));
		landing_state.merge_state_status = String::from("BLOCKED");

		let error = super::validate_adopt_landing_state(&landing_state)
			.expect_err("manual takeover must reject failed required checks");

		assert!(error.to_string().contains("failed required checks"));
	}

	#[test]
	fn adopt_existing_worktree_mapping_accepts_same_project_and_path() {
		let temp_dir = TempDir::new().expect("temp worktree should exist");
		let branch_name = "x/pubfi-pub-718";
		let issue = sample_issue("In Progress");
		let mapping = sample_worktree_at(branch_name, temp_dir.path());
		let canonical_worktree =
			fs::canonicalize(temp_dir.path()).expect("temp worktree should canonicalize");

		super::validate_adopt_existing_worktree_mapping(
			"pubfi",
			&issue,
			&mapping,
			&canonical_worktree,
		)
		.expect("matching mapping should be accepted");
	}

	#[test]
	fn adopt_existing_worktree_mapping_accepts_stale_branch_for_same_path() {
		let retained_dir = TempDir::new().expect("retained worktree should exist");
		let issue = sample_issue("In Progress");
		let mapping = sample_worktree_at("x/pubfi-pub-718-old", retained_dir.path());
		let retained_worktree =
			fs::canonicalize(retained_dir.path()).expect("retained worktree should canonicalize");

		super::validate_adopt_existing_worktree_mapping(
			"pubfi",
			&issue,
			&mapping,
			&retained_worktree,
		)
		.expect("stale mapping branch should be adopted when path matches");
	}

	#[test]
	fn adopt_existing_worktree_mapping_rejects_stale_path() {
		let retained_dir = TempDir::new().expect("retained worktree should exist");
		let current_dir = TempDir::new().expect("current worktree should exist");
		let issue = sample_issue("In Progress");
		let mapping = sample_worktree_at("x/pubfi-pub-718", retained_dir.path());
		let current_worktree =
			fs::canonicalize(current_dir.path()).expect("current worktree should canonicalize");
		let error = super::validate_adopt_existing_worktree_mapping(
			"pubfi",
			&issue,
			&mapping,
			&current_worktree,
		)
		.expect_err("stale mapping path must be rejected");

		assert!(error.to_string().contains("already has a retained worktree mapping at"));
	}

	#[test]
	fn manual_adopt_run_id_is_stable_for_head() {
		let head_oid = "0123456789abcdef0123456789abcdef01234567";
		let run_id = super::manual_adopt_run_id("XY-944", 2, head_oid);

		assert_eq!(run_id, "xy-944-manual-adopt-2-0123456789ab");
		assert_eq!(run_id, super::manual_adopt_run_id("XY-944", 2, head_oid));
	}

	#[test]
	fn adopt_private_event_records_manual_takeover_lifecycle_evidence() {
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let validation = super::AdoptValidation {
			issue: sample_issue("In Review"),
			branch_name: branch_name.to_owned(),
			worktree_path: Path::new("/tmp/PUB-718").to_path_buf(),
			run_id: String::from("pub-718-manual-adopt-2-1123456789ab"),
			attempt_number: 2,
			landing_state: sample_landing_state(pr_url, branch_name, head_oid),
			local_head_oid: head_oid.to_owned(),
			worktree_path_for_event: Some(String::from(".worktrees/PUB-718")),
			active_label_present: false,
			success_state_transition: None,
			previous_worktree_mapping: None,
		};

		super::append_review_handoff_adopt_private_event(
			&state_store,
			"pubfi",
			&validation,
			"local_markers_written",
			false,
		)
		.expect("adopt private event should append");
		super::append_review_handoff_adopt_private_event(
			&state_store,
			"pubfi",
			&validation,
			"active_label_checked",
			true,
		)
		.expect("adopt active-label private event should append");

		let events = state_store
			.list_private_execution_events(
				"pubfi",
				&validation.issue.id,
				&validation.run_id,
				validation.attempt_number,
			)
			.expect("private events should read");
		let event = events.first().expect("adopt event should exist");
		let payload = event.payload();
		let second_event = events.get(1).expect("active-label adopt event should exist");
		let second_payload = second_event.payload();

		assert_eq!(events.len(), 2);
		assert_eq!(event.event_type(), REVIEW_HANDOFF_ADOPT_EVENT);
		assert_eq!(payload["schema"], "decodex.review_handoff_recovery_private_event/1");
		assert_eq!(payload["event"], REVIEW_HANDOFF_ADOPT_EVENT);
		assert_eq!(payload["writeback_stage"], "local_markers_written");
		assert_eq!(payload["manual_takeover_adopt"], true);
		assert_eq!(payload["active_label_restored"], false);
		assert_eq!(payload["pr_url"], pr_url);
		assert_eq!(payload["pr_head_sha"], head_oid);
		assert_eq!(payload["next_action"], "continue retained post-review lifecycle");
		assert_eq!(second_event.event_type(), REVIEW_HANDOFF_ADOPT_EVENT);
		assert_eq!(second_payload["writeback_stage"], "active_label_checked");
		assert_eq!(second_payload["active_label_restored"], true);
	}

	#[test]
	fn rebind_private_event_records_retained_lifecycle_evidence() {
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let validation = super::RebindValidation {
			issue: sample_issue("In Review"),
			worktree: sample_worktree(branch_name),
			run_id: String::from("pub-718-attempt-2-1123456789ab"),
			attempt_number: 2,
			landing_state: sample_landing_state(pr_url, branch_name, head_oid),
			local_head_oid: head_oid.to_owned(),
			worktree_path_for_event: Some(String::from(".worktrees/PUB-718")),
			active_label_present: true,
			restore_active_label: false,
			mode: super::RebindMode::RefreshExistingHandoff,
			success_state_transition: None,
			clear_needs_attention_label: false,
		};

		super::append_review_handoff_rebind_private_event(
			&state_store,
			"pubfi",
			&validation,
			"local_markers_written",
			false,
		)
		.expect("rebind private event should append");

		let events = state_store
			.list_private_execution_events(
				"pubfi",
				&validation.issue.id,
				&validation.run_id,
				validation.attempt_number,
			)
			.expect("private events should read");
		let event = events.first().expect("rebind event should exist");
		let payload = event.payload();

		assert_eq!(events.len(), 1);
		assert_eq!(event.event_type(), REVIEW_HANDOFF_REBIND_EVENT);
		assert_eq!(payload["schema"], "decodex.review_handoff_recovery_private_event/1");
		assert_eq!(payload["event"], REVIEW_HANDOFF_REBIND_EVENT);
		assert_eq!(payload["writeback_stage"], "local_markers_written");
		assert_eq!(payload["mode"], "refresh_existing_handoff");
		assert_eq!(payload["active_label_present"], true);
		assert_eq!(payload["active_label_restored"], false);
		assert_eq!(payload["pr_url"], pr_url);
		assert_eq!(payload["pr_head_sha"], head_oid);
		assert_eq!(payload["next_action"], "continue retained post-review lifecycle");
	}

	#[test]
	fn rebind_lifecycle_marker_write_failure_clears_partial_handoff_marker() {
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);

		let error = super::write_review_lifecycle_markers_with_rollback(
			&state_store,
			"pubfi",
			"issue-id",
			&handoff,
			&orchestration,
			|| -> crate::prelude::Result<()> {
				Err(crate::prelude::eyre::eyre!("orchestration marker write failed"))
			},
		)
		.expect_err("orchestration write failure should be returned");

		assert!(error.to_string().contains("orchestration marker write failed"));
		assert!(
			state_store
				.review_lifecycle_record("pubfi", "issue-id", branch_name)
				.expect("lifecycle read should succeed")
				.is_none()
		);
		assert!(
			state_store
				.review_handoff_marker("pubfi", "issue-id", branch_name)
				.expect("handoff read should succeed")
				.is_none()
		);
	}

	#[test]
	fn diagnostic_treats_descendant_handoff_head_as_bound() {
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let (temp_dir, original_head, current_head) = temp_git_worktree(branch_name);
		let worktree = sample_worktree_at(branch_name, temp_dir.path());
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			original_head,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, &current_head);
		let diagnostic = super::diagnostic_binding(super::HandoffDiagnosticRequest {
			service_id: "pubfi",
			issue_identifier: "PUB-718",
			issue_state_name: "In Review",
			success_state: "In Review",
			in_progress_state: "In Progress",
			failure_state: "Todo",
			worktree: &worktree,
			existing_handoff: Some(&handoff),
			existing_orchestration: None,
			local_branch_name: Some(branch_name),
			local_head_oid: Some(&current_head),
			worktree_clean: Some(true),
			pr_inspection: Some(&landing_state),
			active_label_present: Some(true),
		});

		assert_eq!(diagnostic.classification, REVIEW_HANDOFF_BOUND_CLASSIFICATION);
		assert_eq!(diagnostic.reason, "review_handoff_record_present");
		assert_eq!(diagnostic.mismatched_field, None);
	}

	#[test]
	fn diagnostic_requires_rebind_when_current_marker_state_transition_pending() {
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
		let diagnostic = super::diagnostic_binding(super::HandoffDiagnosticRequest {
			service_id: "pubfi",
			issue_identifier: "PUB-718",
			issue_state_name: "In Progress",
			success_state: "In Review",
			in_progress_state: "In Progress",
			failure_state: "Todo",
			worktree: &worktree,
			existing_handoff: Some(&handoff),
			existing_orchestration: Some(&orchestration),
			local_branch_name: Some(branch_name),
			local_head_oid: Some(head_oid),
			worktree_clean: Some(true),
			pr_inspection: Some(&landing_state),
			active_label_present: Some(true),
		});

		assert_eq!(diagnostic.classification, REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION);
		assert_eq!(diagnostic.reason, "review_handoff_state_transition_pending");
		assert_eq!(diagnostic.mismatched_field.as_deref(), Some("issue.state"));
		assert!(diagnostic.next_action.contains("rebind PUB-718"));
		assert!(diagnostic.next_action.contains("pending issue-state transition"));
	}

	#[test]
	fn diagnostic_requires_refresh_when_handoff_head_is_stale() {
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let (temp_dir, original_head, rebased_head) = temp_rebased_git_worktree(branch_name);
		let worktree = sample_worktree_at(branch_name, temp_dir.path());
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			original_head,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, &rebased_head);
		let diagnostic = super::diagnostic_binding(super::HandoffDiagnosticRequest {
			service_id: "pubfi",
			issue_identifier: "PUB-718",
			issue_state_name: "In Review",
			success_state: "In Review",
			in_progress_state: "In Progress",
			failure_state: "Todo",
			worktree: &worktree,
			existing_handoff: Some(&handoff),
			existing_orchestration: None,
			local_branch_name: Some(branch_name),
			local_head_oid: Some(&rebased_head),
			worktree_clean: Some(true),
			pr_inspection: Some(&landing_state),
			active_label_present: Some(true),
		});

		assert_eq!(diagnostic.classification, REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION);
		assert_eq!(diagnostic.reason, "review_handoff_lineage_mismatch");
		assert_eq!(diagnostic.pr_head_oid.as_deref(), Some(rebased_head.as_str()));
		assert_eq!(diagnostic.mismatched_field.as_deref(), Some("review_handoff.pr_head_oid"));
		assert!(diagnostic.next_action.contains("rebind PUB-718"));
		assert!(diagnostic.next_action.contains("--dry-run"));
	}

	#[test]
	fn diagnostic_requires_refresh_when_orchestration_head_is_stale() {
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"0123456789abcdef0123456789abcdef01234567",
			"waiting_for_result",
			None,
			None,
			None,
			0,
			0,
			None,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
		let diagnostic = super::diagnostic_binding(super::HandoffDiagnosticRequest {
			service_id: "pubfi",
			issue_identifier: "PUB-718",
			issue_state_name: "In Review",
			success_state: "In Review",
			in_progress_state: "In Progress",
			failure_state: "Todo",
			worktree: &worktree,
			existing_handoff: Some(&handoff),
			existing_orchestration: Some(&orchestration),
			local_branch_name: Some(branch_name),
			local_head_oid: Some(head_oid),
			worktree_clean: Some(true),
			pr_inspection: Some(&landing_state),
			active_label_present: Some(true),
		});

		assert_eq!(diagnostic.classification, REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION);
		assert_eq!(diagnostic.reason, "review_orchestration_head_mismatch");
		assert_eq!(diagnostic.mismatched_field.as_deref(), Some("review_orchestration.head_sha"));
	}

	#[test]
	fn diagnostic_bound_handoff_reports_missing_active_ownership_recovery() {
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
		let diagnostic = super::diagnostic_binding(super::HandoffDiagnosticRequest {
			service_id: "pubfi",
			issue_identifier: "PUB-718",
			issue_state_name: "In Review",
			success_state: "In Review",
			in_progress_state: "In Progress",
			failure_state: "Todo",
			worktree: &worktree,
			existing_handoff: Some(&handoff),
			existing_orchestration: Some(&orchestration),
			local_branch_name: Some(branch_name),
			local_head_oid: Some(head_oid),
			worktree_clean: Some(true),
			pr_inspection: Some(&landing_state),
			active_label_present: Some(false),
		});

		assert_eq!(diagnostic.classification, REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION);
		assert_eq!(diagnostic.reason, "active_ownership_label_missing");
		assert_eq!(diagnostic.mismatched_field.as_deref(), Some("issue.labels"));
		assert!(diagnostic.next_action.contains("decodex:active:pubfi"));
		assert!(diagnostic.next_action.contains("Restore explicit lane ownership"));
	}

	#[test]
	fn diagnostic_reports_rebind_for_failure_state_ownership_drift() {
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
		let diagnostic = super::diagnostic_binding(super::HandoffDiagnosticRequest {
			service_id: "pubfi",
			issue_identifier: "PUB-718",
			issue_state_name: "Todo",
			success_state: "In Review",
			in_progress_state: "In Progress",
			failure_state: "Todo",
			worktree: &worktree,
			existing_handoff: Some(&handoff),
			existing_orchestration: Some(&orchestration),
			local_branch_name: Some(branch_name),
			local_head_oid: Some(head_oid),
			worktree_clean: Some(true),
			pr_inspection: Some(&landing_state),
			active_label_present: Some(false),
		});

		assert_eq!(diagnostic.classification, REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION);
		assert_eq!(diagnostic.reason, "active_ownership_label_missing");
		assert_eq!(diagnostic.mismatched_field.as_deref(), Some("issue.labels"));
		assert!(diagnostic.next_action.contains("rebind PUB-718"));
		assert!(diagnostic.next_action.contains("--dry-run"));
		assert!(!diagnostic.next_action.contains("Restore explicit lane ownership"));
	}

	#[test]
	fn diagnostic_reports_rebind_for_failure_state_drift_with_active_label() {
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
		let diagnostic = super::diagnostic_binding(super::HandoffDiagnosticRequest {
			service_id: "pubfi",
			issue_identifier: "PUB-718",
			issue_state_name: "Todo",
			success_state: "In Review",
			in_progress_state: "In Progress",
			failure_state: "Todo",
			worktree: &worktree,
			existing_handoff: Some(&handoff),
			existing_orchestration: Some(&orchestration),
			local_branch_name: Some(branch_name),
			local_head_oid: Some(head_oid),
			worktree_clean: Some(true),
			pr_inspection: Some(&landing_state),
			active_label_present: Some(true),
		});

		assert_eq!(diagnostic.classification, REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION);
		assert_eq!(diagnostic.reason, "review_handoff_failure_state_drift");
		assert_eq!(diagnostic.mismatched_field.as_deref(), Some("issue.state"));
		assert!(diagnostic.next_action.contains("rebind PUB-718"));
		assert!(diagnostic.next_action.contains("--dry-run"));
	}

	#[test]
	fn rebind_validation_refreshes_existing_same_branch_pr_marker() {
		let workflow = sample_workflow();
		let issue = sample_issue("In Review");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			"0123456789abcdef0123456789abcdef01234567",
		);
		let landing_state =
			sample_landing_state(pr_url, branch_name, "1123456789abcdef0123456789abcdef01234567");
		let (run_id, attempt_number, mode) = super::validate_existing_handoff_refresh(
			workflow.frontmatter().tracker(),
			&issue,
			&worktree,
			&handoff,
			None,
			&landing_state,
			"1123456789abcdef0123456789abcdef01234567",
		)
		.expect("stale existing marker should be refreshable");

		assert_eq!(run_id, "pub-718-attempt-1");
		assert_eq!(attempt_number, 1);
		assert_eq!(mode, super::RebindMode::RefreshExistingHandoff);
	}

	#[test]
	fn rebind_validation_rejects_stale_marker_failure_state_drift_recovery() {
		let workflow = sample_workflow();
		let issue = sample_issue("Todo");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			"0123456789abcdef0123456789abcdef01234567",
		);
		let landing_state =
			sample_landing_state(pr_url, branch_name, "1123456789abcdef0123456789abcdef01234567");
		let (_run_id, _attempt_number, mode) = super::validate_existing_handoff_refresh(
			workflow.frontmatter().tracker(),
			&issue,
			&worktree,
			&handoff,
			None,
			&landing_state,
			"1123456789abcdef0123456789abcdef01234567",
		)
		.expect("stale existing marker should require marker refresh first");

		assert_eq!(mode, super::RebindMode::RefreshExistingHandoff);

		let error = super::validate_rebind_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&issue,
			mode,
		)
		.expect_err("stale marker refresh must not repair failure-state drift");

		assert!(error.to_string().contains("review handoff rebind requires"));
	}

	#[test]
	fn rebind_validation_rejects_current_existing_marker_as_noop() {
		let workflow = sample_workflow();
		let issue = sample_issue("In Review");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
		let error = super::validate_existing_handoff_refresh(
			workflow.frontmatter().tracker(),
			&issue,
			&worktree,
			&handoff,
			Some(&orchestration),
			&landing_state,
			head_oid,
		)
		.expect_err("current existing marker should not be rebound");

		assert!(error.to_string().contains("no rebind is needed"));
	}

	#[test]
	fn rebind_validation_completes_current_existing_marker_state_transition() {
		let workflow = sample_workflow();
		let issue = sample_issue("In Progress");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
		let (run_id, attempt_number, mode) = super::validate_existing_handoff_refresh(
			workflow.frontmatter().tracker(),
			&issue,
			&worktree,
			&handoff,
			Some(&orchestration),
			&landing_state,
			head_oid,
		)
		.expect("current marker should allow state-only handoff completion");

		assert_eq!(run_id, "pub-718-attempt-1");
		assert_eq!(attempt_number, 1);
		assert_eq!(mode, super::RebindMode::CompleteExistingHandoffState);
	}

	#[test]
	fn rebind_validation_completes_current_existing_marker_failure_state_drift() {
		let workflow = sample_workflow();
		let issue = sample_issue("Todo");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
		let (run_id, attempt_number, mode) = super::validate_existing_handoff_refresh(
			workflow.frontmatter().tracker(),
			&issue,
			&worktree,
			&handoff,
			Some(&orchestration),
			&landing_state,
			head_oid,
		)
		.expect("current marker should allow failure-state drift completion");

		assert_eq!(run_id, "pub-718-attempt-1");
		assert_eq!(attempt_number, 1);
		assert_eq!(mode, super::RebindMode::CompleteExistingHandoffState);
	}

	#[test]
	fn review_handoff_rebind_event_validation_accepts_required_fields() {
		let mut record = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "pubfi",
				issue_id: "issue-id",
				issue_identifier: "PUB-718",
				run_id: "pub-718-attempt-1",
				attempt_number: 1,
			},
			REVIEW_HANDOFF_REBIND_EVENT,
			super::current_timestamp(),
			"anchor",
		);

		record.branch = Some(String::from("x/pubfi-pub-718"));
		record.worktree_path = Some(String::from(".worktrees/PUB-718"));
		record.pr_url = Some(String::from("https://github.com/hack-ink/pubfi-mono-v2/pull/14"));
		record.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		record.pr_base_ref = Some(String::from("main"));
		record.commit_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		record.validation_result = Some(String::from("passed"));
		record.summary = Some(String::from("Explicit operator rebind restored lifecycle record."));
		record.evidence = Some(vec![String::from("existing_review_lifecycle_record=absent")]);

		records::validate_linear_execution_event_record(&record)
			.expect("rebind event should validate");
	}

	#[test]
	fn review_handoff_adopt_event_validation_accepts_required_fields() {
		let mut record = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "decodex",
				issue_id: "issue-id",
				issue_identifier: "XY-944",
				run_id: "xy-944-manual-adopt-1",
				attempt_number: 1,
			},
			REVIEW_HANDOFF_ADOPT_EVENT,
			super::current_timestamp(),
			"anchor",
		);

		record.branch = Some(String::from("xy/xy-944-manual-takeover-adopt"));
		record.worktree_path = Some(String::from(".worktrees/XY-944"));
		record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/344"));
		record.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		record.pr_base_ref = Some(String::from("main"));
		record.commit_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		record.validation_result = Some(String::from("passed"));
		record.summary =
			Some(String::from("Explicit operator manual takeover adopted review handoff."));
		record.evidence = Some(vec![String::from("manual_takeover_adopt=true")]);

		records::validate_linear_execution_event_record(&record)
			.expect("adopt event should validate");
	}

	#[test]
	fn merged_closeout_recovery_events_validate() {
		let mut closeout = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "pubfi-mono",
				issue_id: "issue-id",
				issue_identifier: "PUB-1549",
				run_id: "pub-1549-attempt-1-1781240781",
				attempt_number: 1,
			},
			super::LEGACY_MANUAL_CLOSEOUT_EVENT,
			super::current_timestamp(),
			"anchor-closeout",
		);

		closeout.branch = Some(String::from("x/pubfi-mono-pub-1549"));
		closeout.worktree_path = Some(String::from(".worktrees/PUB-1549"));
		closeout.pr_url = Some(String::from("https://github.com/helixbox/pubfi-mono/pull/309"));
		closeout.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		closeout.pr_base_ref = Some(String::from("main"));
		closeout.commit_sha = Some(String::from("1123456789abcdef0123456789abcdef01234567"));
		closeout.validation_result = Some(String::from("passed"));
		closeout.target_state = Some(String::from("Done"));
		closeout.summary = Some(String::from("Merged closeout recovery recorded."));

		records::validate_linear_execution_event_record(&closeout)
			.expect("merged closeout event should validate");

		let mut cleanup = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "pubfi-mono",
				issue_id: "issue-id",
				issue_identifier: "PUB-1549",
				run_id: "pub-1549-attempt-1-1781240781",
				attempt_number: 1,
			},
			"cleanup_complete",
			super::timestamp_after_seconds(1),
			"anchor-cleanup",
		);

		cleanup.branch = Some(String::from("x/pubfi-mono-pub-1549"));
		cleanup.worktree_path = Some(String::from(".worktrees/PUB-1549"));
		cleanup.pr_url = Some(String::from("https://github.com/helixbox/pubfi-mono/pull/309"));
		cleanup.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		cleanup.pr_base_ref = Some(String::from("main"));
		cleanup.commit_sha = Some(String::from("1123456789abcdef0123456789abcdef01234567"));
		cleanup.cleanup_status = Some(String::from("merged_closeout_reconciled"));
		cleanup.target_state = Some(String::from("Done"));
		cleanup.summary = Some(String::from("Merged closeout recovery marked cleanup complete."));

		records::validate_linear_execution_event_record(&cleanup)
			.expect("merged closeout cleanup event should validate");
	}

	#[test]
	fn review_handoff_rebind_event_requires_evidence() {
		let mut record = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "pubfi",
				issue_id: "issue-id",
				issue_identifier: "PUB-718",
				run_id: "pub-718-attempt-1",
				attempt_number: 1,
			},
			REVIEW_HANDOFF_REBIND_EVENT,
			super::current_timestamp(),
			"anchor",
		);

		record.branch = Some(String::from("x/pubfi-pub-718"));
		record.pr_url = Some(String::from("https://github.com/hack-ink/pubfi-mono-v2/pull/14"));
		record.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		record.pr_base_ref = Some(String::from("main"));
		record.commit_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		record.validation_result = Some(String::from("passed"));
		record.summary = Some(String::from("Explicit operator rebind restored lifecycle record."));

		let error = records::validate_linear_execution_event_record(&record)
			.expect_err("rebind event without evidence should fail");

		assert!(error.contains("evidence"));
	}
}
