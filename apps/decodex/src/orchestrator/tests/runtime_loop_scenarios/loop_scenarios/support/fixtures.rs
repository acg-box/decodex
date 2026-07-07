use color_eyre::Report;
use serde_json::Value;

use crate::{
	execution_program::{
		ExecutionDispatchAction, ExecutionLinearIssueMapping, ExecutionProgramEvaluation,
		ExecutionProgramNodeStage, ExecutionQueueIntent, ExecutionWorkflowPolicy,
	},
	loop_contract::DecisionContract,
	orchestrator::{RepoGateFailure, RepoGateFailureKind},
};

pub(crate) fn loop_scenario_decodex_workflow_policy() -> ExecutionWorkflowPolicy {
	ExecutionWorkflowPolicy::new(
		"decodex",
		vec![String::from("Todo")],
		vec![String::from("Done"), String::from("Canceled")],
		"decodex:manual-only",
		"decodex:needs-attention",
	)
	.expect("policy should build")
}

pub(crate) fn loop_scenario_program_nodes() -> Vec<crate::execution_program::ExecutionProgramNode> {
	vec![
		loop_scenario_node(
			"node-ready",
			ExecutionProgramNodeStage::Runtime,
			"Implement the accepted runtime node",
			ExecutionQueueIntent::ReadyToQueue,
			"XY-861",
		),
		loop_scenario_node(
			"node-blocked",
			ExecutionProgramNodeStage::Eval,
			"Validate after the runtime node completes",
			ExecutionQueueIntent::ReadyToQueue,
			"XY-862",
		)
		.with_dependencies(vec![
			crate::execution_program::ExecutionProgramDependency::new("node-ready")
				.expect("dependency should build"),
		])
		.expect("dependency should attach"),
		loop_scenario_node(
			"node-uncovered-direction",
			ExecutionProgramNodeStage::Research,
			"Pause uncovered direction for contract feedback",
			ExecutionQueueIntent::Paused,
			"XY-863",
		),
		loop_scenario_active_node(),
	]
}

pub(crate) fn loop_scenario_active_node() -> crate::execution_program::ExecutionProgramNode {
	loop_scenario_node(
		"node-active",
		ExecutionProgramNodeStage::Handoff,
		"Retain handoff ownership without queueing",
		ExecutionQueueIntent::Active,
		"XY-864",
	)
	.with_linear_issue(
		ExecutionLinearIssueMapping::new("linear-node-active", "XY-864", "Todo")
			.expect("issue mapping should build")
			.with_active_label(true),
	)
	.expect("active issue should attach")
}

pub(crate) fn loop_scenario_review_payload(
	nonclean_rounds: i64,
	private_marker: &'static str,
) -> Value {
	serde_json::json!({
		"phase": "handoff",
		"status": "findings",
		"head_sha": "0123456789abcdef0123456789abcdef01234567",
		"nonclean_rounds": nonclean_rounds,
		"review": {
			"accepted_findings": [{
				"summary": "Ready node lacks a scenario fixture",
				"evidence": [private_marker]
			}],
			"rejected_findings": [{
				"summary": "Stale comment did not apply to current head"
			}]
		}
	})
}

pub(crate) fn loop_scenario_uncovered_payload() -> Value {
	serde_json::json!({
		"reason": "uncovered_direction",
		"fingerprint": "contract-gap:operator-feedback",
		"paused_node_id": "node-uncovered-direction",
		"feedback_target": "decision_contract",
		"other_ready_node_ids": ["node-ready"],
	})
}

pub(crate) fn loop_scenario_repo_gate_failure() -> Report {
	Report::new(RepoGateFailure::new(
		RepoGateFailureKind::VerifyCommandFailed,
		String::from("Repo verify command `cargo make test` failed: same assertion failed"),
	))
}

