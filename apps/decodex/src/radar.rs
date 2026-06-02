//! Rust-owned Radar artifact contracts and file validation.

use std::{
	collections::{BTreeMap, BTreeSet},
	env,
	fmt::{self, Display, Formatter},
	fs,
	path::{Path, PathBuf},
	process::{self, Command},
};

use serde::Serialize;
use serde_json::{self, Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::prelude::eyre;

const BUNDLE_SCHEMA: &str = "github_change_bundle/v1";
const RELEASE_DELTA_SCHEMA: &str = "release_delta/v1";
const SIGNAL_SCHEMA: &str = "signal_entry/v1";
const SOCIAL_POST_SCHEMA: &str = "social_post/v1";
const UPSTREAM_IMPACT_SCHEMA: &str = "upstream_impact/v1";
const UPSTREAM_REVIEW_QUEUE_SCHEMA: &str = "upstream_review_queue/v1";
const UPSTREAM_REVIEW_SCHEMA: &str = "upstream_review/v1";
const DEFAULT_VALIDATION_PATHS: &[&str] = &[
	"artifacts/github/bundles",
	"artifacts/github/review-queue",
	"artifacts/github/reviews",
	"artifacts/github/impact",
	"artifacts/social/x",
	"site/src/content/signals",
	"site/src/content/release-deltas",
];
const ANALYSIS_MODES: &[&str] = &["commit_only", "pr_first"];
const SIGNAL_CONFIDENCE: &[&str] = &["confirmed", "likely", "weak"];
const SIGNAL_IMPACT: &[&str] = &["high", "low", "medium"];
const SIGNAL_KINDS: &[&str] = &["behavior_change", "capability", "try_now"];
const SOCIAL_BLOCK_REASONS: &[&str] =
	&["daily_cap_exceeded", "duplicate", "insufficient_evidence", "policy_block"];
const SOCIAL_POST_MODES: &[&str] = &[
	"operator_impact",
	"practical_explainer",
	"release_pulse",
	"release_rollup",
	"thread",
	"watch_note",
];
const SOCIAL_POST_PRIORITIES: &[&str] = &["critical", "high", "low", "normal"];
const SOCIAL_POST_STATUSES: &[&str] = &["blocked", "failed", "published", "skipped"];
const SOCIAL_POST_WORTHINESS: &[&str] = &["block", "publish", "skip"];
const SOURCE_ITEM_KINDS: &[&str] = &["commit", "pull_request"];
const UPSTREAM_IMPACT_KINDS: &[&str] =
	&["browser_observation", "changelog", "commit", "pull_request", "release", "signal"];
const UPSTREAM_REVIEW_ACTION_TYPES: &[&str] =
	&["linear_followup", "none", "signal_entry", "social_post", "upstream_impact"];
const UPSTREAM_REVIEW_NEXT_STEPS: &[&str] = &["ai_review_required"];
const UPSTREAM_REVIEW_PRIORITIES: &[&str] = &["critical", "high", "low", "normal"];
const UPSTREAM_SOURCE_STATES: &[&str] = &["closed", "commit_only", "merged", "open"];
const UPSTREAM_SUBJECT_KINDS: &[&str] = &["commit", "pr"];
const GENERIC_COMMIT_TITLES: &[&str] =
	&["update", "fix", "fix.", "fix tests", "fix tests.", "merge fixes", "flaky syntax"];
const CONFIG_FEATURE_CATALOG_PATH: &str = "site/src/generated/codex-config-features.json";
const BUILD_CHANGE_BUNDLE_SCRIPT: &str = "scripts/github/build_change_bundle.py";
const BUILD_RELEASE_DELTA_SCRIPT: &str = "scripts/github/build_release_delta.py";
const RUN_CODEX_ANALYSIS_SCRIPT: &str = "scripts/github/run_codex_analysis.py";

/// Request to validate Radar JSON artifacts.
#[derive(Debug)]
pub(crate) struct RadarValidateRequest {
	/// Explicit files or directories to validate. Defaults to current checked Radar collections.
	pub(crate) paths: Vec<PathBuf>,
}

/// Request to render one `signal_entry/v1` artifact from a bundle and analysis draft.
#[derive(Debug)]
pub(crate) struct RadarRenderSignalRequest {
	/// Path to a `github_change_bundle/v1` JSON artifact.
	pub(crate) bundle: PathBuf,
	/// Path to a Codex-owned `analysis_draft` JSON artifact.
	pub(crate) analysis: PathBuf,
	/// Path to write the rendered `signal_entry/v1` artifact.
	pub(crate) out: PathBuf,
	/// Optional publication timestamp override.
	pub(crate) published_at: Option<String>,
}

/// Summary of a rendered signal artifact.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RadarRenderSignalReport {
	/// Path that received the rendered signal artifact.
	pub(crate) out: PathBuf,
}

/// Request to backfill unpublished signals from a release-delta comparison window.
#[derive(Debug)]
pub(crate) struct RadarBackfillReleaseRangeRequest {
	/// GitHub repository in `owner/name` format.
	pub(crate) repo: String,
	/// Release-delta artifact to read or refresh.
	pub(crate) release_delta: PathBuf,
	/// Stable tag to use as the comparison start.
	pub(crate) stable_tag: Option<String>,
	/// Preview tag to use as the comparison end.
	pub(crate) preview_tag: Option<String>,
	/// Directory containing published signal entries.
	pub(crate) signals_dir: PathBuf,
	/// Directory for generated GitHub bundles.
	pub(crate) bundles_dir: PathBuf,
	/// Directory for Codex-owned analysis drafts.
	pub(crate) analysis_dir: PathBuf,
	/// Optional GitHub token environment variable name passed through to helper scripts.
	pub(crate) token_env: Option<String>,
	/// Codex executable to pass to the AI analysis boundary.
	pub(crate) codex_bin: String,
	/// Optional Codex model override for the AI analysis boundary.
	pub(crate) model: Option<String>,
	/// Optional PR count cap for partial runs.
	pub(crate) max_prs: Option<usize>,
	/// Print selected targets without writing generated content.
	pub(crate) dry_run: bool,
	/// Refresh the release-delta artifact into a temporary file before selecting targets.
	pub(crate) refresh_release_delta_first: bool,
	/// Stable release limit passed through only when refreshing first.
	pub(crate) refresh_stable_limit: Option<usize>,
	/// Preview release limit passed through only when refreshing first.
	pub(crate) refresh_preview_limit: Option<usize>,
	/// Compare-pair limit passed through only when refreshing first.
	pub(crate) refresh_pair_limit: Option<usize>,
	/// Python executable used for non-ported deterministic helpers and the AI boundary.
	pub(crate) python_bin: String,
}

/// Summary of a release-window backfill selection or run.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct RadarBackfillReleaseRangeReport {
	/// Stable tag selected for the comparison.
	pub(crate) stable_tag: String,
	/// Preview tag selected for the comparison.
	pub(crate) preview_tag: String,
	/// PR numbers selected for backfill.
	pub(crate) target_prs: Vec<u64>,
	/// Number of signal entries created by this run.
	pub(crate) created: usize,
	/// Whether the command only previewed targets.
	pub(crate) dry_run: bool,
}
impl Display for RadarBackfillReleaseRangeReport {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
		write!(formatter, "{}", serde_json::to_string_pretty(self).map_err(|_| fmt::Error)?)
	}
}

/// Summary of a Radar validation pass.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RadarValidationReport {
	/// Number of JSON files parsed and validated.
	pub(crate) checked_files: usize,
}

#[derive(Debug)]
struct ArtifactValidation {
	schema: Option<String>,
	errors: Vec<String>,
}

#[derive(Debug)]
struct ValidationState {
	seen_signal_slugs: BTreeMap<String, PathBuf>,
}
impl ValidationState {
	fn new() -> Self {
		Self { seen_signal_slugs: BTreeMap::new() }
	}
}

#[derive(Debug, Default)]
struct ReleaseOptionTags {
	stable: BTreeSet<String>,
	preview: BTreeSet<String>,
}

#[derive(Debug)]
struct PreparedReleaseDelta {
	path: PathBuf,
	cleanup_dir: Option<PathBuf>,
}
impl Drop for PreparedReleaseDelta {
	fn drop(&mut self) {
		if let Some(path) = &self.cleanup_dir {
			let _ = fs::remove_dir_all(path);
		}
	}
}

#[derive(Debug, Eq, PartialEq)]
struct ReleaseSelection {
	stable_tag: String,
	preview_tag: String,
	pr_numbers: Vec<u64>,
}

#[derive(Debug)]
struct BackfillPaths {
	bundle: PathBuf,
	analysis: PathBuf,
	signal: PathBuf,
}

/// Validate the requested Radar artifact paths.
pub(crate) fn validate(
	request: &RadarValidateRequest,
) -> crate::prelude::Result<RadarValidationReport> {
	let paths = validation_paths(&request.paths);
	let files = collect_json_files(&paths)?;
	let mut state = ValidationState::new();
	let mut errors = Vec::new();

	for path in &files {
		let payload = load_json(path)?;
		let validation = validate_artifact(&payload);

		if validation.schema.as_deref() == Some(SIGNAL_SCHEMA) {
			validate_signal_slug_uniqueness(path, &payload, &mut state, &mut errors);
		}

		for error in validation.errors {
			errors.push(format!("{}: {error}", path.display()));
		}
	}

	if errors.is_empty() {
		Ok(RadarValidationReport { checked_files: files.len() })
	} else {
		Err(eyre::eyre!("Radar validation failed:\n- {}", errors.join("\n- ")))
	}
}

/// Render one `signal_entry/v1` artifact from a validated bundle and analysis draft.
pub(crate) fn render_signal(
	request: &RadarRenderSignalRequest,
) -> crate::prelude::Result<RadarRenderSignalReport> {
	let bundle = load_json(&request.bundle)?;
	let analysis = load_json(&request.analysis)?;

	validate_expected_schema(&bundle, BUNDLE_SCHEMA, "Bundle")?;
	validate_analysis_draft(&analysis)?;

	let root = repo_root()?;
	let known_features = load_known_feature_names(&root)?;
	let config_flags = rendered_config_flags(&bundle, &analysis, &known_features);
	let signal =
		rendered_signal(&bundle, &analysis, request.published_at.as_deref(), config_flags)?;

	validate_expected_schema(&signal, SIGNAL_SCHEMA, "Signal")?;
	write_json(&request.out, &signal)?;

	Ok(RadarRenderSignalReport { out: request.out.clone() })
}

