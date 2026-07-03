use std::{
	fs,
	path::{Path, PathBuf},
};

use serde_json::Value;

use crate::{
	RadarRenderSignalRequest, RadarValidateRequest, RefreshKind, ValidationState,
	core_io::material_json,
	render_signal,
	tests::{self, TestEnvVars},
	validate, validate_signal_slug_uniqueness,
};

#[test]
fn accepts_valid_bundle_and_rejects_missing_commits() {
	let mut bundle = tests::valid_bundle();

	tests::assert_errors(&bundle, []);

	bundle["commits"] = serde_json::json!([]);

	tests::assert_errors(&bundle, ["commits must be a non-empty list"]);
}

#[test]
fn accepts_valid_signal_and_rejects_missing_try_effect() {
	let mut signal = tests::valid_signal();

	tests::assert_errors(&signal, []);

	signal["kind"] = serde_json::json!("try_now");
	signal["how_to_try"] = serde_json::json!("Run radar validate.");

	tests::assert_errors(&signal, ["expected_effect is required when how_to_try is present"]);
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

	tests::assert_errors(&draft, ["schema must be one of"]);
	tests::assert_path_errors(
		".agent/automations/radar/cache/generated/analysis/openai-codex-pr-29893.analysis.json",
		&draft,
		[],
	);

	draft["proof_points"] = serde_json::json!([]);

	tests::assert_path_errors(
		".agent/automations/radar/cache/generated/analysis/openai-codex-pr-29893.analysis.json",
		&draft,
		["proof_points must be a non-empty list"],
	);
}

#[test]
fn rejects_current_multi_agent_v2_signal_assign_task_without_followup_context() {
	let mut signal = tests::valid_signal();

	signal["title"] = serde_json::json!("MultiAgentV2 assign_task guidance");
	signal["summary"] =
		serde_json::json!("MultiAgentV2 operators should use assign_task for more work.");

	tests::assert_errors(
		&signal,
		[
			"MultiAgentV2 assign_task must also mention current followup_task",
			"must describe assign_task as legacy",
		],
	);

	signal["summary"] = serde_json::json!(
		"MultiAgentV2 renamed the legacy assign_task trigger-turn tool to followup_task."
	);

	tests::assert_errors(&signal, []);
}

#[test]
fn validates_multi_agent_v2_feature_catalog_reference() {
	let mut catalog = tests::valid_config_feature_catalog();

	tests::assert_errors(&catalog, []);

	catalog["features"][0]["reference_description"] =
		serde_json::json!("Enable MultiAgentV2 trigger-turn tool assign_task.");

	tests::assert_errors(
		&catalog,
		[
			"reference_description must mention current followup_task behavior",
			"reference_description must label assign_task as legacy or renamed context",
		],
	);
}

#[test]
fn current_multi_agent_v2_references_do_not_require_assign_task() {
	let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.and_then(Path::parent)
		.expect("apps/decodex should live two levels under the repo root");
	let mut offenders = Vec::new();

	for relative_root in [
		"README.md",
		"apps/decodex/src",
		"automations/radar/skills",
		"docs/reference",
		"docs/spec",
		"docs/runbook",
		"plugins/decodex/skills",
		"scripts",
		".agent/automations/radar/cache/site-content/signals",
		".agent/automations/radar/cache/generated",
		"site/src/lib",
	] {
		tests::collect_assign_task_reference_violations(
			&repo_root.join(relative_root),
			repo_root,
			&mut offenders,
		);
	}

	assert!(
		offenders.is_empty(),
		"current-facing MultiAgentV2 references must use followup_task and reserve \
			 assign_task for legacy or renamed context: {}",
		offenders.join(", ")
	);
}

#[test]
fn material_refresh_comparison_ignores_only_generated_at() {
	let mut first = tests::valid_release_delta();
	let mut second = first.clone();

	first["generated_at"] = serde_json::json!("2026-06-01T00:00:00Z");
	second["generated_at"] = serde_json::json!("2026-06-02T00:00:00Z");

	assert_eq!(
		self::material_json(&first, &RefreshKind::ReleaseDelta),
		self::material_json(&second, &RefreshKind::ReleaseDelta)
	);

	second["stable_release"]["tag_name"] = serde_json::json!("rust-v0.1.1");

	assert_ne!(
		self::material_json(&first, &RefreshKind::ReleaseDelta),
		self::material_json(&second, &RefreshKind::ReleaseDelta)
	);
}

