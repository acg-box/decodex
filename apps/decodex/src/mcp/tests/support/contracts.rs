use crate::loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind};

pub(in crate::mcp::tests) fn accepted_mcp_goal_contract() -> DecisionContract {
	let mut contract: DecisionContract = serde_json::from_value(serde_json::json!({
		"schema": crate::loop_contract::DECISION_CONTRACT_SCHEMA,
		"record_version": crate::loop_contract::DECISION_CONTRACT_RECORD_VERSION,
		"contract_id": "mcp-goal-contract",
		"status": "draft_latent",
		"source_intent": {
			"summary": "Expose MCP planning tools.",
			"user_utterance": "arrange MCP planning tools",
			"source_issue_identifier": "XY-852"
		},
		"research_provenance": [
			{
				"kind": "spec",
				"reference": "docs/spec/runtime.md",
				"summary": "MCP planning tools are schema-bound."
			}
		],
		"research_evidence": [
			{
				"claim": "Goal intake can preview generated issue briefs.",
				"support": "Program Intake dry-run renders public-safe issue plans.",
				"source_ref": "docs/spec/loop-runtime.md"
			}
		],
		"research_options": [
			{
				"option": "Expose a small schema-bound planning facade.",
				"status": "selected",
				"tradeoffs": ["Keeps internal graph mechanics out of tool output."]
			}
		],
		"accepted_authority": {
			"accepted_objectives": ["Expose schema-bound MCP planning tools."],
			"non_goals": ["Do not expose raw Program graph mutation."],
			"constraints": ["Dry-run must not persist Program Intake rows."],
			"assumptions": ["The promoted contract owns issue shaping."],
			"objections": ["Apply must require explicit authority."],
			"stop_conditions": ["Stop when authority is missing."]
		},
		"execution_readiness": {
			"summary": "Ready for issue shaping.",
			"ready_for_issue_shaping": true,
			"missing_decisions": [],
			"validation_expectations": ["MCP intake dry-run returns public-safe issue rows."],
			"risk_notes": ["Do not expose internal Program node ids."],
			"proposed_issues": [
				{
					"key": "mcp-planning-tools",
					"title": "Expose schema-bound MCP planning tools.",
					"objective": "Expose schema-bound MCP planning tools.",
					"stage": "runtime",
					"dependencies": [],
					"conflict_domains": ["module:decodex-research-intake-tools"],
					"acceptance": ["Planning tools are listed through tools/list."],
					"validation": ["cargo test -p decodex mcp::tests -- --nocapture"],
					"risk": ["Do not expose internal graph mechanics."],
					"queue_intent": "ready_to_queue"
				}
			],
			"conflict_domains": ["module:decodex-research-intake-tools"]
		},
		"links": {
			"generated_issue_ids": [],
			"generated_issue_identifiers": [],
			"execution_program_node_ids": []
		},
		"evidence_boundary": {
			"private_evidence_refs": [],
			"public_projection_refs": [],
			"public_summary": "MCP planning tools are ready for issue shaping."
		}
	}))
	.expect("contract should deserialize");

	contract
		.promote(
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-18T00:00:00Z",
				"test",
				Some(String::from("Accepted for MCP intake dry-run testing.")),
			)
			.expect("promotion should build"),
		)
		.expect("contract should promote");

	contract
}

pub(in crate::mcp::tests) fn latent_decision_contract_fixture() -> DecisionContract {
	serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/decision_contract/research_x_latent_contract.json"
	)))
	.expect("research X latent contract fixture should deserialize")
}