/// Select and optionally execute release-window signal backfills.
pub(crate) fn backfill_release_range(
	request: &RadarBackfillReleaseRangeRequest,
) -> crate::prelude::Result<RadarBackfillReleaseRangeReport> {
	let root = repo_root()?;
	let prepared_release_delta = prepare_release_delta_path(request, &root)?;
	let release_delta = load_json(&prepared_release_delta.path)?;
	let selection = selected_release_comparison(
		&release_delta,
		request.stable_tag.as_deref(),
		request.preview_tag.as_deref(),
	)?;
	let signals_dir = resolve_against(&root, &request.signals_dir);
	let published = published_pr_numbers(&signals_dir)?;
	let mut target_prs = selection
		.pr_numbers
		.into_iter()
		.filter(|number| !published.contains(number))
		.collect::<Vec<_>>();

	if let Some(limit) = request.max_prs {
		target_prs.truncate(limit);
	}

	let mut report = RadarBackfillReleaseRangeReport {
		stable_tag: selection.stable_tag,
		preview_tag: selection.preview_tag,
		target_prs,
		created: 0,
		dry_run: request.dry_run,
	};

	if request.dry_run {
		return Ok(report);
	}

	for pr_number in &report.target_prs {
		let paths = signal_backfill_paths(&request.repo, *pr_number, request);
		let note = format!(
			"Backfilled from release compare range {}...{}",
			report.stable_tag, report.preview_tag
		);

		run_build_change_bundle(&root, request, *pr_number, &paths.bundle, &note)?;
		run_codex_analysis(&root, request, &paths.bundle, &paths.analysis)?;
		render_signal(&RadarRenderSignalRequest {
			bundle: resolve_against(&root, &paths.bundle),
			analysis: resolve_against(&root, &paths.analysis),
			out: resolve_against(&root, &paths.signal),
			published_at: None,
		})?;

		report.created += 1;
	}

	validate(&RadarValidateRequest { paths: vec![resolve_against(&root, &request.signals_dir)] })?;
	run_build_release_delta(&root, request, &request.release_delta, false)?;

	Ok(report)
}

fn validate_expected_schema(
	value: &Value,
	schema: &str,
	label: &str,
) -> crate::prelude::Result<()> {
	let validation = validate_artifact(value);

	if validation.schema.as_deref() != Some(schema) {
		return Err(eyre::eyre!("{label} schema must be {schema}"));
	}
	if !validation.errors.is_empty() {
		return Err(eyre::eyre!(
			"{label} validation failed:\n- {}",
			validation.errors.join("\n- ")
		));
	}

	Ok(())
}

fn validate_analysis_draft(value: &Value) -> crate::prelude::Result<()> {
	let Some(draft) = value.as_object() else {
		return Err(eyre::eyre!("Analysis draft must be an object"));
	};
	let mut errors = Vec::new();

	for field in ["kind", "title", "summary", "why_it_matters", "confidence", "impact"] {
		if !is_non_empty_string(draft.get(field)) {
			errors.push(format!("{field} is required in analysis draft"));
		}
	}

	if !matches_one_of(draft.get("kind"), SIGNAL_KINDS) {
		errors.push(format!("kind must be one of {}", choices(SIGNAL_KINDS)));
	}
	if !matches_one_of(draft.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", choices(SIGNAL_CONFIDENCE)));
	}
	if !matches_one_of(draft.get("impact"), SIGNAL_IMPACT) {
		errors.push(format!("impact must be one of {}", choices(SIGNAL_IMPACT)));
	}
	if non_empty_array(draft.get("proof_points")).is_none() {
		errors.push("proof_points must be a non-empty list".into());
	}
	if string_field(draft, "kind") == Some("try_now")
		&& !is_truthy_json_value(draft.get("how_to_try"))
	{
		errors.push("how_to_try is required when kind is try_now".into());
	}
	if is_truthy_json_value(draft.get("how_to_try"))
		&& !is_truthy_json_value(draft.get("expected_effect"))
	{
		errors.push("expected_effect is required when how_to_try is present".into());
	}
	if errors.is_empty() {
		Ok(())
	} else {
		Err(eyre::eyre!("Analysis draft validation failed:\n- {}", errors.join("\n- ")))
	}
}

fn rendered_signal(
	bundle: &Value,
	analysis: &Value,
	published_at_override: Option<&str>,
	config_flags: Vec<String>,
) -> crate::prelude::Result<Value> {
	let bundle = bundle_object(bundle)?;
	let analysis = analysis_object(analysis)?;
	let title = required_string(analysis, "title", "analysis draft")?;
	let slug = string_field(analysis, "slug")
		.filter(|value| !value.is_empty())
		.map(str::to_owned)
		.unwrap_or_else(|| slugify(title));
	let mut signal = Map::new();

	signal.insert("schema".into(), serde_json::json!(SIGNAL_SCHEMA));
	signal.insert("slug".into(), serde_json::json!(slug));
	signal.insert("lane".into(), serde_json::json!("github"));

	for field in ["kind", "title", "summary", "why_it_matters", "confidence", "impact"] {
		signal.insert(
			field.into(),
			serde_json::json!(required_string(analysis, field, "analysis draft")?),
		);
	}

	signal.insert(
		"published_at".into(),
		serde_json::json!(pick_published_at(bundle, analysis, published_at_override)?),
	);
	signal.insert("config_flags".into(), serde_json::json!(config_flags));
	signal.insert(
		"proof_points".into(),
		analysis
			.get("proof_points")
			.cloned()
			.ok_or_else(|| eyre::eyre!("analysis draft proof_points is required"))?,
	);
	signal.insert("source_refs".into(), rendered_source_refs(bundle)?);

	for field in ["how_to_try", "expected_effect", "caveats", "watch_state"] {
		if is_truthy_json_value(analysis.get(field)) {
			signal.insert(
				field.into(),
				analysis
					.get(field)
					.cloned()
					.ok_or_else(|| eyre::eyre!("analysis draft {field} is required"))?,
			);
		}
	}

	Ok(Value::Object(signal))
}

fn bundle_object(value: &Value) -> crate::prelude::Result<&Map<String, Value>> {
	value.as_object().ok_or_else(|| eyre::eyre!("Bundle must be an object"))
}

fn analysis_object(value: &Value) -> crate::prelude::Result<&Map<String, Value>> {
	value.as_object().ok_or_else(|| eyre::eyre!("Analysis draft must be an object"))
}

fn required_string<'a>(
	object: &'a Map<String, Value>,
	field: &str,
	label: &str,
) -> crate::prelude::Result<&'a str> {
	string_field(object, field)
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("{label} {field} must be a non-empty string"))
}

fn rendered_config_flags(
	bundle: &Value,
	analysis: &Value,
	known_features: &BTreeSet<String>,
) -> Vec<String> {
	let raw_flags = analysis
		.get("config_flags")
		.filter(|value| !value.is_null())
		.or_else(|| bundle.get("extracted_flags"));

	normalized_config_flags(raw_flags, known_features)
}

fn normalized_config_flags(
	raw_flags: Option<&Value>,
	known_features: &BTreeSet<String>,
) -> Vec<String> {
	let Some(raw_flags) = raw_flags.and_then(Value::as_array) else {
		return Vec::new();
	};
	let mut normalized = Vec::new();
	let mut seen = BTreeSet::new();

	for flag in raw_flags {
		let Some(raw_value) = flag.as_str() else {
			continue;
		};
		let mut value = raw_value.trim().to_owned();

		if value.is_empty() || seen.contains(&value) {
			continue;
		}

		if let Some(feature_name) = normalize_feature_flag(&value, known_features) {
			value = format!("features.{feature_name} = true");
		}

		if !(value.starts_with("--")
			|| value.contains('=')
			|| value.ends_with(".json")
			|| value.ends_with(".toml"))
		{
			continue;
		}

		seen.insert(value.clone());
		normalized.push(value);
	}

	normalized
}

fn load_known_feature_names(root: &Path) -> crate::prelude::Result<BTreeSet<String>> {
	let path = root.join(CONFIG_FEATURE_CATALOG_PATH);

	if !path.exists() {
		return Ok(BTreeSet::new());
	}

	let payload = load_json(&path)?;
	let names = payload
		.get("features")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|item| item.get("name").and_then(Value::as_str))
		.filter(|name| !name.is_empty())
		.map(str::to_owned)
		.collect();

	Ok(names)
}

fn normalize_feature_flag(value: &str, known_features: &BTreeSet<String>) -> Option<String> {
	let lower = value.to_ascii_lowercase();

	if let Some(candidate) = lower.strip_prefix("--enable ") {
		let candidate = candidate.trim();

		return known_feature_name(candidate, known_features);
	}

	let candidate = lower.strip_prefix("features.").unwrap_or(&lower);
	let (name, enabled) =
		candidate.split_once('=').map_or((candidate.trim(), true), |(name, enabled)| {
			(name.trim(), enabled.trim() == "true")
		});

	enabled.then(|| known_feature_name(name, known_features)).flatten()
}

fn known_feature_name(value: &str, known_features: &BTreeSet<String>) -> Option<String> {
	let valid = !value.is_empty()
		&& value.chars().all(|character| {
			character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
		});

	if valid && known_features.contains(value) { Some(value.to_owned()) } else { None }
}

fn rendered_source_refs(bundle: &Map<String, Value>) -> crate::prelude::Result<Value> {
	let commits = bundle_commits(bundle)?;
	let commit_urls = commits
		.iter()
		.filter_map(|commit| commit.get("url").and_then(Value::as_str))
		.collect::<Vec<_>>();
	let mut refs = Map::new();

	refs.insert("repo".into(), serde_json::json!(required_string(bundle, "repo", "bundle")?));
	refs.insert("commit_urls".into(), serde_json::json!(commit_urls));
	refs.insert("items".into(), serde_json::json!(rendered_source_items(bundle)?));

	if let Some(pr_url) = bundle
		.get("primary_pr")
		.and_then(Value::as_object)
		.and_then(|primary_pr| string_field(primary_pr, "url"))
	{
		refs.insert("pr_url".into(), serde_json::json!(pr_url));
	}

	Ok(Value::Object(refs))
}

fn rendered_source_items(
	bundle: &Map<String, Value>,
) -> crate::prelude::Result<Vec<Map<String, Value>>> {
	let mut items = Vec::new();

	if let Some(primary_pr) = bundle.get("primary_pr").and_then(Value::as_object)
		&& let (Some(url), Some(title)) =
			(string_field(primary_pr, "url"), string_field(primary_pr, "title"))
	{
		let mut item = Map::new();

		item.insert("kind".into(), serde_json::json!("pull_request"));
		item.insert("title".into(), serde_json::json!(first_line(title)));
		item.insert("url".into(), serde_json::json!(url));

		if let Some(number) = primary_pr.get("number").and_then(Value::as_i64) {
			item.insert("meta".into(), serde_json::json!(format!("#{number}")));
		}

		items.push(item);
	}

	items.extend(rendered_commit_items(bundle)?);

	Ok(items)
}

