use crate::tests::{assertions, fixtures};

#[test]
fn accepts_valid_upstream_review_upgrade_action_and_rejects_stale_action() {
	let mut review = fixtures::valid_upstream_review();

	assertions::assert_errors(&review, []);

	review["next_actions"][0]["type"] = serde_json::json!("signal_entry");

	assertions::assert_errors(&review, []);

	review["next_actions"][0]["type"] = serde_json::json!("linear_followup");

	assertions::assert_errors(&review, ["next_actions[0].type must be one of"]);

	review["next_actions"][0]["type"] = serde_json::json!("publish_now");

	assertions::assert_errors(&review, ["next_actions[0].type must be one of"]);
}

#[test]
fn accepts_valid_upstream_impact_and_rejects_bad_angle() {
	let mut impact = fixtures::valid_upstream_impact();

	assertions::assert_errors(&impact, []);

	impact.as_object_mut().expect("impact should be an object").remove("reviewed_at");

	assertions::assert_errors(&impact, ["reviewed_at must be a non-empty string"]);

	impact["reviewed_at"] = serde_json::json!("not-a-timestamp");

	assertions::assert_errors(&impact, ["reviewed_at must be an RFC3339 timestamp"]);

	impact["reviewed_at"] = serde_json::json!("2026-06-01T00:00:00Z");
	impact["publisher_angle"] = serde_json::json!("viral_thread");

	assertions::assert_errors(&impact, ["publisher_angle must be one of"]);
}
