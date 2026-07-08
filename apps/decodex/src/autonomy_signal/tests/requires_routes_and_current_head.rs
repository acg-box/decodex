use crate::autonomy_signal::{
	AutonomySignal, AutonomySignalSourceType,
	tests::{self},
};

#[test]
fn requires_routes_and_current_head() {
	let mut input = tests::signal_input();

	input.source_type = AutonomySignalSourceType::Review;

	assert!(AutonomySignal::review_feedback_cluster(input.clone()).is_err());

	input.review_evidence = Some(tests::review_evidence());

	let signal = AutonomySignal::review_feedback_cluster(input)
		.expect("review signal should require normalized route evidence");

	assert_eq!(signal.review_evidence().expect("review evidence").finding_routes.len(), 1);
	assert_eq!(signal.head_sha(), Some("3273e45234aa3346e194a7a9e48cd1c58c3e408c"));
}
