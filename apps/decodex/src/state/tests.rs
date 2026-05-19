#[cfg(unix)] use std::os::fd::{AsRawFd, IntoRawFd};
use std::{
	fs,
	path::Path,
	process, slice,
	sync::{Arc, Barrier},
	thread,
};

#[cfg(unix)] use libc::{F_GETFD, FD_CLOEXEC};
use tempfile::TempDir;

use crate::{
	state::{
		self, ChildAgentActivitySummary, CodexAccountActivitySummary, CodexAccountMarker,
		DispatchSlotLimit, EffectiveRuntimeMarker, PreacquiredLeaseGuards, ProjectRegistration,
		ProtocolActivityMarker, ProtocolActivitySummary, RUN_ACTIVITY_MARKER_FILE,
		RUN_OPERATION_REPO_GATE, ReviewHandoffMarker, ReviewOrchestrationMarker, StateStore,
	},
	tracker::records::{LinearExecutionEventIdentity, LinearExecutionEventRecord},
};

const IN_PROGRESS_STATE: &str = "In Progress";

#[cfg(unix)]
fn fd_has_close_on_exec(fd: i32) -> bool {
	let flags = unsafe { libc::fcntl(fd, F_GETFD) };

	assert_ne!(flags, -1, "fcntl(F_GETFD) should succeed for test fd {fd}");

	flags & FD_CLOEXEC != 0
}

#[test]
fn review_markers_roundtrip_preserve_required_fields() {
	let store = StateStore::open_in_memory().expect("state store should open");
	let handoff = ReviewHandoffMarker::new(
		"run-1",
		2,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);

	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("review handoff marker should persist");

	let restored_handoff = store
		.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
		.expect("review handoff marker should read")
		.expect("review handoff marker should exist");

	assert_eq!(restored_handoff, handoff);

	let orchestration = ReviewOrchestrationMarker::new(
		"run-1",
		2,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"waiting_for_ack",
		Some(1_234),
		Some(1_775_200_000),
		Some(3),
		1,
		2,
		Some(1_775_200_900),
	);

	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &orchestration)
		.expect("review orchestration marker should persist");

	let restored_orchestration = store
		.review_orchestration_marker("pubfi", "PUB-101", &handoff)
		.expect("review orchestration marker should read")
		.expect("review orchestration marker should exist");

	assert_eq!(restored_orchestration, orchestration);
}

#[test]
fn clear_review_markers_for_handoff_preserves_other_branches() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let removed_handoff = ReviewHandoffMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let removed_orchestration = ReviewOrchestrationMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);
	let kept_handoff = ReviewHandoffMarker::new(
		"run-2",
		1,
		"x/decodex-pub-101-review",
		"https://github.com/hack-ink/decodex/pull/102",
		"main",
		"x/decodex-pub-101-review",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let kept_orchestration = ReviewOrchestrationMarker::new(
		"run-2",
		1,
		"x/decodex-pub-101-review",
		"https://github.com/hack-ink/decodex/pull/102",
		"18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);

	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &removed_handoff)
		.expect("removed handoff marker should persist");
	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &removed_orchestration)
		.expect("removed orchestration marker should persist");
	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &kept_handoff)
		.expect("kept handoff marker should persist");
	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &kept_orchestration)
		.expect("kept orchestration marker should persist");
	store
		.clear_review_markers_for_handoff(
			"pubfi",
			"PUB-101",
			&removed_handoff,
			&removed_orchestration,
		)
		.expect("exact review markers should clear");

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");

	assert!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("removed handoff marker should read")
			.is_none()
	);
	assert_eq!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101-review")
			.expect("kept handoff marker should read"),
		Some(kept_handoff.clone())
	);
	assert_eq!(
		reopened
			.review_orchestration_marker("pubfi", "PUB-101", &kept_handoff)
			.expect("kept orchestration marker should read"),
		Some(kept_orchestration)
	);
}

#[test]
fn missing_review_markers_return_absent() {
	let store = StateStore::open_in_memory().expect("state store should open");
	let handoff = ReviewHandoffMarker::new(
		"run-1",
		2,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);

	assert!(
		store
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("review handoff marker should read")
			.is_none()
	);
	assert!(
		store
			.review_orchestration_marker("pubfi", "PUB-101", &handoff)
			.expect("review orchestration marker should read")
			.is_none()
	);
}

#[test]
fn persistent_review_markers_survive_stale_store_persist_and_are_visible() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");
	let handoff = ReviewHandoffMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let orchestration = ReviewOrchestrationMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);

	writer
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("handoff marker should persist");
	writer
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &orchestration)
		.expect("orchestration marker should persist");

	let observed_handoff = observer
		.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
		.expect("observer should read handoff marker")
		.expect("observer should see marker written by another store");

	assert_eq!(observed_handoff, handoff);

	observer
		.record_run_attempt("run-2", "PUB-202", 1, "running")
		.expect("stale observer should persist unrelated runtime state");

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");

	assert_eq!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("reopened store should read handoff marker"),
		Some(handoff.clone())
	);
	assert_eq!(
		reopened
			.review_orchestration_marker("pubfi", "PUB-101", &handoff)
			.expect("reopened store should read orchestration marker"),
		Some(orchestration)
	);
	assert!(
		reopened.run_attempt("run-2").expect("run attempt should read").is_some(),
		"unrelated stale-store persist should still keep its own update"
	);
}

