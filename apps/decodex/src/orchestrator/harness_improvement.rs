use super::{IssueRunPlan, Result, StateStore, Value};

use serde::Serialize;

use crate::{
	execution_program::ExecutionConflictDomain,
	state::{DecisionContractRecord, ExecutionProgramRecord, PrivateExecutionEvent},
	tracker::records::LinearExecutionEventRecord,
};

mod candidates;
mod model;
mod payload;
mod record;

use candidates::harness_improvement_candidates;
use model::{
	HARNESS_OUTCOME_EVENT_TYPE, HARNESS_OUTCOME_SCHEMA, HarnessAuthorityBoundaryOutcome,
	HarnessLinearProjectionSummary, HarnessManualAttentionOutcome, HarnessOutcomeContract,
	HarnessOutcomePayload, HarnessOutcomeProgram, HarnessOutcomeProgramNode, HarnessOutcomeSignals,
	HarnessOutcomeSource, HarnessPhaseGoalOutcome, HarnessPrLifecycleOutcome, HarnessRepairOutcome,
	HarnessReviewOutcome, HarnessSourceIntent, HarnessValidationOutcome,
};
pub(crate) use model::{
	HarnessImprovementCandidateSummary, HarnessOutcomeKind, HarnessOutcomeRecordInput,
};
use payload::{
	harness_contracts_for_issue, harness_outcome_payload, harness_programs_for_contracts,
};
#[cfg(test)] pub(crate) use record::record_harness_outcome_for_issue_run;
pub(crate) use record::{
	harness_improvement_candidates_from_private_events, record_harness_outcome_best_effort,
};
