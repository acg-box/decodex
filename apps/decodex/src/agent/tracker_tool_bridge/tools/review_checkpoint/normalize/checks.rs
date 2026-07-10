use crate::agent::tracker_tool_bridge::{
	ReviewCheckpointChecksArgs, tools::review_checkpoint::normalize::shared,
};

pub(super) fn normalize_review_checkpoint_checks(
	checks: ReviewCheckpointChecksArgs,
) -> Result<ReviewCheckpointChecksArgs, String> {
	Ok(ReviewCheckpointChecksArgs {
		intended_behavior: shared::normalize_required_review_text(
			checks.intended_behavior,
			"checks.intended_behavior",
		)?,
		regression_risk: shared::normalize_required_review_text(
			checks.regression_risk,
			"checks.regression_risk",
		)?,
		missing_tests: shared::normalize_required_review_text(
			checks.missing_tests,
			"checks.missing_tests",
		)?,
		migration_fallout: shared::normalize_required_review_text(
			checks.migration_fallout,
			"checks.migration_fallout",
		)?,
		operator_facing_fallout: shared::normalize_required_review_text(
			checks.operator_facing_fallout,
			"checks.operator_facing_fallout",
		)?,
		loop_decision_contract: shared::normalize_required_review_text(
			checks.loop_decision_contract,
			"checks.loop_decision_contract",
		)?,
	})
}
