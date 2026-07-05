use std::collections::BTreeSet;

use crate::orchestrator::harness_improvement::{
	HarnessImprovementCandidateSummary, model::HarnessPhaseGoalOutcome,
};

#[derive(Default)]
pub(super) struct HarnessOutcomeSignals {
	pub(super) phase_goals: Vec<HarnessPhaseGoalOutcome>,
	pub(super) validation_failure_count: usize,
	pub(super) validation_failure_classes: BTreeSet<String>,
	pub(super) review_statuses: BTreeSet<String>,
	pub(super) accepted_finding_count: usize,
	pub(super) rejected_finding_count: usize,
	pub(super) nonclean_rounds: i64,
	pub(super) repair_phase_events: usize,
	pub(super) guardrail_reasons: BTreeSet<String>,
	pub(super) authority_boundary_dispositions: BTreeSet<String>,
	pub(super) authority_boundary_failed_check_count: usize,
	pub(super) authority_boundary_candidates: Vec<HarnessImprovementCandidateSummary>,
	pub(super) architecture_recovery_budget_exhausted_count: usize,
}
