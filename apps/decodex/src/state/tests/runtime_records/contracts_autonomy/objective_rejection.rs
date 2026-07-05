use crate::{
	autonomy_objective::{
		AutonomyObjectiveRejection, AutonomyObjectiveState, AutonomyObjectiveSupersession,
	},
	state::{StateStore, tests},
};

#[test]
fn autonomy_objective_rejection_and_explicit_supersession_keep_provenance() {
	let store = StateStore::open_in_memory().expect("in-memory state store should open");

	store
		.upsert_autonomy_objective_draft("decodex", tests::autonomy_objective_fixture(1))
		.expect("objective draft v1 should persist");

	let rejected = store
		.reject_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			1,
			AutonomyObjectiveRejection::new(
				"operator",
				"2026-06-22T10:05:00Z",
				"conversation",
				"Objective version needs narrower surfaces.",
			)
			.expect("rejection should validate"),
		)
		.expect("objective draft should reject");

	assert_eq!(rejected.state(), AutonomyObjectiveState::Rejected);
	assert_eq!(
		rejected.objective().rejection().expect("rejection should exist").reason(),
		"Objective version needs narrower surfaces."
	);
	assert_eq!(
		store
			.autonomy_objective("decodex", "quality-autonomy", 1)
			.expect("rejected objective should read")
			.expect("rejected objective should exist")
			.state(),
		AutonomyObjectiveState::Rejected
	);
	assert!(
		store
			.accept_autonomy_objective_version(
				"decodex",
				"quality-autonomy",
				1,
				tests::sample_objective_acceptance()
			)
			.is_err(),
		"rejected objective versions cannot later become accepted authority"
	);

	store
		.upsert_autonomy_objective_draft("decodex", tests::autonomy_objective_fixture(2))
		.expect("objective draft v2 should persist");

	let superseded = store
		.supersede_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			2,
			AutonomyObjectiveSupersession::new(
				"quality-autonomy",
				3,
				"operator",
				"2026-06-22T10:10:00Z",
				"conversation",
				"Draft was replaced before acceptance.",
			)
			.expect("supersession should validate"),
		)
		.expect("objective draft should supersede");

	assert_eq!(superseded.state(), AutonomyObjectiveState::Superseded);
	assert_eq!(
		superseded
			.objective()
			.supersession()
			.expect("supersession should exist")
			.superseded_by_version(),
		3
	);
	assert_eq!(
		store
			.autonomy_objective("decodex", "quality-autonomy", 2)
			.expect("superseded objective should read")
			.expect("superseded objective should exist")
			.state(),
		AutonomyObjectiveState::Superseded
	);
	assert_eq!(
		store
			.upsert_autonomy_objective_draft("decodex", tests::autonomy_objective_fixture(3))
			.expect("objective draft v3 should persist")
			.state(),
		AutonomyObjectiveState::Draft
	);
	assert!(
		store
			.supersede_autonomy_objective_version(
				"decodex",
				"quality-autonomy",
				3,
				AutonomyObjectiveSupersession::new(
					"quality-autonomy",
					3,
					"operator",
					"2026-06-22T10:11:00Z",
					"conversation",
					"Invalid self-supersession.",
				)
				.expect("self-supersession payload should build"),
			)
			.is_err(),
		"same objective version cannot supersede itself"
	);
}
