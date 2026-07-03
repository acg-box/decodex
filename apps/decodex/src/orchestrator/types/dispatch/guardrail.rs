#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LoopGuardrailReason {
	ValidationRepeat,
	NoEffectiveDiff,
	RemainingDeltaUnchanged,
	ReviewChurn,
	ReviewHandoffStateDrift,
	DependencyProgramStale,
	UncoveredDirection,
	AmbiguousRetainedProgress,
}
impl LoopGuardrailReason {
	pub(crate) fn error_class(self) -> &'static str {
		match self {
			Self::ValidationRepeat => "validation_repeat",
			Self::NoEffectiveDiff => "no_effective_diff",
			Self::RemainingDeltaUnchanged => "remaining_delta_unchanged",
			Self::ReviewChurn => "review_churn",
			Self::ReviewHandoffStateDrift => "review_handoff_state_drift",
			Self::DependencyProgramStale => "dependency_program_stale",
			Self::UncoveredDirection => "uncovered_direction",
			Self::AmbiguousRetainedProgress => "ambiguous_retained_progress",
		}
	}

	pub(crate) fn from_error_class(error_class: &str) -> Option<Self> {
		match error_class {
			"validation_repeat" | "validation_failure_repeated" => Some(Self::ValidationRepeat),
			"no_effective_diff" => Some(Self::NoEffectiveDiff),
			"remaining_delta_unchanged" => Some(Self::RemainingDeltaUnchanged),
			"review_churn" | "review_policy_exhausted" => Some(Self::ReviewChurn),
			"review_handoff_state_drift" | "review_handoff_rebind_required" =>
				Some(Self::ReviewHandoffStateDrift),
			"dependency_program_stale" | "dependency_blocked" => Some(Self::DependencyProgramStale),
			"uncovered_direction" | "research_contract_required" => Some(Self::UncoveredDirection),
			"ambiguous_retained_progress" | "ownership_ambiguous" =>
				Some(Self::AmbiguousRetainedProgress),
			_ => None,
		}
	}

	pub(crate) fn terminal_next_action(self, recovery_gate: &str) -> String {
		match self {
			Self::ValidationRepeat => format!(
				"inspect the repeated validation failure, preserved worktree, and prior repair attempts; change repair strategy or route the issue to architecture/research review manually, {recovery_gate}"
			),
			Self::NoEffectiveDiff => format!(
				"inspect the retained worktree and retry evidence; do not continue automatic repair until a human identifies a concrete next diff or resets the lane, {recovery_gate}"
			),
			Self::RemainingDeltaUnchanged => format!(
				"inspect the unchanged remaining delta and validation evidence; decide the next bounded repair manually before requeueing, {recovery_gate}"
			),
			Self::ReviewChurn => format!(
				"inspect the repeated review findings and current head; decide the next repair or architecture review manually before requeueing, {recovery_gate}"
			),
			Self::ReviewHandoffStateDrift => format!(
				"inspect the retained review handoff marker, clean review checkpoint, PR head, and issue state; restore or rebind the post-review lifecycle before clearing attention, {recovery_gate}"
			),
			Self::DependencyProgramStale => format!(
				"inspect the dependency blocker and Execution Program readiness evidence; refresh dependencies or split/research the program before requeueing, {recovery_gate}"
			),
			Self::UncoveredDirection => format!(
				"capture the missing direction in a research or decision contract before continuing execution, {recovery_gate}"
			),
			Self::AmbiguousRetainedProgress => format!(
				"inspect retained partial progress and ownership evidence; choose resume, reset, or manual repair explicitly before clearing the guard, {recovery_gate}"
			),
		}
	}
}
