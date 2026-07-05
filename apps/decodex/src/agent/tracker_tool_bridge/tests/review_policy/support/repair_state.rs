use crate::{
	agent::tracker_tool_bridge::ReviewHandoffContext,
	state::{
		ReviewHandoffMarker, ReviewOrchestrationMarker, ReviewPolicyCheckpointInput, StateStore,
	},
};

pub(in crate::agent::tracker_tool_bridge::tests) fn seed_review_repair_apply_state(
	state_store: &StateStore,
	review_context: &ReviewHandoffContext,
	issue_id: &str,
	pr_url: &str,
	external_round_count: i64,
) {
	let review_handoff = ReviewHandoffMarker::new(
		String::from("pub-618-attempt-2-100"),
		2,
		review_context.branch_name.clone(),
		String::from(pr_url),
		String::from("main"),
		review_context.branch_name.clone(),
		String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
	);

	state_store
		.upsert_review_handoff_marker(&review_context.service_id, issue_id, &review_handoff)
		.expect("original review handoff marker should persist");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: &review_context.service_id,
			issue_id,
			run_id: &review_context.run_id,
			attempt_number: review_context.attempt_number,
			phase: "repair",
			review_level: review_context.review_level.as_str(),
			status: "clean",
			head_sha: "18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("repair review checkpoint should persist");
	state_store
		.upsert_review_orchestration_marker(
			&review_context.service_id,
			issue_id,
			&ReviewOrchestrationMarker::new(
				review_handoff.run_id().to_owned(),
				review_handoff.attempt_number(),
				review_handoff.branch_name().to_owned(),
				pr_url.to_owned(),
				String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
				"repair_required",
				Some(91),
				Some(1_763_600_000),
				Some(0),
				0,
				external_round_count,
				None,
			),
		)
		.expect("review orchestration marker should persist");
}
