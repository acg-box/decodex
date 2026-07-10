use crate::{
	autonomy_signal::{AutonomySignal, tests},
	state::StateStore,
};

#[test]
fn autonomy_signal_persistent_store_round_trips_signal_payload() {
	let tempdir = tempfile::tempdir().expect("tempdir should create");
	let db_path = tempdir.path().join("runtime.sqlite3");
	let signal = {
		let store = StateStore::open(&db_path).expect("store should open");

		tests::accept_objective(&store, 1);

		let signal = AutonomySignal::runtime_health(tests::signal_input())
			.expect("runtime signal should validate");

		assert_eq!(signal.schema(), "decodex.autonomy_signal/1");
		assert_eq!(signal.record_version(), 1);

		store.record_autonomy_signal("decodex", signal.clone()).expect("signal should store");

		signal
	};
	let reopened = StateStore::open(&db_path).expect("store should reopen");
	let stored = reopened
		.autonomy_signal("decodex", signal.id())
		.expect("signal read should succeed")
		.expect("signal should exist");

	assert_eq!(stored.signal(), &signal);
	assert_eq!(stored.signal().schema(), "decodex.autonomy_signal/1");
	assert_eq!(stored.signal().record_version(), 1);
	assert_eq!(stored.signal().source_refs(), ["status:XY-1085:runtime-health"]);
	assert!(stored.signal().primary_source_refs().is_empty());
}
