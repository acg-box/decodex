#[derive(Debug)]
pub(in crate::recovery) struct RebindSuccessStateTransition {
	pub(in crate::recovery) state_name: String,
	pub(in crate::recovery) state_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::recovery) enum RebindMode {
	RestoreMissingHandoff,
	RestoreMissingHandoffAfterWritebackFailure,
	RefreshExistingHandoff,
	CompleteExistingHandoffState,
}
impl RebindMode {
	pub(in crate::recovery) fn as_str(self) -> &'static str {
		match self {
			Self::RestoreMissingHandoff => "restore_missing_handoff",
			Self::RestoreMissingHandoffAfterWritebackFailure => {
				"restore_missing_handoff_after_writeback_failure"
			},
			Self::RefreshExistingHandoff => "refresh_existing_handoff",
			Self::CompleteExistingHandoffState => "complete_existing_handoff_state",
		}
	}

	pub(in crate::recovery) fn allows_failure_state_drift_repair(self) -> bool {
		matches!(
			self,
			Self::RestoreMissingHandoffAfterWritebackFailure | Self::CompleteExistingHandoffState
		)
	}

	pub(in crate::recovery) fn allows_partial_handoff_state_completion(self) -> bool {
		matches!(
			self,
			Self::RestoreMissingHandoff
				| Self::RestoreMissingHandoffAfterWritebackFailure
				| Self::CompleteExistingHandoffState
		)
	}

	pub(in crate::recovery) fn evidence_value(self) -> &'static str {
		match self {
			Self::RestoreMissingHandoff => "absent",
			Self::RestoreMissingHandoffAfterWritebackFailure => "absent_after_writeback_failure",
			Self::RefreshExistingHandoff => "refreshed",
			Self::CompleteExistingHandoffState => "current_state_transition",
		}
	}

	pub(in crate::recovery) fn summary_action(self) -> &'static str {
		match self {
			Self::RestoreMissingHandoff | Self::RestoreMissingHandoffAfterWritebackFailure => {
				"restored retained review lifecycle record"
			},
			Self::RefreshExistingHandoff => "refreshed retained review lifecycle record",
			Self::CompleteExistingHandoffState => "completed retained review handoff state",
		}
	}
}
