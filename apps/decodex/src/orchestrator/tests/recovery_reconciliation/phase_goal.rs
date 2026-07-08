use std::{fs, path::Path};

use serde::Serialize;
use time::OffsetDateTime;

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self, CONTINUATION_PENDING_RUN_STATUS, RunLeaseDisposition, VALIDATION_EVIDENCE_EVENT_TYPE,
		tests::{self, FakeTracker, TEST_SERVICE_ID},
	},
	state::{self, StateStore},
	worktree::WorktreeManager,
};

struct StalledPhaseGoalOutcome {
	run_status: String,
	retry_kind: Option<String>,
	retry_ready_at: Option<i64>,
	comments: Vec<String>,
	label_additions_empty: bool,
	event_types: Vec<String>,
	validation_evidence_reason: Option<String>,
	handoff_next_recorded: bool,
}

fn record_active_validation_ready_phase_goal_progress(
	state_store: &StateStore,
	worktree_path: &Path,
	issue_id: &str,
	run_id: &str,
	progress_phase: &str,
	blockers: impl Serialize,
) {
	let head_sha = tests::git_output(worktree_path, &["rev-parse", "HEAD"]);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			issue_id,
			run_id,
			1,
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
				"payload": {
					"phase": "implement_to_validation_ready",
					"status": "active",
				},
			}),
		)
		.expect("phase goal should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			issue_id,
			run_id,
			1,
			"progress_checkpoint",
			serde_json::json!({
				"phase": progress_phase,
				"docs_impact": "none",
				"blockers": blockers,
				"verification": ["cargo make check"],
				"head_sha": head_sha,
			}),
		)
		.expect("progress checkpoint should record");
}

fn apply_stalled_phase_goal_reconciliation(
	progress_phase: &str,
	blockers: impl Serialize,
) -> StalledPhaseGoalOutcome {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new("pubfi", config.repo_root(), config.worktree_root());
	let run_id = "run-stalled-phase-goal";
	let worktree_path = config.worktree_root().join("PUB-101-phase-goal");

	tests::git_status_success(
		config.repo_root(),
		&[
			"worktree",
			"add",
			"-b",
			"x/pubfi-pub-101-phase-goal",
			".worktrees/PUB-101-phase-goal",
			"main",
		],
	);
	fs::write(worktree_path.join("README.md"), "validation-ready retained patch\n")
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
			"x/pubfi-pub-101-phase-goal",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	record_active_validation_ready_phase_goal_progress(
		&state_store,
		&worktree_path,
		&issue.id,
		run_id,
		progress_phase,
		blockers,
	);

	state_store
		.append_event(run_id, 1, "turn/diff/updated", "{\"changes\":1}")
		.expect("stalled dirty issue protocol event should record");

	let now =
		OffsetDateTime::now_utc().unix_timestamp() + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1;
	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		now,
	)
	.expect("stalled phase-goal inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(
		actions[0].disposition,
		RunLeaseDisposition::StalledRetainedPartialProgress { .. }
	));

	orchestrator::apply_run_lease_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		actions,
	)
	.expect("phase-goal reconciliation should apply");

	let run_status = state_store
		.run_attempt(run_id)
		.expect("run attempt lookup should succeed")
		.expect("run attempt should exist")
		.status()
		.to_owned();
	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("activity marker should load")
		.expect("activity marker should exist");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, run_id, 1)
		.expect("private events should load");

	StalledPhaseGoalOutcome {
		run_status,
		retry_kind: marker.retry_kind().map(str::to_owned),
		retry_ready_at: marker.retry_ready_at_unix_epoch(),
		comments: tracker.comments.borrow().clone(),
		label_additions_empty: tracker.label_additions.borrow().is_empty(),
		event_types: events.iter().map(|event| event.event_type().to_owned()).collect(),
		validation_evidence_reason: events
			.iter()
			.rev()
			.find(|event| event.event_type() == VALIDATION_EVIDENCE_EVENT_TYPE)
			.and_then(|event| event.payload()["reason_code"].as_str().map(str::to_owned)),
		handoff_next_recorded: events.iter().any(|event| {
			event.event_type() == "phase_goal_next"
				&& event.payload()["phase"] == "handoff_evidence"
		}),
	}
}

#[test]
fn schedules_continuation_without_attention() {
	let outcome = apply_stalled_phase_goal_reconciliation("verifying", serde_json::json!([]));

	assert_eq!(outcome.run_status, CONTINUATION_PENDING_RUN_STATUS);
	assert_eq!(outcome.retry_kind.as_deref(), Some("continuation"));
	assert!(outcome.retry_ready_at.is_some());
	assert!(outcome.comments.is_empty());
	assert!(outcome.label_additions_empty);
	assert!(outcome.event_types.iter().any(|event| event == "phase_goal_recovery"));
	assert_eq!(outcome.validation_evidence_reason.as_deref(), Some("accepted"));
	assert!(outcome.handoff_next_recorded);
}

#[test]
fn stalled_retained_phase_goal_reconciliation_preserves_attention_when_blocked() {
	let outcome = apply_stalled_phase_goal_reconciliation(
		"blocked",
		serde_json::json!(["external evidence is missing"]),
	);

	assert_ne!(outcome.retry_kind.as_deref(), Some("continuation"));
	assert!(outcome.comments.iter().any(|comment| {
		comment.contains("decodex retained partial progress and needs attention")
			&& comment.contains("partial_progress_retained")
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
