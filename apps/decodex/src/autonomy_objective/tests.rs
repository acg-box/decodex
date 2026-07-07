use crate::autonomy_objective::{
	AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
	AutonomyObjectiveRejection, AutonomyObjectiveState, AutonomyObjectiveSupersession,
};

fn objective_fixture() -> AutonomyObjectiveContract {
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
		"allowed_signal_kinds": ["validation_regression", "review_feedback_cluster"],
		"validation_gates": ["cargo make check"],
		"review_policy": "independent current-head review required",
		"memory_policy": "read-only source-linked memory only",
		"report_policy": "public-safe summaries only"
	}))
	.expect("objective fixture should parse")
}

fn sample_acceptance() -> AutonomyObjectiveAcceptance {
	AutonomyObjectiveAcceptance::new(
		"operator",
		AutonomyObjectiveActorKind::User,
		"2026-06-22T00:00:00Z",
		"conversation",
	)
	.expect("acceptance should validate")
}

#[test]
fn autonomy_objective_acceptance_is_explicit_lifecycle() {
	let mut objective = objective_fixture();

	objective.validate().expect("draft should validate");
	objective.accept(sample_acceptance()).expect("draft should accept");

	assert_eq!(objective.state(), AutonomyObjectiveState::Accepted);
	assert_eq!(objective.acceptance().expect("acceptance should exist").accepted_by(), "operator");
	assert!(objective.rejection().is_none());
}

#[test]
fn rejected_and_superseded_objectives_keep_provenance() {
	let mut rejected = objective_fixture();

	rejected
		.reject(
			AutonomyObjectiveRejection::new(
				"operator",
				"2026-06-22T00:00:00Z",
				"conversation",
				"Wrong surface.",
			)
			.expect("rejection should validate"),
		)
		.expect("draft should reject");

	assert_eq!(rejected.state(), AutonomyObjectiveState::Rejected);
	assert_eq!(rejected.rejection().expect("rejection should exist").reason(), "Wrong surface.");

	let mut superseded = objective_fixture();

	superseded.accept(sample_acceptance()).expect("draft should accept");
	superseded
		.supersede(
			AutonomyObjectiveSupersession::new(
				"quality-autonomy",
				2,
				"operator",
				"2026-06-22T00:05:00Z",
				"conversation",
				"Accepted replacement objective version.",
			)
			.expect("supersession should validate"),
		)
		.expect("accepted version should supersede");

	assert_eq!(superseded.state(), AutonomyObjectiveState::Superseded);
	assert_eq!(
		superseded.supersession().expect("supersession should exist").superseded_by_version(),
		2
	);
}

#[test]
fn lifecycle_metadata_is_not_inferred_or_accepted_on_drafts() {
	let mut objective = serde_json::to_value(objective_fixture()).expect("fixture should encode");

	objective["acceptance"] = serde_json::json!({
		"accepted_by": "operator",
		"accepted_by_kind": "user",
		"accepted_at": "2026-06-22T00:00:00Z",
		"acceptance_source": "conversation"
	});

	let objective =
		serde_json::from_value::<AutonomyObjectiveContract>(objective).expect("payload parses");

	assert!(objective.validate().is_err());
}

#[test]
fn superseded_objectives_reject_mixed_terminal_provenance() {
	let mut objective = serde_json::to_value(objective_fixture()).expect("fixture should encode");

	objective["state"] = serde_json::json!("superseded");
	objective["supersession"] = serde_json::json!({
		"superseded_by_objective_id": "quality-autonomy",
		"superseded_by_version": 2,
		"superseded_by": "operator",
		"superseded_at": "2026-06-22T00:05:00Z",
		"supersession_source": "conversation",
		"reason": "Accepted replacement objective version."
	});
	objective["rejection"] = serde_json::json!({
		"rejected_by": "operator",
		"rejected_at": "2026-06-22T00:04:00Z",
		"rejection_source": "conversation",
		"reason": "Contradictory terminal state."
	});

	let objective =
		serde_json::from_value::<AutonomyObjectiveContract>(objective).expect("payload parses");

	assert!(objective.validate().is_err());
}

#[test]
fn superseded_objectives_reject_self_or_older_same_objective_version() {
	for (objective_version, superseded_by_version) in [(1, 1), (2, 1)] {
		let mut objective =
			serde_json::to_value(objective_fixture()).expect("fixture should encode");

		objective["version"] = serde_json::json!(objective_version);
		objective["state"] = serde_json::json!("superseded");
		objective["supersession"] = serde_json::json!({
			"superseded_by_objective_id": "quality-autonomy",
			"superseded_by_version": superseded_by_version,
			"superseded_by": "operator",
			"superseded_at": "2026-06-22T00:05:00Z",
			"supersession_source": "conversation",
			"reason": "Invalid replacement version."
		});

		let objective =
			serde_json::from_value::<AutonomyObjectiveContract>(objective).expect("payload parses");

		assert!(objective.validate().is_err());
	}
}
