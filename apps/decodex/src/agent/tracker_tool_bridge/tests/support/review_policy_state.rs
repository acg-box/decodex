use crate::{
	agent::tracker_tool_bridge::{
		ReviewExecutionMode, ReviewHandoffContext, TrackerToolBridge, tests::support::fixtures,
	},
	state::{
		ReviewLifecycleHandoffFixture, ReviewLifecycleTransitionFixture, ReviewPolicyCheckpoint,
		ReviewPolicyCheckpointInput, StateStore,
	},
	tracker::TrackerIssue,
};

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

pub(crate) fn persisted_review_lifecycle_handoff_fixture(
	bridge: &TrackerToolBridge<'_>,
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
) -> ReviewLifecycleHandoffFixture {
	bridge_state_store(bridge)
		.review_lifecycle_handoff_fixture(
			&review_context.service_id,
			&issue.id,
			&review_context.branch_name,
		)
		.expect("review lifecycle handoff fixture should read")
		.expect("review lifecycle handoff fixture should exist")
}

pub(crate) fn persisted_review_lifecycle_transition_fixture(
	bridge: &TrackerToolBridge<'_>,
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
	review_handoff: &ReviewLifecycleHandoffFixture,
) -> ReviewLifecycleTransitionFixture {
	bridge_state_store(bridge)
		.review_lifecycle_transition_fixture(&review_context.service_id, &issue.id, review_handoff)
		.expect("review lifecycle transition fixture should read")
		.expect("review lifecycle transition fixture should exist")
}
