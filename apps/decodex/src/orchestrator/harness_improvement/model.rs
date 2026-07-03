use std::collections::BTreeSet;

use crate::orchestrator::harness_improvement::Serialize;

pub(super) const HARNESS_OUTCOME_SCHEMA: &str = "decodex.harness_outcome/1";
pub(super) const HARNESS_OUTCOME_EVENT_TYPE: &str = "harness_outcome";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HarnessOutcomeKind {
	ReviewHandoff,
	ReviewRepair,
	Closeout,
	RetryableFailure,
	TerminalFailure,
	ManualAttention,
}
impl HarnessOutcomeKind {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::ReviewHandoff => "review_handoff",
			Self::ReviewRepair => "review_repair",
			Self::Closeout => "closeout",
			Self::RetryableFailure => "retryable_failure",
			Self::TerminalFailure => "terminal_failure",
			Self::ManualAttention => "manual_attention",
		}
	}

	pub(super) fn validation_result(
		self,
		explicit: Option<&str>,
		signals: &HarnessOutcomeSignals,
	) -> String {
		if let Some(result) = explicit {
			return result.to_owned();
		}

		if signals.validation_failure_count > 0 {
			return String::from("failed");
		}
		if matches!(self, Self::ReviewHandoff | Self::ReviewRepair | Self::Closeout) {
			return String::from("passed");
		}

		String::from("not_recorded")
	}
}

pub(crate) struct HarnessOutcomeRecordInput<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) issue_identifier: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) outcome: HarnessOutcomeKind,
	pub(crate) error_class: Option<&'a str>,
	pub(crate) validation_result: Option<&'a str>,
	pub(crate) pr_url: Option<&'a str>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct HarnessImprovementCandidateSummary {
	pub(crate) kind: String,
	pub(crate) reason_code: String,
	pub(crate) target: String,
	pub(crate) source_event_count: usize,
	pub(crate) recommendation: String,
}

#[derive(Serialize)]
pub(super) struct HarnessOutcomePayload {
	pub(super) schema: &'static str,
	pub(super) record_version: u16,
	pub(super) source: HarnessOutcomeSource,
	pub(super) contracts: Vec<HarnessOutcomeContract>,
	pub(super) execution_programs: Vec<HarnessOutcomeProgram>,
	pub(super) phase_goal_outcomes: Vec<HarnessPhaseGoalOutcome>,
	pub(super) validation: HarnessValidationOutcome,
	pub(super) repair: HarnessRepairOutcome,
	pub(super) review: HarnessReviewOutcome,
	pub(super) authority_boundary: HarnessAuthorityBoundaryOutcome,
	pub(super) manual_attention: Option<HarnessManualAttentionOutcome>,
	pub(super) pr_lifecycle: HarnessPrLifecycleOutcome,
	pub(super) linear_projection: HarnessLinearProjectionSummary,
	pub(super) improvement_candidates: Vec<HarnessImprovementCandidateSummary>,
}

#[derive(Serialize)]
pub(super) struct HarnessOutcomeSource {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) issue_identifier: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) outcome: String,
	pub(super) source_intents: Vec<HarnessSourceIntent>,
}

#[derive(Serialize)]
pub(super) struct HarnessSourceIntent {
	pub(super) contract_id: String,
	pub(super) status: String,
	pub(super) summary: String,
	pub(super) source_issue_identifier: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct HarnessOutcomeContract {
	pub(super) contract_id: String,
	pub(super) status: String,
	pub(super) source_issue_id: Option<String>,
	pub(super) ready_for_issue_shaping: bool,
	pub(super) missing_decision_count: usize,
	pub(super) generated_issue_ids: Vec<String>,
	pub(super) generated_issue_identifiers: Vec<String>,
	pub(super) execution_program_node_ids: Vec<String>,
	pub(super) conflict_domains: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct HarnessOutcomeProgram {
	pub(super) program_id: String,
	pub(super) source_contract_id: Option<String>,
	pub(super) node_count: usize,
	pub(super) nodes: Vec<HarnessOutcomeProgramNode>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct HarnessOutcomeProgramNode {
	pub(super) node_id: String,
	pub(super) program_stage: String,
	pub(super) queue_intent: String,
	pub(super) linear_issue_id: Option<String>,
	pub(super) linear_issue_identifier: Option<String>,
	pub(super) conflict_domains: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct HarnessPhaseGoalOutcome {
	pub(super) event_type: String,
	pub(super) phase: Option<String>,
	pub(super) signal: Option<String>,
	pub(super) status: Option<String>,
}

#[derive(Serialize)]
pub(super) struct HarnessValidationOutcome {
	pub(super) result: String,
	pub(super) failure_count: usize,
	pub(super) failure_classes: Vec<String>,
}

#[derive(Serialize)]
pub(super) struct HarnessRepairOutcome {
	pub(super) attempt_number: i64,
	pub(super) repair_attempt_observed: bool,
	pub(super) repair_phase_events: usize,
}

#[derive(Serialize)]
pub(super) struct HarnessReviewOutcome {
	pub(super) statuses: Vec<String>,
	pub(super) accepted_finding_count: usize,
	pub(super) rejected_finding_count: usize,
	pub(super) nonclean_rounds: i64,
}

#[derive(Serialize)]
pub(super) struct HarnessAuthorityBoundaryOutcome {
	pub(super) dispositions: Vec<String>,
	pub(super) failed_check_count: usize,
	pub(super) improvement_signal_count: usize,
}

#[derive(Serialize)]
pub(super) struct HarnessManualAttentionOutcome {
	pub(super) reason_code: String,
}

#[derive(Serialize)]
pub(super) struct HarnessPrLifecycleOutcome {
	pub(super) outcome: String,
	pub(super) pr_urls: Vec<String>,
}

#[derive(Serialize)]
pub(super) struct HarnessLinearProjectionSummary {
	pub(super) event_types: Vec<String>,
	pub(super) final_event_type: Option<String>,
	pub(super) final_error_class: Option<String>,
	pub(super) final_terminal_path: Option<String>,
}

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
