use crate::{
	recovery::{
		GHOST_LANE_BLOCKED_CLASSIFICATION, GHOST_LANE_CLASSIFICATION,
		MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION,
	},
	state::ProjectRunStatus,
};

pub(super) fn ghost_lane_diagnostic_outcome(
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
	mcp_test_fixture: bool,
	blockers: &[String],
) -> (String, String, String) {
	if blockers.is_empty() {
		let (classification, reason) = if mcp_test_fixture {
			(
				String::from(MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION),
				String::from("tracker_issue_missing_and_only_mcp_test_control_fixture_evidence"),
			)
		} else {
			(
				String::from(GHOST_LANE_CLASSIFICATION),
				String::from("tracker_issue_missing_and_no_live_or_retained_lane_evidence"),
			)
		};

		return (
			classification,
			reason,
			format!(
				"Run `decodex recover ghost-lane cleanup {} --dry-run`, then rerun without `--dry-run` if the report stays safe.",
				issue_identifier.unwrap_or(run.issue_id())
			),
		);
	}

	(
		String::from(GHOST_LANE_BLOCKED_CLASSIFICATION),
		String::from("safety_check_blocked"),
		String::from(
			"Preserve attention and inspect the listed blockers before using a recovery command.",
		),
	)
}
