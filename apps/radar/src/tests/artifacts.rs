use super::*;

#[test]
fn accepts_valid_bundle_and_rejects_missing_commits() {
	let mut bundle = valid_bundle();

	assert_errors(&bundle, []);

	bundle["commits"] = serde_json::json!([]);

	assert_errors(&bundle, ["commits must be a non-empty list"]);
}

#[test]
fn accepts_valid_signal_and_rejects_missing_try_effect() {
	let mut signal = valid_signal();

	assert_errors(&signal, []);

	signal["kind"] = serde_json::json!("try_now");
	signal["how_to_try"] = serde_json::json!("Run radar validate.");

	assert_errors(&signal, ["expected_effect is required when how_to_try is present"]);
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

	assert_errors(&draft, ["schema must be one of"]);
	assert_path_errors(
		".agent/automations/decodex/cache/generated/analysis/openai-codex-pr-29893.analysis.json",
		&draft,
		[],
	);

	draft["proof_points"] = serde_json::json!([]);

	assert_path_errors(
		".agent/automations/decodex/cache/generated/analysis/openai-codex-pr-29893.analysis.json",
		&draft,
		["proof_points must be a non-empty list"],
	);
}

#[test]
fn rejects_current_multi_agent_v2_signal_assign_task_without_followup_context() {
	let mut signal = valid_signal();

	signal["title"] = serde_json::json!("MultiAgentV2 assign_task guidance");
	signal["summary"] =
		serde_json::json!("MultiAgentV2 operators should use assign_task for more work.");

	assert_errors(
		&signal,
		[
			"MultiAgentV2 assign_task must also mention current followup_task",
			"must describe assign_task as legacy",
		],
	);

	signal["summary"] = serde_json::json!(
		"MultiAgentV2 renamed the legacy assign_task trigger-turn tool to followup_task."
	);

	assert_errors(&signal, []);
}

