use serde::{Deserialize, Serialize};

use crate::prelude::Result;

pub(crate) trait PhaseGoalController {
	fn initial_phase_goal(&self) -> Result<Option<PhaseGoalSpec>>;
	fn phase_goal_completed(&self, phase: PhaseGoalKind) -> Result<PhaseGoalTransition>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PhaseGoalKind {
	ImplementToValidationReady,
	RepairValidationFailures,
	RepairAcceptedReviewFindings,
	ReviewRepairEvidence,
	HandoffEvidence,
}
impl PhaseGoalKind {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::ImplementToValidationReady => "implement_to_validation_ready",
			Self::RepairValidationFailures => "repair_validation_failures",
			Self::RepairAcceptedReviewFindings => "repair_accepted_review_findings",
			Self::ReviewRepairEvidence => "review_repair_evidence",
			Self::HandoffEvidence => "handoff_evidence",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PhaseGoalTransition {
	Continue(PhaseGoalSpec),
	#[cfg_attr(not(test), allow(dead_code))]
	ScheduleContinuation(PhaseGoalSpec),
	CompleteRun,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PhaseGoalSpec {
	pub(crate) phase: PhaseGoalKind,
	pub(crate) objective: String,
	pub(crate) token_budget: Option<i64>,
}
impl PhaseGoalSpec {
	pub(crate) fn new(
		phase: PhaseGoalKind,
		objective: impl Into<String>,
		token_budget: Option<i64>,
	) -> Self {
		Self { phase, objective: objective.into(), token_budget }
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct PhaseGoalRunStatus {
	pub(crate) phase: PhaseGoalKind,
	pub(crate) status: String,
}
