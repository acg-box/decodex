use crate::tests::support::{self};

#[test]
fn social_post_rejects_low_quality_public_text() {
	let mut attribution = support::valid_social_post();

	attribution["text"] = serde_json::json!(["Automated by @hackink: tracking this."]);

	support::assert_social_errors(
		&attribution,
		["text[0] must not include automation attribution"],
	);

	let mut generic = support::valid_social_post();

	generic["text"] = serde_json::json!(["Watching this."]);

	support::assert_social_errors(&generic, ["must name a concrete source-backed"]);
}

#[test]
fn accepts_valid_social_candidate_and_requires_shared_handoff_for_radar_inputs() {
	let mut candidate = support::valid_social_candidate();

	support::assert_social_errors(&candidate, []);

	candidate["source_refs"]
		.as_object_mut()
		.expect("source refs should be object")
		.remove("upstream_impacts");

	support::assert_social_errors(
		&candidate,
		["source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff"],
	);
}