#[test]
fn rejects_duplicate_signal_slugs_across_files() {
	let signal = tests::valid_signal();
	let mut state = ValidationState::new();
	let mut errors = Vec::new();

	self::validate_signal_slug_uniqueness(
		&PathBuf::from(".agent/automations/radar/cache/site-content/signals/one.json"),
		&signal,
		&mut state,
		&mut errors,
	);
	self::validate_signal_slug_uniqueness(
		&PathBuf::from(".agent/automations/radar/cache/site-content/signals/two.json"),
		&signal,
		&mut state,
		&mut errors,
	);

	assert_eq!(errors.len(), 1);
	assert!(errors[0].contains("duplicate slug"));
}

#[test]
fn accepts_valid_radar_archive_manifest() {
	let manifest = tests::valid_radar_archive_manifest();

	tests::assert_errors(&manifest, []);
}

#[test]
fn rejects_radar_archive_manifest_without_external_assets() {
	let mut manifest = tests::valid_radar_archive_manifest();

	manifest["retention_days"] = serde_json::json!(30);

	manifest.as_object_mut().expect("manifest should be object").remove("archive_asset");

	tests::assert_errors(
		&manifest,
		["retention_days must be 21", "archive_asset must be an object"],
	);
}

#[test]
fn path_validation_accepts_historical_archive_retention_policy() {
	let mut manifest = tests::valid_radar_archive_manifest();

	manifest["created_at"] = serde_json::json!("2026-05-13T07:52:56Z");
	manifest["retention_days"] = serde_json::json!(28);

	tests::assert_errors(&manifest, ["retention_days must be 21"]);
	tests::assert_path_errors(
		".agent/automations/radar/cache/archive/index/2026-05-13-pre-2026-04-13.json",
		&manifest,
		[],
	);
}

#[test]
fn accepts_valid_release_delta_and_rejects_missing_default_pair() {
	let mut release_delta = tests::valid_release_delta();

	tests::assert_errors(&release_delta, []);

	release_delta["comparisons"][0]["prerelease_tag_name"] =
		serde_json::json!("rust-v0.2.0-alpha.2");

	tests::assert_errors(
		&release_delta,
		["comparisons must include the default stable/prerelease pair"],
	);
}

#[test]
fn accepts_valid_review_queue_and_rejects_duplicate_subject() {
	let mut queue = tests::valid_review_queue();

	tests::assert_errors(&queue, []);

	queue["subjects"] =
		serde_json::json!([tests::valid_queue_subject(), tests::valid_queue_subject()]);
	queue["counts"]["subjects_queued"] = serde_json::json!(2);

	tests::assert_errors(&queue, ["duplicates pr:22414"]);
}

#[test]
fn accepts_valid_upstream_review_upgrade_action_and_rejects_stale_action() {
	let mut review = tests::valid_upstream_review();

	tests::assert_errors(&review, []);

	review["next_actions"][0]["type"] = serde_json::json!("control_plane_upgrade_candidate");

	tests::assert_errors(&review, []);

	review["next_actions"][0]["type"] = serde_json::json!("linear_followup");

	tests::assert_errors(&review, ["next_actions[0].type must be one of"]);

	review["next_actions"][0]["type"] = serde_json::json!("publish_now");

	tests::assert_errors(&review, ["next_actions[0].type must be one of"]);
}

#[test]
fn path_validation_accepts_historical_upstream_review_linear_followup_only_before_cutoff() {
	let mut review = tests::valid_upstream_review();

	review["reviewed_at"] = serde_json::json!("2026-06-11T20:07:07Z");
	review["next_actions"][0]["type"] = serde_json::json!("linear_followup");

	tests::assert_errors(&review, ["next_actions[0].type must be one of"]);
	tests::assert_path_errors(
		".agent/automations/radar/cache/github/reviews/openai-codex-pr-25018.review.json",
		&review,
		[],
	);

	review["reviewed_at"] = serde_json::json!("2026-06-12T00:00:00Z");

	tests::assert_path_errors(
		".agent/automations/radar/cache/github/reviews/openai-codex-pr-25018.review.json",
		&review,
		["next_actions[0].type must be one of"],
	);
}

#[test]
fn accepts_valid_upstream_impact_and_rejects_bad_angle() {
	let mut impact = tests::valid_upstream_impact();

	tests::assert_errors(&impact, []);

	impact["publisher_angle"] = serde_json::json!("viral_thread");

	tests::assert_errors(&impact, ["publisher_angle must be one of"]);
}

