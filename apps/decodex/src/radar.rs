//! Rust-owned Radar artifact contracts and file validation.

use std::{
	collections::{BTreeMap, BTreeSet, HashSet},
	env,
	fs::{self, OpenOptions},
	io::Write as _,
	iter,
	path::{Path, PathBuf},
	process::{self, Command},
	sync::OnceLock,
	time::Duration,
};

use regex::Regex;
use reqwest::StatusCode;
use serde_json::{self, Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::prelude::eyre;

mod artifact_validation;
mod github_api;
mod github_bundle_client;
mod github_token;
mod ledger;
mod release_delta;
mod requests;
mod review_queue;
mod signal_render;
mod social_publish;
mod source_bundle;

#[cfg(test)]
use artifact_validation::has_legacy_multi_agent_v2_context;
use artifact_validation::{
	ValidationState, validate_active_social_publish_reservation_uniqueness,
	validate_analysis_draft, validate_artifact, validate_artifact_errors,
	validate_artifact_for_path, validate_signal_file, validate_signal_slug_uniqueness,
	validate_terminal_social_post_idempotency_key_uniqueness,
};
use github_api::GitHubApi;
use github_bundle_client::GithubClient;
use github_token::github_token;
use ledger::RadarLedger;
pub(crate) use release_delta::{backfill_release_range, refresh_release_delta};
use review_queue::RecentCommit;
use review_queue::build_review_queue;
use signal_render::{rendered_config_flags, rendered_signal};
pub(crate) use social_publish::reserve_social_publish;
use source_bundle::{build_commit_bundle_from_sources, build_pr_bundle_from_sources};

pub(crate) use ledger::{
	default_ledger_path, ledger_artifact_link, ledger_bootstrap, ledger_ingest,
	ledger_ingest_existing, ledger_summary,
};

pub(crate) use requests::{
	RadarBackfillReleaseRangeReport, RadarBackfillReleaseRangeRequest, RadarBundleBuildRequest,
	RadarBundleValidateRequest, RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest,
	RadarLedgerIngestExistingRequest, RadarLedgerIngestRequest, RadarLedgerSummaryRequest,
	RadarRefreshQueueReport, RadarRefreshQueueRequest, RadarRefreshReleaseDeltaReport,
	RadarRefreshReleaseDeltaRequest, RadarRenderSignalReport, RadarRenderSignalRequest,
	RadarSocialReservePublishReport, RadarSocialReservePublishRequest, RadarValidateRequest,
	RadarValidationReport,
};

const BUNDLE_SCHEMA: &str = "github_change_bundle/v1";
const CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA: &str = "control_plane_upgrade_candidate/v1";
const DEFAULT_LEDGER_PATH: &str = ".agent/automations/decodex/cache/github/radar.sqlite3";
const DEFAULT_MIN_STABLE_TAG: &str = "rust-v0.116.0";
const DEFAULT_PAIR_LIMIT: usize = 24;
const DEFAULT_PREVIEW_LIMIT: usize = 0;
const DEFAULT_QUEUE_OUT: &str =
	".agent/automations/decodex/cache/github/review-queue/openai-codex-latest.json";
const DEFAULT_RELEASE_DELTA_OUT: &str =
	".agent/automations/decodex/cache/site-content/release-deltas/openai-codex-latest.json";
const DEFAULT_SEARCH_LIMIT: usize = 40;
const DEFAULT_SIGNALS_DIR: &str = ".agent/automations/decodex/cache/site-content/signals";
const DEFAULT_STABLE_LIMIT: usize = 0;
const DEFAULT_TAG_PREFIX: &str = "rust-v";
const RELEASE_DELTA_SCHEMA: &str = "release_delta/v1";
const SCHEMA_VERSION: i64 = 4;
const SIGNAL_SCHEMA: &str = "signal_entry/v1";
const SOCIAL_CANDIDATE_SCHEMA: &str = "social_candidate/v1";
const SOCIAL_POST_SCHEMA: &str = "social_post/v1";
const SOCIAL_PUBLISH_RESERVATION_SCHEMA: &str = "social_publish_reservation/v1";
const UPSTREAM_IMPACT_SCHEMA: &str = "upstream_impact/v1";
const UPSTREAM_REVIEW_QUEUE_SCHEMA: &str = "upstream_review_queue/v1";
const UPSTREAM_REVIEW_SCHEMA: &str = "upstream_review/v1";
const CONFIG_FEATURE_CATALOG_SCHEMA: &str = "codex_config_feature_catalog/v1";
const ANALYSIS_DRAFT_KIND: &str = "analysis_draft";
const RADAR_ARCHIVE_HISTORICAL_RETENTION_CUTOFF: &str = "2026-06-07T00:00:00Z";
const UPSTREAM_REVIEW_LINEAR_FOLLOWUP_CUTOFF: &str = "2026-06-12T00:00:00Z";
const SIGNAL_CONFIDENCE: &[&str] = &["confirmed", "likely", "weak"];
const UPSTREAM_SUBJECT_KINDS: &[&str] = &["commit", "pr"];
const DEFAULT_VALIDATION_PATHS: &[&str] = &[
	".agent/automations/decodex/cache/github/bundles",
	".agent/automations/decodex/cache/github/review-queue",
	".agent/automations/decodex/cache/github/reviews",
	".agent/automations/decodex/cache/github/impact",
	".agent/automations/decodex/cache/github/control-plane-upgrades",
	".agent/automations/decodex/cache/github/social-candidates",
	".agent/automations/decodex/cache/social/x",
	".agent/automations/decodex/cache/site-content/signals",
	".agent/automations/decodex/cache/site-content/release-deltas",
	".agent/automations/decodex/cache/generated",
];
const GENERIC_COMMIT_TITLES: &[&str] =
	&["update", "fix", "fix.", "fix tests", "fix tests.", "merge fixes", "flaky syntax"];
const CONFIG_FEATURE_CATALOG_PATH: &str =
	".agent/automations/decodex/cache/generated/codex-config-features.json";
const RUN_CODEX_ANALYSIS_SCRIPT: &str = "automations/decodex/scripts/github/run_codex_analysis.py";
const HIGH_VALUE_SURFACES: &[&str] = &[
	"app_server_protocol",
	"mcp_plugins",
	"browser_chrome",
	"sandbox_permissions",
	"config_hooks",
	"auth_accounts",
	"model_provider",
];
const ATTENTION_RULES: &[(&str, &[&str])] = &[
	(
		"new_feature",
		&["feat", "feature", "add ", "adds ", "support", "enable", "implement", "introduce"],
	),
	("deprecated_removed", &["deprecat", "remove", "removed", "delete", "disable", "no longer"]),
	(
		"protocol_change",
		&[
			"protocol",
			"schema",
			"api",
			"json-rpc",
			"jsonrpc",
			"notification",
			"request",
			"response",
		],
	),
	("breaking_change", &["breaking", "break ", "rename", "migration", "incompat", "no longer"]),
	(
		"security_policy",
		&["sandbox", "permission", "approval", "full access", "network", "denylist", "allowlist"],
	),
	("rate_limit", &["rate limit", "ratelimit", "quota", "usage limit", "message cap"]),
	("auth_account", &["auth", "account", "login", "token"]),
	("release_packaging", &["release", "appcast", "sparkle", "beta", "version"]),
];
const SURFACE_RULES: &[(&str, &[&str])] = &[
	("app_server_protocol", &["app-server", "app_server", "protocol", "jsonrpc", "json-rpc"]),
	("mcp_plugins", &["mcp", "plugin", "tool-search", "tool_search"]),
	("browser_chrome", &["browser", "chrome", "webview"]),
	(
		"sandbox_permissions",
		&["sandbox", "permission", "approval", "policy", "denylist", "allowlist"],
	),
	("config_hooks", &["config", "hook", "settings", "toml"]),
	("auth_accounts", &["auth", "account", "login", "token"]),
	("model_provider", &["model", "provider", "rate-limit", "ratelimit", "quota"]),
	("cli_tui", &["cli", "tui", "terminal", "chatwidget"]),
	("release_packaging", &["release", "appcast", "sparkle", "version", "install", "package"]),
	("docs_examples", &["docs/", "readme", "example"]),
	("tests_ci", &["test", "tests", ".github", "ci", "fixture"]),
];
const REVIEW_STATUSES: &[&str] =
	&["archived", "control_plane", "deprecated", "seen", "signal", "skipped", "social", "watch"];
const ARTIFACT_KINDS: &[&str] = &[
	"analysis",
	"archive_manifest",
	"bundle",
	"control_plane_upgrade_candidate",
	"ledger_export",
	"release_delta",
	"signal",
	"social_candidate",
	"social_post",
	"upstream_impact",
];
const GITHUB_REQUEST_ATTEMPTS: usize = 4;
const GITHUB_REQUEST_BACKOFF: Duration = Duration::from_secs(1);
const GITHUB_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const RETRYABLE_GITHUB_STATUS_CODES: &[StatusCode] = &[
	StatusCode::TOO_MANY_REQUESTS,
	StatusCode::INTERNAL_SERVER_ERROR,
	StatusCode::BAD_GATEWAY,
	StatusCode::SERVICE_UNAVAILABLE,
	StatusCode::GATEWAY_TIMEOUT,
];

#[derive(Debug)]
enum RefreshKind {
	Queue,
	ReleaseDelta,
}

pub(crate) fn refresh_queue(
	request: &RadarRefreshQueueRequest,
) -> crate::prelude::Result<RadarRefreshQueueReport> {
	let root = repo_root()?;
	let api = GitHubApi::new(github_token(request.token_env.as_deref()))?;
	let build = build_review_queue(request, &root, &api)?;
	let errors = validate_artifact_errors(&build.queue);

	if !errors.is_empty() {
		eyre::bail!("Upstream review queue validation failed:\n- {}", errors.join("\n- "));
	}
	if request.dry_run {
		println!("{}", pretty_json(&build.queue)?);

		return Ok(queue_report(
			&build.queue,
			false,
			build.ledger_enabled,
			&root,
			&request.queue_out,
		));
	}

	let out = absolute_repo_path(&root, &request.queue_out);
	let changed = write_json_if_material_changed(&out, &build.queue, RefreshKind::Queue)?;

	Ok(queue_report(&build.queue, changed, build.ledger_enabled, &root, &request.queue_out))
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
		let validation = validate_artifact_for_path(path, &payload);

		if validation.schema.as_deref() == Some(SIGNAL_SCHEMA) {
			validate_signal_slug_uniqueness(path, &payload, &mut state, &mut errors);
		}
		if validation.schema.as_deref() == Some(SOCIAL_POST_SCHEMA) {
			validate_terminal_social_post_idempotency_key_uniqueness(
				path,
				&payload,
				&mut state,
				&mut errors,
			);
		}
		if validation.schema.as_deref() == Some(SOCIAL_PUBLISH_RESERVATION_SCHEMA) {
			validate_active_social_publish_reservation_uniqueness(
				path,
				&payload,
				&mut state,
				&mut errors,
			);
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

/// Build a deterministic GitHub change bundle and write it to disk.
pub(crate) fn build_bundle(request: &RadarBundleBuildRequest) -> crate::prelude::Result<PathBuf> {
	let token = github_token(request.token_env.as_deref());
	let client = GithubClient::new(token.as_deref())?;
	let bundle = match (request.pr, request.commit.as_deref()) {
		(Some(pr_number), _) => client.build_pr_bundle(&request.repo, pr_number, &request.notes)?,
		(None, Some(commit_sha)) => {
			let promoted_pr = if request.force_commit_only {
				None
			} else {
				client.maybe_promote_commit_to_pr(&request.repo, commit_sha)
			};

			match promoted_pr {
				Some(pr_number) => {
					client.build_pr_bundle(&request.repo, pr_number, &request.notes)?
				},
				None => client.build_commit_bundle(&request.repo, commit_sha, &request.notes)?,
			}
		},
		(None, None) => eyre::bail!("one of --pr or --commit is required"),
	};

	write_json(&request.out, &bundle)?;

	Ok(request.out.clone())
}

/// Validate GitHub change bundle artifacts only.
pub(crate) fn validate_bundles(
	request: &RadarBundleValidateRequest,
) -> crate::prelude::Result<RadarValidationReport> {
	let files = collect_bundle_json_files(&request.paths)?;
	let mut errors = Vec::new();

	for path in &files {
		let payload = load_json(path)?;
		let validation = validate_artifact(&payload);

		if validation.schema.as_deref() != Some(BUNDLE_SCHEMA) {
			errors.push(format!("{}: schema must be {BUNDLE_SCHEMA}", path.display()));
		}

		for error in validation.errors {
			errors.push(format!("{}: {error}", path.display()));
		}
	}

	if errors.is_empty() {
		Ok(RadarValidationReport { checked_files: files.len() })
	} else {
		Err(eyre::eyre!("Bundle validation failed:\n- {}", errors.join("\n- ")))
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

fn repo_default_branch(api: &GitHubApi, repo: &str) -> crate::prelude::Result<String> {
	let payload = api.get(&format!("https://api.github.com/repos/{repo}"))?.payload;

	required_value_string(&payload, "default_branch")
		.map_err(|error| eyre::eyre!("Unable to resolve default branch for {repo}: {error}"))
}

fn absolute_repo_path(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() { path.to_path_buf() } else { root.join(path) }
}

fn ledger_path(root: &Path, request: &RadarRefreshQueueRequest) -> Option<PathBuf> {
	(!request.no_ledger).then(|| absolute_repo_path(root, &request.ledger))
}

fn sorted_json_files(path: &Path) -> crate::prelude::Result<Vec<PathBuf>> {
	if !path.exists() {
		return Ok(Vec::new());
	}

	let mut files = fs::read_dir(path)?
		.map(|entry| entry.map(|entry| entry.path()))
		.collect::<std::result::Result<Vec<_>, _>>()?;

	files.retain(|path| {
		path.is_file() && path.extension().is_some_and(|extension| extension == "json")
	});
	files.sort();

	Ok(files)
}

fn collect_bundle_json_files(paths: &[PathBuf]) -> crate::prelude::Result<Vec<PathBuf>> {
	if paths.is_empty() {
		eyre::bail!("at least one bundle JSON file or directory is required");
	}

	let mut files = Vec::new();

	for path in paths {
		if path.is_dir() {
			files.extend(sorted_json_files(path)?);
		} else if path.is_file() {
			files.push(path.clone());
		} else {
			eyre::bail!("Bundle validation path does not exist: {}", path.display());
		}
	}

	files.sort();

	Ok(files)
}

fn write_json_if_material_changed(
	path: &Path,
	payload: &Value,
	kind: RefreshKind,
) -> crate::prelude::Result<bool> {
	if let Ok(existing) = load_json(path)
		&& material_json(&existing, &kind) == material_json(payload, &kind)
	{
		return Ok(false);
	}
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)?;
	}

	fs::write(path, format!("{}\n", pretty_json(payload)?))?;

	Ok(true)
}

fn material_json(payload: &Value, kind: &RefreshKind) -> Value {
	let mut normalized = payload.clone();

	match kind {
		RefreshKind::Queue | RefreshKind::ReleaseDelta => {
			if let Some(object) = normalized.as_object_mut() {
				object.insert("generated_at".to_owned(), Value::String(String::new()));
			}
		},
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

fn short_sha(value: &str) -> String {
	value.chars().take(7).collect()
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

fn repo_root() -> crate::prelude::Result<PathBuf> {
	let mut candidate = env::current_dir()?;

	loop {
		if candidate.join("automations/decodex/scripts/github/README.md").is_file()
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

fn resolve_against(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() { path.to_path_buf() } else { root.join(path) }
}

fn path_arg(root: &Path, path: &Path) -> String {
	path.strip_prefix(root).unwrap_or(path).display().to_string()
}

fn pretty_json(payload: &Value) -> crate::prelude::Result<String> {
	serde_json::to_string_pretty(payload).map_err(Into::into)
}

fn body_excerpt(body: &str) -> String {
	let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");

	if compact.chars().count() > 500 {
		format!("{}...", compact.chars().take(500).collect::<String>())
	} else {
		compact
	}
}

fn required_value_string(payload: &Value, field: &str) -> crate::prelude::Result<String> {
	payload
		.get(field)
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_owned)
		.ok_or_else(|| eyre::eyre!("{field} must be a non-empty string"))
}

fn optional_value_string(payload: &Value, field: &str) -> Option<String> {
	payload.get(field).and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned)
}

fn required_value_u64(payload: &Value, field: &str) -> crate::prelude::Result<u64> {
	payload
		.get(field)
		.and_then(Value::as_u64)
		.ok_or_else(|| eyre::eyre!("{field} must be a positive integer"))
}

fn required_value_i64(payload: &Value, field: &str) -> crate::prelude::Result<i64> {
	payload
		.get(field)
		.and_then(Value::as_i64)
		.ok_or_else(|| eyre::eyre!("{field} must be an integer"))
}

fn truncate_patch_excerpt(value: &str) -> String {
	let compact = value.trim();

	if compact.chars().count() > 900 {
		format!("{}...", compact.chars().take(900).collect::<String>())
	} else {
		compact.to_owned()
	}
}

fn string_array(value: Option<&Value>) -> Vec<String> {
	value
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|item| item.as_str().map(str::to_owned))
		.collect()
}

fn string_array_from_value(value: &Value) -> Vec<String> {
	string_array(Some(value))
}

fn extract_commit_sha_from_url(url: &str) -> Option<String> {
	let sha = url.rsplit_once("/commit/")?.1;

	(sha.len() >= 7 && sha.len() <= 40 && sha.chars().all(|ch| ch.is_ascii_hexdigit()))
		.then(|| sha.to_owned())
}

fn extract_pr_number_from_url(url: &str) -> Option<u64> {
	let number = url.rsplit_once("/pull/")?.1;

	(!number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit()))
		.then(|| number.parse::<u64>().ok())
		.flatten()
}

fn percent_encode(value: &str) -> String {
	let mut encoded = String::new();

	for byte in value.bytes() {
		if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
			encoded.push(char::from(byte));
		} else {
			encoded.push_str(&format!("%{byte:02X}"));
		}
	}

	encoded
}

fn queue_report(
	queue: &Value,
	changed: bool,
	ledger_enabled: bool,
	root: &Path,
	queue_out: &Path,
) -> RadarRefreshQueueReport {
	let counts = queue.get("counts").and_then(Value::as_object);

	RadarRefreshQueueReport {
		changed,
		recent_commits_scanned: count_field(counts, "recent_commits_scanned"),
		published_subjects_seen: count_field(counts, "published_subjects_seen"),
		subjects_queued: count_field(counts, "subjects_queued"),
		ledger_enabled,
		queue_out: absolute_repo_path(root, queue_out),
	}
}

fn count_field(counts: Option<&Map<String, Value>>, field: &str) -> usize {
	counts
		.and_then(|counts| counts.get(field))
		.and_then(Value::as_u64)
		.and_then(|value| usize::try_from(value).ok())
		.unwrap_or_default()
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

fn write_json(path: &Path, payload: &Value) -> crate::prelude::Result<()> {
	if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
		fs::create_dir_all(parent)?;
	}

	let mut output = serde_json::to_string_pretty(payload)?;

	output.push('\n');

	let parent = path.parent().unwrap_or_else(|| Path::new("."));
	let file_name = path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("JSON output path must end in a valid file name"))?;
	let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
	let write_result = (|| -> crate::prelude::Result<()> {
		let mut file = OpenOptions::new().write(true).create_new(true).open(&temp_path)?;

		file.write_all(output.as_bytes())?;
		file.sync_all()?;

		fs::rename(&temp_path, path)?;

		Ok(())
	})();

	if write_result.is_err() {
		let _ = fs::remove_file(&temp_path);
	}

	write_result?;

	Ok(())
}

fn write_new_json(path: &Path, payload: &Value) -> crate::prelude::Result<()> {
	if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
		fs::create_dir_all(parent)?;
	}

	let mut output = serde_json::to_string_pretty(payload)?;

	output.push('\n');

	let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;

	file.write_all(output.as_bytes())?;
	file.sync_all()?;

	Ok(())
}

fn require_member(value: &str, allowed: &[&str], label: &str) -> crate::prelude::Result<()> {
	if allowed.contains(&value) {
		Ok(())
	} else {
		eyre::bail!("{label} must be one of {}", choices(allowed))
	}
}

fn choices(values: &[&str]) -> String {
	let quoted = values.iter().map(|value| format!("'{value}'")).collect::<Vec<_>>().join(", ");

	format!("[{quoted}]")
}

fn utc_now_iso() -> crate::prelude::Result<String> {
	Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

fn object_value<'a>(
	value: &'a Value,
	label: &str,
) -> crate::prelude::Result<&'a Map<String, Value>> {
	value.as_object().ok_or_else(|| eyre::eyre!("{label} must be an object"))
}

