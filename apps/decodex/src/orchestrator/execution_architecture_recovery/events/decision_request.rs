use crate::orchestrator::execution_architecture_recovery::{
	AuthorityBoundaryPolicyDecision, AuthorityDecisionOption, AuthorityDecisionRequestInput,
	IssueRunPlan, LoopGuardrailStopRequested, ServiceConfig,
};

pub(super) fn architecture_recovery_decision_request_input<'a>(
	project: &'a ServiceConfig,
	issue_run: &'a IssueRunPlan,
	stop: &'a LoopGuardrailStopRequested,
	boundary_check_record_id: i64,
	decision_request_id: &'a str,
	reason_code: &'a str,
	final_reason: &'a str,
) -> AuthorityDecisionRequestInput<'a> {
	AuthorityDecisionRequestInput {
		project_id: project.service_id(),
		issue_id: &issue_run.issue.id,
		issue_identifier: &issue_run.issue.identifier,
		run_id: &issue_run.run_id,
		attempt_number: issue_run.attempt_number,
		boundary_check_record_id,
		decision_request_id,
		reason_code,
		boundary_type: "architecture_recovery",
		proposed_change: "Continue loop recovery with a materially different architecture strategy.",
		why_exceeds_authority: final_reason,
		options: vec![
			AuthorityDecisionOption {
				label: "Authorize recovery",
				description: "Update the issue, Decision Contract, or policy to allow this recovery.",
			},
			AuthorityDecisionOption {
				label: "Keep stopped",
				description: "Leave the lane in manual attention until the boundary is resolved.",
			},
		],
		recommendation: "Resolve the authority boundary before requeueing the lane.",
		resume_condition: "Accept, reject, or revise the requested authority in the issue, Decision Contract, or project policy before clearing needs-attention.",
		retained_worktree_evidence: vec![issue_run.worktree.branch_name.as_str()],
		retained_diff_evidence: vec![stop.fingerprint.as_str()],
		recovery_attempt_context: vec![stop.reason.error_class()],
	}
}

pub(super) fn architecture_recovery_policy_recovery_guidance(
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> &'static str {
	match policy_decision {
		AuthorityBoundaryPolicyDecision::AutoContinue => {
			"request human attention only if the next viable action would change product behavior, public API/config contract, security, data, credential, billing, validation standards, or accepted authority"
		},
		AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence => {
			"preserve enhanced evidence for the changed high-risk surfaces before review handoff or landing"
		},
		AuthorityBoundaryPolicyDecision::BlockLanding => {
			"keep landing blocked until validation or review-policy evidence is restored"
		},
		AuthorityBoundaryPolicyDecision::RequiresHumanDecision => {
			"request human attention before continuing recovery"
		},
	}
}
