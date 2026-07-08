use crate::orchestrator::{
	PostReviewLaneDecision,
	kernel::{
		action::OwnedLaneAction,
		command::{CommandFact, CommandIntent, CommandIntentKind},
		decision::OwnedLaneDecision,
		post_review::model::PostReviewLaneKernelInput,
	},
};

pub(crate) fn build_post_review_command_intent(
	issue_id: &str,
	run_id: Option<&str>,
	reason: &str,
	kind: CommandIntentKind,
) -> CommandIntent {
	let input = PostReviewLaneKernelInput {
		issue_id,
		run_id,
		lifecycle_present: true,
		proposed_decision: post_review_decision_for_command_kind(kind),
		reason,
		retry_budget_exhausted: false,
	};

	post_review_command_intent(&input, kind)
}

pub(super) fn post_review_command_intents(
	input: &PostReviewLaneKernelInput<'_>,
	decision: &OwnedLaneDecision,
) -> Vec<CommandIntent> {
	if decision.decision_class == OwnedLaneAction::ManualInterventionRequired {
		return decision.command_intents.clone();
	}

	let Some(kind) = post_review_command_kind(input) else {
		return decision.command_intents.clone();
	};

	vec![post_review_command_intent(input, kind)]
}

pub(super) fn post_review_reason_is_cleanup(reason: &str) -> bool {
	reason.contains("cleanup")
}

fn post_review_decision_for_command_kind(kind: CommandIntentKind) -> PostReviewLaneDecision {
	match kind {
		CommandIntentKind::RequestExternalReview
		| CommandIntentKind::ProbeExternalReviewAcknowledgement
		| CommandIntentKind::ResendExternalReviewRequest
		| CommandIntentKind::SyncReviewLifecycleAuthority
		| CommandIntentKind::WaitExternal => PostReviewLaneDecision::WaitForReview,
		CommandIntentKind::StartReviewRepair => PostReviewLaneDecision::NeedsReviewRepair,
		CommandIntentKind::StartRetainedLanding | CommandIntentKind::LandReadyPullRequest =>
			PostReviewLaneDecision::ReadyToLand,
		CommandIntentKind::StartRetainedCloseout | CommandIntentKind::FinishRetainedCleanup =>
			PostReviewLaneDecision::Continue,
		_ => PostReviewLaneDecision::Block,
	}
}

fn post_review_command_kind(input: &PostReviewLaneKernelInput<'_>) -> Option<CommandIntentKind> {
	match input.proposed_decision {
		PostReviewLaneDecision::ReadyToLand => Some(CommandIntentKind::StartRetainedLanding),
		PostReviewLaneDecision::NeedsReviewRepair => Some(CommandIntentKind::StartReviewRepair),
		PostReviewLaneDecision::WaitForReview => match input.reason {
			"external_review_request_pending" => Some(CommandIntentKind::RequestExternalReview),
			"external_review_ack_pending" =>
				Some(CommandIntentKind::ProbeExternalReviewAcknowledgement),
			_ => Some(CommandIntentKind::WaitExternal),
		},
		PostReviewLaneDecision::Continue =>
			if post_review_reason_is_cleanup(input.reason) {
				Some(CommandIntentKind::FinishRetainedCleanup)
			} else {
				Some(CommandIntentKind::StartRetainedCloseout)
			},
		PostReviewLaneDecision::CloseoutBlocked
		| PostReviewLaneDecision::CleanupBlocked
		| PostReviewLaneDecision::Block => None,
	}
}

fn post_review_command_intent(
	input: &PostReviewLaneKernelInput<'_>,
	kind: CommandIntentKind,
) -> CommandIntent {
	CommandIntent::new(
		kind,
		post_review_idempotency_key(input, kind),
		post_review_command_preconditions(kind),
		post_review_command_postconditions(kind),
	)
}

fn post_review_idempotency_key(
	input: &PostReviewLaneKernelInput<'_>,
	kind: CommandIntentKind,
) -> String {
	let run_id = input.run_id.unwrap_or("no-run");

	format!("{}:{run_id}:{}:{}", input.issue_id, kind.as_str(), input.reason)
}

fn post_review_command_preconditions(kind: CommandIntentKind) -> Vec<CommandFact> {
	let mut preconditions = vec![
		CommandFact::AuthorityComplete,
		CommandFact::IssueStillOwned,
		CommandFact::NoContradictoryAuthority,
		CommandFact::PostReviewLifecyclePresent,
	];

	match kind {
		CommandIntentKind::RequestExternalReview => {
			preconditions.push(CommandFact::ReadyToLandPrerequisitesSatisfied);
		},
		CommandIntentKind::ProbeExternalReviewAcknowledgement => {
			preconditions.push(CommandFact::ExternalReviewRequestPresent);
		},
		CommandIntentKind::ResendExternalReviewRequest => {
			preconditions.push(CommandFact::ExternalReviewAcknowledgementPending);
			preconditions.push(CommandFact::ExternalReviewRequestRetryAvailable);
		},
		CommandIntentKind::StartRetainedLanding => {
			preconditions.push(CommandFact::ReadyToLandPrerequisitesSatisfied);
		},
		CommandIntentKind::FinishRetainedCleanup => {
			preconditions.push(CommandFact::TerminalCleanupPending);
		},
		CommandIntentKind::WaitExternal
		| CommandIntentKind::StartReviewRepair
		| CommandIntentKind::StartRetainedCloseout
		| CommandIntentKind::SyncReviewLifecycleAuthority => {},
		_ => {},
	}

	preconditions
}

fn post_review_command_postconditions(kind: CommandIntentKind) -> Vec<CommandFact> {
	match kind {
		CommandIntentKind::RequestExternalReview => vec![CommandFact::ExternalReviewRequested],
		CommandIntentKind::ProbeExternalReviewAcknowledgement => {
			vec![CommandFact::ExternalReviewAcknowledgementObserved]
		},
		CommandIntentKind::ResendExternalReviewRequest => {
			vec![CommandFact::ExternalReviewRequested]
		},
		CommandIntentKind::StartReviewRepair => vec![CommandFact::ReviewRepairStarted],
		CommandIntentKind::StartRetainedLanding => vec![CommandFact::RetainedLandingStarted],
		CommandIntentKind::StartRetainedCloseout => vec![CommandFact::RetainedCloseoutStarted],
		CommandIntentKind::FinishRetainedCleanup => vec![CommandFact::RetainedCleanupCompleted],
		CommandIntentKind::SyncReviewLifecycleAuthority => {
			vec![CommandFact::ReviewLifecycleAuthorityCurrent]
		},
		CommandIntentKind::WaitExternal => vec![CommandFact::ExternalSignalStillPending],
		_ => Vec::new(),
	}
}