fn string_field<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
	object.get(field).and_then(Value::as_str)
}

fn required_string<'a>(
	object: &'a Map<String, Value>,
	field: &str,
	label: &str,
) -> crate::prelude::Result<&'a str> {
	string_field(object, field)
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("{label} must be a non-empty string"))
}

fn optional_string<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
	object.get(field).and_then(Value::as_str)
}

fn is_truthy_json_value(value: Option<&Value>) -> bool {
	match value {
		Some(Value::Null) | None => false,
		Some(Value::String(value)) => !value.is_empty(),
		Some(_) => true,
	}
}

fn non_empty_array(value: Option<&Value>) -> Option<&Vec<Value>> {
	value.and_then(Value::as_array).filter(|values| !values.is_empty())
}

fn first_line(value: &str) -> String {
	value.trim().lines().next().unwrap_or("").into()
}

#[cfg(test)]
mod tests {
	use std::{
		env,
		ffi::OsString,
		fs,
		path::{Path, PathBuf},
		process::Command,
	};

	use serde_json::{self, Value};

	use crate::radar::{
		self, RadarBackfillReleaseRangeRequest, RadarBundleValidateRequest,
		RadarLedgerArtifactLinkRequest, RadarLedgerBootstrapRequest,
		RadarLedgerIngestExistingRequest, RadarRenderSignalRequest,
		RadarSocialReservePublishRequest, RadarValidateRequest, RefreshKind,
	};