#[test]
fn persistent_event_appenders_can_write_distinct_runs_concurrently() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let first = StateStore::open(&state_path).expect("first state store should open");
	let second = StateStore::open(&state_path).expect("second state store should open");

	first.record_run_attempt("run-a", "PUB-101", 1, "running").expect("first run should record");
	second.record_run_attempt("run-b", "PUB-102", 1, "running").expect("second run should record");

	let barrier = Arc::new(Barrier::new(2));
	let first_barrier = Arc::clone(&barrier);
	let first_writer = thread::spawn(move || {
		first_barrier.wait();

		for sequence_number in 1..=40 {
			first
				.append_event("run-a", sequence_number, "item/agentMessage/delta", "{}")
				.expect("first event writer should append");
		}
	});
	let second_writer = thread::spawn(move || {
		barrier.wait();

		for sequence_number in 1..=40 {
			second
				.append_event("run-b", sequence_number, "item/agentMessage/delta", "{}")
				.expect("second event writer should append");
		}
	});

	first_writer.join().expect("first event writer should finish");
	second_writer.join().expect("second event writer should finish");

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");

	assert_eq!(reopened.event_count("run-a").expect("first event count should load"), 40);
	assert_eq!(reopened.event_count("run-b").expect("second event count should load"), 40);
}

#[test]
fn persistent_append_event_does_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let first = StateStore::open(&state_path).expect("first state store should open");
	let second = StateStore::open(&state_path).expect("second state store should open");

	first.record_run_attempt("run-a", "PUB-101", 1, "running").expect("first run should record");
	second.record_run_attempt("run-b", "PUB-102", 1, "running").expect("second run should record");
	second
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("second store should append an unrelated event");
	first
		.append_event("run-a", 1, "item/agentMessage/delta", "{}")
		.expect("first store should append without full journal refresh");

	let state = first.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.events.contains_key("run-b"),
		"append_event should not refresh the full persistent event journal into the local cache"
	);

	drop(state);

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");

	assert_eq!(reopened.event_count("run-a").expect("first event count should load"), 1);
	assert_eq!(reopened.event_count("run-b").expect("second event count should load"), 1);
}

#[test]
fn persistent_run_attempt_update_does_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let first = StateStore::open(&state_path).expect("first state store should open");
	let second = StateStore::open(&state_path).expect("second state store should open");

	second.record_run_attempt("run-b", "PUB-102", 1, "running").expect("second run should record");
	second
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("second store should append an unrelated event");
	first.record_run_attempt("run-a", "PUB-101", 1, "running").expect("first run should record");
	first.update_run_thread("run-a", "thread-a").expect("first run thread should update");
	first.update_run_turn("run-a", "turn-a").expect("first run turn should update");
	first.update_run_status("run-a", "succeeded").expect("first run status should update");

	let state = first.inner.lock().expect("test should inspect the local cache");

	assert!(
		!state.events.contains_key("run-b"),
		"run attempt updates should not refresh the full persistent event journal into the local cache"
	);

	drop(state);

	let reopened = StateStore::open(&state_path).expect("reopened state store should open");
	let attempt = reopened
		.run_attempt("run-a")
		.expect("run attempt lookup should succeed")
		.expect("run attempt should persist");

	assert_eq!(attempt.status(), "succeeded");
	assert_eq!(attempt.thread_id(), Some("thread-a"));
	assert_eq!(attempt.turn_id(), Some("turn-a"));
	assert_eq!(reopened.event_count("run-b").expect("second event count should load"), 1);
}

#[test]
fn persistent_project_run_listing_does_not_refresh_full_event_journal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer = StateStore::open(&state_path).expect("observer state store should open");
	let writer = StateStore::open(&state_path).expect("writer state store should open");

	observer
		.record_run_attempt("run-a", "PUB-101", 1, "running")
		.expect("observer run should record");
	observer
		.upsert_lease("pubfi", "PUB-101", "run-a", IN_PROGRESS_STATE)
		.expect("observer lease should record");
	observer
		.upsert_worktree("pubfi", "PUB-101", "x/pubfi-pub-101", "/tmp/worktrees/pub-101")
		.expect("observer worktree should record");
	observer.append_event("run-a", 1, "item/started", "{}").expect("observer event should append");
	writer.record_run_attempt("run-b", "PUB-102", 1, "running").expect("writer run should record");
	writer
		.append_event("run-b", 1, "item/agentMessage/delta", "{}")
		.expect("writer event should append");

	let runs = observer.list_active_runs("pubfi").expect("active runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-a");
	assert_eq!(runs[0].event_count(), 1);
	assert_eq!(runs[0].last_event_type(), Some("item/started"));

	let state = observer.inner.lock().expect("test should inspect the local cache");

	assert!(
		state.events.is_empty(),
		"operator run listing should refresh event summaries without materializing event rows"
	);
	assert_eq!(
		state
			.event_summaries
			.get("run-b")
			.expect("unrelated persistent run should have a summary")
			.event_count,
		1
	);
}

#[test]
fn manages_issue_leases() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
		.expect("lease should be inserted");

	let lease = store
		.lease_for_issue("PUB-101")
		.expect("lease read should succeed")
		.expect("lease should exist");

	assert_eq!(lease.issue_id(), "PUB-101");
	assert_eq!(lease.run_id(), "run-1");
	assert_eq!(lease.project_id(), "pubfi");
	assert_eq!(lease.issue_state(), IN_PROGRESS_STATE);

	store.clear_lease("PUB-101").expect("lease should be deleted");

	assert!(store.lease_for_issue("PUB-101").expect("lease lookup should succeed").is_none());
}

