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
