use crate::tests::{assertions, fixtures};

#[test]
fn accepts_valid_bundle_and_rejects_missing_commits() {
	let mut bundle = fixtures::valid_bundle();

	assertions::assert_errors(&bundle, []);

	bundle["commits"] = serde_json::json!([]);

	assertions::assert_errors(&bundle, ["commits must be a non-empty list"]);
}

#[test]
fn accepts_valid_signal_and_rejects_missing_try_effect() {
	let mut signal = fixtures::valid_signal();

	assertions::assert_errors(&signal, []);

	signal["kind"] = serde_json::json!("try_now");
	signal["how_to_try"] = serde_json::json!("Run radar validate.");

	assertions::assert_errors(&signal, ["expected_effect is required when how_to_try is present"]);
}

#[test]
fn path_validation_accepts_generated_analysis_drafts_without_schema() {
	let mut draft = serde_json::json!({
		"kind": "behavior_change",
		"title": "Remote control avoids duplicate account headers",
		"summary": "Merged PR centralizes remote-control HTTP auth header construction.",
		"why_it_matters": "Remote-control requests avoid duplicate account headers.",
		"confidence": "confirmed",
		"impact": "low",
		"proof_points": ["The source helper inserts the account header once."],
		"slug": "remote-control-account-header-deduped",
		"config_flags": [],
		"how_to_try": null,
		"expected_effect": null,
		"caveats": null,
		"watch_state": null
	});

	assertions::assert_errors(&draft, ["schema must be one of"]);
	assertions::assert_path_errors(
		".agent/automations/radar/cache/generated/analysis/openai-codex-pr-29893.analysis.json",
		&draft,
		[],
	);

	draft["proof_points"] = serde_json::json!([]);

	assertions::assert_path_errors(
		".agent/automations/radar/cache/generated/analysis/openai-codex-pr-29893.analysis.json",
		&draft,
		["proof_points must be a non-empty list"],
	);
}

#[test]
fn empty_json_values_do_not_satisfy_try_instructions_or_effects() {
	for empty in [
		serde_json::Value::Null,
		serde_json::json!(""),
		serde_json::json!(false),
		serde_json::json!(0),
		serde_json::json!(0.0),
		serde_json::json!([]),
		serde_json::json!({}),
	] {
		let mut signal = fixtures::valid_signal();
		signal["kind"] = serde_json::json!("try_now");
		signal["how_to_try"] = empty.clone();
		signal["expected_effect"] = serde_json::json!("A visible result");
		assertions::assert_errors(&signal, ["how_to_try is required"]);
		assert!(
			crate::validate_analysis_draft(&signal)
				.unwrap_err()
				.to_string()
				.contains("how_to_try is required")
		);

		signal["how_to_try"] = serde_json::json!("Run the example");
		signal["expected_effect"] = empty.clone();
		assertions::assert_errors(&signal, ["expected_effect is required"]);
		assert!(
			crate::validate_analysis_draft(&signal)
				.unwrap_err()
				.to_string()
				.contains("expected_effect is required")
		);

		signal["kind"] = serde_json::json!("capability");
		signal["how_to_try"] = empty.clone();
		signal["caveats"] = serde_json::json!([]);
		let rendered =
			crate::rendered_signal(&fixtures::valid_bundle(), &signal, None, vec![]).unwrap();
		for field in ["how_to_try", "expected_effect", "caveats"] {
			assert!(rendered.get(field).is_none(), "empty {field} should be omitted: {empty}");
		}
	}
}

#[test]
fn bundle_validation_checks_required_field_types_without_rejecting_empty_content() {
	let mut valid = fixtures::valid_bundle();
	valid["files"][0]["additions"] = serde_json::json!(0);
	valid["files"][0]["deletions"] = serde_json::json!(0);
	assertions::assert_errors(&valid, []);
	for (pointer, value, error) in [
		("/files/0/path", serde_json::Value::Null, "files[0].path must be a non-empty string"),
		("/files/0/status", serde_json::json!(42), "files[0].status must be a non-empty string"),
		(
			"/files/0/additions",
			serde_json::json!("0"),
			"files[0].additions must be a non-negative integer",
		),
		(
			"/files/0/deletions",
			serde_json::json!(-1),
			"files[0].deletions must be a non-negative integer",
		),
		(
			"/primary_pr/number",
			serde_json::Value::Null,
			"primary_pr.number must be a positive integer",
		),
		(
			"/primary_pr/number",
			serde_json::json!(0),
			"primary_pr.number must be a positive integer",
		),
		(
			"/primary_pr/number",
			serde_json::json!(u64::MAX),
			"primary_pr.number must be a positive integer",
		),
		("/primary_pr/title", serde_json::json!(""), "primary_pr.title must be a non-empty string"),
		("/primary_pr/body", serde_json::Value::Null, "primary_pr.body must be a string"),
		(
			"/primary_pr/state",
			serde_json::json!(false),
			"primary_pr.state must be a non-empty string",
		),
		("/primary_pr/labels", serde_json::json!([42]), "primary_pr.labels must be a list"),
		("/primary_pr/url", serde_json::Value::Null, "primary_pr.url must be a non-empty string"),
	] {
		let mut bundle = valid.clone();
		*bundle.pointer_mut(pointer).unwrap() = value;
		assertions::assert_errors(&bundle, [error]);
	}
	valid["analysis_mode"] = serde_json::json!("commit_only");
	valid["primary_pr"] = serde_json::Value::Null;
	assertions::assert_errors(&valid, []);
}
