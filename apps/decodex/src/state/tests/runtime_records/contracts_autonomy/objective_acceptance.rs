use tempfile::TempDir;

use crate::{
	autonomy_objective::AutonomyObjectiveState,
	state::{StateStore, tests},
};

#[test]
fn autonomy_objective_draft_accept_current_history_and_supersession_persist() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let store = StateStore::open(&state_path).expect("state store should open");
	let draft_v1 = store
		.upsert_autonomy_objective_draft("decodex", tests::autonomy_objective_fixture(1))
		.expect("objective draft v1 should persist");

	assert_eq!(draft_v1.project_id(), "decodex");
	assert_eq!(draft_v1.objective_id(), "quality-autonomy");
	assert_eq!(draft_v1.version(), 1);
	assert_eq!(draft_v1.state(), AutonomyObjectiveState::Draft);
	assert_eq!(
		store
			.autonomy_objective("decodex", "quality-autonomy", 1)
			.expect("draft objective should read")
			.expect("draft objective should exist")
			.state(),
		AutonomyObjectiveState::Draft
	);

	let accepted_v1 = store
		.accept_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			1,
			tests::sample_objective_acceptance(),
		)
		.expect("objective v1 should accept");

	assert_eq!(accepted_v1.state(), AutonomyObjectiveState::Accepted);
	assert_eq!(
		accepted_v1.objective().acceptance().expect("acceptance should be retained").accepted_by(),
		"operator"
	);
	assert_eq!(
		store
			.autonomy_objective("decodex", "quality-autonomy", 1)
			.expect("accepted objective should read")
			.expect("accepted objective should exist")
			.state(),
		AutonomyObjectiveState::Accepted
	);
	assert!(
		store
			.upsert_autonomy_objective_draft("decodex", tests::autonomy_objective_fixture(1))
			.is_err(),
		"accepted objective versions must not be overwritten as drafts"
	);

	store
		.upsert_autonomy_objective_draft("decodex", tests::autonomy_objective_fixture(2))
		.expect("objective draft v2 should persist");

	let accepted_v2 = store
		.accept_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			2,
			tests::sample_objective_acceptance(),
		)
		.expect("objective v2 should accept and supersede v1");

	assert_eq!(accepted_v2.version(), 2);
	assert_eq!(accepted_v2.state(), AutonomyObjectiveState::Accepted);

	let current = store
		.current_accepted_autonomy_objective("decodex", "quality-autonomy")
		.expect("current accepted objective should read")
		.expect("current accepted objective should exist");

	assert_eq!(current.version(), 2);

	let reopened = StateStore::open(&state_path).expect("state store should reopen");
	let history = reopened
		.list_autonomy_objective_history("decodex", "quality-autonomy")
		.expect("objective history should list");

	assert_eq!(history.len(), 2);
	assert_eq!(history[0].version(), 1);
	assert_eq!(history[0].state(), AutonomyObjectiveState::Superseded);
	assert_eq!(
		history[0]
			.objective()
			.supersession()
			.expect("supersession should be retained")
			.superseded_by_version(),
		2
	);
	assert_eq!(
		history[0].objective().summary(),
		"Improve Decodex autonomy quality version 1.",
		"superseding an accepted version must preserve its objective body"
	);
	assert_eq!(history[1].version(), 2);
	assert_eq!(history[1].state(), AutonomyObjectiveState::Accepted);
}
