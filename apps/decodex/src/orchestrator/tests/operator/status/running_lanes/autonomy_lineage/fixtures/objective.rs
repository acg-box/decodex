use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
	},
	orchestrator::tests::operator::status::running_lanes::autonomy_lineage::fixtures::{
		OBJECTIVE_ID, SERVICE_ID,
	},
	state::StateStore,
};

pub(super) fn accept_autonomy_objective(state_store: &StateStore) {
	state_store
		.upsert_autonomy_objective_draft(SERVICE_ID, autonomy_objective_fixture(SERVICE_ID))
		.expect("objective draft should persist");
	state_store
		.accept_autonomy_objective_version(
			SERVICE_ID,
			OBJECTIVE_ID,
			1,
			AutonomyObjectiveAcceptance::new(
				"operator",
				AutonomyObjectiveActorKind::User,
				"2026-06-23T00:00:00Z",
				"linear:XY-1089",
			)
			.expect("acceptance should build"),
		)
		.expect("objective should accept");
}

fn autonomy_objective_fixture(service_id: &str) -> AutonomyObjectiveContract {
	serde_json::from_value(serde_json::json!({
		"schema": "decodex.autonomy_objective/1",
		"record_version": 1,
		"project_id": service_id,
		"id": OBJECTIVE_ID,
		"version": 1,
		"state": "draft",
		"summary": "Surface autonomy lineage in operator readback.",
		"goals": ["Expose objective, signal, proposal, decision, and intake lineage."],
		"non_goals": ["Do not expose raw private evidence payloads."],
		"metrics": ["Operator can explain autonomy state without SQLite."],
		"allowed_surfaces": ["apps/decodex/src/orchestrator", "openwiki/specs"],
		"allowed_signal_kinds": ["runtime_health"],
		"validation_gates": ["cargo test -p decodex operator --lib"],
		"review_policy": "independent review before handoff",
		"memory_policy": "runtime records only",
		"report_policy": "public-safe derived query views only"
	}))
	.expect("objective fixture should parse")
}