#[test]
fn accepts_valid_control_plane_upgrade_candidate_and_rejects_direct_mutation() {
	let mut candidate = tests::valid_control_plane_upgrade_candidate();

	tests::assert_errors(&candidate, []);

	candidate["authority"]["mutation_allowed"] = serde_json::json!(true);

	tests::assert_errors(&candidate, ["authority.mutation_allowed must be false"]);

	let mut missing_shared_handoff = tests::valid_control_plane_upgrade_candidate();

	missing_shared_handoff["source_refs"]
		.as_object_mut()
		.expect("source refs should be an object")
		.remove("upstream_impacts");

	tests::assert_errors(
		&missing_shared_handoff,
		["source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff"],
	);

	let mut missing_contract = tests::valid_control_plane_upgrade_candidate();

	missing_contract["authority"]["decision_contract_required"] = serde_json::json!(false);

	tests::assert_errors(&missing_contract, ["authority.decision_contract_required must be true"]);

	let mut missing_program = tests::valid_control_plane_upgrade_candidate();

	missing_program["authority"]
		.as_object_mut()
		.expect("authority should be an object")
		.remove("program_intake_required");

	tests::assert_errors(&missing_program, ["authority.program_intake_required must be true"]);
}

#[test]
fn default_github_token_falls_back_to_workflow_token() {
	let _env = TestEnvVars::set(&[
		("GITHUB_PAT_X", Some("")),
		("GITHUB_PAT_Y", Some("")),
		("GITHUB_TOKEN", Some("workflow-token")),
	]);

	assert_eq!(super::github_token(None).as_deref(), Some("workflow-token"));
}

#[test]
fn explicit_github_token_env_does_not_fall_back_to_workflow_token() {
	let _env = TestEnvVars::set(&[
		("DECODEX_TEST_MISSING_RADAR_TOKEN", None),
		("GITHUB_TOKEN", Some("workflow-token")),
	]);

	assert_eq!(super::github_token(Some("DECODEX_TEST_MISSING_RADAR_TOKEN")), None);
}

#[test]
fn validates_json_files_from_directory() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let path = temp_dir.path().join("bundle.json");

	fs::write(&path, tests::valid_bundle().to_string()).expect("fixture should be written");

	let report =
		self::validate(&RadarValidateRequest { paths: vec![temp_dir.path().to_path_buf()] })
			.expect("valid temporary bundle should pass");

	assert_eq!(report.checked_files, 1);
}

#[test]
fn renders_signal_from_bundle_and_analysis_fixture() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let bundle_path = temp_dir.path().join("bundle.json");
	let analysis_path = temp_dir.path().join("analysis.json");
	let signal_path = temp_dir.path().join("signal.json");
	let analysis = serde_json::json!({
		"kind": "capability",
		"title": "Unix sockets for remote Codex",
		"summary": "Remote Codex can use Unix socket endpoints.",
		"why_it_matters": "Operators can use local socket transports.",
		"confidence": "confirmed",
		"impact": "medium",
		"proof_points": ["PR #22414 adds endpoint handling."],
		"slug": null,
		"config_flags": [],
		"how_to_try": null,
		"expected_effect": null,
		"caveats": null,
		"watch_state": null
	});

	fs::write(&bundle_path, tests::valid_bundle().to_string()).expect("bundle should be written");
	fs::write(&analysis_path, analysis.to_string()).expect("analysis should be written");

	let report = self::render_signal(&RadarRenderSignalRequest {
		bundle: bundle_path,
		analysis: analysis_path,
		out: signal_path.clone(),
		published_at: None,
	})
	.expect("rendered signal should pass validation");
	let rendered: Value = serde_json::from_str(
		&fs::read_to_string(&signal_path).expect("rendered signal should be readable"),
	)
	.expect("rendered signal should parse");

	assert_eq!(report.out, signal_path);
	assert_eq!(rendered["schema"], "signal_entry/v1");
	assert_eq!(rendered["slug"], "unix-sockets-for-remote-codex");
	assert_eq!(rendered["published_at"], "2026-06-01T00:00:00Z");
	assert_eq!(rendered["source_refs"]["items"][0]["meta"], serde_json::json!("#22414"));
	assert_eq!(rendered["source_refs"]["items"][1]["meta"], "abc123");
	assert!(rendered.get("how_to_try").is_none());
}
