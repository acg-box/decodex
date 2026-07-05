use crate::{agent::app_server::tests::recorder::RunRecorder, state::StateStore};

#[test]
fn recorder_treats_matching_protocol_replay_as_idempotent() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let payload = r#"{"threadId":"thread-1","attemptNumber":5}"#;
	let mut recorder = RunRecorder::new(&state_store, "run-1", 1, None);

	recorder.record("thread/archive", payload).expect("archive event should record");

	recorder.next_sequence = 1;

	recorder
		.record("thread/archive", payload)
		.expect("matching protocol replay should not fail the app-server run");

	assert_eq!(state_store.event_count("run-1").expect("event count should load"), 1);
	assert_eq!(recorder.next_sequence, 2);
}
