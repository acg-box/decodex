use std::{
	sync::{Arc, Barrier},
	thread,
};

use tempfile::TempDir;

use crate::state::StateStore;

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
