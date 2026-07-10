use std::path::{Path, PathBuf};

use crate::{
	RefreshKind, ValidationState, core_io,
	tests::{assertions, fixtures, reference_audit},
};

#[test]
fn rejects_current_multi_agent_v2_signal_assign_task_without_followup_context() {
	let mut signal = fixtures::valid_signal();

	signal["title"] = serde_json::json!("MultiAgentV2 assign_task guidance");
	signal["summary"] =
		serde_json::json!("MultiAgentV2 operators should use assign_task for more work.");

	assertions::assert_errors(
		&signal,
		[
			"MultiAgentV2 assign_task must also mention current followup_task",
			"must describe assign_task as legacy",
		],
	);

	signal["summary"] = serde_json::json!(
		"MultiAgentV2 renamed the legacy assign_task trigger-turn tool to followup_task."
	);

	assertions::assert_errors(&signal, []);
}

#[test]
fn validates_multi_agent_v2_feature_catalog_reference() {
	let mut catalog = fixtures::valid_config_feature_catalog();

	assertions::assert_errors(&catalog, []);

	catalog["features"][0]["reference_description"] =
		serde_json::json!("Enable MultiAgentV2 trigger-turn tool assign_task.");

	assertions::assert_errors(
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
		"plugins/decodex/skills",
		"scripts",
		".agent/automations/radar/cache/site-content/signals",
		".agent/automations/radar/cache/generated",
		"site/src/lib",
	] {
		reference_audit::collect_assign_task_reference_violations(
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
	let mut first = fixtures::valid_release_delta();
	let mut second = first.clone();

	first["generated_at"] = serde_json::json!("2026-06-01T00:00:00Z");
	second["generated_at"] = serde_json::json!("2026-06-02T00:00:00Z");

	assert_eq!(
		core_io::material_json(&first, &RefreshKind::ReleaseDelta),
		core_io::material_json(&second, &RefreshKind::ReleaseDelta)
	);

	second["stable_release"]["tag_name"] = serde_json::json!("rust-v0.1.1");

	assert_ne!(
		core_io::material_json(&first, &RefreshKind::ReleaseDelta),
		core_io::material_json(&second, &RefreshKind::ReleaseDelta)
	);
}

#[test]
fn rejects_duplicate_signal_slugs_across_files() {
	let signal = fixtures::valid_signal();
	let mut state = ValidationState::new();
	let mut errors = Vec::new();

	crate::validate_signal_slug_uniqueness(
		&PathBuf::from(".agent/automations/radar/cache/site-content/signals/one.json"),
		&signal,
		&mut state,
		&mut errors,
	);
	crate::validate_signal_slug_uniqueness(
		&PathBuf::from(".agent/automations/radar/cache/site-content/signals/two.json"),
		&signal,
		&mut state,
		&mut errors,
	);

	assert_eq!(errors.len(), 1);
	assert!(errors[0].contains("duplicate slug"));
}