	struct TestEnvVars {
		_lock: crate::test_support::TestEnvLockGuard,
		previous: Vec<(String, Option<OsString>)>,
	}

	impl TestEnvVars {
		fn set(vars: &[(&str, Option<&str>)]) -> Self {
			let lock = crate::test_support::TestEnvVarGuard::lock();
			let previous = vars
				.iter()
				.map(|(key, _)| ((*key).to_owned(), env::var_os(key)))
				.collect::<Vec<_>>();

			for (key, value) in vars {
				match value {
					Some(value) => unsafe { env::set_var(key, value) },
					None => unsafe { env::remove_var(key) },
				}
			}

			Self { _lock: lock, previous }
		}
	}

	impl Drop for TestEnvVars {
		fn drop(&mut self) {
			for (key, previous) in self.previous.drain(..).rev() {
				match previous {
					Some(previous) => unsafe { env::set_var(key, previous) },
					None => unsafe { env::remove_var(key) },
				}
			}
		}
	}

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
		let mut state = crate::radar::ValidationState::new();
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
		let mut state = crate::radar::ValidationState::new();
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
		let mut state = crate::radar::ValidationState::new();
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
		let report =
			radar::reserve_social_publish(&request).expect("dry-run reservation should pass");

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
		let mut state = crate::radar::ValidationState::new();
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
		let mut state = crate::radar::ValidationState::new();
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
		let mut state = crate::radar::ValidationState::new();
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
	fn analysis_helper_fails_closed_without_explicit_boundary_opt_in() {
		let _env = TestEnvVars::set(&[("DECODEX_ALLOW_CODEX_ANALYSIS", None)]);
		let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
			.parent()
			.and_then(Path::parent)
			.expect("apps/decodex should live two levels under the repo root");
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let bundle_path = temp_dir.path().join("missing-bundle.json");
		let output_path = temp_dir.path().join("analysis.json");
		let output = Command::new("python3")
			.current_dir(repo_root)
			.arg(repo_root.join(super::RUN_CODEX_ANALYSIS_SCRIPT))
			.arg("--bundle")
			.arg(&bundle_path)
			.arg("--out")
			.arg(&output_path)
			.arg("--repo-root")
			.arg(repo_root)
			.output()
			.expect("Python analysis helper smoke command should execute");
		let stderr = String::from_utf8_lossy(&output.stderr);

		assert!(!output.status.success());
		assert!(
			stderr.contains("requires --allow-ai-analysis-boundary"),
			"unexpected stderr: {stderr}"
		);
		assert!(!output_path.exists());
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

	#[test]
	fn ledger_bootstrap_migrates_social_draft_artifact_kind() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let db_path = temp_dir.path().join("radar.sqlite3");
		let connection =
			rusqlite::Connection::open(&db_path).expect("temporary ledger should open");

		connection
			.execute_batch(
				"
				CREATE TABLE artifact_link (
				  repo TEXT NOT NULL,
				  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
				  subject_id TEXT NOT NULL,
				  artifact_kind TEXT NOT NULL CHECK (
				    artifact_kind IN (
				      'bundle',
				      'analysis',
				      'signal',
				      'upstream_impact',
				      'social_draft',
				      'release_delta',
				      'archive_manifest',
				      'ledger_export'
				    )
				  ),
				  path TEXT NOT NULL,
				  sha256 TEXT NOT NULL,
				  size_bytes INTEGER NOT NULL,
				  created_at TEXT NOT NULL,
				  PRIMARY KEY (repo, subject_kind, subject_id, artifact_kind, path)
				);
				INSERT INTO artifact_link (
				  repo,
				  subject_kind,
				  subject_id,
				  artifact_kind,
				  path,
				  sha256,
				  size_bytes,
				  created_at
				)
				VALUES (
				  'openai/codex',
				  'pr',
				  '22414',
				  'social_draft',
				  '.agent/automations/decodex/cache/social/x/posts/2026-06-01/example.json',
				  'abc123',
				  10,
				  '2026-06-01T00:00:00Z'
				);
				",
			)
			.expect("legacy artifact link schema should be created");

		drop(connection);

		radar::ledger_bootstrap(&RadarLedgerBootstrapRequest { db_path: db_path.clone() })
			.expect("ledger bootstrap should migrate social_draft rows");

		let connection = rusqlite::Connection::open(&db_path).expect("migrated ledger should open");
		let artifact_kind: String = connection
			.query_row("SELECT artifact_kind FROM artifact_link", [], |row| row.get(0))
			.expect("artifact kind should be readable");
		let schema_version: String = connection
			.query_row("SELECT value FROM metadata WHERE key = 'schema_version'", [], |row| {
				row.get(0)
			})
			.expect("schema version should be readable");

		assert_eq!(artifact_kind, "social_post");
		assert_eq!(schema_version, "4");
	}

