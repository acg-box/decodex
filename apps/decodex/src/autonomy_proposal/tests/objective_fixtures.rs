use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
	},
	state::StateStore,
};

pub(crate) fn objective_fixture() -> AutonomyObjectiveContract {
	let mut objective = objective_draft_fixture();

	objective
		.accept(
			AutonomyObjectiveAcceptance::new(
				"operator",
				AutonomyObjectiveActorKind::User,
				"2026-06-22T00:00:00Z",
				"conversation",
			)
			.expect("acceptance should validate"),
		)
		.expect("objective should accept");

	objective
}

pub(crate) fn store_accepted_objective(store: &StateStore) -> AutonomyObjectiveContract {
	store
		.upsert_autonomy_objective_draft("decodex", objective_draft_fixture())
		.expect("objective should store");

	store
		.accept_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			1,
			AutonomyObjectiveAcceptance::new(
				"operator",
				AutonomyObjectiveActorKind::User,
				"2026-06-22T00:00:00Z",
				"conversation",
			)
			.expect("acceptance should validate"),
		)
		.expect("objective should accept")
		.objective()
		.clone()
}

fn objective_draft_fixture() -> AutonomyObjectiveContract {
	serde_json::from_value(serde_json::json!({
		"schema": "decodex.autonomy_objective/1",
		"record_version": 1,
		"project_id": "decodex",
		"id": "quality-autonomy",
		"version": 1,
		"state": "draft",
		"summary": "Improve Decodex autonomy quality under explicit authority.",
		"goals": ["Reduce repeated validation and review churn."],
		"non_goals": ["Do not bypass Decision Contract authority."],
		"metrics": ["Validation retry count stays below objective tolerance."],
		"allowed_surfaces": ["apps/decodex/src", "docs/spec"],
		"allowed_signal_kinds": ["runtime_health", "review_feedback_cluster"],
		"validation_gates": ["cargo test -p decodex autonomy_proposal --lib"],
		"review_policy": "independent current-head review required",
		"memory_policy": "read-only source-linked memory only",
		"report_policy": "public-safe summaries only"
	}))
	.expect("draft objective should parse")
}