pub(crate) fn loop_scenario_assert_harness_candidates(
	payload: &Value,
	private_review_marker: &str,
	private_authority_marker: &str,
) {
	let candidates = payload["improvement_candidates"]
		.as_array()
		.expect("improvement candidates should be an array");
	let candidate_json =
		serde_json::to_string(candidates).expect("candidate summaries should serialize");

	assert!(
		candidates.iter().any(|candidate| {
			candidate["kind"] == "underspecified_decision_contract"
				&& candidate["reason_code"] == "uncovered_direction"
		}),
		"uncovered direction should recommend a contract improvement"
	);
	assert!(
		candidates.iter().any(|candidate| {
			candidate["kind"] == "weak_prompt"
				&& candidate["reason_code"] == "accepted_review_findings"
		}),
		"accepted review findings should recommend a prompt or fixture improvement"
	);
	assert_eq!(payload["authority_boundary"]["failed_check_count"], 1);
	assert_eq!(payload["authority_boundary"]["improvement_signal_count"], 3);
	assert!(
		candidates.iter().any(|candidate| {
			candidate["kind"] == "underspecified_decision_contract"
				&& candidate["reason_code"] == "authority_underspecified"
		}),
		"authority underspecification should recommend a contract improvement"
	);
	assert!(
		candidates.iter().any(|candidate| {
			candidate["kind"] == "missing_issue_template_field"
				&& candidate["reason_code"] == "authority_boundary_template_gap"
		}),
		"authority gaps should recommend issue-template hardening"
	);
	assert!(
		candidates.iter().any(|candidate| {
			candidate["kind"] == "missing_validator"
				&& candidate["reason_code"] == "authority_boundary_validator_gap"
		}),
		"authority gaps should recommend validator hardening"
	);
	assert!(
		candidates.iter().any(|candidate| {
			candidate["kind"] == "recovery_budget_exhausted"
				&& candidate["reason_code"] == "architecture_recovery_exhausted"
		}),
		"exhausted architecture recovery should recommend recovery-budget hardening"
	);
	assert!(
		!candidate_json.contains(private_review_marker),
		"harness recommendations must summarize private events without leaking raw payloads"
	);
	assert!(
		!candidate_json.contains(private_authority_marker),
		"authority-boundary recommendations must not leak raw changed-surface payloads"
	);
}

pub(crate) fn loop_scenario_research_x_contract() -> DecisionContract {
	serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/decision_contract/decision_x_latent_contract.json"
	)))
	.expect("decision X fixture should deserialize")
}

pub(crate) fn loop_scenario_node(
	node_id: &str,
	stage: ExecutionProgramNodeStage,
	objective: &str,
	queue_intent: ExecutionQueueIntent,
	issue_identifier: &str,
) -> crate::execution_program::ExecutionProgramNode {
	crate::execution_program::ExecutionProgramNode::new(node_id, stage, objective, queue_intent)
		.expect("node should build")
		.with_conflict_domains(vec![
			crate::execution_program::ExecutionConflictDomain::new(
				crate::execution_program::ExecutionConflictDomainKind::Module,
				format!("runtime/{node_id}"),
			)
			.expect("conflict domain should build"),
		])
		.expect("conflict domain should attach")
		.with_acceptance_expectations(vec![format!(
			"{issue_identifier} satisfies the promoted contract",
		)])
		.expect("acceptance expectations should attach")
		.with_validation_expectations(vec![format!(
			"{issue_identifier} has deterministic validation",
		)])
		.expect("validation expectations should attach")
		.with_linear_issue(
			ExecutionLinearIssueMapping::new(format!("linear-{node_id}"), issue_identifier, "Todo")
				.expect("issue mapping should build"),
		)
		.expect("issue mapping should attach")
}

pub(crate) fn loop_scenario_dispatch_action(
	evaluation: &ExecutionProgramEvaluation,
	node_id: &str,
) -> Option<ExecutionDispatchAction> {
	evaluation
		.nodes()
		.iter()
		.find(|node| node.node_id() == node_id)
		.unwrap_or_else(|| panic!("missing node evaluation `{node_id}`"))
		.dispatch_action()
}
