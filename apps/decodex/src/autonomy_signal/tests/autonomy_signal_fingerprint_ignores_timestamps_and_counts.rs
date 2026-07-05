use crate::autonomy_signal::{AutonomySignal, tests};

#[test]
fn autonomy_signal_fingerprint_ignores_timestamps_and_counts() {
	let signal = AutonomySignal::runtime_health(tests::signal_input())
		.expect("runtime signal should validate");
	let mut input = tests::signal_input();

	input.captured_at = String::from("2026-06-22T00:05:00Z");
	input.created_at = String::from("2026-06-22T00:05:05Z");

	input.observed_counts.insert(String::from("validation_retry_count"), 7);

	let changed = AutonomySignal::runtime_health(input)
		.expect("runtime signal with volatile fields should validate");

	assert_eq!(signal.fingerprint(), changed.fingerprint());
	assert_eq!(signal.id(), changed.id());
}
