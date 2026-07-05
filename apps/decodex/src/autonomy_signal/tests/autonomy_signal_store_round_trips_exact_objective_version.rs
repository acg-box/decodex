use crate::{
	autonomy_signal::{
		AutonomySignal, AutonomySignalFreshness, AutonomySignalPrivacy,
		tests::{self},
	},
	state::StateStore,
};

#[test]
fn autonomy_signal_store_round_trips_exact_objective_version() {
	let store = StateStore::open_in_memory().expect("store should open");

	tests::accept_objective(&store, 1);

	let signal_v1 = AutonomySignal::runtime_health(tests::signal_input())
		.expect("runtime signal should validate");
	let stored_v1 =
		store.record_autonomy_signal("decodex", signal_v1.clone()).expect("signal should store");

	assert_eq!(stored_v1.signal().objective_version(), 1);
	assert_eq!(stored_v1.signal().freshness(), AutonomySignalFreshness::Fresh);
	assert_eq!(stored_v1.signal().gaps(), ["No external dashboard readback included."]);
	assert_eq!(stored_v1.signal().privacy(), AutonomySignalPrivacy::LocalPrivate);

	tests::accept_objective(&store, 2);

	let mut input_v2 = tests::signal_input();

	input_v2.objective_version = 2;
	input_v2.source_refs = vec![String::from("status:XY-1085:runtime-health:v2")];

	let signal_v2 =
		AutonomySignal::runtime_health(input_v2).expect("runtime signal should validate");

	store.record_autonomy_signal("decodex", signal_v2).expect("v2 signal should store");

	let v1_signals = store
		.list_autonomy_signals_for_objective("decodex", "quality-autonomy", 1)
		.expect("v1 signals should list");
	let v2_signals = store
		.list_autonomy_signals_for_objective("decodex", "quality-autonomy", 2)
		.expect("v2 signals should list");

	assert_eq!(v1_signals.len(), 1);
	assert_eq!(v1_signals[0].signal().id(), signal_v1.id());
	assert_eq!(v2_signals.len(), 1);
	assert_ne!(v1_signals[0].signal().id(), v2_signals[0].signal().id());
}
