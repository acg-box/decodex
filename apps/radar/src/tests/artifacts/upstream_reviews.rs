use crate::tests::{assertions, fixtures};

#[test]
fn accepts_valid_upstream_review_upgrade_action_and_rejects_stale_action() {
	let mut review = fixtures::valid_upstream_review();

	assertions::assert_errors(&review, []);

	review["next_actions"][0]["type"] = serde_json::json!("control_plane_upgrade_candidate");

	assertions::assert_errors(&review, []);

	review["next_actions"][0]["type"] = serde_json::json!("linear_followup");

	assertions::assert_errors(&review, ["next_actions[0].type must be one of"]);

	review["next_actions"][0]["type"] = serde_json::json!("publish_now");

	assertions::assert_errors(&review, ["next_actions[0].type must be one of"]);
}

#[test]
fn path_validation_accepts_historical_upstream_review_linear_followup_only_before_cutoff() {
	let mut review = fixtures::valid_upstream_review();

	review["reviewed_at"] = serde_json::json!("2026-06-11T20:07:07Z");
	review["next_actions"][0]["type"] = serde_json::json!("linear_followup");

	assertions::assert_errors(&review, ["next_actions[0].type must be one of"]);
	assertions::assert_path_errors(
		".agent/automations/radar/cache/github/reviews/openai-codex-pr-25018.review.json",
		&review,
		[],
	);

	review["reviewed_at"] = serde_json::json!("2026-06-12T00:00:00Z");

	assertions::assert_path_errors(
		".agent/automations/radar/cache/github/reviews/openai-codex-pr-25018.review.json",
		&review,
		["next_actions[0].type must be one of"],
	);
}

#[test]
fn accepts_valid_upstream_impact_and_rejects_bad_angle() {
	let mut impact = fixtures::valid_upstream_impact();

	assertions::assert_errors(&impact, []);

	impact["publisher_angle"] = serde_json::json!("viral_thread");

	assertions::assert_errors(&impact, ["publisher_angle must be one of"]);
}

#[test]
fn accepts_valid_control_plane_upgrade_candidate_and_rejects_direct_mutation() {
	let mut candidate = fixtures::valid_control_plane_upgrade_candidate();

	assertions::assert_errors(&candidate, []);

	candidate["authority"]["mutation_allowed"] = serde_json::json!(true);

	assertions::assert_errors(&candidate, ["authority.mutation_allowed must be false"]);

	let mut missing_shared_handoff = fixtures::valid_control_plane_upgrade_candidate();

	missing_shared_handoff["source_refs"]
		.as_object_mut()
		.expect("source refs should be an object")
		.remove("upstream_impacts");

	assertions::assert_errors(
		&missing_shared_handoff,
		["source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff"],
	);

	let mut missing_contract = fixtures::valid_control_plane_upgrade_candidate();

	missing_contract["authority"]["decision_contract_required"] = serde_json::json!(false);

	assertions::assert_errors(
		&missing_contract,
		["authority.decision_contract_required must be true"],
	);

	let mut missing_program = fixtures::valid_control_plane_upgrade_candidate();

	missing_program["authority"]
		.as_object_mut()
		.expect("authority should be an object")
		.remove("program_intake_required");

	assertions::assert_errors(&missing_program, ["authority.program_intake_required must be true"]);
}
