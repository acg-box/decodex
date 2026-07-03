use std::path::PathBuf;

use tempfile::TempDir;

use crate::{
	agent::app_server::tests::{
		ContinuingCompletionHandler, ProbeDynamicToolHandler, RejectingCompletionHandler,
		RejectingContinuationGuard, RunRecorder, TurnCompletionStatus, YieldingContinuationGuard,
	},
	state::StateStore,
};

#[test]
fn run_recorder_keeps_events_when_marker_write_fails() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let missing_worktree = PathBuf::from(temp_dir.path()).join("missing-worktree");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, Some(&missing_worktree));

	recorder.mark_activity().expect("marker failures should be non-fatal");
	recorder.record("turn/started", "{\"turn\":\"1\"}").expect("event should record");

	assert_eq!(state_store.event_count("run-1").expect("event count should load"), 1);
}

#[test]
fn completion_classification_uses_dynamic_tool_handler() {
	let error = super::classify_turn_completion(Some(&RejectingCompletionHandler), "finished")
		.expect_err("completion classifier should be consulted");

	assert!(error.to_string().contains("terminal finalization missing"));
}

#[test]
fn completion_classification_defaults_to_complete_without_handler() {
	assert_eq!(
		super::classify_turn_completion(None, "finished")
			.expect("missing dynamic handler should not fail completion"),
		TurnCompletionStatus::Complete
	);
}

#[test]
fn probe_handler_allows_completion_classification() {
	assert_eq!(
		super::classify_turn_completion(Some(&ProbeDynamicToolHandler), "PROBE_OK")
			.expect("probe handler should not override completion validation"),
		TurnCompletionStatus::Complete
	);
}

#[test]
fn nonterminal_single_turn_completion_stays_invalid() {
	let error = super::reject_nonterminal_single_turn_completion(
		Some(&ContinuingCompletionHandler),
		"unfinished",
	)
	.expect_err("single-turn mode should preserve terminal completion validation");

	assert!(error.to_string().contains("terminal finalization missing"));
}

#[test]
fn continuation_boundary_reached_yields_when_guard_allows_it() {
	assert!(
		super::continuation_boundary_reached(Some(&YieldingContinuationGuard), 2)
			.expect("yielding guard should allow a clean continuation boundary")
	);
}

#[test]
fn continuation_boundary_reached_rejects_invalid_boundary() {
	let error = super::continuation_boundary_reached(Some(&RejectingContinuationGuard), 1)
		.expect_err("invalid continuation boundaries should surface as errors");

	assert!(error.to_string().contains("turn 1 hit an invalid continuation boundary"));
}