#[test]
fn tracks_issue_specific_leases_without_project_limit() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	assert!(
		store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first lease acquisition should succeed")
	);
	assert!(
		store
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("second lease acquisition should succeed for another issue")
	);
	assert!(
		!store
			.try_acquire_lease("pubfi", "PUB-101", "run-3", IN_PROGRESS_STATE)
			.expect("duplicate issue acquisition should be rejected")
	);
	assert!(
		store
			.try_acquire_lease("other", "PUB-201", "run-4", IN_PROGRESS_STATE)
			.expect("other project should still acquire its own slot")
	);
}

#[test]
fn shared_dispatch_slots_honor_configured_limit_across_process_local_stores() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store_one = StateStore::open_in_memory().expect("first store should open");
	let store_two = StateStore::open_in_memory().expect("second store should open");
	let store_three = StateStore::open_in_memory().expect("third store should open");

	store_one
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("first store should configure dispatch slot root");
	store_two
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("second store should configure dispatch slot root");
	store_three
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("third store should configure dispatch slot root");

	assert!(
		store_one
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first shared lease acquisition should succeed")
	);
	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("second store should acquire the second shared slot")
	);
	assert!(
		!store_three
			.try_acquire_lease("pubfi", "PUB-103", "run-3", IN_PROGRESS_STATE)
			.expect("third store should observe the configured shared slots as busy")
	);

	store_one.clear_lease("PUB-101").expect("shared lease should clear");

	assert!(
		store_three
			.try_acquire_lease("pubfi", "PUB-103", "run-3", IN_PROGRESS_STATE)
			.expect("shared slot should reopen after one of the configured leases clears")
	);
}

#[test]
fn shared_dispatch_slots_support_unlimited_across_process_local_stores() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store_one = StateStore::open_in_memory().expect("first store should open");
	let store_two = StateStore::open_in_memory().expect("second store should open");
	let store_three = StateStore::open_in_memory().expect("third store should open");

	store_one
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), DispatchSlotLimit::Unlimited)
		.expect("first store should configure unlimited dispatch slots");
	store_two
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), DispatchSlotLimit::Unlimited)
		.expect("second store should configure unlimited dispatch slots");
	store_three
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), DispatchSlotLimit::Unlimited)
		.expect("third store should configure unlimited dispatch slots");

	assert!(
		store_one
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first shared lease acquisition should succeed")
	);
	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("second store should acquire another shared slot")
	);
	assert!(
		store_three
			.try_acquire_lease("pubfi", "PUB-103", "run-3", IN_PROGRESS_STATE)
			.expect("third store should acquire another shared slot")
	);
}

#[test]
fn failed_shared_slot_attempt_releases_issue_claim_before_retry() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store_one = StateStore::open_in_memory().expect("first store should open");
	let store_two = StateStore::open_in_memory().expect("second store should open");

	store_one
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("first store should configure dispatch slot root");
	store_two
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("second store should configure dispatch slot root");

	assert!(
		store_one
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first store should acquire the only shared slot")
	);
	assert!(
		!store_two
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("second store should fail while the only slot is busy")
	);

	store_one.clear_lease("PUB-101").expect("shared lease should clear");

	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("retry should succeed after the failed contender releases its issue claim")
	);
}

#[test]
fn shared_issue_claim_blocks_duplicate_issue_across_process_local_stores() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store_one = StateStore::open_in_memory().expect("first store should open");
	let store_two = StateStore::open_in_memory().expect("second store should open");

	store_one
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("first store should configure dispatch slot root");
	store_two
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("second store should configure dispatch slot root");

	assert!(
		store_one
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first issue claim should succeed")
	);
	assert!(
		!store_two
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("duplicate issue claim should be rejected across processes")
	);
	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-102", "run-3", IN_PROGRESS_STATE)
			.expect("another issue should still be able to use the remaining slot")
	);
}

#[test]
fn shared_issue_claim_reopens_same_issue_after_clear_across_process_local_stores() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store_one = StateStore::open_in_memory().expect("first store should open");
	let store_two = StateStore::open_in_memory().expect("second store should open");

	store_one
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("first store should configure dispatch slot root");
	store_two
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("second store should configure dispatch slot root");

	assert!(
		store_one
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("first issue claim should succeed")
	);
	assert!(
		!store_two
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("duplicate issue claim should be rejected while the first lease is active")
	);

	store_one.clear_lease("PUB-101").expect("shared issue claim should clear");

	assert!(
		store_two
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("same issue claim should reopen after the first lease clears")
	);
}

#[test]
fn shared_issue_claim_listing_reports_other_process_state() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let remote_store = StateStore::open_in_memory().expect("remote store should open");
	let observer_store = StateStore::open_in_memory().expect("observer store should open");

	remote_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("remote store should configure dispatch slot root");
	observer_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("observer store should configure dispatch slot root");

	assert!(
		remote_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("remote issue claim should succeed")
	);

	let leases = observer_store
		.list_active_shared_leases("pubfi")
		.expect("shared claim listing should succeed");

	assert_eq!(leases.len(), 1);
	assert_eq!(leases[0].issue_id(), "PUB-101");
	assert_eq!(leases[0].run_id(), "run-1");
	assert_eq!(leases[0].issue_state(), IN_PROGRESS_STATE);
}

