use crate::{
	orchestrator::{
		kernel::{
			command::{CommandFact, CommandIntent, CommandIntentKind},
			post_review,
		},
		retained_review_orchestration::RetainedReviewLane,
	},
	prelude::{Result, eyre},
};

pub(super) fn retained_review_command_intent(
	lane: &RetainedReviewLane,
	kind: CommandIntentKind,
	reason: &str,
) -> CommandIntent {
	post_review::build_post_review_command_intent(
		&lane.snapshot.issue.id,
		Some(lane.orchestration_marker.run_id()),
		reason,
		kind,
	)
}

pub(super) fn retained_review_command_intent_for_issue(
	issue_id: &str,
	run_id: Option<&str>,
	kind: CommandIntentKind,
	reason: &str,
) -> CommandIntent {
	post_review::build_post_review_command_intent(issue_id, run_id, reason, kind)
}

pub(super) fn retained_review_command_adapter(
	command_intent: CommandIntent,
	expected_kind: CommandIntentKind,
) -> Result<CommandIntent> {
	if command_intent.kind != expected_kind {
		eyre::bail!(
			"Retained review command adapter expected `{}` intent, got `{}`.",
			expected_kind.as_str(),
			command_intent.kind.as_str(),
		);
	}
	if command_intent.idempotency_key.trim().is_empty() {
		eyre::bail!(
			"Retained review command adapter requires a non-empty `{}` idempotency key.",
			expected_kind.as_str(),
		);
	}
	if command_intent.expected_postconditions.is_empty() {
		eyre::bail!(
			"Retained review command adapter requires `{}` expected postconditions.",
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
				"Retained review command adapter rejected `{}` without `{}` precondition.",
				expected_kind.as_str(),
				required.as_str(),
			);
		}
	}

	Ok(command_intent)
}

#[cfg(test)]
mod tests {
	use crate::orchestrator::{
		kernel::command::{CommandFact, CommandIntent, CommandIntentKind},
		retained_review_orchestration::command,
	};

	#[test]
	fn retained_review_command_adapter_accepts_kernel_built_marker_sync_intent() {
		let intent = command::retained_review_command_intent_for_issue(
			"PUB-101",
			Some("run-1"),
			CommandIntentKind::SyncReviewOrchestrationMarker,
			"review_orchestration_marker_created",
		);
		let accepted = command::retained_review_command_adapter(
			intent,
			CommandIntentKind::SyncReviewOrchestrationMarker,
		)
		.expect("kernel-built marker sync intent should pass retained adapter");

		assert_eq!(accepted.kind, CommandIntentKind::SyncReviewOrchestrationMarker);
		assert!(
			accepted
				.expected_postconditions
				.contains(&CommandFact::ReviewOrchestrationMarkerCurrent)
		);
	}

	#[test]
	fn retained_review_command_adapter_rejects_intent_without_kernel_contract() {
		let intent = CommandIntent::new(
			CommandIntentKind::StartRetainedLanding,
			"PUB-101:run-1",
			vec![],
			vec![],
		);
		let error = command::retained_review_command_adapter(
			intent,
			CommandIntentKind::StartRetainedLanding,
		)
		.expect_err("adapter should reject an intent without kernel contract facts");

		assert!(error.to_string().contains("expected postconditions"));
	}
}
