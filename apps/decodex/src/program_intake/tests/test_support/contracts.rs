use serde_json::Value;

use crate::loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind};

pub(crate) fn latent_goal_contract() -> DecisionContract {
	serde_json::from_value(latent_goal_contract_payload())
		.expect("goal contract should deserialize")
}

pub(crate) fn accepted_goal_contract() -> DecisionContract {
	let mut contract = latent_goal_contract();

	contract
		.promote(
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-10T00:00:00Z",
				"conversation",
				Some(String::from("User asked Decodex to arrange this goal.")),
			)
			.expect("promotion should build"),
		)
		.expect("contract should promote");

	contract
}

fn latent_goal_contract_payload() -> Value {
	serde_json::json!({
		"schema": crate::loop_contract::DECISION_CONTRACT_SCHEMA,
		"record_version": crate::loop_contract::DECISION_CONTRACT_RECORD_VERSION,
		"contract_id": "goal-intake-contract",
		"status": "draft_latent",
		"source_intent": latent_goal_source_intent(),
		"research_provenance": latent_goal_research_provenance(),
		"research_evidence": latent_goal_research_evidence(),
		"research_options": [],
		"accepted_authority": latent_goal_accepted_authority(),
		"execution_readiness": latent_goal_execution_readiness(),
		"links": {
			"generated_issue_ids": [],
			"generated_issue_identifiers": [],
		"execution_program_node_ids": []
	},
	"evidence_boundary": {
		"private_evidence_refs": [],
		"public_projection_refs": [],
			"public_summary": "Goal intake contract ready for issue shaping."
		}
	})
}

fn latent_goal_source_intent() -> Value {
	serde_json::json!({
		"summary": "Ship promoted goal intake.",
		"user_utterance": "arrange this goal",
		"source_issue_identifier": "XY-852",
	})
}

fn latent_goal_research_provenance() -> Value {
	serde_json::json!([
		{
			"kind": "autonomy_proposal",
			"reference": "autonomy_proposal:test-proposal",
			"summary": "Accepted autonomy proposal produced this Decision Contract candidate."
		},
		{
			"kind": "autonomy_objective",
			"reference": "decodex:quality-autonomy@1",
			"summary": "Accepted autonomy objective version."
		},
		{
			"kind": "spec",
			"reference": "apps/decodex/src/loop_contract.rs",
			"summary": "Promoted contracts can shape normal Linear issues."
		}
	])
}

fn latent_goal_research_evidence() -> Value {
	serde_json::json!([
		{
			"kind": "autonomy_signal:runtime_health",
			"claim": "Autonomy signal `autonomy_signal:test-signal` contributed.",
			"support": "freshness=fresh; evidence_class=repo_source; confidence=high",
			"source_ref": "autonomy_signal:test-signal"
		},
		{
			"claim": "Goal intake needs generated issues and an internal program.",
			"support": "The loop-runtime spec defines Program Intake records.",
			"source_ref": "apps/decodex/src/loop_contract.rs"
		}
	])
}

fn latent_goal_accepted_authority() -> Value {
	serde_json::json!({
		"accepted_objectives": [
			"Materialize accepted goal intake into normal Linear issues.",
			"Persist the internal Execution Program without exposing graph mechanics."
		],
		"non_goals": ["Do not run implementation from goal intake."],
		"constraints": ["Linear receives only public-safe issue briefs and sparse links."],
		"assumptions": ["The source issue anchors the generated issue team."],
		"objections": [],
		"stop_conditions": [
			"Stop when promotion authority or required decisions are missing."
		]
	})
}

fn latent_goal_execution_readiness() -> Value {
	serde_json::json!({
		"summary": "Ready for issue shaping after promotion.",
		"ready_for_issue_shaping": true,
		"missing_decisions": [],
		"validation_expectations": ["Run cargo make test before handoff."],
		"risk_notes": ["Generated issue descriptions must stay natural-language."],
		"proposed_issues": [goal_intake_runtime_issue(), goal_intake_links_issue()],
		"conflict_domains": ["module:runtime", "file:apps/decodex/src/loop_contract.rs"]
	})
}

fn goal_intake_runtime_issue() -> Value {
	serde_json::json!({
		"key": "goal-intake-runtime",
		"title": "Implement goal intake CLI/API behavior.",
		"objective": "Implement goal intake CLI/API behavior.",
		"stage": "runtime",
		"dependencies": [],
		"conflict_domains": ["module:runtime"],
		"acceptance": ["Goal intake dry-run renders generated issue briefs without mutation."],
		"validation": ["Run cargo make test before handoff."],
		"risk": ["Generated issue descriptions must stay natural-language."],
		"queue_intent": "ready_to_queue"
	})
}

fn goal_intake_links_issue() -> Value {
	serde_json::json!({
		"key": "goal-intake-links",
		"title": "Persist Execution Program links for generated issues.",
		"objective": "Persist Execution Program links for generated issues.",
		"stage": "runtime",
		"dependencies": ["goal-intake-runtime"],
		"conflict_domains": ["module:runtime", "file:apps/decodex/src/loop_contract.rs"],
		"acceptance": [
			"Apply links generated issue identifiers and execution nodes back to the accepted contract."
		],
		"validation": ["Run cargo make test before handoff."],
		"risk": ["Generated issue descriptions must stay natural-language."],
		"queue_intent": "ready_to_queue"
	})
}