#[cfg(unix)]
#[test]
fn adopted_dispatch_slot_blocks_after_parent_releases_local_guard() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let parent_store = StateStore::open_in_memory().expect("parent store should open");
	let child_store = StateStore::open_in_memory().expect("child store should open");
	let contender_store = StateStore::open_in_memory().expect("contender store should open");

	parent_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("parent store should configure dispatch slot root");
	child_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("child store should configure dispatch slot root");
	contender_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("contender store should configure dispatch slot root");

	assert!(
		parent_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("parent should acquire the shared slot")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child("PUB-101")
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child("PUB-101")
		.expect("child should inherit the shared dispatch-slot fd");

	child_store
		.adopt_preacquired_lease(
			"pubfi",
			"PUB-101",
			"run-1",
			IN_PROGRESS_STATE,
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");
	parent_store
		.release_dispatch_slot("PUB-101")
		.expect("parent should release its local guard after handoff");

	assert!(
		!contender_store
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("child-held guard should keep the slot busy")
	);

	child_store.clear_lease("PUB-101").expect("child lease should clear");
}

#[cfg(unix)]
#[test]
fn adopted_issue_claim_blocks_same_issue_after_parent_clears_local_guard() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let parent_store = StateStore::open_in_memory().expect("parent store should open");
	let child_store = StateStore::open_in_memory().expect("child store should open");
	let contender_store = StateStore::open_in_memory().expect("contender store should open");

	parent_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("parent store should configure dispatch slot root");
	child_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("child store should configure dispatch slot root");
	contender_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("contender store should configure dispatch slot root");

	assert!(
		parent_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("parent should acquire the shared issue claim")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child("PUB-101")
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child("PUB-101")
		.expect("child should inherit the shared dispatch-slot fd");

	child_store
		.adopt_preacquired_lease(
			"pubfi",
			"PUB-101",
			"run-1",
			IN_PROGRESS_STATE,
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");
	parent_store
		.clear_lease("PUB-101")
		.expect("parent should drop its local lease without unlocking the child handoff");

	assert!(
		!contender_store
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("same issue should stay claimed while the child still holds the handoff fd")
	);

	child_store.clear_lease("PUB-101").expect("child lease should clear");
}

#[cfg(unix)]
#[test]
fn parent_can_release_handed_off_guards_without_dropping_runtime_lease() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let parent_store = StateStore::open_in_memory().expect("parent store should open");
	let child_store = StateStore::open_in_memory().expect("child store should open");
	let contender_store = StateStore::open_in_memory().expect("contender store should open");

	parent_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("parent store should configure dispatch slot root");
	child_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("child store should configure dispatch slot root");
	contender_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 2)
		.expect("contender store should configure dispatch slot root");

	assert!(
		parent_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("parent should acquire the shared issue claim")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child("PUB-101")
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child("PUB-101")
		.expect("child should inherit the shared dispatch-slot fd");

	child_store
		.adopt_preacquired_lease(
			"pubfi",
			"PUB-101",
			"run-1",
			IN_PROGRESS_STATE,
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");
	parent_store
		.release_handed_off_guards("PUB-101")
		.expect("parent should release process-local guards after handoff");

	assert!(
		parent_store
			.lease_for_issue("PUB-101")
			.expect("parent lease lookup should succeed")
			.is_some(),
		"parent must keep the runtime lease visible after dropping local fd guards"
	);
	assert!(
		!contender_store
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("same issue should stay claimed by the child handoff")
	);
	assert!(
		contender_store
			.try_acquire_lease("pubfi", "PUB-102", "run-2", IN_PROGRESS_STATE)
			.expect("another issue should acquire the second dispatch slot")
	);

	child_store.clear_lease("PUB-101").expect("child lease should clear");
}

#[cfg(unix)]
#[test]
fn adopted_preacquired_lease_restores_close_on_exec_on_inherited_fds() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let parent_store = StateStore::open_in_memory().expect("parent store should open");
	let child_store = StateStore::open_in_memory().expect("child store should open");

	parent_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("parent store should configure dispatch slot root");
	child_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("child store should configure dispatch slot root");

	assert!(
		parent_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("parent should acquire the shared slot")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child("PUB-101")
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child("PUB-101")
		.expect("child should inherit the shared dispatch-slot fd");
	let issue_claim_fd = child_issue_claim.as_raw_fd();
	let dispatch_slot_fd = child_guard.as_raw_fd();

	assert!(
		!fd_has_close_on_exec(issue_claim_fd),
		"handoff issue-claim fd should clear close-on-exec before exec"
	);
	assert!(
		!fd_has_close_on_exec(dispatch_slot_fd),
		"handoff dispatch-slot fd should clear close-on-exec before exec"
	);

	child_store
		.adopt_preacquired_lease(
			"pubfi",
			"PUB-101",
			"run-1",
			IN_PROGRESS_STATE,
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");

	assert!(
		fd_has_close_on_exec(issue_claim_fd),
		"adopted issue-claim fd must restore close-on-exec before spawning grandchildren"
	);
	assert!(
		fd_has_close_on_exec(dispatch_slot_fd),
		"adopted dispatch-slot fd must restore close-on-exec before spawning grandchildren"
	);

	child_store.clear_lease("PUB-101").expect("child lease should clear");
	parent_store.clear_lease("PUB-101").expect("parent lease should clear");
}

#[cfg(unix)]
#[test]
fn adopted_child_clear_releases_lock_when_descendant_keeps_inherited_fds_open() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let parent_store = StateStore::open_in_memory().expect("parent store should open");
	let child_store = StateStore::open_in_memory().expect("child store should open");
	let contender_store = StateStore::open_in_memory().expect("contender store should open");

	parent_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("parent store should configure dispatch slot root");
	child_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("child store should configure dispatch slot root");
	contender_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path(), 1)
		.expect("contender store should configure dispatch slot root");

	assert!(
		parent_store
			.try_acquire_lease("pubfi", "PUB-101", "run-1", IN_PROGRESS_STATE)
			.expect("parent should acquire the shared slot")
	);

	let child_issue_claim = parent_store
		.clone_issue_claim_for_child("PUB-101")
		.expect("child should inherit the shared issue-claim fd");
	let (child_guard, child_slot_index) = parent_store
		.clone_dispatch_slot_for_child("PUB-101")
		.expect("child should inherit the shared dispatch-slot fd");
	let _descendant_issue_claim =
		child_issue_claim.try_clone().expect("descendant should inherit the issue-claim fd");
	let _descendant_guard =
		child_guard.try_clone().expect("descendant should inherit the dispatch-slot fd");

	child_store
		.adopt_preacquired_lease(
			"pubfi",
			"PUB-101",
			"run-1",
			IN_PROGRESS_STATE,
			PreacquiredLeaseGuards {
				issue_claim_fd: child_issue_claim.into_raw_fd(),
				dispatch_slot_fd: child_guard.into_raw_fd(),
				dispatch_slot_index: child_slot_index,
			},
		)
		.expect("child should adopt the inherited lease guard");
	parent_store.clear_lease("PUB-101").expect("parent should drop its local handoff guard");
	child_store.clear_lease("PUB-101").expect("child lease should clear");

	assert!(
		contender_store
			.try_acquire_lease("pubfi", "PUB-101", "run-2", IN_PROGRESS_STATE)
			.expect("descendant-held fds must not keep the cleared lease claimed"),
		"clearing an adopted child lease must release the shared claim and slot even if a descendant still holds inherited fds"
	);
}

#[test]
fn records_run_attempts_and_events() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.record_run_attempt("run-1", "PUB-101", 1, "running")
		.expect("run attempt should be recorded");
	store.update_run_thread("run-1", "thread-1").expect("thread id should be attached");
	store
		.append_event("run-1", 1, "turn/started", "{\"turn\":\"1\"}")
		.expect("event should be recorded");

	let run_attempt = store
		.run_attempt("run-1")
		.expect("run attempt query should succeed")
		.expect("run attempt should exist");

	assert_eq!(run_attempt.issue_id(), "PUB-101");
	assert_eq!(run_attempt.attempt_number(), 1);
	assert_eq!(run_attempt.status(), "running");
	assert_eq!(run_attempt.thread_id(), Some("thread-1"));
	assert_eq!(store.event_count("run-1").expect("event count should succeed"), 1);
	assert_eq!(store.next_attempt_number("PUB-101").expect("next attempt should load"), 2);
	assert_eq!(
		store.retry_budget_attempt_count("PUB-101").expect("retry budget count should load"),
		0
	);

	store.update_run_status("run-1", "interrupted").expect("status should update");

	let updated = store
		.run_attempt("run-1")
		.expect("run attempt query should succeed")
		.expect("run attempt should exist");

	assert_eq!(updated.status(), "interrupted");
	assert!(
		store
			.last_run_activity_unix_epoch("run-1")
			.expect("last activity lookup should succeed")
			.is_some()
	);
}