fn rendered_commit_items(
	bundle: &Map<String, Value>,
) -> crate::prelude::Result<Vec<Map<String, Value>>> {
	let mut fallback_items = Vec::new();
	let mut picked_items = Vec::new();
	let mut seen_titles = BTreeSet::new();

	for commit in bundle_commits(bundle)? {
		let title = first_line(commit.get("message").and_then(Value::as_str).unwrap_or_default());

		if title.is_empty()
			|| !seen_titles.insert(title.clone())
			|| title.starts_with("Merge branch ")
		{
			continue;
		}

		let entry = rendered_commit_item(commit, &title)?;

		fallback_items.push(entry.clone());

		if !GENERIC_COMMIT_TITLES.contains(&title.to_ascii_lowercase().as_str()) {
			picked_items.push(entry);
		}
	}

	Ok(if picked_items.is_empty() { fallback_items } else { picked_items })
}

fn rendered_commit_item(
	commit: &Map<String, Value>,
	title: &str,
) -> crate::prelude::Result<Map<String, Value>> {
	let sha = required_string(commit, "sha", "bundle commit")?;
	let mut entry = Map::new();

	entry.insert("kind".into(), serde_json::json!("commit"));
	entry.insert("title".into(), serde_json::json!(title));
	entry.insert("url".into(), serde_json::json!(required_string(commit, "url", "bundle commit")?));
	entry.insert("meta".into(), serde_json::json!(short_sha(sha)));

	Ok(entry)
}

fn bundle_commits(bundle: &Map<String, Value>) -> crate::prelude::Result<Vec<&Map<String, Value>>> {
	bundle
		.get("commits")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("Bundle commits must be a list"))?
		.iter()
		.map(|commit| {
			commit.as_object().ok_or_else(|| eyre::eyre!("Bundle commit must be an object"))
		})
		.collect()
}

fn pick_published_at(
	bundle: &Map<String, Value>,
	analysis: &Map<String, Value>,
	override_value: Option<&str>,
) -> crate::prelude::Result<String> {
	if let Some(value) = override_value.filter(|value| !value.is_empty()) {
		return Ok(value.to_owned());
	}
	if let Some(value) = string_field(analysis, "published_at").filter(|value| !value.is_empty()) {
		return Ok(value.to_owned());
	}
	if let Some(value) = bundle
		.get("primary_pr")
		.and_then(Value::as_object)
		.and_then(|primary_pr| string_field(primary_pr, "merged_at"))
		.filter(|value| !value.is_empty())
	{
		return Ok(value.to_owned());
	}

	let first_commit = bundle_commits(bundle)?
		.into_iter()
		.next()
		.ok_or_else(|| eyre::eyre!("Bundle commits must be a non-empty list"))?;

	Ok(string_field(first_commit, "committed_at")
		.filter(|value| !value.is_empty())
		.map(str::to_owned)
		.unwrap_or_else(utc_now_iso))
}

fn short_sha(value: &str) -> String {
	value.chars().take(7).collect()
}

fn first_line(value: &str) -> String {
	value.trim().lines().next().unwrap_or_default().to_owned()
}

fn slugify(value: &str) -> String {
	let mut slug = String::new();
	let mut previous_was_separator = false;

	for character in value.chars().flat_map(char::to_lowercase) {
		if character.is_ascii_lowercase() || character.is_ascii_digit() {
			slug.push(character);

			previous_was_separator = false;
		} else if !previous_was_separator && !slug.is_empty() {
			slug.push('-');

			previous_was_separator = true;
		}
	}

	while slug.ends_with('-') {
		slug.pop();
	}

	if slug.is_empty() { "signal".into() } else { slug }
}

fn utc_now_iso() -> String {
	match OffsetDateTime::now_utc().replace_microsecond(0) {
		Ok(timestamp) =>
			timestamp.format(&Rfc3339).unwrap_or_else(|_| "1970-01-01T00:00:00Z".into()),
		Err(_) => "1970-01-01T00:00:00Z".into(),
	}
}

fn write_json(path: &Path, value: &Value) -> crate::prelude::Result<()> {
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)?;
	}

	fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;

	Ok(())
}

fn repo_root() -> crate::prelude::Result<PathBuf> {
	let mut candidate = env::current_dir()?;

	loop {
		if candidate.join("scripts/github/README.md").is_file()
			&& candidate.join("apps/decodex/src/radar.rs").is_file()
		{
			return Ok(candidate);
		}
		if !candidate.pop() {
			return Err(eyre::eyre!(
				"Unable to find Decodex repository root from current directory"
			));
		}
	}
}

fn selected_release_comparison(
	payload: &Value,
	stable_tag: Option<&str>,
	preview_tag: Option<&str>,
) -> crate::prelude::Result<ReleaseSelection> {
	validate_expected_schema(payload, RELEASE_DELTA_SCHEMA, "Release-delta")?;

	let entry =
		payload.as_object().ok_or_else(|| eyre::eyre!("Release-delta must be an object"))?;
	let target_stable = stable_tag
		.map(str::to_owned)
		.or_else(|| release_tag(entry.get("stable_release")))
		.ok_or_else(|| eyre::eyre!("stable release tag could not be selected"))?;
	let target_preview = preview_tag
		.map(str::to_owned)
		.or_else(|| release_tag(entry.get("prerelease")))
		.ok_or_else(|| eyre::eyre!("preview release tag could not be selected"))?;
	let comparisons = entry
		.get("comparisons")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("Release-delta comparisons must be a list"))?;

	for comparison in comparisons {
		let Some(comparison) = comparison.as_object() else {
			continue;
		};

		if string_field(comparison, "stable_tag_name") == Some(target_stable.as_str())
			&& string_field(comparison, "prerelease_tag_name") == Some(target_preview.as_str())
		{
			return Ok(ReleaseSelection {
				stable_tag: target_stable,
				preview_tag: target_preview,
				pr_numbers: comparison_pr_numbers(comparison),
			});
		}
	}

	Err(eyre::eyre!("No comparison found for {target_stable} -> {target_preview}"))
}

fn release_tag(value: Option<&Value>) -> Option<String> {
	value
		.and_then(Value::as_object)
		.and_then(|release| string_field(release, "tag_name"))
		.filter(|tag| !tag.is_empty())
		.map(str::to_owned)
}

fn comparison_pr_numbers(comparison: &Map<String, Value>) -> Vec<u64> {
	comparison
		.get("compare")
		.and_then(Value::as_object)
		.and_then(|compare| compare.get("pr_numbers"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_u64)
		.collect()
}

fn published_pr_numbers(signals_dir: &Path) -> crate::prelude::Result<BTreeSet<u64>> {
	let mut published = BTreeSet::new();
	let mut files = Vec::new();

	for entry in fs::read_dir(signals_dir)? {
		let path = entry?.path();

		if path.extension().is_some_and(|extension| extension == "json") {
			files.push(path);
		}
	}

	files.sort();

	for path in files {
		let payload = load_json(&path)?;

		validate_expected_schema(&payload, SIGNAL_SCHEMA, "Signal")?;

		if let Some(pr_number) = payload
			.get("source_refs")
			.and_then(Value::as_object)
			.and_then(|refs| string_field(refs, "pr_url"))
			.and_then(pr_number_from_url)
		{
			published.insert(pr_number);
		}
	}

	Ok(published)
}

fn pr_number_from_url(value: &str) -> Option<u64> {
	let marker = "/pull/";
	let index = value.rfind(marker)?;
	let number = &value[index + marker.len()..];

	(!number.is_empty() && number.chars().all(|character| character.is_ascii_digit()))
		.then(|| number.parse().ok())
		.flatten()
}

fn prepare_release_delta_path(
	request: &RadarBackfillReleaseRangeRequest,
	root: &Path,
) -> crate::prelude::Result<PreparedReleaseDelta> {
	if !request.refresh_release_delta_first {
		return Ok(PreparedReleaseDelta {
			path: resolve_against(root, &request.release_delta),
			cleanup_dir: None,
		});
	}

	let temp_root = env::temp_dir().join(format!(
		"decodex-prerelease-delta-{}-{}",
		process::id(),
		OffsetDateTime::now_utc().unix_timestamp_nanos()
	));

	fs::create_dir_all(&temp_root)?;

	let release_delta = temp_root.join("release-delta.json");

	run_build_release_delta(root, request, &release_delta, true)?;

	Ok(PreparedReleaseDelta { path: release_delta, cleanup_dir: Some(temp_root) })
}

fn run_build_change_bundle(
	root: &Path,
	request: &RadarBackfillReleaseRangeRequest,
	pr_number: u64,
	out: &Path,
	note: &str,
) -> crate::prelude::Result<()> {
	let mut command = helper_command(root, request, BUILD_CHANGE_BUNDLE_SCRIPT);

	command.args([
		"--repo",
		request.repo.as_str(),
		"--pr",
		&pr_number.to_string(),
		"--out",
		&path_arg(root, out),
		"--note",
		note,
	]);

	if let Some(token_env) = &request.token_env {
		command.args(["--token-env", token_env]);
	}

	run_helper(command, BUILD_CHANGE_BUNDLE_SCRIPT)
}

fn run_codex_analysis(
	root: &Path,
	request: &RadarBackfillReleaseRangeRequest,
	bundle: &Path,
	out: &Path,
) -> crate::prelude::Result<()> {
	let mut command = helper_command(root, request, RUN_CODEX_ANALYSIS_SCRIPT);

	command.args([
		"--bundle",
		&path_arg(root, bundle),
		"--out",
		&path_arg(root, out),
		"--repo-root",
		&root.display().to_string(),
		"--codex-bin",
		request.codex_bin.as_str(),
	]);

	if let Some(model) = &request.model {
		command.args(["--model", model]);
	}

	run_helper(command, RUN_CODEX_ANALYSIS_SCRIPT)
}

fn run_build_release_delta(
	root: &Path,
	request: &RadarBackfillReleaseRangeRequest,
	out: &Path,
	include_refresh_limits: bool,
) -> crate::prelude::Result<()> {
	let mut command = helper_command(root, request, BUILD_RELEASE_DELTA_SCRIPT);

	command.args([
		"--repo",
		request.repo.as_str(),
		"--signals-dir",
		&path_arg(root, &request.signals_dir),
		"--out",
		&path_arg(root, out),
	]);

	if let Some(token_env) = &request.token_env {
		command.args(["--token-env", token_env]);
	}

	if include_refresh_limits {
		push_optional_limit(&mut command, "--stable-limit", request.refresh_stable_limit);
		push_optional_limit(&mut command, "--preview-limit", request.refresh_preview_limit);
		push_optional_limit(&mut command, "--pair-limit", request.refresh_pair_limit);
	}

	run_helper(command, BUILD_RELEASE_DELTA_SCRIPT)
}

fn push_optional_limit(command: &mut Command, flag: &str, value: Option<usize>) {
	if let Some(value) = value {
		command.args([flag, &value.to_string()]);
	}
}

fn helper_command(
	root: &Path,
	request: &RadarBackfillReleaseRangeRequest,
	script: &str,
) -> Command {
	let mut command = Command::new(&request.python_bin);

	command.current_dir(root).arg(root.join(script));

	command
}

fn run_helper(mut command: Command, script: &str) -> crate::prelude::Result<()> {
	let output = command.output()?;

	if output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
	let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
	let details = if !stderr.is_empty() {
		stderr
	} else if !stdout.is_empty() {
		stdout
	} else {
		"unknown error".into()
	};

	Err(eyre::eyre!("{script} failed: {details}"))
}

fn signal_backfill_paths(
	repo: &str,
	pr_number: u64,
	request: &RadarBackfillReleaseRangeRequest,
) -> BackfillPaths {
	let stem = format!("{}-pr-{pr_number}", repo_path_stem(repo));

	BackfillPaths {
		bundle: request.bundles_dir.join(format!("{stem}.json")),
		analysis: request.analysis_dir.join(format!("{stem}.analysis.json")),
		signal: request.signals_dir.join(format!("{stem}.json")),
	}
}

fn repo_path_stem(repo: &str) -> String {
	repo.chars()
		.map(
			|character| {
				if character.is_ascii_alphanumeric() { character.to_ascii_lowercase() } else { '-' }
			},
		)
		.collect::<String>()
		.trim_matches('-')
		.to_owned()
}

fn resolve_against(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() { path.to_path_buf() } else { root.join(path) }
}

fn path_arg(root: &Path, path: &Path) -> String {
	path.strip_prefix(root).unwrap_or(path).display().to_string()
}

fn validation_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
	if paths.is_empty() {
		DEFAULT_VALIDATION_PATHS.iter().map(PathBuf::from).collect()
	} else {
		paths.to_vec()
	}
}

