use crate::orchestrator::{
	execution_failure::{Result, eyre},
	kernel::{
		command::{CommandFact, CommandIntent, CommandIntentKind},
		post_review,
	},
};

pub(super) fn review_handoff_drift_marker_rebind_command_intent(
	issue_id: &str,
	run_id: &str,
) -> CommandIntent {
	post_review::build_post_review_command_intent(
		issue_id,
		Some(run_id),
		"review_handoff_state_drift_orchestration_rebound",
		CommandIntentKind::SyncReviewOrchestrationMarker,
	)
}

pub(super) fn review_handoff_drift_command_adapter(
	command_intent: CommandIntent,
	expected_kind: CommandIntentKind,
) -> Result<CommandIntent> {
	if command_intent.kind != expected_kind {
		eyre::bail!(
			"Review handoff drift command adapter expected `{}` intent, got `{}`.",
			expected_kind.as_str(),
			command_intent.kind.as_str(),
		);
	}
	if command_intent.idempotency_key.trim().is_empty() {
		eyre::bail!(
			"Review handoff drift command adapter requires a non-empty `{}` idempotency key.",
			expected_kind.as_str(),
		);
	}
	if command_intent.expected_postconditions.is_empty() {
		eyre::bail!(
			"Review handoff drift command adapter requires `{}` expected postconditions.",
			expected_kind.as_str(),
		);
	}

	for required in [
		CommandFact::AuthorityComplete,
		CommandFact::IssueStillOwned,
		CommandFact::NoContradictoryAuthority,
		CommandFact::PostReviewLifecyclePresent,
	] {
		if !command_intent.preconditions.contains(&required) {
			eyre::bail!(
				"Review handoff drift command adapter rejected `{}` without `{}` precondition.",
				expected_kind.as_str(),
				required.as_str(),
			);
		}
	}

	Ok(command_intent)
}