#[test]
fn run_activity_marker_round_trips_marker_surfaces() {
	assert_run_activity_marker_round_trips_clearable_auxiliary_fields();
	assert_run_activity_marker_round_trips_thread_and_protocol_summary_fields();
	assert_run_activity_marker_round_trips_child_agent_activity_summary();
	assert_run_activity_marker_round_trips_account_summary();
}

fn assert_run_activity_marker_round_trips_clearable_auxiliary_fields() {
	let temp_dir = TempDir::new().expect("tempdir should create");

	state::write_run_activity_marker_for_process(temp_dir.path(), "run-1", 1, process::id())
		.expect("activity marker should write");
	state::write_run_retry_schedule(temp_dir.path(), "run-1", 1, "failure", 12_345)
		.expect("retry schedule should write");
	state::write_run_review_policy_state(
		temp_dir.path(),
		"run-1",
		1,
		"handoff",
		"findings",
		"abc123",
		2,
	)
	.expect("review policy state should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.run_id(), "run-1");
	assert_eq!(marker.attempt_number(), 1);

	if let Some(host_boot_id) = state::current_host_boot_id() {
		assert_eq!(marker.host_boot_id(), Some(host_boot_id.as_str()));
		assert!(
			fs::read_to_string(temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE))
				.expect("activity marker body should load")
				.contains(&format!("host_boot_id={host_boot_id}\n")),
			"activity markers should record the host boot identity for reboot-safe liveness"
		);
	}
	if let Some(process_start_identity) = state::current_process_start_identity() {
		assert_eq!(marker.process_start_identity(), Some(process_start_identity.as_str()));
		assert!(
			fs::read_to_string(temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE))
				.expect("activity marker body should load")
				.contains(&format!("process_start_identity={process_start_identity}\n")),
			"activity markers should record the process start identity for PID-reuse-safe liveness"
		);
	}

	assert_eq!(marker.retry_kind(), Some("failure"));
	assert_eq!(marker.retry_ready_at_unix_epoch(), Some(12_345));
	assert_eq!(marker.review_policy_phase(), Some("handoff"));
	assert_eq!(marker.review_policy_status(), Some("findings"));
	assert_eq!(marker.review_policy_head_sha(), Some("abc123"));
	assert_eq!(marker.review_policy_nonclean_rounds(), Some(2));

	state::clear_run_retry_schedule(temp_dir.path()).expect("retry schedule should clear");

	let retry_cleared = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should reload")
		.expect("marker snapshot should still exist");

	assert_eq!(retry_cleared.retry_kind(), None);
	assert_eq!(retry_cleared.retry_ready_at_unix_epoch(), None);
	assert_eq!(retry_cleared.review_policy_phase(), Some("handoff"));

	state::clear_run_review_policy_state(temp_dir.path())
		.expect("review policy state should clear");

	let policy_cleared = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should reload")
		.expect("marker snapshot should still exist");

	assert_eq!(policy_cleared.review_policy_phase(), None);
	assert_eq!(policy_cleared.review_policy_status(), None);
	assert_eq!(policy_cleared.review_policy_head_sha(), None);
	assert_eq!(policy_cleared.review_policy_nonclean_rounds(), None);
}