	#[test]
	fn ledger_ingests_existing_bundle_analysis_and_signal_artifacts() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let bundles_dir = temp_dir.path().join("bundles");
		let analysis_dir = temp_dir.path().join("analysis");
		let signals_dir = temp_dir.path().join("signals");
		let db_path = temp_dir.path().join("radar.sqlite3");

		fs::create_dir_all(&bundles_dir).expect("bundles directory should be created");
		fs::create_dir_all(&analysis_dir).expect("analysis directory should be created");
		fs::create_dir_all(&signals_dir).expect("signals directory should be created");
		fs::write(bundles_dir.join("openai-codex-pr-22414.json"), valid_bundle().to_string())
			.expect("bundle fixture should be written");
		fs::write(
			analysis_dir.join("openai-codex-pr-22414.analysis.json"),
			r#"{"kind":"capability"}"#,
		)
		.expect("analysis fixture should be written");
		fs::write(signals_dir.join("openai-codex-pr-22414.json"), valid_signal().to_string())
			.expect("signal fixture should be written");

		let summary = radar::ledger_ingest_existing(&RadarLedgerIngestExistingRequest {
			db_path: db_path.clone(),
			bundles_dir,
			analysis_dir,
			signals_dir,
		})
		.expect("existing artifacts should ingest");