fn collect_json_files(paths: &[PathBuf]) -> crate::prelude::Result<Vec<PathBuf>> {
	let mut files = Vec::new();

	for path in paths {
		collect_json_path(path, &mut files)?;
	}

	files.sort();

	Ok(files)
}

fn collect_json_path(path: &Path, files: &mut Vec<PathBuf>) -> crate::prelude::Result<()> {
	if path.is_dir() {
		let mut children = fs::read_dir(path)?
			.map(|entry| entry.map(|entry| entry.path()))
			.collect::<std::result::Result<Vec<_>, _>>()?;

		children.sort();

		for child in children {
			collect_json_path(&child, files)?;
		}
	} else if path.is_file() {
		if path.extension().is_some_and(|extension| extension == "json") {
			files.push(path.to_path_buf());
		}
	} else {
		return Err(eyre::eyre!("Radar validation path does not exist: {}", path.display()));
	}

	Ok(())
}

fn load_json(path: &Path) -> crate::prelude::Result<Value> {
	let raw = fs::read_to_string(path)?;

	serde_json::from_str(&raw)
		.map_err(|error| eyre::eyre!("Failed to parse JSON from {}: {error}", path.display()))
}

fn validate_artifact(payload: &Value) -> ArtifactValidation {
	let Some(entry) = payload.as_object() else {
		return ArtifactValidation {
			schema: None,
			errors: vec!["artifact must be an object".into()],
		};
	};
	let schema = entry.get("schema").and_then(Value::as_str).map(str::to_owned);
	let mut errors = Vec::new();

	match schema.as_deref() {
		Some(BUNDLE_SCHEMA) => validate_bundle(entry, &mut errors),
		Some(RELEASE_DELTA_SCHEMA) => validate_release_delta(entry, &mut errors),
		Some(SIGNAL_SCHEMA) => validate_signal(entry, &mut errors),
		Some(SOCIAL_POST_SCHEMA) => validate_social_post(entry, &mut errors),
		Some(UPSTREAM_IMPACT_SCHEMA) => validate_upstream_impact(entry, &mut errors),
		Some(UPSTREAM_REVIEW_QUEUE_SCHEMA) => validate_upstream_review_queue(entry, &mut errors),
		Some(UPSTREAM_REVIEW_SCHEMA) => validate_upstream_review(entry, &mut errors),
		Some(_) | None => errors.push(format!("schema must be one of {}", known_schemas())),
	}

	ArtifactValidation { schema, errors }
}

fn validate_bundle(bundle: &Map<String, Value>, errors: &mut Vec<String>) {
	if string_field(bundle, "repo").is_none_or(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if !matches_one_of(bundle.get("analysis_mode"), ANALYSIS_MODES) {
		errors.push(format!("analysis_mode must be one of {}", choices(ANALYSIS_MODES)));
	}
	if !is_non_empty_string(bundle.get("default_branch")) {
		errors.push("default_branch must be a non-empty string".into());
	}

	validate_bundle_commits(bundle.get("commits"), errors);
	validate_bundle_files(bundle.get("files"), errors);

	if string_field(bundle, "analysis_mode") == Some("pr_first") {
		validate_bundle_pr(bundle.get("primary_pr"), errors);
	}
}

fn validate_bundle_commits(commits: Option<&Value>, errors: &mut Vec<String>) {
	let Some(commits) = non_empty_array(commits) else {
		errors.push("commits must be a non-empty list".into());

		return;
	};

	for (index, commit) in commits.iter().enumerate() {
		let Some(commit) = commit.as_object() else {
			errors.push(format!("commits[{index}] must be an object"));

			continue;
		};

		for field in ["sha", "message", "url"] {
			if !is_non_empty_string(commit.get(field)) {
				errors.push(format!("commits[{index}].{field} must be a non-empty string"));
			}
		}
	}
}

fn validate_bundle_files(files: Option<&Value>, errors: &mut Vec<String>) {
	let Some(files) = non_empty_array(files) else {
		errors.push("files must be a non-empty list".into());

		return;
	};

	for (index, item) in files.iter().enumerate() {
		let Some(item) = item.as_object() else {
			errors.push(format!("files[{index}] must be an object"));

			continue;
		};

		for field in ["path", "status", "additions", "deletions"] {
			if !item.contains_key(field) {
				errors.push(format!("files[{index}].{field} is required"));
			}
		}
	}
}

fn validate_bundle_pr(primary_pr: Option<&Value>, errors: &mut Vec<String>) {
	let Some(primary_pr) = primary_pr.and_then(Value::as_object) else {
		errors.push("primary_pr is required when analysis_mode is pr_first".into());

		return;
	};

	for field in ["number", "title", "body", "state", "labels", "url"] {
		if !primary_pr.contains_key(field) {
			errors.push(format!("primary_pr.{field} is required"));
		}
	}
}

fn validate_signal(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if string_field(entry, "lane") != Some("github") {
		errors.push("lane must be github for the MVP".into());
	}
	if !matches_one_of(entry.get("kind"), SIGNAL_KINDS) {
		errors.push(format!("kind must be one of {}", choices(SIGNAL_KINDS)));
	}
	if !matches_one_of(entry.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", choices(SIGNAL_CONFIDENCE)));
	}
	if !matches_one_of(entry.get("impact"), SIGNAL_IMPACT) {
		errors.push(format!("impact must be one of {}", choices(SIGNAL_IMPACT)));
	}

	for field in ["slug", "title", "published_at", "summary", "why_it_matters"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	validate_signal_lists(entry, errors);
	validate_signal_try_fields(entry, errors);
	validate_signal_source_refs(entry.get("source_refs"), errors);
}

fn validate_signal_lists(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if non_empty_array(entry.get("proof_points")).is_none() {
		errors.push("proof_points must be a non-empty list".into());
	}
}

fn validate_signal_try_fields(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let config_flags_present = optional_array(entry.get("config_flags"), "config_flags", errors)
		.is_some_and(|values| !values.is_empty());
	let how_to_try = entry.get("how_to_try");

	if (string_field(entry, "kind") == Some("try_now") || config_flags_present)
		&& !is_truthy_json_value(how_to_try)
	{
		errors.push("how_to_try is required for try_now or flag-backed entries".into());
	}
	if is_truthy_json_value(how_to_try) && !is_truthy_json_value(entry.get("expected_effect")) {
		errors.push("expected_effect is required when how_to_try is present".into());
	}

	validate_optional_string_list(entry.get("caveats"), "caveats", errors);

	let watch_state = entry.get("watch_state");

	if watch_state.is_some() && !watch_state.is_some_and(|value| is_non_empty_string(Some(value))) {
		errors.push("watch_state must be a non-empty string when present".into());
	}
}

fn validate_signal_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};

	if string_field(refs, "repo").is_none_or(|repo| !repo.contains('/')) {
		errors.push("source_refs.repo must be owner/name".into());
	}

	validate_signal_source_items(refs.get("items"), errors);

	let pr_url = refs.get("pr_url");
	let commit_urls = refs.get("commit_urls");
	let items = refs.get("items");

	if pr_url.is_none()
		&& is_empty_or_missing_array(commit_urls)
		&& is_empty_or_missing_array(items)
	{
		errors.push("source_refs must include pr_url, commit URLs, or source_refs.items".into());
	}
	if pr_url.is_some_and(|url| !is_https_string(Some(url))) {
		errors.push("source_refs.pr_url must be an https URL when present".into());
	}
	if commit_urls.is_some_and(|urls| !is_https_string_array(urls)) {
		errors.push("source_refs.commit_urls must be a list of https URLs".into());
	}
}

fn validate_signal_source_items(items: Option<&Value>, errors: &mut Vec<String>) {
	let Some(items) = items else {
		return;
	};

	if items.as_array().is_some_and(Vec::is_empty) {
		return;
	}

	let valid = items.as_array().is_some_and(|items| {
		items.iter().all(|item| {
			item.as_object().is_some_and(|item| {
				matches_one_of(item.get("kind"), SOURCE_ITEM_KINDS)
					&& is_non_empty_string(item.get("title"))
					&& is_https_string(item.get("url"))
					&& item.get("meta").is_none_or(|meta| meta.as_str().is_some())
			})
		})
	});

	if !valid {
		errors.push("source_refs.items must be a list of titled source entries".into());
	}
}

