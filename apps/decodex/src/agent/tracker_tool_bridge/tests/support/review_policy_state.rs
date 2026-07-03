use crate::{
	agent::tracker_tool_bridge::{
		ReviewExecutionMode, ReviewHandoffContext, TrackerToolBridge, tests::support::fixtures,
	},
	state::{
		ReviewHandoffMarker, ReviewOrchestrationMarker, ReviewPolicyCheckpoint,
		ReviewPolicyCheckpointInput, StateStore,
	},
	tracker::TrackerIssue,
};

pub(crate) fn seed_docs_impact_checkpoint(
	state_store: &StateStore,
	review_context: &ReviewHandoffContext,
	issue_id: &str,
	phase: &str,
	head_sha: &str,
) {
	state_store
		.append_private_execution_event(
			&review_context.service_id,
			issue_id,
			&review_context.run_id,
			review_context.attempt_number,
			"progress_checkpoint",
			serde_json::json!({
				"phase": phase,
				"docs_impact": "none",
				"head_sha": head_sha
			}),
		)
		.expect("docs impact checkpoint should seed");
}

pub(crate) fn write_review_policy_checkpoint(
	bridge: &TrackerToolBridge<'_>,
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
	phase: &str,
	status: &str,
	head_sha: &str,
	nonclean_rounds: i64,
) {
	bridge_state_store(bridge)
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: &review_context.service_id,
			issue_id: &issue.id,
			run_id: &review_context.run_id,
			attempt_number: review_context.attempt_number,
			phase,
			review_level: review_context.review_level.as_str(),
			status,
			head_sha,
			nonclean_rounds,
			details_json: "{}",
		})
		.expect("review policy state should write");
}

pub(crate) fn write_clean_review_checkpoint(
	bridge: &TrackerToolBridge<'_>,
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
) {
	let phase = match review_context.mode {
		ReviewExecutionMode::Handoff => "handoff",
		ReviewExecutionMode::Repair => "repair",
		ReviewExecutionMode::Closeout => {
			panic!("closeout does not support review checkpoints")
		},
	};

	write_review_policy_checkpoint(
		bridge,
		issue,
		review_context,
		phase,
		"clean",
		&fixtures::sample_local_repo().head_oid,
		0,
	);
}

pub(crate) fn bridge_state_store<'a>(bridge: &TrackerToolBridge<'a>) -> &'a StateStore {
	bridge.state_store.expect("test bridge should have a runtime state store")
}

pub(crate) fn persisted_review_policy_checkpoint(
	bridge: &TrackerToolBridge<'_>,
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
) -> ReviewPolicyCheckpoint {
	let phase = match review_context.mode {
		ReviewExecutionMode::Handoff => "handoff",
		ReviewExecutionMode::Repair => "repair",
		ReviewExecutionMode::Closeout => {
			panic!("closeout does not support review checkpoints")
		},
	};

	bridge_state_store(bridge)
		.review_policy_checkpoint(
			&review_context.service_id,
			&issue.id,
			&review_context.run_id,
			review_context.attempt_number,
			phase,
		)
		.expect("review policy checkpoint should read")
		.expect("review policy checkpoint should exist")
}

pub(crate) fn assert_review_policy_checkpoint_cleared(
	bridge: &TrackerToolBridge<'_>,
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
) {
	let phase = match review_context.mode {
		ReviewExecutionMode::Handoff => "handoff",
		ReviewExecutionMode::Repair => "repair",
		ReviewExecutionMode::Closeout => {
			panic!("closeout does not support review checkpoints")
		},
	};

	assert!(
		bridge_state_store(bridge)
			.review_policy_checkpoint(
				&review_context.service_id,
				&issue.id,
				&review_context.run_id,
				review_context.attempt_number,
				phase,
			)
			.expect("review policy checkpoint should read")
			.is_none(),
		"review policy checkpoint should be cleared after completion"
	);
}

pub(crate) fn persisted_review_handoff_marker(
	bridge: &TrackerToolBridge<'_>,
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
) -> ReviewHandoffMarker {
	bridge_state_store(bridge)
		.review_handoff_marker(&review_context.service_id, &issue.id, &review_context.branch_name)
		.expect("review handoff marker should read")
		.expect("review handoff marker should exist")
}

pub(crate) fn persisted_review_orchestration_marker(
	bridge: &TrackerToolBridge<'_>,
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
	review_handoff: &ReviewHandoffMarker,
) -> ReviewOrchestrationMarker {
	bridge_state_store(bridge)
		.review_orchestration_marker(&review_context.service_id, &issue.id, review_handoff)
		.expect("review orchestration marker should read")
		.expect("review orchestration marker should exist")
}
