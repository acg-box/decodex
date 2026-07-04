use std::cmp::Ordering;

use crate::state::{
	AutonomyProposalRuntimeRecord, AutonomySignalRuntimeRecord, DecisionContractRuntimeRecord,
	ExecutionProgramRuntimeRecord, LinearExecutionEventRuntimeRecord,
	PrivateExecutionEventRuntimeRecord, ProgramIntakePlanRecord, ProgramIssueMappingRecord,
};

pub(in crate::state) fn compare_linear_execution_event_runtime_records(
	left: &LinearExecutionEventRuntimeRecord,
	right: &LinearExecutionEventRuntimeRecord,
) -> Ordering {
	left.event_unix
		.cmp(&right.event_unix)
		.then_with(|| left.recorded_at_unix.cmp(&right.recorded_at_unix))
		.then_with(|| left.record.idempotency_key.cmp(&right.record.idempotency_key))
}

pub(in crate::state) fn compare_private_execution_event_runtime_records(
	left: &PrivateExecutionEventRuntimeRecord,
	right: &PrivateExecutionEventRuntimeRecord,
) -> Ordering {
	left.record_id.cmp(&right.record_id)
}

#[allow(dead_code)]
pub(in crate::state) fn compare_decision_contract_runtime_records(
	left: &DecisionContractRuntimeRecord,
	right: &DecisionContractRuntimeRecord,
) -> Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.contract.contract_id().cmp(right.contract.contract_id()))
}

#[allow(dead_code)]
pub(in crate::state) fn compare_autonomy_signal_runtime_records(
	left: &AutonomySignalRuntimeRecord,
	right: &AutonomySignalRuntimeRecord,
) -> Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.signal.id().cmp(right.signal.id()))
}

#[allow(dead_code)]
pub(in crate::state) fn compare_recent_autonomy_signal_runtime_records(
	left: &AutonomySignalRuntimeRecord,
	right: &AutonomySignalRuntimeRecord,
) -> Ordering {
	right
		.updated_at_unix
		.cmp(&left.updated_at_unix)
		.then_with(|| left.signal.id().cmp(right.signal.id()))
}

#[allow(dead_code)]
pub(in crate::state) fn compare_recent_autonomy_proposal_runtime_records(
	left: &AutonomyProposalRuntimeRecord,
	right: &AutonomyProposalRuntimeRecord,
) -> Ordering {
	right
		.updated_at_unix
		.cmp(&left.updated_at_unix)
		.then_with(|| left.proposal.id().cmp(right.proposal.id()))
}

#[allow(dead_code)]
pub(in crate::state) fn compare_execution_program_runtime_records(
	left: &ExecutionProgramRuntimeRecord,
	right: &ExecutionProgramRuntimeRecord,
) -> Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.program.program_id().cmp(right.program.program_id()))
}

pub(in crate::state) fn compare_program_intake_plan_records(
	left: &ProgramIntakePlanRecord,
	right: &ProgramIntakePlanRecord,
) -> Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.program_id.cmp(&right.program_id))
		.then_with(|| left.plan_id.cmp(&right.plan_id))
}

pub(in crate::state) fn compare_program_issue_mapping_records(
	left: &ProgramIssueMappingRecord,
	right: &ProgramIssueMappingRecord,
) -> Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.program_id.cmp(&right.program_id))
		.then_with(|| left.node_id.cmp(&right.node_id))
}
