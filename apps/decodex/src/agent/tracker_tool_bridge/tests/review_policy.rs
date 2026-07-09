mod review_policy_churn;
mod review_policy_compact_guardrails;
mod review_policy_completion_gates;
mod review_policy_finding_routes;
mod review_policy_handoff_surface;
mod review_policy_payload_contract;
mod review_policy_repair_apply;

use serde_json::Value;

use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolCallResponse, DynamicToolHandler, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		PullRequestDetails, ReviewHandoffContext, TrackerToolBridge,
		tests::{self, FakeLocalRepoInspector, FakePullRequestInspector, LocalRepoDetails},
	},
	state::{
		ReviewLifecycleHandoffFixture, ReviewLifecycleTransitionFixture,
		ReviewPolicyCheckpointInput, StateStore,
	},
};

pub(in crate::agent::tracker_tool_bridge::tests) fn sample_review_repair_apply_inspectors(
	pr_url: &str,
) -> (FakePullRequestInspector, FakeLocalRepoInspector) {
	let inspector = FakePullRequestInspector::new(vec![
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from(pr_url),
		}),
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from(pr_url),
		}),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(LocalRepoDetails {
			default_branch: String::from("main"),
			head_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_tree_oid: String::from("f8a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			repository_name: String::from("decodex"),
			repository_owner: String::from("hack-ink"),
			review_blocking_changes: Vec::new(),
		}),
		Ok(LocalRepoDetails {
			default_branch: String::from("main"),
			head_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_tree_oid: String::from("f8a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			repository_name: String::from("decodex"),
			repository_owner: String::from("hack-ink"),
			review_blocking_changes: Vec::new(),
		}),
	]);

	(inspector, local_repo_inspector)
}

pub(in crate::agent::tracker_tool_bridge::tests) fn review_checks_json() -> Value {
	serde_json::json!({
		"intended_behavior": "Checked the implementation against the issue requirements.",
		"regression_risk": "Checked shared runtime regression risk for the touched path.",
		"missing_tests": "Checked whether the current change needs additional tests.",
		"openwiki_config_drift": "Checked OpenWiki and config drift for the runtime behavior change.",
		"migration_fallout": "Checked additive runtime-store migration fallout.",
		"operator_facing_fallout": "Checked Linear and operator-facing fallout.",
		"loop_decision_contract": "Compared the change against the accepted Loop/Decision Contract and found no mismatch."
	})
}

pub(in crate::agent::tracker_tool_bridge::tests) fn handoff_review_contract_json() -> Value {
	review_contract_json("full_current_head_review")
}

pub(in crate::agent::tracker_tool_bridge::tests) fn low_risk_handoff_review_contract_json() -> Value
{
	review_contract_with_risk_json("full_current_head_review", "low")
}

pub(in crate::agent::tracker_tool_bridge::tests) fn repair_review_contract_json() -> Value {
	review_contract_json("repair_verification")
}

pub(in crate::agent::tracker_tool_bridge::tests) fn review_contract_json(
	review_type: &str,
) -> Value {
	review_contract_with_risk_json(review_type, "localized")
}

pub(in crate::agent::tracker_tool_bridge::tests) fn review_contract_with_risk_json(
	review_type: &str,
	risk_tier: &str,
) -> Value {
	serde_json::json!({
		"workflow_policy_source": "registered_project_workflow",
		"review_type": review_type,
		"risk_tier": risk_tier,
		"objective": "Review the current committed lane head against the accepted issue contract.",
		"scope": ["Current committed lane diff and directly owned behavior."],
		"non_goals": ["Do not widen into unrelated cleanup or unowned product direction."],
		"required_checks": ["Intended behavior, regression risk, tests, OpenWiki/config drift, migration fallout, operator-facing fallout, and Loop/Decision Contract alignment."],
		"allowed_expansion_triggers": ["Safety, authority-boundary, data-loss, security, live-mutation, public-API, migration, or operator-facing regression."],
		"validation_evidence": ["Repo-native validation was rerun for the committed lane head before review."]
	})
}

pub(in crate::agent::tracker_tool_bridge::tests) fn compact_review_cost_control_json() -> Value {
	serde_json::json!({
		"review_class": "compact_current_head_review",
		"risk_class": "low",
		"changed_surface_count": 2,
		"changed_surface_summary": [
			"Review checkpoint prompt guidance changed.",
			"Review checkpoint readback metadata changed."
		],
		"high_risk_surfaces": [],
		"current_head_evidence": true,
		"validation_backed": true,
		"validation_current": true,
		"evidence_sufficient": true,
		"reviewer_judgment": "The reviewer independently checked intended behavior and adversarial risk and found a low-risk small current-head lane."
	})
}