		assert_eq!(summary.get("bundles_ingested"), Some(&1));
		assert_eq!(summary.get("upstream_commits"), Some(&1));
		assert_eq!(summary.get("radar_reviews"), Some(&1));
		assert_eq!(summary.get("artifact_links"), Some(&3));

		let connection = rusqlite::Connection::open(&db_path).expect("ingested ledger should open");
		let review: (String, String) = connection
			.query_row(
				"SELECT status, confidence FROM radar_review WHERE subject_kind = 'pr'",
				[],
				|row| Ok((row.get(0)?, row.get(1)?)),
			)
			.expect("review row should be readable");

		assert_eq!(review, ("signal".into(), "confirmed".into()));
	}

	#[test]
	fn ledger_artifact_link_records_social_post_artifacts() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let db_path = temp_dir.path().join("radar.sqlite3");
		let social_post_path = temp_dir.path().join("post.json");

		fs::write(&social_post_path, r#"{"schema":"social_post/v1"}"#)
			.expect("social post fixture should be written");

		let summary = radar::ledger_artifact_link(&RadarLedgerArtifactLinkRequest {
			db_path: db_path.clone(),
			repo: "openai/codex".into(),
			subject_kind: "pr".into(),
			subject_id: "22414".into(),
			artifact_kind: "social_post".into(),
			path: social_post_path,
		})
		.expect("artifact link should be recorded");

		assert_eq!(summary.get("artifact_links"), Some(&1));

		let connection =
			rusqlite::Connection::open(&db_path).expect("ledger should open after artifact link");
		let artifact_kind: String = connection
			.query_row("SELECT artifact_kind FROM artifact_link", [], |row| row.get(0))
			.expect("artifact link row should be readable");

		assert_eq!(artifact_kind, "social_post");
	}

	#[test]
	fn ledger_artifact_link_records_social_candidate_after_schema_migration() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let db_path = temp_dir.path().join("radar.sqlite3");
		let social_candidate_path = temp_dir.path().join("candidate.json");
		let connection =
			rusqlite::Connection::open(&db_path).expect("temporary ledger should open");

		connection
			.execute_batch(
				"
				CREATE TABLE artifact_link (
				  repo TEXT NOT NULL,
				  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
				  subject_id TEXT NOT NULL,
				  artifact_kind TEXT NOT NULL CHECK (
				    artifact_kind IN (
				      'bundle',
				      'analysis',
				      'signal',
				      'upstream_impact',
				      'social_post',
				      'release_delta',
				      'archive_manifest',
				      'ledger_export'
				    )
				  ),
				  path TEXT NOT NULL,
				  sha256 TEXT NOT NULL,
				  size_bytes INTEGER NOT NULL,
				  created_at TEXT NOT NULL,
				  PRIMARY KEY (repo, subject_kind, subject_id, artifact_kind, path)
				);
				",
			)
			.expect("legacy artifact link schema should be created");

		drop(connection);

		fs::write(&social_candidate_path, r#"{"schema":"social_candidate/v1"}"#)
			.expect("social candidate fixture should be written");
		radar::ledger_bootstrap(&RadarLedgerBootstrapRequest { db_path: db_path.clone() })
			.expect("ledger bootstrap should add social_candidate artifact kind");

		let summary = radar::ledger_artifact_link(&RadarLedgerArtifactLinkRequest {
			db_path: db_path.clone(),
			repo: "openai/codex".into(),
			subject_kind: "pr".into(),
			subject_id: "22414".into(),
			artifact_kind: "social_candidate".into(),
			path: social_candidate_path,
		})
		.expect("social candidate artifact link should be recorded");

		assert_eq!(summary.get("artifact_links"), Some(&1));
	}

	#[test]
	fn ledger_artifact_link_records_control_plane_upgrade_candidate_after_schema_migration() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let db_path = temp_dir.path().join("radar.sqlite3");
		let candidate_path = temp_dir.path().join("upgrade.json");
		let connection =
			rusqlite::Connection::open(&db_path).expect("temporary ledger should open");

		connection
			.execute_batch(
				"
				CREATE TABLE artifact_link (
				  repo TEXT NOT NULL,
				  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('commit', 'pr')),
				  subject_id TEXT NOT NULL,
				  artifact_kind TEXT NOT NULL CHECK (
				    artifact_kind IN (
				      'bundle',
				      'analysis',
				      'signal',
				      'upstream_impact',
				      'social_candidate',
				      'social_post',
				      'release_delta',
				      'archive_manifest',
				      'ledger_export'
				    )
				  ),
				  path TEXT NOT NULL,
				  sha256 TEXT NOT NULL,
				  size_bytes INTEGER NOT NULL,
				  created_at TEXT NOT NULL,
				  PRIMARY KEY (repo, subject_kind, subject_id, artifact_kind, path)
				);
				",
			)
			.expect("legacy artifact link schema should be created");

		drop(connection);

		fs::write(&candidate_path, r#"{"schema":"control_plane_upgrade_candidate/v1"}"#)
			.expect("upgrade candidate fixture should be written");
		radar::ledger_bootstrap(&RadarLedgerBootstrapRequest { db_path: db_path.clone() })
			.expect("ledger bootstrap should add control-plane upgrade artifact kind");

		let summary = radar::ledger_artifact_link(&RadarLedgerArtifactLinkRequest {
			db_path: db_path.clone(),
			repo: "openai/codex".into(),
			subject_kind: "pr".into(),
			subject_id: "22414".into(),
			artifact_kind: "control_plane_upgrade_candidate".into(),
			path: candidate_path,
		})
		.expect("control-plane upgrade artifact link should be recorded");

		assert_eq!(summary.get("artifact_links"), Some(&1));
	}

	#[test]
	fn builds_pr_bundle_from_fixture_payloads() {
		let patch = format!("{} --config FEATURE_FLAG=1", "a".repeat(910));
		let pr = serde_json::json!({
			"number": 22_414,
			"title": "Add Unix socket endpoint support",
			"body": "Fixes #123 and enables --sandbox.",
			"state": "closed",
			"merged_at": "2026-06-01T00:00:00Z",
			"labels": [{"name": "enhancement"}],
			"html_url": "https://github.com/openai/codex/pull/22414"
		});
		let commits = vec![serde_json::json!({
			"sha": "abc123",
			"html_url": "https://github.com/openai/codex/commit/abc123",
			"author": {"login": "alice"},
			"commit": {
				"message": "Add Unix socket endpoint support\n\nRefs openai/codex#456",
				"author": {
					"name": "Alice",
					"date": "2026-06-01T00:00:00Z"
				}
			}
		})];
		let files = vec![serde_json::json!({
			"filename": "docs/examples/socket.md",
			"status": "modified",
			"additions": 12,
			"deletions": 1,
			"patch": patch
		})];
		let bundle = super::build_pr_bundle_from_sources(
			"openai/codex",
			&pr,
			&commits,
			&files,
			"main",
			&["fixture note".into()],
		)
		.expect("PR bundle should build from fixture payloads");

		assert_errors(&bundle, []);

		assert_eq!(bundle["analysis_mode"], "pr_first");
		assert_eq!(bundle["primary_pr"]["state"], "merged");
		assert_eq!(bundle["primary_pr"]["labels"], serde_json::json!(["enhancement"]));
		assert_eq!(bundle["linked_issues"], serde_json::json!(["#123", "openai/codex#456"]));
		assert_eq!(
			bundle["extracted_flags"],
			serde_json::json!(["--sandbox", "--config", "FEATURE_FLAG=1"])
		);
		assert_eq!(bundle["docs_refs"], serde_json::json!(["docs/examples/socket.md"]));
		assert_eq!(bundle["examples_refs"], serde_json::json!(["docs/examples/socket.md"]));
		assert_eq!(bundle["notes"][1], "fixture note");

		let patch_excerpt =
			bundle["files"][0]["patch_excerpt"].as_str().expect("patch excerpt should be present");

		assert!(patch_excerpt.ends_with("..."));
		assert_eq!(patch_excerpt.chars().count(), 903);
	}

	#[test]
	fn validates_bundle_directories_and_rejects_other_schemas() {
		let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
		let bundle_path = temp_dir.path().join("bundle.json");
		let signal_path = temp_dir.path().join("signal.json");

		fs::write(&bundle_path, valid_bundle().to_string()).expect("bundle should be written");

		let report = radar::validate_bundles(&RadarBundleValidateRequest {
			paths: vec![temp_dir.path().to_path_buf()],
		})
		.expect("bundle directory should validate");

		assert_eq!(report.checked_files, 1);

		fs::write(&signal_path, valid_signal().to_string()).expect("signal should be written");

		let error = radar::validate_bundles(&RadarBundleValidateRequest {
			paths: vec![temp_dir.path().to_path_buf()],
		})
		.expect_err("non-bundle schema should be rejected by bundle validation");
		let message = error.to_string();

		assert!(message.contains("schema must be github_change_bundle/v1"));
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

	fn assert_path_errors<const N: usize>(path: &str, payload: &Value, expected: [&str; N]) {
		let validation = radar::validate_artifact_for_path(Path::new(path), payload);

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

	fn valid_config_feature_catalog() -> Value {
		serde_json::json!({
			"schema": "codex_config_feature_catalog/v1",
			"source_url": "https://raw.githubusercontent.com/openai/codex/main/codex-rs/core/config.schema.json",
			"generated_at": "2026-06-02T00:00:00Z",
			"feature_count": 1,
			"features": [
				{
					"name": "multi_agent_v2",
					"config_path": "features.multi_agent_v2",
					"toml_assignment": "multi_agent_v2 = true",
					"toml_snippet": "[features]\nmulti_agent_v2 = true",
					"cli_enable_flag": "--enable multi_agent_v2",
					"schema_url": "https://raw.githubusercontent.com/openai/codex/main/codex-rs/core/config.schema.json",
					"reference_url": "https://developers.openai.com/codex/config-reference",
					"reference_description": "Enable MultiAgentV2 tools including followup_task; legacy assign_task appears only in older rollout traces.",
					"github_search_url": "https://github.com/openai/codex/search?q=%22multi_agent_v2%22&type=code"
				}
			]
		})
	}

	fn collect_assign_task_reference_violations(
		path: &Path,
		repo_root: &Path,
		offenders: &mut Vec<String>,
	) {
		let Ok(metadata) = fs::metadata(path) else {
			return;
		};

		if metadata.is_dir() {
			let entries = fs::read_dir(path).expect("reference audit directory should be readable");

			for entry in entries {
				let entry = entry.expect("reference audit directory entry should be readable");

				collect_assign_task_reference_violations(&entry.path(), repo_root, offenders);
			}

			return;
		}
		if !metadata.is_file() || !should_audit_multi_agent_v2_reference_file(path) {
			return;
		}

		let text = fs::read_to_string(path).expect("reference audit file should be utf-8 text");
		let lower = text.to_ascii_lowercase();

		if !lower.contains("assign_task") {
			return;
		}
		if lower.contains("followup_task") && radar::has_legacy_multi_agent_v2_context(&lower) {
			return;
		}

		let relative = path.strip_prefix(repo_root).unwrap_or(path);

		offenders.push(relative.display().to_string());
	}

	fn should_audit_multi_agent_v2_reference_file(path: &Path) -> bool {
		let extension = path.extension().and_then(|value| value.to_str());

		matches!(extension, Some("json" | "md" | "py" | "rs" | "ts" | "tsx"))
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

	fn valid_control_plane_upgrade_candidate() -> Value {
		serde_json::json!({
			"schema": "control_plane_upgrade_candidate/v1",
			"slug": "openai-codex-pr-22414-control-plane",
			"repo": "openai/codex",
			"status": "proposed",
			"source_refs": {
				"upstream_reviews": [
					".agent/automations/decodex/cache/github/reviews/openai-codex-pr-22414.review.json"
				],
				"upstream_impacts": [
					".agent/automations/decodex/cache/github/impact/openai-codex-pr-22414.json"
				],
				"urls": ["https://github.com/openai/codex/pull/22414"]
			},
			"observed_change": "Remote Codex can use Unix socket endpoints.",
			"control_plane_impact": "compat_risk",
			"upgrade_path": "compat_risk_mitigation",
			"affected_surfaces": ["app-server protocol"],
			"target_codex": {
				"channel": "stable",
				"version": "0.142.2",
				"tag": "rust-v0.142.2",
				"release_url": "https://github.com/openai/codex/releases/tag/rust-v0.142.2",
				"compatibility_status": "needs_review",
				"matrix_ref": "docs/reference/codex-compatibility-matrix.md#codex-01422"
			},
			"authority": {
				"decision_contract_required": true,
				"program_intake_required": true,
				"mutation_allowed": false,
				"objective_id": "decodex-self-iteration"
			},
			"reason": "The upstream app-server transport change may affect Decodex Control Plane compatibility.",
			"validation_gates": ["decodex probe stdio://", "cargo test -p decodex app_server --lib"],
			"stop_conditions": ["Missing accepted Decision Contract", "Probe failure against the target Codex build"],
			"acceptance_criteria": [
				"Compatibility impact is proven or dismissed with source-backed evidence."
			]
		})
	}

	fn valid_social_candidate() -> Value {
		serde_json::json!({
			"schema": "social_candidate/v1",
			"slug": "openai-codex-pr-22414",
			"repo": "openai/codex",
			"channel": "x",
			"target_account": "decodexspace",
			"mode": "operator_impact",
			"priority": "normal",
			"audience": "Codex operators",
			"candidate_text": [
				"Remote Codex can now use Unix socket endpoints. Source: https://github.com/openai/codex/pull/22414"
			],
			"source_refs": {
				"upstream_reviews": [".agent/automations/decodex/cache/github/reviews/openai-codex-pr-22414.review.json"],
				"upstream_impacts": [".agent/automations/decodex/cache/github/impact/openai-codex-pr-22414.json"],
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
				"reason": "The source-backed review has a clear operator impact angle.",
				"idempotency_key": "x:decodexspace:openai-codex-pr-22414:operator_impact"
			}
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
			"media_refs": ["https://x.com/decodexspace/status/1/photo/1"]
		})
	}

	fn valid_social_publish_reservation() -> Value {
		serde_json::json!({
			"schema": "social_publish_reservation/v1",
			"slug": "openai-codex-pr-22414",
			"channel": "x",
			"target_account": "decodexspace",
			"controller_account": "hackink",
			"mode": "operator_impact",
			"status": "active",
			"idempotency_key": "x:decodexspace:operator_impact:openai-codex-pr-22414",
			"reserved_at": "2026-06-02T02:50:00Z",
			"expires_at": "2026-06-02T03:50:00Z",
			"day": "2026-06-02",
			"timezone": "Asia/Shanghai",
			"candidate_refs": {
				"social_candidates": [
					".agent/automations/decodex/cache/github/social-candidates/openai-codex-pr-22414.json"
				],
				"urls": ["https://github.com/openai/codex/pull/22414"]
			},
			"duplicate_keys": [
				"Remote Codex can now use Unix socket endpoints.",
				"https://github.com/openai/codex/pull/22414"
			],
			"owner": {
				"automation_id": "decodex-x-publisher",
				"branch": "automation/decodex-x-publisher-2026-06-02-pr-22414",
				"pr_url": "https://github.com/hack-ink/decodex/pull/1",
				"run_id": "2026-06-02T02:50:00Z"
			},
			"evidence_notes": [
				"Created before compose after durable records and live profile readback were clear."
			]
		})
	}

	fn valid_radar_archive_manifest() -> Value {
		serde_json::json!({
			"schema": "radar_archive_manifest/v1",
			"archive_id": "radar-archive-2026-06-02",
			"created_at": "2026-06-02T03:30:00Z",
			"retention_days": 21,
			"source_commit": "0123456789abcdef0123456789abcdef01234567",
			"release_tag": "radar-archive-2026-06-02",
			"release_url": "https://github.com/hack-ink/decodex/releases/tag/radar-archive-2026-06-02",
			"archive_asset": {
				"name": "radar-archive-2026-06-02.tar.zst",
				"size_bytes": 1_024,
				"sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
			},
			"checksum_asset": {
				"name": "SHA256SUMS",
				"sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
			},
			"files": [
				{
					"path": ".agent/automations/decodex/cache/github/bundles/openai-codex-pr-22414.json",
					"kind": "bundle",
					"sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
					"size_bytes": 512
				}
			]
		})
	}

	fn social_reserve_request(root: &Path, dry_run: bool) -> RadarSocialReservePublishRequest {
		RadarSocialReservePublishRequest {
			slug: "openai-codex-pr-22414".into(),
			mode: "operator_impact".into(),
			idempotency_key: "x:decodexspace:operator_impact:openai-codex-pr-22414".into(),
			reserved_at: "2026-06-02T02:50:00Z".into(),
			expires_at: "2026-06-02T03:50:00Z".into(),
			day: "2026-06-02".into(),
			timezone: "Asia/Shanghai".into(),
			candidate_paths: Vec::new(),
			urls: vec!["https://github.com/openai/codex/pull/22414".into()],
			duplicate_keys: vec![
				"Remote Codex can now use Unix socket endpoints.".into(),
				"https://github.com/openai/codex/pull/22414".into(),
			],
			out_dir: root.join("reservations"),
			posts_dir: root.join("posts"),
			automation_id: Some("decodex-x-publisher".into()),
			run_id: Some("2026-06-02T02:50:00Z".into()),
			branch: Some("xy/agent-home-cutover".into()),
			daily_limit: 8,
			dry_run,
		}
	}
}
