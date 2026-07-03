use crate::orchestrator::execution_architecture_recovery::{
	AuthorityBoundaryDisposition, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface,
	DecisionContractRecord, IssueRunPlan, LoopGuardrailStopRequested, Report, ServiceConfig,
};

pub(super) struct ArchitectureRecoveryBoundary {
	pub(super) disposition: AuthorityBoundaryDisposition,
	pub(super) policy_decision: AuthorityBoundaryPolicyDecision,
	pub(super) final_reason: &'static str,
	pub(super) boundary_type: AuthorityBoundarySurface,
}

pub(super) struct ArchitectureRecoveryPacketInput<'a> {
	pub(super) project: &'a ServiceConfig,
	pub(super) issue_run: &'a IssueRunPlan,
	pub(super) loop_guardrail_stop: &'a LoopGuardrailStopRequested,
	pub(super) error: &'a Report,
	pub(super) contracts: &'a [DecisionContractRecord],
	pub(super) boundary_check_record_id: i64,
	pub(super) boundary_disposition: AuthorityBoundaryDisposition,
	pub(super) boundary_policy_decision: AuthorityBoundaryPolicyDecision,
	pub(super) boundary_final_reason: &'a str,
	pub(super) reason_code: &'a str,
	pub(super) recovery_attempt_number: usize,
	pub(super) prior_started_count: usize,
}

pub(super) struct ArchitectureRecoveryTerminalEventInput<'a> {
	pub(super) project: &'a ServiceConfig,
	pub(super) issue_run: &'a IssueRunPlan,
	pub(super) stop: &'a LoopGuardrailStopRequested,
	pub(super) boundary_check_record_id: i64,
	pub(super) boundary_disposition: AuthorityBoundaryDisposition,
	pub(super) boundary_policy_decision: AuthorityBoundaryPolicyDecision,
	pub(super) boundary_final_reason: &'a str,
	pub(super) reason_code: &'a str,
	pub(super) recovery_attempt_number: usize,
}