fn validate_release_delta(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if string_field(entry, "repo").is_none_or(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if !is_non_empty_string(entry.get("tag_prefix")) {
		errors.push("tag_prefix must be a non-empty string".into());
	}
	if !is_non_empty_string(entry.get("generated_at")) {
		errors.push("generated_at must be a non-empty string".into());
	}

	let tag_prefix = string_field(entry, "tag_prefix").unwrap_or_default();

	validate_release_object(
		entry.get("stable_release"),
		"stable_release",
		tag_prefix,
		false,
		errors,
	);
	validate_release_object(entry.get("prerelease"), "prerelease", tag_prefix, true, errors);
	validate_compare_object(entry.get("compare"), "compare", errors);
	validate_string_list(entry.get("tracked_signal_slugs"), "tracked_signal_slugs", errors);

	let option_tags = validate_release_options(entry.get("release_options"), errors);

	validate_release_comparisons(entry, &option_tags, errors);
}

fn validate_release_object(
	release: Option<&Value>,
	field_name: &str,
	tag_prefix: &str,
	expect_prerelease: bool,
	errors: &mut Vec<String>,
) {
	let Some(release) = release.and_then(Value::as_object) else {
		errors.push(format!("{field_name} must be an object"));

		return;
	};

	for field in ["tag_name", "name", "published_at", "url"] {
		if !is_non_empty_string(release.get(field)) {
			errors.push(format!("{field_name}.{field} must be a non-empty string"));
		}
	}

	if string_field(release, "tag_name").is_some_and(|tag_name| !tag_name.starts_with(tag_prefix)) {
		errors.push(format!("{field_name}.tag_name must start with tag_prefix"));
	}
	if release.get("prerelease").and_then(Value::as_bool) != Some(expect_prerelease) {
		let expected = if expect_prerelease { "true" } else { "false" };

		errors.push(format!("{field_name}.prerelease must be {expected}"));
	}
}

fn validate_compare_object(compare: Option<&Value>, label: &str, errors: &mut Vec<String>) {
	let Some(compare) = compare.and_then(Value::as_object) else {
		errors.push(format!("{label} must be an object"));

		return;
	};

	if !is_non_empty_string(compare.get("status")) {
		errors.push(format!("{label}.status must be a non-empty string"));
	}

	for field in ["ahead_by", "total_commits"] {
		if compare.get(field).and_then(Value::as_i64).is_none() {
			errors.push(format!("{label}.{field} must be an integer"));
		}
	}

	if !is_https_string(compare.get("url")) {
		errors.push(format!("{label}.url must be an https URL"));
	}

	validate_optional_string_list(
		compare.get("commit_shas"),
		&format!("{label}.commit_shas"),
		errors,
	);
	validate_optional_positive_integer_list(
		compare.get("pr_numbers"),
		&format!("{label}.pr_numbers"),
		errors,
	);
}

fn validate_release_options(
	options: Option<&Value>,
	errors: &mut Vec<String>,
) -> ReleaseOptionTags {
	let mut tags = ReleaseOptionTags::default();
	let Some(options) = options.and_then(Value::as_object) else {
		errors.push("release_options must be an object".into());

		return tags;
	};

	validate_release_option_group(
		options.get("stable"),
		"release_options.stable",
		false,
		errors,
		&mut tags.stable,
	);
	validate_release_option_group(
		options.get("preview"),
		"release_options.preview",
		true,
		errors,
		&mut tags.preview,
	);

	tags
}

fn validate_release_option_group(
	values: Option<&Value>,
	label: &str,
	expect_prerelease: bool,
	errors: &mut Vec<String>,
	tags: &mut BTreeSet<String>,
) {
	let Some(values) = non_empty_array(values) else {
		errors.push(format!("{label} must be a non-empty list"));

		return;
	};

	for (index, release) in values.iter().enumerate() {
		let Some(release) = release.as_object() else {
			errors.push(format!("{label}[{index}] must be an object"));

			continue;
		};

		if let Some(tag_name) = string_field(release, "tag_name") {
			if tag_name.is_empty() {
				errors.push(format!("{label}[{index}].tag_name must be a non-empty string"));
			} else {
				tags.insert(tag_name.to_owned());
			}
		} else {
			errors.push(format!("{label}[{index}].tag_name must be a non-empty string"));
		}

		if release.get("prerelease").and_then(Value::as_bool) != Some(expect_prerelease) {
			let expected = if expect_prerelease { "true" } else { "false" };

			errors.push(format!("{label}[{index}].prerelease must be {expected}"));
		}
	}
}

fn validate_release_comparisons(
	entry: &Map<String, Value>,
	option_tags: &ReleaseOptionTags,
	errors: &mut Vec<String>,
) {
	let Some(comparisons) = non_empty_array(entry.get("comparisons")) else {
		errors.push("comparisons must be a non-empty list".into());

		return;
	};
	let stable_release = entry.get("stable_release").and_then(Value::as_object);
	let prerelease = entry.get("prerelease").and_then(Value::as_object);
	let mut has_default_comparison = false;

	for (index, comparison) in comparisons.iter().enumerate() {
		let Some(comparison) = comparison.as_object() else {
			errors.push(format!("comparisons[{index}] must be an object"));

			continue;
		};

		validate_release_comparison_tags(comparison, index, option_tags, errors);

		if comparison_matches_default(comparison, stable_release, prerelease) {
			has_default_comparison = true;
		}

		validate_compare_object(
			comparison.get("compare"),
			&format!("comparisons[{index}].compare"),
			errors,
		);
		validate_string_list(
			comparison.get("tracked_signal_slugs"),
			&format!("comparisons[{index}].tracked_signal_slugs"),
			errors,
		);
	}

	if !has_default_comparison {
		errors.push("comparisons must include the default stable/prerelease pair".into());
	}
}

fn validate_release_comparison_tags(
	comparison: &Map<String, Value>,
	index: usize,
	option_tags: &ReleaseOptionTags,
	errors: &mut Vec<String>,
) {
	match string_field(comparison, "stable_tag_name") {
		Some("") =>
			errors.push(format!("comparisons[{index}].stable_tag_name must be a non-empty string")),
		Some(tag_name)
			if !option_tags.stable.is_empty() && !option_tags.stable.contains(tag_name) =>
			errors.push(format!(
				"comparisons[{index}].stable_tag_name must exist in release_options.stable"
			)),
		Some(_) => {},
		None =>
			errors.push(format!("comparisons[{index}].stable_tag_name must be a non-empty string")),
	}
	match string_field(comparison, "prerelease_tag_name") {
		Some("") => errors
			.push(format!("comparisons[{index}].prerelease_tag_name must be a non-empty string")),
		Some(tag_name)
			if !option_tags.preview.is_empty() && !option_tags.preview.contains(tag_name) =>
			errors.push(format!(
				"comparisons[{index}].prerelease_tag_name must exist in release_options.preview"
			)),
		Some(_) => {},
		None => errors
			.push(format!("comparisons[{index}].prerelease_tag_name must be a non-empty string")),
	}
}

fn comparison_matches_default(
	comparison: &Map<String, Value>,
	stable_release: Option<&Map<String, Value>>,
	prerelease: Option<&Map<String, Value>>,
) -> bool {
	let stable_tag = stable_release.and_then(|release| string_field(release, "tag_name"));
	let prerelease_tag = prerelease.and_then(|release| string_field(release, "tag_name"));

	string_field(comparison, "stable_tag_name") == stable_tag
		&& string_field(comparison, "prerelease_tag_name") == prerelease_tag
}

fn validate_upstream_review_queue(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if string_field(entry, "repo").is_none_or(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}
	if !is_non_empty_string(entry.get("generated_at")) {
		errors.push("generated_at must be a non-empty string".into());
	}

	validate_upstream_review_queue_source(entry.get("source"), errors);

	let subjects = validate_upstream_review_subjects(entry.get("subjects"), errors);

	validate_upstream_review_counts(entry.get("counts"), subjects, errors);
}

fn validate_upstream_review_queue_source(source: Option<&Value>, errors: &mut Vec<String>) {
	let Some(source) = source.and_then(Value::as_object) else {
		errors.push("source must be an object".into());

		return;
	};

	if !is_non_empty_string(source.get("default_branch")) {
		errors.push("source.default_branch must be a non-empty string".into());
	}
	if source.get("search_limit").and_then(Value::as_i64).is_none_or(|value| value < 1) {
		errors.push("source.search_limit must be a positive integer".into());
	}
}

fn validate_upstream_review_subjects(subjects: Option<&Value>, errors: &mut Vec<String>) -> usize {
	let Some(subjects) = subjects.and_then(Value::as_array) else {
		errors.push("subjects must be a list".into());

		return 0;
	};
	let mut seen = BTreeSet::new();

	for (index, subject) in subjects.iter().enumerate() {
		let Some(subject) = subject.as_object() else {
			errors.push(format!("subjects[{index}] must be an object"));

			continue;
		};

		validate_upstream_review_subject(subject, index, &mut seen, errors);
	}

	subjects.len()
}

fn validate_upstream_review_subject(
	subject: &Map<String, Value>,
	index: usize,
	seen: &mut BTreeSet<(String, String)>,
	errors: &mut Vec<String>,
) {
	let subject_kind = string_field(subject, "subject_kind");
	let subject_id = string_field(subject, "subject_id");

	if !matches_one_of(subject.get("subject_kind"), UPSTREAM_SUBJECT_KINDS) {
		errors.push(format!(
			"subjects[{index}].subject_kind must be one of {}",
			choices(UPSTREAM_SUBJECT_KINDS)
		));
	}
	if !is_non_empty_string(subject.get("subject_id")) {
		errors.push(format!("subjects[{index}].subject_id must be a non-empty string"));
	}

	if let (Some(subject_kind), Some(subject_id)) = (subject_kind, subject_id) {
		let key = (subject_kind.to_owned(), subject_id.to_owned());

		if !seen.insert(key) {
			errors.push(format!("subjects[{index}] duplicates {subject_kind}:{subject_id}"));
		}
	}

	validate_upstream_review_subject_fields(subject, index, errors);
}

fn validate_upstream_review_subject_fields(
	subject: &Map<String, Value>,
	index: usize,
	errors: &mut Vec<String>,
) {
	for field in ["title", "url", "review_reason"] {
		if !is_non_empty_string(subject.get(field)) {
			errors.push(format!("subjects[{index}].{field} must be a non-empty string"));
		}
	}

	if !is_https_string(subject.get("url")) {
		errors.push(format!("subjects[{index}].url must be an https URL"));
	}
	if !matches_one_of(subject.get("source_state"), UPSTREAM_SOURCE_STATES) {
		errors.push(format!(
			"subjects[{index}].source_state must be one of {}",
			choices(UPSTREAM_SOURCE_STATES)
		));
	}
	if !matches_one_of(subject.get("review_priority"), UPSTREAM_REVIEW_PRIORITIES) {
		errors.push(format!(
			"subjects[{index}].review_priority must be one of {}",
			choices(UPSTREAM_REVIEW_PRIORITIES)
		));
	}
	if !matches_one_of(subject.get("next_step"), UPSTREAM_REVIEW_NEXT_STEPS) {
		errors.push(format!(
			"subjects[{index}].next_step must be one of {}",
			choices(UPSTREAM_REVIEW_NEXT_STEPS)
		));
	}

	validate_non_empty_string_list(
		subject.get("commit_shas"),
		&format!("subjects[{index}].commit_shas"),
		errors,
	);

	for field in ["surface_hints", "attention_flags", "sample_paths"] {
		validate_optional_string_list(
			subject.get(field),
			&format!("subjects[{index}].{field}"),
			errors,
		);
	}

	if subject.get("changed_file_count").and_then(Value::as_i64).is_none_or(|value| value < 0) {
		errors.push(format!("subjects[{index}].changed_file_count must be a non-negative integer"));
	}
}

fn validate_upstream_review_counts(
	counts: Option<&Value>,
	subjects: usize,
	errors: &mut Vec<String>,
) {
	let Some(counts) = counts.and_then(Value::as_object) else {
		errors.push("counts must be an object".into());

		return;
	};

	if counts.get("subjects_queued").and_then(Value::as_u64) != Some(subjects as u64) {
		errors.push("counts.subjects_queued must equal len(subjects)".into());
	}

	for field in
		["recent_commits_scanned", "published_subjects_seen", "critical", "high", "normal", "low"]
	{
		if counts.get(field).and_then(Value::as_i64).is_none_or(|value| value < 0) {
			errors.push(format!("counts.{field} must be a non-negative integer"));
		}
	}
}

fn validate_upstream_review(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	for field in ["slug", "repo", "reviewed_at", "observed_change"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	if string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}

	validate_upstream_review_subject_object(entry.get("subject"), errors);
	validate_upstream_review_source_refs(entry.get("source_refs"), errors);

	for field in ["changed_surfaces", "evidence"] {
		validate_non_empty_string_list(entry.get(field), field, errors);
	}

	validate_upstream_review_optional_strings(entry, errors);

	if !matches_one_of(entry.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", choices(SIGNAL_CONFIDENCE)));
	}

	validate_upstream_review_actions(entry.get("next_actions"), errors);
}

fn validate_upstream_review_subject_object(subject: Option<&Value>, errors: &mut Vec<String>) {
	let Some(subject) = subject.and_then(Value::as_object) else {
		errors.push("subject must be an object".into());

		return;
	};

	if !matches_one_of(subject.get("subject_kind"), UPSTREAM_SUBJECT_KINDS) {
		errors.push(format!(
			"subject.subject_kind must be one of {}",
			choices(UPSTREAM_SUBJECT_KINDS)
		));
	}
	if !is_non_empty_string(subject.get("subject_id")) {
		errors.push("subject.subject_id must be a non-empty string".into());
	}

	validate_optional_string_list(subject.get("commit_shas"), "subject.commit_shas", errors);
}

fn validate_upstream_review_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let valid = non_empty_array(refs.get("items")).is_some_and(|items| {
		items.iter().all(|item| {
			item.as_object().is_some_and(|item| {
				is_non_empty_string(item.get("kind"))
					&& is_non_empty_string(item.get("title"))
					&& is_https_string(item.get("url"))
			})
		})
	});

	if !valid {
		errors.push(
			"source_refs.items must be a non-empty list of titled https source entries".into(),
		);
	}
}

