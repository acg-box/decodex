use crate::state::{LoopGuardrailCheckpointInput, StateStore};

#[test]
fn loop_guardrail_checkpoints_track_fingerprints_and_retarget_issue() {
	let store = StateStore::open_in_memory().expect("state store should open");
	let first = store
		.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			reason: "validation_repeat",
			fingerprint: "fp-a",
			run_id: "run-1",
			attempt_number: 1,
			details_json: "{}",
		})
		.expect("first loop guardrail observation should persist");

	assert_eq!(first.consecutive_count(), 1);
	assert_eq!(first.reason(), "validation_repeat");

	let second = store
		.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			reason: "validation_repeat",
			fingerprint: "fp-a",
			run_id: "run-2",
			attempt_number: 2,
			details_json: "{\"attempt\":2}",
		})
		.expect("same fingerprint should increment");

	assert_eq!(second.consecutive_count(), 2);
	assert_eq!(second.run_id(), "run-2");
	assert_eq!(second.attempt_number(), 2);
	assert!(second.updated_at_unix() > 0);

	let reset = store
		.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-101",
			reason: "validation_repeat",
			fingerprint: "fp-b",
			run_id: "run-3",
			attempt_number: 3,
			details_json: "{\"attempt\":3}",
		})
		.expect("new fingerprint should reset");

	assert_eq!(reset.consecutive_count(), 1);
	assert_eq!(reset.fingerprint(), "fp-b");
	assert_eq!(reset.details_json(), "{\"attempt\":3}");
	assert!(!reset.updated_at().is_empty());

	store
		.canonicalize_issue_identity("PUB-101", "linear-id-101")
		.expect("issue identity should retarget");

	assert!(
		store
			.loop_guardrail_checkpoint("pubfi", "PUB-101", "validation_repeat")
			.expect("old checkpoint should read")
			.is_none(),
		"legacy issue identity should be cleared after retarget"
	);

	let canonical = store
		.loop_guardrail_checkpoint("pubfi", "linear-id-101", "validation_repeat")
		.expect("canonical checkpoint should read")
		.expect("canonical checkpoint should exist");

	assert_eq!(canonical.project_id(), "pubfi");
	assert_eq!(canonical.issue_id(), "linear-id-101");
	assert_eq!(canonical.fingerprint(), "fp-b");
	assert_eq!(canonical.consecutive_count(), 1);

	store
		.clear_loop_guardrail_checkpoints_for_issue("pubfi", "linear-id-101")
		.expect("checkpoint should clear");

	assert!(
		store
			.loop_guardrail_checkpoint("pubfi", "linear-id-101", "validation_repeat")
			.expect("cleared checkpoint should read")
			.is_none()
	);
}
