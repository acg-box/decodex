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
		.expect("apps/radar should live two levels under the repo root");
	let mut offenders = Vec::new();

	for relative_root in [
		"README.md",
		"apps/decodex-gpui/src",
		"crates/decodex-runtime/src",
		"automations/radar/skills",
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

#[test]
fn active_radar_surfaces_have_no_retired_contracts() {
	let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.and_then(std::path::Path::parent)
		.expect("workspace root should exist")
		.to_path_buf();
	let mut files = Vec::new();

	for relative in [
		"apps/radar",
		"apps/decodex-publisher",
		"automations/radar",
		"automations/decodex",
		"openwiki",
	] {
		collect_active_radar_files(&root.join(relative), &root, &mut files);
	}
	files.sort();
	files.dedup();

	for path in files {
		let relative =
			path.strip_prefix(&root).expect("active Radar file should be under the workspace root");
		if is_frozen_or_negative_fixture(relative) {
			continue;
		}
		let relative = relative.to_string_lossy();
		let text = std::fs::read_to_string(&path)
			.unwrap_or_else(|error| panic!("{relative} should be readable: {error}"));
		let lower = text.to_ascii_lowercase();

		for retired in [
			"archive_manifest",
			"archive manifest",
			"radar-archive",
			"ledger_export",
			"linear_followup",
			"github release assets",
			"release/archive state",
			"cold archived artifacts",
			"radar artifact release archives",
			"remote archive",
		] {
			assert!(
				!lower.contains(retired),
				"{relative} retains retired Radar contract {retired}"
			);
		}
	}

	assert!(!crate::REVIEW_STATUSES.contains(&"archived"));
	assert!(!crate::ARTIFACT_KINDS.contains(&"ledger_export"));
}

fn collect_active_radar_files(path: &Path, root: &Path, files: &mut Vec<PathBuf>) {
	let metadata = std::fs::symlink_metadata(path)
		.unwrap_or_else(|error| panic!("{} should be inspectable: {error}", path.display()));
	assert!(!metadata.file_type().is_symlink(), "{} must not be a symlink", path.display());

	if metadata.is_file() {
		let extension = path.extension().and_then(|value| value.to_str());
		if matches!(extension, Some("json" | "md" | "py" | "rs" | "toml")) {
			files.push(path.to_path_buf());
		}

		return;
	}

	for entry in std::fs::read_dir(path)
		.unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
	{
		let entry = entry.unwrap_or_else(|error| {
			panic!("{} should contain readable entries: {error}", path.display())
		});
		let relative = entry
			.path()
			.strip_prefix(root)
			.expect("active Radar entry should be under the workspace root")
			.to_path_buf();
		if !is_frozen_or_negative_fixture(&relative) {
			collect_active_radar_files(&entry.path(), root, files);
		}
	}
}

fn is_frozen_or_negative_fixture(relative: &Path) -> bool {
	const EXCLUDED: &[&str] = &[
		"openwiki/evidence",
		"apps/radar/src/tests/artifacts/catalog_reference.rs",
		"apps/radar/src/tests/artifacts/upstream_reviews.rs",
		"apps/radar/src/tests/automation/ledger/ledger_bootstrap_rejects_obsolete_schema.rs",
	];
	let relative = relative.to_string_lossy();

	EXCLUDED
		.iter()
		.any(|excluded| relative == *excluded || relative.starts_with(&format!("{excluded}/")))
}