fn validate_upstream_review_optional_strings(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	for field in [
		"user_visible_path",
		"control_plane_relevance",
		"compatibility_risk",
		"adoption_opportunity",
		"community_value",
		"deprecated_or_breaking_notes",
		"caveats",
	] {
		if entry.get(field).is_some_and(|value| !value.is_string() && !value.is_null()) {
			errors.push(format!("{field} must be a string when present"));
		}
	}
}

fn validate_upstream_review_actions(next_actions: Option<&Value>, errors: &mut Vec<String>) {
	let Some(next_actions) = non_empty_array(next_actions) else {
		errors.push("next_actions must be a non-empty list".into());

		return;
	};

	for (index, action) in next_actions.iter().enumerate() {
		let Some(action) = action.as_object() else {
			errors.push(format!("next_actions[{index}] must be an object"));

			continue;
		};

		if !matches_one_of(action.get("type"), UPSTREAM_REVIEW_ACTION_TYPES) {
			errors.push(format!(
				"next_actions[{index}].type must be one of {}",
				choices(UPSTREAM_REVIEW_ACTION_TYPES)
			));
		}
		if !is_non_empty_string(action.get("reason")) {
			errors.push(format!("next_actions[{index}].reason must be a non-empty string"));
		}
	}
}

fn validate_upstream_impact(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	for field in ["slug", "repo", "observed_change"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	if string_field(entry, "repo").is_some_and(|repo| !repo.contains('/')) {
		errors.push("repo must be owner/name".into());
	}

	validate_upstream_impact_source_refs(entry.get("source_refs"), errors);

	if !matches_one_of(entry.get("public_signal_decision"), &["defer", "publish", "skip"]) {
		errors.push("public_signal_decision must be one of ['defer', 'publish', 'skip']".into());
	}
	if !matches_one_of(
		entry.get("control_plane_impact"),
		&["adopt_now", "candidate", "compat_risk", "none", "watch"],
	) {
		errors.push("control_plane_impact must be one of ['adopt_now', 'candidate', 'compat_risk', 'none', 'watch']".into());
	}
	if !matches_one_of(
		entry.get("publisher_angle"),
		&["none", "operator_impact", "practical_explainer", "release_pulse", "watch_note"],
	) {
		errors.push("publisher_angle must be one of ['none', 'operator_impact', 'practical_explainer', 'release_pulse', 'watch_note']".into());
	}
	if !matches_one_of(entry.get("confidence"), SIGNAL_CONFIDENCE) {
		errors.push(format!("confidence must be one of {}", choices(SIGNAL_CONFIDENCE)));
	}

	validate_non_empty_string_list(entry.get("evidence"), "evidence", errors);

	for field in ["candidate_followups", "social_notes", "caveats"] {
		validate_optional_string_list(entry.get(field), field, errors);
	}
}

fn validate_upstream_impact_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let valid = non_empty_array(refs.get("items")).is_some_and(|items| {
		items.iter().all(|item| {
			item.as_object().is_some_and(|item| {
				matches_one_of(item.get("kind"), UPSTREAM_IMPACT_KINDS)
					&& is_non_empty_string(item.get("title"))
					&& is_https_string(item.get("url"))
					&& item.get("meta").is_none_or(|meta| is_non_empty_string(Some(meta)))
			})
		})
	});

	if !valid {
		errors.push(
			"source_refs.items must be a non-empty list of titled https source entries".into(),
		);
	}
}

fn validate_social_post(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	for field in ["slug", "audience"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	validate_social_post_constants(entry, errors);
	validate_social_post_text(entry.get("text"), errors);
	validate_social_post_source_refs(entry.get("source_refs"), errors);

	for field in ["evidence_notes", "claims"] {
		if non_empty_array(entry.get(field)).is_none() {
			errors.push(format!("{field} must be a non-empty list"));
		}
	}

	validate_social_post_claims(entry.get("claims"), errors);
	validate_social_post_decision(entry, errors);
	validate_social_post_status_payload(entry, errors);

	for field in ["caveats", "media_refs"] {
		validate_optional_string_list(entry.get(field), field, errors);
	}
}

fn validate_social_post_constants(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	if string_field(entry, "channel") != Some("x") {
		errors.push("channel must be x".into());
	}
	if string_field(entry, "target_account") != Some("decodexspace") {
		errors.push("target_account must be decodexspace".into());
	}
	if string_field(entry, "controller_account") != Some("hackink") {
		errors.push("controller_account must be hackink".into());
	}
	if !matches_one_of(entry.get("mode"), SOCIAL_POST_MODES) {
		errors.push(format!("mode must be one of {}", choices(SOCIAL_POST_MODES)));
	}
	if !matches_one_of(entry.get("status"), SOCIAL_POST_STATUSES) {
		errors.push(format!("status must be one of {}", choices(SOCIAL_POST_STATUSES)));
	}
}

fn validate_social_post_text(text: Option<&Value>, errors: &mut Vec<String>) {
	let valid = non_empty_array(text).is_some_and(|text| {
		text.iter()
			.all(|item| item.as_str().is_some_and(|text| !text.is_empty() && text.len() <= 280))
	});

	if !valid {
		errors.push("text must be a non-empty list of X-sized strings".into());
	}
}

fn validate_social_post_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};
	let has_refs = ["signals", "upstream_impacts", "upstream_reviews", "urls"]
		.iter()
		.any(|field| non_empty_array(refs.get(*field)).is_some());

	if !has_refs {
		errors.push(
			"source_refs must include signals, upstream_impacts, upstream_reviews, or urls".into(),
		);
	}
	if refs.get("urls").is_some_and(|urls| !is_https_string_array(urls)) {
		errors.push("source_refs.urls must be a list of https URLs".into());
	}
}

fn validate_social_post_claims(claims: Option<&Value>, errors: &mut Vec<String>) {
	let Some(claims) = claims.and_then(Value::as_array) else {
		return;
	};

	for (index, claim) in claims.iter().enumerate() {
		let Some(claim) = claim.as_object() else {
			errors.push(format!("claims[{index}] must be an object"));

			continue;
		};

		for field in ["text", "evidence"] {
			if !is_non_empty_string(claim.get(field)) {
				errors.push(format!("claims[{index}].{field} must be a non-empty string"));
			}
		}

		if !matches_one_of(claim.get("confidence"), SIGNAL_CONFIDENCE) {
			errors.push(format!(
				"claims[{index}].confidence must be one of {}",
				choices(SIGNAL_CONFIDENCE)
			));
		}
	}
}

