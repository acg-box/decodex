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

#[cfg(test)] use artifact_validation::has_legacy_multi_agent_v2_context;
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
use review_queue::{RecentCommit, build_review_queue};
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
				Some(pr_number) =>
					client.build_pr_bundle(&request.repo, pr_number, &request.notes)?,
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

#[cfg(test)] mod tests;
