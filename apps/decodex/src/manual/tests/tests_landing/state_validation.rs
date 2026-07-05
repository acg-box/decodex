use crate::manual::{self, LandExecutionMode, tests};

#[test]
fn landing_state_validation_blocks_base_drift_except_after_merge() {
	let error = manual::validate_landing_state(
		&tests::sample_landing_state(),
		"https://github.com/hack-ink/decodex/pull/64",
		"main",
		"XY-225",
		"deadbeef",
	)
	.expect_err("non-default-base PR should be rejected");

	assert!(error.to_string().contains("targets base branch `release/1.x`"));
	assert!(error.to_string().contains("only lands into `main`"));

	let mut landing_state = tests::sample_landing_state();

	landing_state.state = String::from("MERGED");

	let mode = manual::validate_landing_state(
		&landing_state,
		"https://github.com/hack-ink/decodex/pull/64",
		"release/1.x",
		"XY-225",
		"deadbeef",
	)
	.expect("merged PR should resume closeout");

	assert_eq!(mode, LandExecutionMode::CloseoutOnly);
}

#[test]
fn landing_state_validation_explains_unknown_mergeability_after_retry() {
	let mut landing_state = tests::sample_landing_state();

	landing_state.base_ref_name = String::from("main");
	landing_state.mergeable = String::from("UNKNOWN");

	let error = manual::validate_landing_state(
		&landing_state,
		"https://github.com/hack-ink/decodex/pull/64",
		"main",
		"XY-225",
		"deadbeef",
	)
	.expect_err("unknown mergeability should not land");

	assert!(error.to_string().contains("mergeability is still unknown after retry"));
	assert!(error.to_string().contains("retry `decodex land`"));
}

#[test]
fn landing_state_validation_treats_pending_checks_as_wait_even_when_merge_blocked() {
	let mut landing_state = tests::sample_landing_state();

	landing_state.base_ref_name = String::from("main");
	landing_state.merge_state_status = String::from("BLOCKED");
	landing_state.status_check_rollup_state = Some(String::from("PENDING"));

	let error = manual::validate_landing_state(
		&landing_state,
		"https://github.com/hack-ink/decodex/pull/64",
		"main",
		"XY-225",
		"deadbeef",
	)
	.expect_err("pending checks should wait rather than report a generic blocked merge state");

	assert!(error.to_string().contains("still waiting on checks"));
	assert!(error.to_string().contains("statusCheckRollup=`PENDING`"));
}

#[test]
fn landing_state_validation_rejects_blocked_merge_state_after_green_gates() {
	let mut landing_state = tests::sample_landing_state();

	landing_state.base_ref_name = String::from("main");
	landing_state.merge_state_status = String::from("BLOCKED");

	let error = manual::validate_landing_state(
		&landing_state,
		"https://github.com/hack-ink/decodex/pull/64",
		"main",
		"XY-225",
		"deadbeef",
	)
	.expect_err("blocked merge state should not land without a policy change");

	assert!(error.to_string().contains("not ready to land"));
	assert!(error.to_string().contains("mergeStateStatus=`BLOCKED`"));
}
