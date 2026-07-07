use crate::orchestrator::kernel::{
	command::{CommandFact, CommandIntent, CommandIntentKind},
	facts::LaneObservation,
	reason::ReasonCode,
};

pub(super) fn intent(
	observation: &LaneObservation,
	kind: CommandIntentKind,
	reason: ReasonCode,
) -> CommandIntent {
	let run_part = observation.run_id.as_deref().unwrap_or("no-run");
	let idempotency_key =
		format!("{}:{run_part}:{}:{}", observation.issue_id, kind.as_str(), reason.as_str());

	CommandIntent::new(
		kind,
		idempotency_key,
		intent_preconditions(kind),
		intent_expected_postconditions(kind),
	)
}

fn intent_preconditions(kind: CommandIntentKind) -> Vec<CommandFact> {
	if kind == CommandIntentKind::RequestManualIntervention {
		return Vec::new();
	}

	let mut preconditions = vec![
		CommandFact::AuthorityComplete,
		CommandFact::IssueStillOwned,
		CommandFact::NoContradictoryAuthority,
	];

	match kind {
		CommandIntentKind::ContinueAttempt => {
			preconditions.push(CommandFact::ActiveOwnedWorkPresent);
		},
		CommandIntentKind::WaitExternal => {
			preconditions.push(CommandFact::ExternalSignalStillPending);
		},
		CommandIntentKind::ScheduleRetry => {
			preconditions.push(CommandFact::RetryBudgetAvailable);
			preconditions.push(CommandFact::NoHumanAttentionSignal);
		},
		CommandIntentKind::ResumeRetainedLane => {
			preconditions.push(CommandFact::RetainedLaneReusable);
		},
		CommandIntentKind::RequestManualIntervention => {},
		CommandIntentKind::LandReadyPullRequest => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
			preconditions.push(CommandFact::ReadyToLandPrerequisitesSatisfied);
		},
		CommandIntentKind::FinishTerminalCleanup => {
			preconditions.push(CommandFact::TerminalCleanupPending);
		},
		CommandIntentKind::RequestExternalReview => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
			preconditions.push(CommandFact::ReadyToLandPrerequisitesSatisfied);
		},
		CommandIntentKind::ProbeExternalReviewAcknowledgement => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
			preconditions.push(CommandFact::ExternalReviewRequestPresent);
		},
		CommandIntentKind::ResendExternalReviewRequest => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
			preconditions.push(CommandFact::ExternalReviewAcknowledgementPending);
			preconditions.push(CommandFact::ExternalReviewRequestRetryAvailable);
		},
		CommandIntentKind::StartReviewRepair => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
		},
		CommandIntentKind::StartRetainedLanding => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
			preconditions.push(CommandFact::ReadyToLandPrerequisitesSatisfied);
		},
		CommandIntentKind::StartRetainedCloseout => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
		},
		CommandIntentKind::FinishRetainedCleanup => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
			preconditions.push(CommandFact::TerminalCleanupPending);
		},
		CommandIntentKind::SyncReviewLifecycleAuthority => {
			preconditions.push(CommandFact::PostReviewLifecyclePresent);
		},
		CommandIntentKind::ObserveLoopGuardrailCheckpoint => {
			preconditions.push(CommandFact::OpenTrackerBlockersPresent);
		},
		CommandIntentKind::ClearLoopGuardrailCheckpoint => {
			preconditions.push(CommandFact::OpenTrackerBlockersResolved);
		},
	}

	preconditions
}

fn intent_expected_postconditions(kind: CommandIntentKind) -> Vec<CommandFact> {
	match kind {
		CommandIntentKind::ContinueAttempt => vec![CommandFact::ActiveOwnedWorkPresent],
		CommandIntentKind::WaitExternal => vec![CommandFact::ExternalSignalStillPending],
		CommandIntentKind::ScheduleRetry => vec![CommandFact::RetryScheduled],
		CommandIntentKind::ResumeRetainedLane => vec![CommandFact::RetainedLaneResumed],
		CommandIntentKind::RequestManualIntervention => {
			vec![CommandFact::HumanInterventionRecorded]
		},
		CommandIntentKind::LandReadyPullRequest => vec![CommandFact::LandingSequenceStarted],
		CommandIntentKind::FinishTerminalCleanup => vec![CommandFact::TerminalCleanupCompleted],
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
		CommandIntentKind::ObserveLoopGuardrailCheckpoint => {
			vec![CommandFact::LoopGuardrailCheckpointObserved]
		},
		CommandIntentKind::ClearLoopGuardrailCheckpoint => {
			vec![CommandFact::LoopGuardrailCheckpointCleared]
		},
	}
}