fn validate_social_post_decision(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let Some(decision) = entry.get("decision").and_then(Value::as_object) else {
		errors.push("decision must be an object".into());

		return;
	};

	if !matches_one_of(decision.get("worthiness"), SOCIAL_POST_WORTHINESS) {
		errors.push(format!(
			"decision.worthiness must be one of {}",
			choices(SOCIAL_POST_WORTHINESS)
		));
	}
	if !matches_one_of(decision.get("priority"), SOCIAL_POST_PRIORITIES) {
		errors
			.push(format!("decision.priority must be one of {}", choices(SOCIAL_POST_PRIORITIES)));
	}

	for field in ["idempotency_key", "reason", "day", "timezone"] {
		if !is_non_empty_string(decision.get(field)) {
			errors.push(format!("decision.{field} must be a non-empty string"));
		}
	}

	if decision.get("daily_limit").and_then(Value::as_i64) != Some(8) {
		errors.push("decision.daily_limit must be 8".into());
	}

	validate_social_post_decision_counts(entry, decision, errors);
}

fn validate_social_post_decision_counts(
	entry: &Map<String, Value>,
	decision: &Map<String, Value>,
	errors: &mut Vec<String>,
) {
	for field in ["daily_count_before", "daily_count_after"] {
		if decision.get(field).and_then(Value::as_i64).is_none_or(|value| value < 0) {
			errors.push(format!("decision.{field} must be a non-negative integer"));
		}
	}

	let before = decision.get("daily_count_before").and_then(Value::as_i64);
	let after = decision.get("daily_count_after").and_then(Value::as_i64);
	let post_count = entry.get("text").and_then(Value::as_array).map_or(0, Vec::len) as i64;

	if let (Some(before), Some(after)) = (before, after) {
		if string_field(entry, "status") == Some("published") && after != before + post_count {
			errors.push("decision.daily_count_after must add the published post count".into());
		}
		if string_field(entry, "status") != Some("published") && after != before {
			errors.push("decision.daily_count_after must remain unchanged unless published".into());
		}
	}
}

fn validate_social_post_status_payload(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	match string_field(entry, "status") {
		Some("published") => validate_social_post_publication(entry.get("publication"), errors),
		Some("blocked") => validate_social_post_block(entry, errors),
		Some("failed") if entry.get("failure").and_then(Value::as_object).is_none() =>
			errors.push("failure is required when status is failed".into()),
		Some("skipped") if entry.get("skip").and_then(Value::as_object).is_none() =>
			errors.push("skip is required when status is skipped".into()),
		_ => {},
	}
}

fn validate_social_post_publication(publication: Option<&Value>, errors: &mut Vec<String>) {
	let Some(publication) = publication.and_then(Value::as_object) else {
		errors.push("publication is required when status is published".into());

		return;
	};

	if !matches_one_of(publication.get("publisher"), &["chrome", "x_api"]) {
		errors.push("publication.publisher must be chrome or x_api".into());
	}
	if publication.get("account_verified").and_then(Value::as_bool) != Some(true) {
		errors.push("publication.account_verified must be true".into());
	}
	if publication.get("made_with_ai").and_then(Value::as_bool).is_none() {
		errors.push("publication.made_with_ai must be boolean".into());
	}
	if publication.get("image_template").is_some()
		&& string_field(publication, "image_template") != Some("decodex_signal_card")
	{
		errors.push("publication.image_template must be decodex_signal_card when present".into());
	}
	if !non_empty_array(publication.get("published_urls"))
		.is_some_and(|urls| urls.iter().all(|url| is_https_string(Some(url))))
	{
		errors.push("publication.published_urls must be a non-empty list of https URLs".into());
	}
	if !is_non_empty_string(publication.get("posted_at")) {
		errors.push("publication.posted_at must be a non-empty string".into());
	}
}

fn validate_social_post_block(entry: &Map<String, Value>, errors: &mut Vec<String>) {
	let Some(block) = entry.get("block").and_then(Value::as_object) else {
		errors.push("block is required when status is blocked".into());

		return;
	};

	if !matches_one_of(block.get("reason"), SOCIAL_BLOCK_REASONS) {
		errors.push(format!("block.reason must be one of {}", choices(SOCIAL_BLOCK_REASONS)));
	}

	let count_before = entry
		.get("decision")
		.and_then(Value::as_object)
		.and_then(|decision| decision.get("daily_count_before"))
		.and_then(Value::as_i64);

	if string_field(block, "reason") == Some("daily_cap_exceeded")
		&& count_before.is_none_or(|count| count < 8)
	{
		errors.push("daily_cap_exceeded requires decision.daily_count_before >= 8".into());
	}
	if !is_non_empty_string(block.get("operator_notice")) {
		errors.push("block.operator_notice must be a non-empty string".into());
	}
}

fn validate_signal_slug_uniqueness(
	path: &Path,
	payload: &Value,
	state: &mut ValidationState,
	errors: &mut Vec<String>,
) {
	let Some(slug) = payload.get("slug").and_then(Value::as_str) else {
		return;
	};

	if let Some(existing) = state.seen_signal_slugs.insert(slug.to_owned(), path.to_path_buf()) {
		errors.push(format!(
			"{}: duplicate slug {slug:?} also used by {}",
			path.display(),
			existing.display()
		));
	}
}

fn validate_non_empty_string_list(value: Option<&Value>, label: &str, errors: &mut Vec<String>) {
	let valid = non_empty_array(value).is_some_and(|values| {
		values.iter().all(|item| item.as_str().is_some_and(|item| !item.is_empty()))
	});

	if !valid {
		errors.push(format!("{label} must be a non-empty list of strings"));
	}
}

fn validate_string_list(value: Option<&Value>, label: &str, errors: &mut Vec<String>) {
	let valid = value.and_then(Value::as_array).is_some_and(|values| {
		values.iter().all(|item| item.as_str().is_some_and(|item| !item.is_empty()))
	});

	if !valid {
		errors.push(format!("{label} must be a list"));
	}
}

fn validate_optional_string_list(value: Option<&Value>, label: &str, errors: &mut Vec<String>) {
	let Some(value) = value else {
		return;
	};

	if value.is_null() {
		return;
	}
	if !value.as_array().is_some_and(|values| {
		values.iter().all(|item| item.as_str().is_some_and(|item| !item.is_empty()))
	}) {
		errors.push(format!("{label} must be a list of non-empty strings when present"));
	}
}

fn validate_optional_positive_integer_list(
	value: Option<&Value>,
	label: &str,
	errors: &mut Vec<String>,
) {
	let Some(value) = value else {
		return;
	};

	if value.is_null() {
		return;
	}
	if !value
		.as_array()
		.is_some_and(|values| values.iter().all(|item| item.as_i64().is_some_and(|item| item > 0)))
	{
		errors.push(format!("{label} must be a list of positive integers"));
	}
}

fn optional_array<'a>(
	value: Option<&'a Value>,
	label: &str,
	errors: &mut Vec<String>,
) -> Option<&'a Vec<Value>> {
	match value {
		Some(Value::Array(values)) => Some(values),
		Some(Value::Null) | None => None,
		Some(_) => {
			errors.push(format!("{label} must be a list when present"));

			None
		},
	}
}

fn string_field<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
	object.get(field).and_then(Value::as_str)
}

fn is_non_empty_string(value: Option<&Value>) -> bool {
	value.and_then(Value::as_str).is_some_and(|value| !value.is_empty())
}

fn is_truthy_json_value(value: Option<&Value>) -> bool {
	match value {
		Some(Value::Null) | None => false,
		Some(Value::String(value)) => !value.is_empty(),
		Some(_) => true,
	}
}

fn matches_one_of(value: Option<&Value>, choices: &[&str]) -> bool {
	value.and_then(Value::as_str).is_some_and(|value| choices.contains(&value))
}

fn non_empty_array(value: Option<&Value>) -> Option<&Vec<Value>> {
	value.and_then(Value::as_array).filter(|values| !values.is_empty())
}

fn is_empty_or_missing_array(value: Option<&Value>) -> bool {
	value.and_then(Value::as_array).is_none_or(Vec::is_empty)
}

fn is_https_string(value: Option<&Value>) -> bool {
	value.and_then(Value::as_str).is_some_and(|value| value.starts_with("https://"))
}

fn is_https_string_array(value: &Value) -> bool {
	value.as_array().is_some_and(|values| values.iter().all(|url| is_https_string(Some(url))))
}

fn choices(values: &[&str]) -> String {
	let quoted = values.iter().map(|value| format!("'{value}'")).collect::<Vec<_>>().join(", ");

	format!("[{quoted}]")
}

fn known_schemas() -> String {
	choices(&[
		BUNDLE_SCHEMA,
		RELEASE_DELTA_SCHEMA,
		SIGNAL_SCHEMA,
		SOCIAL_POST_SCHEMA,
		UPSTREAM_IMPACT_SCHEMA,
		UPSTREAM_REVIEW_QUEUE_SCHEMA,
		UPSTREAM_REVIEW_SCHEMA,
	])
}

#[cfg(test)]
mod tests {
	use std::{fs, path::PathBuf};

	use serde_json::{self, Value};

	use crate::radar::{
		self, RadarBackfillReleaseRangeRequest, RadarRenderSignalRequest, RadarValidateRequest,
	};

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
		signal["how_to_try"] = serde_json::json!("Run decodex radar validate.");