fn assert_run_activity_marker_round_trips_thread_and_protocol_summary_fields() {
	let temp_dir = TempDir::new().expect("tempdir should create");

	state::write_run_activity_marker_for_process(temp_dir.path(), "run-1", 1, process::id())
		.expect("activity marker should write");
	state::write_run_thread_marker(temp_dir.path(), "run-1", 1, "thread-1")
		.expect("thread marker should write");
	state::write_run_turn_marker(temp_dir.path(), "run-1", 1, "turn-1")
		.expect("turn marker should write");
	state::write_run_thread_status_marker(
		temp_dir.path(),
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[String::from("waitingOnApproval")],
	)
	.expect("thread status marker should write");
	state::write_run_effective_runtime_marker(
		temp_dir.path(),
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
	.expect("effective runtime marker should write");

	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		waiting_reason: Some(String::from("model_execution")),
		rate_limit_status: Some(String::from("usageLimitExceeded")),
		recent_events: vec![
			state::ProtocolActivityEventSummary {
				event_type: String::from("turn/started"),
				category: String::from("turn"),
				detail: Some(String::from("running")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("turn/completed"),
				category: String::from("turn"),
				detail: Some(String::from("completed")),
			},
		],
	};

	state::write_run_protocol_activity_marker(
		temp_dir.path(),
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 3,
			last_event_type: "turn/completed",
			child_agent_activity: None,
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol summary should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.thread_id(), Some("thread-1"));
	assert_eq!(marker.turn_id(), Some("turn-1"));
	assert_eq!(marker.thread_status(), Some("active"));
	assert_eq!(marker.thread_active_flags(), &[String::from("waitingOnApproval")]);
	assert_eq!(marker.event_count(), 3);
	assert_eq!(marker.last_event_type(), Some("turn/completed"));
	assert_eq!(marker.effective_model(), Some("gpt-5.4"));
	assert_eq!(marker.effective_model_provider(), Some("openai"));
	assert_eq!(marker.effective_cwd(), Some("/tmp/worktree"));
	assert_eq!(marker.effective_approval_policy(), Some("never"));
	assert_eq!(marker.effective_approvals_reviewer(), Some("human"));
	assert_eq!(marker.effective_sandbox_mode(), Some("workspaceWrite"));
	assert_eq!(marker.protocol_activity(), Some(&protocol_activity));
	assert!(marker.last_protocol_activity_unix_epoch().is_some());
	assert_eq!(marker.current_operation(), Some(state::RUN_OPERATION_AGENT_RUN));
	assert!(marker.last_progress_unix_epoch().is_some());
}

fn assert_run_activity_marker_round_trips_child_agent_activity_summary() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let summary = ChildAgentActivitySummary {
		buckets: vec![
			state::ChildAgentActivityBucket {
				name: String::from("Model"),
				wall_seconds: 693,
				event_count: 12,
				tool_call_count: 0,
				input_tokens: 4_270_000,
				output_tokens: 12_000,
				output_bytes: 0,
			},
			state::ChildAgentActivityBucket {
				name: String::from("Browser/Image"),
				wall_seconds: 41,
				event_count: 6,
				tool_call_count: 3,
				input_tokens: 0,
				output_tokens: 0,
				output_bytes: 180_000,
			},
		],
		current_bucket: Some(String::from("Model")),
		current_detail: Some(String::from("waiting after tool output")),
		current_started_unix_epoch: Some(1_800_000_000),
		current_elapsed_seconds: Some(9),
		wall_seconds: 734,
		event_count: 18,
		tool_call_count: 3,
		input_tokens_current: Some(105_000),
		input_tokens_max: Some(105_000),
		input_tokens_cumulative: 4_270_000,
		output_tokens_cumulative: 12_000,
		largest_tool_output_bytes: Some(180_000),
		largest_tool_output_tool: Some(String::from("view_image")),
		large_output_warnings: vec![String::from(
			"view_image repeated 3 large outputs; largest 180000 bytes",
		)],
	};

	state::write_run_protocol_activity_marker(
		temp_dir.path(),
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 18,
			last_event_type: "item/tool/call/response",
			child_agent_activity: Some(&summary),
			protocol_activity: None,
		},
	)
	.expect("protocol summary should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.child_agent_activity(), Some(&summary));
}

