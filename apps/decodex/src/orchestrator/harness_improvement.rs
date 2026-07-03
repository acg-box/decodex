mod candidates;
mod model;
mod payload;
mod record;

pub(crate) use model::{
	HarnessImprovementCandidateSummary, HarnessOutcomeKind, HarnessOutcomeRecordInput,
};
#[cfg(test)] pub(crate) use record::record_harness_outcome_for_issue_run;
pub(crate) use record::{
	harness_improvement_candidates_from_private_events, record_harness_outcome_best_effort,
};

use serde::Serialize;

use crate::{
	execution_program::ExecutionConflictDomain,
	orchestrator::{IssueRunPlan, Result, StateStore, Value},
	state::{DecisionContractRecord, ExecutionProgramRecord, PrivateExecutionEvent},
	tracker::records::LinearExecutionEventRecord,
};
use candidates::harness_improvement_candidates;
use model::{
	HARNESS_OUTCOME_EVENT_TYPE, HARNESS_OUTCOME_SCHEMA, HarnessAuthorityBoundaryOutcome,
	HarnessLinearProjectionSummary, HarnessManualAttentionOutcome, HarnessOutcomeContract,
	HarnessOutcomePayload, HarnessOutcomeProgram, HarnessOutcomeProgramNode, HarnessOutcomeSignals,
	HarnessOutcomeSource, HarnessPhaseGoalOutcome, HarnessPrLifecycleOutcome, HarnessRepairOutcome,
	HarnessReviewOutcome, HarnessSourceIntent, HarnessValidationOutcome,
};
use payload::{
	harness_contracts_for_issue, harness_outcome_payload, harness_programs_for_contracts,
};