		assert_errors(&signal, ["expected_effect is required when how_to_try is present"]);
	}

	#[test]
	fn rejects_duplicate_signal_slugs_across_files() {
		let signal = valid_signal();
		let mut state = crate::radar::ValidationState::new();
		let mut errors = Vec::new();

		radar::validate_signal_slug_uniqueness(
			&PathBuf::from("site/src/content/signals/one.json"),
			&signal,
			&mut state,
			&mut errors,
		);
		radar::validate_signal_slug_uniqueness(
			&PathBuf::from("site/src/content/signals/two.json"),
			&signal,
			&mut state,
			&mut errors,
		);

		assert_eq!(errors.len(), 1);
		assert!(errors[0].contains("duplicate slug"));
	}

	#[test]
	fn accepts_valid_release_delta_and_rejects_missing_default_pair() {
		let mut release_delta = valid_release_delta();

		assert_errors(&release_delta, []);

		release_delta["comparisons"][0]["prerelease_tag_name"] =
			serde_json::json!("rust-v0.2.0-alpha.2");

		assert_errors(
			&release_delta,
			["comparisons must include the default stable/prerelease pair"],
		);
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
	fn accepts_valid_upstream_review_and_rejects_bad_action() {
		let mut review = valid_upstream_review();

		assert_errors(&review, []);

		review["next_actions"][0]["type"] = serde_json::json!("publish_now");

		assert_errors(&review, ["next_actions[0].type must be one of"]);
	}

	#[test]
	fn accepts_valid_upstream_impact_and_rejects_bad_angle() {
		let mut impact = valid_upstream_impact();

		assert_errors(&impact, []);

		impact["publisher_angle"] = serde_json::json!("viral_thread");

		assert_errors(&impact, ["publisher_angle must be one of"]);
	}

	#[test]
	fn accepts_valid_social_post_and_rejects_bad_daily_limit() {
		let mut social_post = valid_social_post();

		assert_errors(&social_post, []);

		social_post["decision"]["daily_limit"] = serde_json::json!(9);

		assert_errors(&social_post, ["decision.daily_limit must be 8"]);
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

	#[test]
	fn dry_run_backfill_selects_unpublished_release_window_prs() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let release_delta_path = temp_dir.path().join("release-delta.json");
		let signals_dir = temp_dir.path().join("signals");
		let mut release_delta = valid_release_delta();

		release_delta["compare"]["pr_numbers"] = serde_json::json!([22_414, 22_415, 22_416]);
		release_delta["comparisons"][0]["compare"]["pr_numbers"] =
			serde_json::json!([22_414, 22_415, 22_416]);

		fs::create_dir_all(&signals_dir).expect("signals directory should be created");
		fs::write(release_delta_path.as_path(), release_delta.to_string())
			.expect("release delta should be written");
		fs::write(signals_dir.join("published.json"), valid_signal().to_string())
			.expect("signal should be written");

		let report = radar::backfill_release_range(&RadarBackfillReleaseRangeRequest {
			repo: "openai/codex".into(),
			release_delta: release_delta_path,
			stable_tag: None,
			preview_tag: None,
			signals_dir,
			bundles_dir: temp_dir.path().join("bundles"),
			analysis_dir: temp_dir.path().join("analysis"),
			token_env: None,
			codex_bin: "codex".into(),
			model: None,
			max_prs: Some(1),
			dry_run: true,
			refresh_release_delta_first: false,
			refresh_stable_limit: None,
			refresh_preview_limit: None,
			refresh_pair_limit: None,
			python_bin: "python3".into(),
		})
		.expect("dry-run backfill should select unpublished PRs");

		assert_eq!(report.stable_tag, "rust-v0.1.0");
		assert_eq!(report.preview_tag, "rust-v0.2.0-alpha.1");
		assert_eq!(report.target_prs, vec![22_415]);
		assert_eq!(report.created, 0);
		assert!(report.dry_run);
	}

	fn assert_errors<const N: usize>(payload: &Value, expected: [&str; N]) {
		let validation = radar::validate_artifact(payload);

		for expected_error in expected {
			assert!(
				validation.errors.iter().any(|error| error.contains(expected_error)),
				"expected error containing {expected_error:?}, got {:?}",
				validation.errors
			);
		}

		if expected.is_empty() {
			assert_eq!(validation.errors, Vec::<String>::new());
		}
	}

	fn valid_bundle() -> Value {
		serde_json::json!({
			"schema": "github_change_bundle/v1",
			"repo": "openai/codex",
			"analysis_mode": "pr_first",
			"default_branch": "main",
			"primary_pr": {
				"number": 22_414,
				"title": "Add Unix socket endpoint support",
				"body": "",
				"state": "merged",
				"merged_at": "2026-06-01T00:00:00Z",
				"labels": [],
				"url": "https://github.com/openai/codex/pull/22414"
			},
			"commits": [
				{
					"sha": "abc123",
					"message": "Add Unix socket endpoint support",
					"url": "https://github.com/openai/codex/commit/abc123"
				}
			],
			"files": [
				{
					"path": "codex-rs/app-server/src/lib.rs",
					"status": "modified",
					"additions": 12,
					"deletions": 1
				}
			]
		})
	}

	fn valid_signal() -> Value {
		serde_json::json!({
			"schema": "signal_entry/v1",
			"slug": "openai-codex-pr-22414",
			"lane": "github",
			"kind": "capability",
			"title": "Unix sockets for remote Codex",
			"published_at": "2026-06-01T00:00:00Z",
			"summary": "Remote Codex can use Unix socket endpoints.",
			"why_it_matters": "Operators can use local socket transports.",
			"confidence": "confirmed",
			"impact": "medium",
			"proof_points": ["PR #22414 adds endpoint handling."],
			"source_refs": {
				"repo": "openai/codex",
				"pr_url": "https://github.com/openai/codex/pull/22414",
				"items": [
					{
						"kind": "pull_request",
						"title": "Add Unix socket endpoint support",
						"url": "https://github.com/openai/codex/pull/22414"
					}
				]
			}
		})
	}

	fn valid_release_delta() -> Value {
		serde_json::json!({
			"schema": "release_delta/v1",
			"repo": "openai/codex",
			"tag_prefix": "rust-v",
			"generated_at": "2026-06-01T00:00:00Z",
			"stable_release": release("rust-v0.1.0", false),
			"prerelease": release("rust-v0.2.0-alpha.1", true),
			"compare": compare(),
			"tracked_signal_slugs": ["openai-codex-pr-22414"],
			"release_options": {
				"stable": [release("rust-v0.1.0", false)],
				"preview": [release("rust-v0.2.0-alpha.1", true)]
			},
			"comparisons": [
				{
					"stable_tag_name": "rust-v0.1.0",
					"prerelease_tag_name": "rust-v0.2.0-alpha.1",
					"compare": compare(),
					"tracked_signal_slugs": ["openai-codex-pr-22414"]
				}
			]
		})
	}

	fn release(tag_name: &str, prerelease: bool) -> Value {
		serde_json::json!({
			"tag_name": tag_name,
			"name": tag_name,
			"published_at": "2026-06-01T00:00:00Z",
			"url": "https://github.com/openai/codex/releases/tag/rust-v0.1.0",
			"prerelease": prerelease
		})
	}

	fn compare() -> Value {
		serde_json::json!({
			"status": "ahead",
			"ahead_by": 1,
			"total_commits": 1,
			"url": "https://github.com/openai/codex/compare/rust-v0.1.0...rust-v0.2.0-alpha.1",
			"commit_shas": ["abc123"],
			"pr_numbers": [22_414]
		})
	}

	fn valid_review_queue() -> Value {
		serde_json::json!({
			"schema": "upstream_review_queue/v1",
			"repo": "openai/codex",
			"generated_at": "2026-06-01T00:00:00Z",
			"source": {
				"default_branch": "main",
				"search_limit": 40
			},
			"subjects": [valid_queue_subject()],
			"counts": {
				"subjects_queued": 1,
				"recent_commits_scanned": 1,
				"published_subjects_seen": 0,
				"critical": 0,
				"high": 1,
				"normal": 0,
				"low": 0
			}
		})
	}

	fn valid_queue_subject() -> Value {
		serde_json::json!({
			"subject_kind": "pr",
			"subject_id": "22414",
			"title": "Add Unix socket endpoint support",
			"url": "https://github.com/openai/codex/pull/22414",
			"source_state": "merged",
			"commit_shas": ["abc123"],
			"changed_file_count": 1,
			"sample_paths": ["codex-rs/app-server/src/lib.rs"],
			"surface_hints": ["app_server_protocol"],
			"attention_flags": [],
			"review_priority": "high",
			"review_reason": "Transport behavior changed.",
			"next_step": "ai_review_required"
		})
	}

	fn valid_upstream_review() -> Value {
		serde_json::json!({
			"schema": "upstream_review/v1",
			"slug": "openai-codex-pr-22414",
			"repo": "openai/codex",
			"subject": {
				"subject_kind": "pr",
				"subject_id": "22414",
				"commit_shas": ["abc123"]
			},
			"source_refs": {
				"items": [
					{
						"kind": "pull_request",
						"title": "Add Unix socket endpoint support",
						"url": "https://github.com/openai/codex/pull/22414"
					}
				]
			},
			"reviewed_at": "2026-06-01T00:00:00Z",
			"observed_change": "Remote Codex can use Unix socket endpoints.",
			"changed_surfaces": ["app server"],
			"confidence": "confirmed",
			"evidence": ["PR #22414 updates app-server endpoint handling."],
			"next_actions": [
				{
					"type": "upstream_impact",
					"reason": "Transport behavior can affect Decodex."
				}
			]
		})
	}

	fn valid_upstream_impact() -> Value {
		serde_json::json!({
			"schema": "upstream_impact/v1",
			"slug": "openai-codex-pr-22414",
			"repo": "openai/codex",
			"source_refs": {
				"items": [
					{
						"kind": "pull_request",
						"title": "Add Unix socket endpoint support",
						"url": "https://github.com/openai/codex/pull/22414"
					}
				]
			},
			"observed_change": "Remote Codex can use Unix socket endpoints.",
			"public_signal_decision": "publish",
			"control_plane_impact": "candidate",
			"publisher_angle": "operator_impact",
			"confidence": "confirmed",
			"evidence": ["PR #22414 updates app-server endpoint handling."]
		})
	}

	fn valid_social_post() -> Value {
		serde_json::json!({
			"schema": "social_post/v1",
			"slug": "openai-codex-pr-22414",
			"channel": "x",
			"target_account": "decodexspace",
			"controller_account": "hackink",
			"mode": "operator_impact",
			"status": "published",
			"audience": "Codex operators",
			"text": ["Remote Codex can now use Unix socket endpoints. Source: https://github.com/openai/codex/pull/22414"],
			"source_refs": {
				"urls": ["https://github.com/openai/codex/pull/22414"]
			},
			"evidence_notes": ["PR #22414 changes remote endpoint handling."],
			"claims": [
				{
					"text": "Remote Codex can use Unix socket endpoints.",
					"evidence": "https://github.com/openai/codex/pull/22414",
					"confidence": "confirmed"
				}
			],
			"decision": {
				"worthiness": "publish",
				"priority": "high",
				"idempotency_key": "x:decodexspace:operator_impact:openai-codex-pr-22414",
				"reason": "High-value Control Plane transport implication.",
				"daily_limit": 8,
				"daily_count_before": 2,
				"daily_count_after": 3,
				"day": "2026-06-02",
				"timezone": "Asia/Shanghai"
			},
			"publication": {
				"posted_at": "2026-06-02T03:00:00Z",
				"published_urls": ["https://x.com/decodexspace/status/1"],
				"publisher": "chrome",
				"account_verified": true,
				"made_with_ai": true,
				"image_template": "decodex_signal_card"
			},
			"media_refs": ["artifacts/social/x/images/openai-codex-pr-22414.png"]
		})
	}
}