fn assert_run_activity_marker_round_trips_account_summary() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let summary = CodexAccountActivitySummary {
		account_fingerprint: String::from("acct_...cdef"),
		email: Some(String::from("account@example.com")),
		plan_type: Some(String::from("pro")),
		status: String::from("selected"),
		refresh_status: String::from("not_needed"),
		checked_at_unix_epoch: Some(1_800_000_010),
		selected_at_unix_epoch: Some(1_800_000_011),
		primary_window_seconds: Some(18_000),
		primary_remaining_percent: Some(72),
		primary_resets_at_unix_epoch: Some(1_800_018_000),
		secondary_window_seconds: Some(604_800),
		secondary_remaining_percent: Some(91),
		secondary_resets_at_unix_epoch: Some(1_800_604_800),
		credits_has_credits: Some(true),
		credits_unlimited: Some(false),
		credits_balance: Some(String::from("9.99")),
		rate_limit_reached_type: None,
		cooldown_until_unix_epoch: None,
		note: Some(String::from("usage probe ok")),
	};

	state::write_run_account_marker(
		temp_dir.path(),
		&CodexAccountMarker {
			run_id: "run-1",
			attempt_number: 1,
			account: &summary,
			accounts: slice::from_ref(&summary),
		},
	)
	.expect("account summary should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.account(), Some(&summary));
	assert_eq!(marker.accounts(), slice::from_ref(&summary));

	let body = fs::read_to_string(temp_dir.path().join(RUN_ACTIVITY_MARKER_FILE))
		.expect("marker body should read");

	assert!(body.contains("account="));
	assert!(body.contains("accounts="));
	assert!(!body.contains("codex_account="));
	assert!(!body.contains("codex_accounts="));
}

#[test]
fn run_operation_marker_resets_stale_per_attempt_fields_on_new_attempt() {
	let temp_dir = TempDir::new().expect("tempdir should create");

	state::write_run_activity_marker_for_process(temp_dir.path(), "run-1", 1, process::id())
		.expect("first activity marker should write");
	state::write_run_thread_marker(temp_dir.path(), "run-1", 1, "thread-1")
		.expect("thread marker should write");
	state::write_run_turn_marker(temp_dir.path(), "run-1", 1, "turn-1")
		.expect("turn marker should write");
	state::write_run_thread_status_marker(
		temp_dir.path(),
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[String::from("waitingOnUserInput")],
	)
	.expect("thread status should write");
	state::write_run_effective_runtime_marker(
		temp_dir.path(),
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
			effective_sandbox_mode: "dangerFullAccess",
		},
	)
	.expect("effective runtime should write");
	state::write_run_protocol_activity_marker(
		temp_dir.path(),
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 3,
			last_event_type: "turn/completed",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol summary should write");
	state::write_run_retry_schedule(temp_dir.path(), "run-1", 1, "failure", 123)
		.expect("retry schedule should write");
	state::write_run_retry_budget_attempt_count(temp_dir.path(), "run-1", 1, 2)
		.expect("retry budget should write");
	state::write_run_review_policy_state(
		temp_dir.path(),
		"run-1",
		1,
		"repair",
		"findings",
		"def456",
		2,
	)
	.expect("review policy should write");
	state::write_run_operation_marker(temp_dir.path(), "run-2", 2, RUN_OPERATION_REPO_GATE)
		.expect("next attempt operation marker should write");

	let marker = state::read_run_activity_marker_snapshot(temp_dir.path())
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");

	assert_eq!(marker.run_id(), "run-2");
	assert_eq!(marker.attempt_number(), 2);
	assert_eq!(marker.current_operation(), Some(state::RUN_OPERATION_REPO_GATE));
	assert!(marker.last_progress_unix_epoch().is_some());
	assert_eq!(marker.thread_id(), None);
	assert_eq!(marker.turn_id(), None);
	assert_eq!(marker.thread_status(), None);
	assert!(marker.thread_active_flags().is_empty());
	assert_eq!(marker.event_count(), 0);
	assert_eq!(marker.last_event_type(), None);
	assert_eq!(marker.protocol_activity(), None);
	assert_eq!(marker.effective_model(), None);
	assert_eq!(marker.effective_model_provider(), None);
	assert_eq!(marker.effective_cwd(), None);
	assert_eq!(marker.effective_approval_policy(), None);
	assert_eq!(marker.effective_approvals_reviewer(), None);
	assert_eq!(marker.effective_sandbox_mode(), None);
	assert_eq!(marker.last_protocol_activity_unix_epoch(), None);
	assert_eq!(marker.retry_kind(), None);
	assert_eq!(marker.retry_ready_at_unix_epoch(), None);
	assert_eq!(
		state::read_run_retry_budget_attempt_count(temp_dir.path())
			.expect("retry budget count should load"),
		Some(2)
	);
	assert_eq!(marker.review_policy_phase(), Some("repair"));
	assert_eq!(marker.review_policy_status(), Some("findings"));
	assert_eq!(marker.review_policy_head_sha(), Some("def456"));
	assert_eq!(marker.review_policy_nonclean_rounds(), Some(2));
}

