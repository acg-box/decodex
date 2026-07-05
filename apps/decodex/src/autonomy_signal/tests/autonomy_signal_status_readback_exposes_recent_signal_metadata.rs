use crate::{
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalFreshness, AutonomySignalPrivacy,
		tests::{self},
	},
	state::StateStore,
};

#[test]
fn autonomy_signal_status_readback_exposes_recent_signal_metadata() {
	let store = StateStore::open_in_memory().expect("store should open");

	tests::accept_objective(&store, 1);

	store
		.record_autonomy_signal(
			"decodex",
			AutonomySignal::runtime_health(tests::signal_input())
				.expect("runtime signal should validate"),
		)
		.expect("signal should store");

	let snapshot =
		store.project_loop_evidence_snapshot("decodex").expect("loop evidence should load");
	let recent = snapshot.recent_autonomy_signals(1);
	let signal = recent[0].signal();

	assert_eq!(signal.objective_id(), "quality-autonomy");
	assert_eq!(signal.objective_version(), 1);
	assert_eq!(signal.freshness(), AutonomySignalFreshness::Fresh);
	assert_eq!(signal.confidence(), AutonomySignalConfidence::Medium);
	assert_eq!(signal.privacy(), AutonomySignalPrivacy::LocalPrivate);
	assert_eq!(signal.gaps(), ["No external dashboard readback included."]);
}
