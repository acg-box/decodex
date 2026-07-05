mod child_agent_activity;
mod clearable_auxiliary;
mod macos_boot;
mod thread_protocol_summary;

use crate::state::tests::run_activity::markers::account_summary;

#[test]
fn run_activity_marker_round_trips_marker_surfaces() {
	clearable_auxiliary::assert_run_activity_marker_round_trips_clearable_auxiliary_fields();
	thread_protocol_summary::assert_run_activity_marker_round_trips_thread_and_protocol_summary_fields();
	child_agent_activity::assert_run_activity_marker_round_trips_child_agent_activity_summary();
	account_summary::assert_run_activity_marker_round_trips_account_summary();
	account_summary::assert_run_activity_marker_preserves_account_summary_after_activity_refresh();
	account_summary::assert_run_activity_marker_preserves_account_summary_after_stale_rewrite();
}
