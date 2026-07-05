use crate::orchestrator::{
	RetryKind,
	kernel::{action::OwnedLaneAction, command::CommandIntentKind},
	lane_decision::model::LaneDecision,
};

impl LaneDecision {
	pub(in crate::orchestrator) fn permits_child_exit_retry_kind(&self, kind: RetryKind) -> bool {
		let required_intent = match kind {
			RetryKind::Continuation => CommandIntentKind::ResumeRetainedLane,
			RetryKind::Failure => CommandIntentKind::ScheduleRetry,
		};

		self.has_command_intent(required_intent)
	}

	pub(in crate::orchestrator) fn permits_phase_repair_retry(&self) -> bool {
		self.has_command_intent(CommandIntentKind::ScheduleRetry)
	}

	pub(in crate::orchestrator) fn blocks_automatic_execution(&self) -> bool {
		self.kernel_decision.decision_class == OwnedLaneAction::ManualInterventionRequired
	}

	fn has_command_intent(&self, kind: CommandIntentKind) -> bool {
		self.kernel_decision.command_intents.iter().any(|intent| intent.kind == kind)
	}
}