#[test]
fn validates_multi_agent_v2_feature_catalog_reference() {
	let mut catalog = valid_config_feature_catalog();

	assert_errors(&catalog, []);

	catalog["features"][0]["reference_description"] =
		serde_json::json!("Enable MultiAgentV2 trigger-turn tool assign_task.");

	assert_errors(
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
		"automations/decodex/skills",
		"docs/reference",
		"docs/spec",
		"docs/runbook",
		"plugins/decodex/skills",
		"scripts",
		".agent/automations/decodex/cache/site-content/signals",
		".agent/automations/decodex/cache/generated",
		"site/src/lib",
	] {
		collect_assign_task_reference_violations(
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
	let mut first = valid_release_delta();
	let mut second = first.clone();

	first["generated_at"] = serde_json::json!("2026-06-01T00:00:00Z");
	second["generated_at"] = serde_json::json!("2026-06-02T00:00:00Z");

	assert_eq!(
		radar::material_json(&first, &RefreshKind::ReleaseDelta),
		radar::material_json(&second, &RefreshKind::ReleaseDelta)
	);

	second["stable_release"]["tag_name"] = serde_json::json!("rust-v0.1.1");

	assert_ne!(
		radar::material_json(&first, &RefreshKind::ReleaseDelta),
		radar::material_json(&second, &RefreshKind::ReleaseDelta)
	);
}

#[test]
fn rejects_duplicate_signal_slugs_across_files() {
	let signal = valid_signal();
	let mut state = crate::ValidationState::new();
	let mut errors = Vec::new();

	radar::validate_signal_slug_uniqueness(
		&PathBuf::from(".agent/automations/decodex/cache/site-content/signals/one.json"),
		&signal,
		&mut state,
		&mut errors,
	);
	radar::validate_signal_slug_uniqueness(
		&PathBuf::from(".agent/automations/decodex/cache/site-content/signals/two.json"),
		&signal,
		&mut state,
		&mut errors,
	);

	assert_eq!(errors.len(), 1);
	assert!(errors[0].contains("duplicate slug"));
}

#[test]
fn rejects_duplicate_terminal_social_post_idempotency_keys_across_files() {
	let social_post = valid_social_post();
	let mut state = crate::ValidationState::new();
	let mut errors = Vec::new();

	radar::validate_terminal_social_post_idempotency_key_uniqueness(
		&PathBuf::from(".agent/automations/decodex/cache/social/x/posts/one.json"),
		&social_post,
		&mut state,
		&mut errors,
	);
	radar::validate_terminal_social_post_idempotency_key_uniqueness(
		&PathBuf::from(".agent/automations/decodex/cache/social/x/posts/two.json"),
		&social_post,
		&mut state,
		&mut errors,
	);

	assert_eq!(errors.len(), 1);
	assert!(errors[0].contains("duplicate terminal social_post idempotency_key"));
}

#[test]
fn permits_failed_social_post_idempotency_key_retry() {
	let mut failed_post = valid_social_post();

	failed_post["status"] = serde_json::json!("failed");

	let published_post = valid_social_post();
	let mut state = crate::ValidationState::new();
	let mut errors = Vec::new();

	radar::validate_terminal_social_post_idempotency_key_uniqueness(
		&PathBuf::from(".agent/automations/decodex/cache/social/x/posts/failed.json"),
		&failed_post,
		&mut state,
		&mut errors,
	);
	radar::validate_terminal_social_post_idempotency_key_uniqueness(
		&PathBuf::from(".agent/automations/decodex/cache/social/x/posts/published.json"),
		&published_post,
		&mut state,
		&mut errors,
	);

	assert!(errors.is_empty());
}

#[test]
fn accepts_valid_social_publish_reservation() {
	let reservation = valid_social_publish_reservation();

	assert_errors(&reservation, []);
}

#[test]
fn accepts_valid_radar_archive_manifest() {
	let manifest = valid_radar_archive_manifest();

	assert_errors(&manifest, []);
}

#[test]
fn rejects_radar_archive_manifest_without_external_assets() {
	let mut manifest = valid_radar_archive_manifest();

	manifest["retention_days"] = serde_json::json!(30);

	manifest.as_object_mut().expect("manifest should be object").remove("archive_asset");

	assert_errors(&manifest, ["retention_days must be 21", "archive_asset must be an object"]);
}

#[test]
fn path_validation_accepts_historical_archive_retention_policy() {
	let mut manifest = valid_radar_archive_manifest();

	manifest["created_at"] = serde_json::json!("2026-05-13T07:52:56Z");
	manifest["retention_days"] = serde_json::json!(28);

	assert_errors(&manifest, ["retention_days must be 21"]);
	assert_path_errors(
		".agent/automations/decodex/cache/archive/index/2026-05-13-pre-2026-04-13.json",
		&manifest,
		[],
	);
}

#[test]
fn social_reserve_publish_dry_run_does_not_write() {
	let temp_dir = tempfile::tempdir().expect("temp dir should create");
	let request = social_reserve_request(temp_dir.path(), true);
	let report = radar::reserve_social_publish(&request).expect("dry-run reservation should pass");

	assert_eq!(report.status, "dry_run");
	assert!(
		!temp_dir.path().join("reservations/2026-06-02/openai-codex-pr-22414.json").exists(),
		"dry-run should not write reservation"
	);
}

#[test]
fn social_reserve_publish_writes_active_reservation_once() {
	let temp_dir = tempfile::tempdir().expect("temp dir should create");
	let request = social_reserve_request(temp_dir.path(), false);
	let report = radar::reserve_social_publish(&request).expect("reservation should pass");

	assert_eq!(report.status, "reserved");
	assert!(
		temp_dir.path().join("reservations/2026-06-02/openai-codex-pr-22414.json").exists(),
		"reservation should be written"
	);

	let duplicate = radar::reserve_social_publish(&request)
		.expect_err("duplicate reservation should fail closed")
		.to_string();

	assert!(duplicate.contains("idempotency_key already has an active reservation"));
}

#[test]
fn rejects_duplicate_active_social_publish_reservation_idempotency_keys() {
	let reservation = valid_social_publish_reservation();
	let mut state = crate::ValidationState::new();
	let mut errors = Vec::new();

	radar::validate_active_social_publish_reservation_uniqueness(
		&PathBuf::from(".agent/automations/decodex/cache/social/x/reservations/one.json"),
		&reservation,
		&mut state,
		&mut errors,
	);
	radar::validate_active_social_publish_reservation_uniqueness(
		&PathBuf::from(".agent/automations/decodex/cache/social/x/reservations/two.json"),
		&reservation,
		&mut state,
		&mut errors,
	);

	assert_eq!(errors.len(), 1);
	assert!(errors[0].contains("duplicate active social_publish_reservation"));
}

#[test]
fn rejects_active_reservation_for_terminal_social_post_idempotency_key() {
	let social_post = valid_social_post();
	let reservation = valid_social_publish_reservation();
	let mut state = crate::ValidationState::new();
	let mut errors = Vec::new();

	radar::validate_terminal_social_post_idempotency_key_uniqueness(
		&PathBuf::from(".agent/automations/decodex/cache/social/x/posts/published.json"),
		&social_post,
		&mut state,
		&mut errors,
	);
	radar::validate_active_social_publish_reservation_uniqueness(
		&PathBuf::from(".agent/automations/decodex/cache/social/x/reservations/active.json"),
		&reservation,
		&mut state,
		&mut errors,
	);

	assert_eq!(errors.len(), 1);
	assert!(errors[0].contains("conflicts with terminal social_post"));
}

#[test]
fn rejects_terminal_social_post_for_active_reservation_idempotency_key() {
	let social_post = valid_social_post();
	let reservation = valid_social_publish_reservation();
	let mut state = crate::ValidationState::new();
	let mut errors = Vec::new();

	radar::validate_active_social_publish_reservation_uniqueness(
		&PathBuf::from(".agent/automations/decodex/cache/social/x/reservations/active.json"),
		&reservation,
		&mut state,
		&mut errors,
	);
	radar::validate_terminal_social_post_idempotency_key_uniqueness(
		&PathBuf::from(".agent/automations/decodex/cache/social/x/posts/published.json"),
		&social_post,
		&mut state,
		&mut errors,
	);

	assert_eq!(errors.len(), 1);
	assert!(errors[0].contains("conflicts with active reservation"));
}

#[test]
fn accepts_valid_release_delta_and_rejects_missing_default_pair() {
	let mut release_delta = valid_release_delta();

	assert_errors(&release_delta, []);

	release_delta["comparisons"][0]["prerelease_tag_name"] =
		serde_json::json!("rust-v0.2.0-alpha.2");

	assert_errors(&release_delta, ["comparisons must include the default stable/prerelease pair"]);
}

#[test]
fn accepts_valid_review_queue_and_rejects_duplicate_subject() {
	let mut queue = valid_review_queue();

	assert_errors(&queue, []);

	queue["subjects"] = serde_json::json!([valid_queue_subject(), valid_queue_subject()]);
	queue["counts"]["subjects_queued"] = serde_json::json!(2);

	assert_errors(&queue, ["duplicates pr:22414"]);
}

#[test]
fn accepts_valid_upstream_review_upgrade_action_and_rejects_stale_action() {
	let mut review = valid_upstream_review();

	assert_errors(&review, []);

	review["next_actions"][0]["type"] = serde_json::json!("control_plane_upgrade_candidate");

	assert_errors(&review, []);

	review["next_actions"][0]["type"] = serde_json::json!("linear_followup");

	assert_errors(&review, ["next_actions[0].type must be one of"]);

	review["next_actions"][0]["type"] = serde_json::json!("publish_now");

	assert_errors(&review, ["next_actions[0].type must be one of"]);
}

#[test]
fn path_validation_accepts_historical_upstream_review_linear_followup_only_before_cutoff() {
	let mut review = valid_upstream_review();

	review["reviewed_at"] = serde_json::json!("2026-06-11T20:07:07Z");
	review["next_actions"][0]["type"] = serde_json::json!("linear_followup");

	assert_errors(&review, ["next_actions[0].type must be one of"]);
	assert_path_errors(
		".agent/automations/decodex/cache/github/reviews/openai-codex-pr-25018.review.json",
		&review,
		[],
	);

	review["reviewed_at"] = serde_json::json!("2026-06-12T00:00:00Z");

	assert_path_errors(
		".agent/automations/decodex/cache/github/reviews/openai-codex-pr-25018.review.json",
		&review,
		["next_actions[0].type must be one of"],
	);
}

#[test]
fn accepts_valid_social_candidate_and_rejects_missing_refs() {
	let mut candidate = valid_social_candidate();

	assert_errors(&candidate, []);

	candidate["source_refs"] = serde_json::json!({});

	assert_errors(&candidate, ["source_refs must include upstream_reviews"]);

	let mut missing_shared_handoff = valid_social_candidate();

	missing_shared_handoff["source_refs"]
		.as_object_mut()
		.expect("source refs should be an object")
		.remove("upstream_impacts");

	assert_errors(
		&missing_shared_handoff,
		["source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff"],
	);
}

#[test]
fn social_candidate_rejects_non_https_source_urls() {
	let mut candidate = valid_social_candidate();

	candidate["source_refs"]["urls"] = serde_json::json!(["http://example.test"]);

	assert_errors(&candidate, ["source_refs.urls must be a list of https URLs"]);
}

#[test]
fn social_candidate_rejects_low_quality_public_text() {
	let mut attribution = valid_social_candidate();

	attribution["candidate_text"] =
		serde_json::json!(["Automated by @hackink: new release available"]);

	assert_errors(&attribution, ["text[0] must not include automation attribution"]);

	let mut overpacked = valid_social_candidate();

	overpacked["candidate_text"] =
		serde_json::json!([format!("{}", "Codex checkpoint ".repeat(18))]);

	assert_errors(&overpacked, ["longer than 260 characters"]);

	let mut generic = valid_social_candidate();

	generic["candidate_text"] = serde_json::json!(["Watching this."]);

	assert_errors(&generic, ["must name a concrete source-backed"]);
}

#[test]
fn accepts_valid_upstream_impact_and_rejects_bad_angle() {
	let mut impact = valid_upstream_impact();

	assert_errors(&impact, []);

	impact["publisher_angle"] = serde_json::json!("viral_thread");

	assert_errors(&impact, ["publisher_angle must be one of"]);
}

#[test]
fn accepts_valid_control_plane_upgrade_candidate_and_rejects_direct_mutation() {
	let mut candidate = valid_control_plane_upgrade_candidate();

	assert_errors(&candidate, []);

	candidate["authority"]["mutation_allowed"] = serde_json::json!(true);

	assert_errors(&candidate, ["authority.mutation_allowed must be false"]);

	let mut missing_shared_handoff = valid_control_plane_upgrade_candidate();

	missing_shared_handoff["source_refs"]
		.as_object_mut()
		.expect("source refs should be an object")
		.remove("upstream_impacts");

	assert_errors(
		&missing_shared_handoff,
		["source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff"],
	);

	let mut missing_contract = valid_control_plane_upgrade_candidate();

	missing_contract["authority"]["decision_contract_required"] = serde_json::json!(false);

	assert_errors(&missing_contract, ["authority.decision_contract_required must be true"]);

	let mut missing_program = valid_control_plane_upgrade_candidate();

	missing_program["authority"]
		.as_object_mut()
		.expect("authority should be an object")
		.remove("program_intake_required");

	assert_errors(&missing_program, ["authority.program_intake_required must be true"]);
}

#[test]
fn accepts_valid_social_post_and_rejects_bad_daily_limit() {
	let mut social_post = valid_social_post();

	assert_errors(&social_post, []);

	social_post["decision"]["daily_limit"] = serde_json::json!(9);

	assert_errors(&social_post, ["decision.daily_limit must be 8"]);
}

#[test]
fn social_post_rejects_low_quality_public_text() {
	let mut attribution = valid_social_post();

	attribution["text"] = serde_json::json!(["Automated by @hackink: tracking this."]);

	assert_errors(&attribution, ["text[0] must not include automation attribution"]);

	let mut overpacked = valid_social_post();

	overpacked["text"] = serde_json::json!([format!("{}", "Codex checkpoint ".repeat(18))]);

	assert_errors(&overpacked, ["longer than 260 characters"]);

	let mut with_source_url = valid_social_post();

	with_source_url["text"] = serde_json::json!([format!(
		"{} https://github.com/openai/codex/pull/22414",
		"Codex checkpoint ".repeat(13)
	)]);

	assert_errors(&with_source_url, []);
}

#[test]
fn accepts_deleted_social_post_lifecycle_and_rejects_quote_eligible_deleted_post() {
	let mut social_post = valid_social_post();

	social_post["post_lifecycle"] = serde_json::json!({
		"current_state": "deleted_by_operator",
		"quote_eligible": false,
		"superseded_by_candidate": ".agent/automations/decodex/cache/github/social-candidates/openai-codex-alpha4.json",
		"reason": "The operator deleted this post and superseded it with a corrected candidate."
	});

	assert_errors(&social_post, []);

	social_post["post_lifecycle"]["quote_eligible"] = serde_json::json!(true);

	assert_errors(
		&social_post,
		["post_lifecycle.quote_eligible can be true only for live published posts"],
	);
}

#[test]
fn default_github_token_falls_back_to_workflow_token() {
	let _env = TestEnvVars::set(&[
		("GITHUB_PAT_X", Some("")),
		("GITHUB_PAT_Y", Some("")),
		("GITHUB_TOKEN", Some("workflow-token")),
	]);

	assert_eq!(super::super::github_token(None).as_deref(), Some("workflow-token"));
}

#[test]
fn explicit_github_token_env_does_not_fall_back_to_workflow_token() {
	let _env = TestEnvVars::set(&[
		("DECODEX_TEST_MISSING_RADAR_TOKEN", None),
		("GITHUB_TOKEN", Some("workflow-token")),
	]);

	assert_eq!(super::super::github_token(Some("DECODEX_TEST_MISSING_RADAR_TOKEN")), None);
}

#[test]
fn validates_json_files_from_directory() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let path = temp_dir.path().join("bundle.json");

	fs::write(&path, valid_bundle().to_string()).expect("fixture should be written");

	let report =
		radar::validate(&RadarValidateRequest { paths: vec![temp_dir.path().to_path_buf()] })
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

	fs::write(&bundle_path, valid_bundle().to_string()).expect("bundle should be written");
	fs::write(&analysis_path, analysis.to_string()).expect("analysis should be written");

	let report = radar::render_signal(&RadarRenderSignalRequest {
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