pub(in crate::agent::tracker_tool_bridge::tests) fn full_review_cost_control_json(
	fallback_reason: &str,
) -> Value {
	serde_json::json!({
		"review_class": "full_current_head_review",
		"risk_class": "localized",
		"changed_surface_count": 6,
		"changed_surface_summary": [
			"Runtime review checkpoint behavior changed.",
			"Operator readback behavior changed."
		],
		"high_risk_surfaces": ["operator-facing runtime review behavior"],
		"current_head_evidence": true,
		"validation_backed": true,
		"validation_current": true,
		"evidence_sufficient": true,
		"reviewer_judgment": "The reviewer used full independent review because compact-review guardrails did not all pass.",
		"fallback_reason": fallback_reason
	})
}

pub(in crate::agent::tracker_tool_bridge::tests) fn accepted_review_findings_json() -> Value {
	accepted_review_findings_with_summary_json(
		"Accepted reviewer finding",
		"Repair the accepted issue before requesting another review checkpoint.",
		1,
	)
}

pub(in crate::agent::tracker_tool_bridge::tests) fn accepted_review_findings_with_summary_json(
	summary: &str,
	guidance: &str,
	line: u64,
) -> Value {
	serde_json::json!([{
		"severity": "medium",
		"summary": summary,
		"evidence": ["The reviewer evidence points at the current lane head."],
		"file": "apps/decodex/src/agent/tracker_tool_bridge/tools.rs",
		"line": line,
		"guidance": guidance
	}])
}

pub(in crate::agent::tracker_tool_bridge::tests) fn accepted_review_findings_for_status_json(
	status: &str,
) -> Value {
	if status == "findings" { accepted_review_findings_json() } else { serde_json::json!([]) }
}

pub(in crate::agent::tracker_tool_bridge::tests) fn route_only_review_route_json(
	route: &str,
) -> Value {
	serde_json::json!([{
		"route": route,
		"severity": "medium",
		"risk_tier": "medium",
		"summary": "Review signal is routed outside current repair.",
		"evidence": ["The reviewer signal was checked against the current lane head."],
		"resolver": "architecture",
		"next_action": "Record the routed review signal without mutating the current repair."
	}])
}

pub(in crate::agent::tracker_tool_bridge::tests) fn sample_dirty_local_repo() -> LocalRepoDetails {
	let mut local_repo = tests::sample_local_repo();

	local_repo.review_blocking_changes = vec![
		String::from("M apps/decodex/src/agent/tracker_tool_bridge/tools.rs"),
		String::from("?? apps/decodex/src/agent/new_review_surface.rs"),
	];

	local_repo
}

pub(in crate::agent::tracker_tool_bridge::tests) fn submit_findings_review_checkpoint(
	bridge: &TrackerToolBridge<'_>,
	evidence: &str,
) -> DynamicToolCallResponse {
	submit_findings_review_checkpoint_with_findings(
		bridge,
		evidence,
		accepted_review_findings_json(),
	)
}

pub(in crate::agent::tracker_tool_bridge::tests) fn submit_findings_review_checkpoint_with_findings(
	bridge: &TrackerToolBridge<'_>,
	evidence: &str,
	accepted_findings: Value,
) -> DynamicToolCallResponse {
	DynamicToolHandler::handle_call(
		bridge,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		serde_json::json!({
			"reviewer": "independent_fresh_context",
			"status": "findings",
			"head_sha": tests::sample_local_repo().head_oid,
			"review_contract": handoff_review_contract_json(),
			"checks": review_checks_json(),
			"evidence": [evidence],
			"accepted_findings": accepted_findings
		}),
	)
}

pub(in crate::agent::tracker_tool_bridge::tests) fn seed_review_repair_apply_state(
	state_store: &StateStore,
	review_context: &ReviewHandoffContext,
	issue_id: &str,
	pr_url: &str,
	external_round_count: i64,
) {
	let review_handoff = ReviewLifecycleHandoffFixture::new(
		String::from("pub-618-attempt-2-100"),
		2,
		review_context.branch_name.clone(),
		String::from(pr_url),
		String::from("main"),
		review_context.branch_name.clone(),
		String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
	);

	state_store
		.upsert_review_lifecycle_handoff_fixture(
			&review_context.service_id,
			issue_id,
			&review_handoff,
		)
		.expect("original review lifecycle handoff fixture should persist");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: &review_context.service_id,
			issue_id,
			run_id: &review_context.run_id,
			attempt_number: review_context.attempt_number,
			phase: "repair",
			review_level: review_context.review_level.as_str(),
			status: "clean",
			head_sha: "18a20f7dfb9526e7421a5f095b1c6adec84e52d6",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("repair review checkpoint should persist");
	state_store
		.upsert_review_lifecycle_transition_fixture(
			&review_context.service_id,
			issue_id,
			&ReviewLifecycleTransitionFixture::new(
				review_handoff.run_id().to_owned(),
				review_handoff.attempt_number(),
				review_handoff.branch_name().to_owned(),
				pr_url.to_owned(),
				String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
				"repair_required",
				Some(91),
				Some(1_763_600_000),
				Some(0),
				0,
				external_round_count,
				None,
			),
		)
		.expect("review lifecycle transition fixture should persist");
}