#[test]
fn counts_retry_budget_attempts_per_issue() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store.record_run_attempt("run-1", "PUB-101", 1, "succeeded").expect("first run should record");
	store.record_run_attempt("run-2", "PUB-101", 2, "failed").expect("second run should record");
	store
		.record_run_attempt("run-3", "PUB-101", 3, "interrupted")
		.expect("third run should record");
	store
		.record_run_attempt("run-5", "PUB-101", 4, "terminal_guarded")
		.expect("guarded run should record");
	store
		.record_run_attempt("run-4", "PUB-102", 1, "failed")
		.expect("other issue run should record");

	assert_eq!(
		store.retry_budget_attempt_count("PUB-101").expect("retry budget count should load"),
		3
	);
	assert_eq!(
		store.retry_budget_attempt_count("PUB-102").expect("retry budget count should load"),
		1
	);
}

#[test]
fn loads_latest_run_attempt_for_issue() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store.record_run_attempt("run-1", "PUB-101", 1, "failed").expect("first run should record");
	store
		.record_run_attempt("run-2", "PUB-101", 2, "terminal_guarded")
		.expect("latest run should record");

	let attempt = store
		.latest_run_attempt_for_issue("PUB-101")
		.expect("latest run lookup should succeed")
		.expect("latest run should exist");

	assert_eq!(attempt.run_id(), "run-2");
	assert_eq!(attempt.attempt_number(), 2);
	assert_eq!(attempt.status(), "terminal_guarded");
}

#[test]
fn manages_worktree_mappings() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_worktree("pubfi", "PUB-101", "x/pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree mapping should be recorded");

	let mapping = store
		.worktree_for_issue("PUB-101")
		.expect("mapping lookup should succeed")
		.expect("mapping should exist");

	assert_eq!(mapping.issue_id(), "PUB-101");
	assert_eq!(mapping.branch_name(), "x/pub-101");
	assert_eq!(mapping.worktree_path(), Path::new("/tmp/worktrees/pub-101"));
	assert_eq!(mapping.project_id(), "pubfi");
	assert_eq!(store.list_worktrees("pubfi").expect("list should succeed").len(), 1);

	store.clear_worktree("PUB-101").expect("mapping should be deleted");

	assert!(store.worktree_for_issue("PUB-101").expect("lookup should succeed").is_none());
}

#[test]
fn persistent_clear_worktree_deletes_review_markers() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let handoff = ReviewHandoffMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);
	let orchestration = ReviewOrchestrationMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);

	store
		.upsert_worktree("pubfi", "PUB-101", "x/decodex-pub-101", "/tmp/worktrees/pub-101")
		.expect("worktree mapping should be recorded");
	store
		.upsert_review_handoff_marker("pubfi", "PUB-101", &handoff)
		.expect("handoff marker should persist");
	store
		.upsert_review_orchestration_marker("pubfi", "PUB-101", &orchestration)
		.expect("orchestration marker should persist");
	store.clear_worktree("PUB-101").expect("worktree cleanup should persist");

	let reopened = StateStore::open(&state_path).expect("reopened store should open");

	assert!(
		reopened.worktree_for_issue("PUB-101").expect("worktree lookup should succeed").is_none()
	);
	assert!(
		reopened
			.review_handoff_marker("pubfi", "PUB-101", "x/decodex-pub-101")
			.expect("handoff lookup should succeed")
			.is_none()
	);
	assert!(
		reopened
			.review_orchestration_marker("pubfi", "PUB-101", &handoff)
			.expect("orchestration lookup should succeed")
			.is_none()
	);
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
		.expect("active run attempt should be recorded");
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
	assert!(runs[0].active_lease());
	assert_eq!(runs[0].thread_id(), Some("thread-1"));
	assert_eq!(runs[0].event_count(), 2);
	assert_eq!(runs[0].last_event_type(), Some("turn/completed"));
	assert_eq!(runs[0].branch_name(), Some("x/pubfi-pub-101"));
	assert_eq!(runs[0].worktree_path(), Some(Path::new("/tmp/worktrees/pub-101")));
	assert_eq!(runs[1].run_id(), "run-2");
	assert!(!runs[1].active_lease());
	assert_eq!(runs[1].event_count(), 0);
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
	store.clear_lease("PUB-101").expect("terminal cleanup should clear active lease");
	store.clear_worktree("PUB-101").expect("terminal cleanup should clear worktree mapping");

	let runs = store.list_recent_runs("pubfi", 10).expect("recent project runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-1");
	assert_eq!(runs[0].status(), "succeeded");
	assert!(!runs[0].active_lease());
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

	let runs = store.list_active_runs("pubfi").expect("active project runs should load");

	assert_eq!(runs.len(), 1);
	assert_eq!(runs[0].run_id(), "run-1");
	assert!(runs[0].active_lease());
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
		"persistent store should recover active leases"
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
	assert!(project.enabled(), "pubfi refresh should replace the previously disabled row");
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
