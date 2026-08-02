use std::time::Duration;

use reqwest::StatusCode;

use crate::paths;

pub(crate) const BUNDLE_SCHEMA: &str = "github_change_bundle/v1";
pub(crate) const BUNDLE_BUILD_RECEIPT_SCHEMA: &str = "radar_bundle_build_receipt/v1";
pub(crate) const CACHE_MAX_AGE_DAYS: u64 = 30;
pub(crate) const CACHE_MAX_BYTES_PER_COLLECTION: u64 = 64 * 1024 * 1024;
pub(crate) const CACHE_MAX_FILES_PER_COLLECTION: usize = 256;
pub(crate) const CONTROL_PLANE_UPGRADE_CANDIDATE_SCHEMA: &str =
	"control_plane_upgrade_candidate/v1";
pub(crate) const DEFAULT_LEDGER_PATH: &str = paths::DEFAULT_LEDGER_PATH;
pub(crate) const DEFAULT_CACHE_ROOT: &str = paths::DEFAULT_CACHE_ROOT;
pub(crate) const DEFAULT_MIN_STABLE_TAG: &str = "rust-v0.116.0";
pub(crate) const DEFAULT_PAIR_LIMIT: usize = 24;
pub(crate) const DEFAULT_PREVIEW_LIMIT: usize = 0;
pub(crate) const DEFAULT_QUEUE_OUT: &str = paths::DEFAULT_QUEUE_OUT;
pub(crate) const DEFAULT_RELEASE_DELTA_OUT: &str = paths::DEFAULT_RELEASE_DELTA_OUT;
pub(crate) const DEFAULT_SEARCH_LIMIT: usize = 40;
pub(crate) const DEFAULT_SIGNALS_DIR: &str = paths::DEFAULT_SIGNALS_DIR;
pub(crate) const DEFAULT_SOURCE_MAX_AGE_HOURS: u64 = 12;
pub(crate) const DEFAULT_STABLE_LIMIT: usize = 0;
pub(crate) const DEFAULT_TAG_PREFIX: &str = "rust-v";
pub(crate) const RELEASE_DELTA_SCHEMA: &str = "release_delta/v1";
pub(crate) const LEDGER_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const LEDGER_MAX_ROWS_PER_TABLE: usize = 10_000;
pub(crate) const RETAINED_CACHE_COLLECTIONS: &[&str] = paths::RETAINED_CACHE_COLLECTIONS;
pub(crate) const SCHEMA_VERSION: i64 = 6;
pub(crate) const SIGNAL_SCHEMA: &str = "signal_entry/v1";
pub(crate) const UPSTREAM_IMPACT_SCHEMA: &str = "upstream_impact/v1";
pub(crate) const UPSTREAM_REVIEW_QUEUE_SCHEMA: &str = "upstream_review_queue/v1";
pub(crate) const UPSTREAM_REVIEW_SCHEMA: &str = "upstream_review/v1";
pub(crate) const CONFIG_FEATURE_CATALOG_SCHEMA: &str = "codex_config_feature_catalog/v1";
pub(crate) const ANALYSIS_DRAFT_KIND: &str = "analysis_draft";
pub(crate) const SIGNAL_CONFIDENCE: &[&str] = &["confirmed", "likely", "weak"];
pub(crate) const UPSTREAM_SUBJECT_KINDS: &[&str] = &["commit", "pr"];
pub(crate) const DEFAULT_VALIDATION_PATHS: &[&str] = paths::DEFAULT_VALIDATION_PATHS;
pub(crate) const GENERIC_COMMIT_TITLES: &[&str] =
	&["update", "fix", "fix.", "fix tests", "fix tests.", "merge fixes", "flaky syntax"];
pub(crate) const CONFIG_FEATURE_CATALOG_PATH: &str = paths::CONFIG_FEATURE_CATALOG_PATH;
pub(crate) const RUN_CODEX_ANALYSIS_SCRIPT: &str = paths::RUN_CODEX_ANALYSIS_SCRIPT;
pub(crate) const HIGH_VALUE_SURFACES: &[&str] = &[
	"app_server_protocol",
	"mcp_plugins",
	"browser_chrome",
	"sandbox_permissions",
	"config_hooks",
	"auth_accounts",
	"model_provider",
];
pub(crate) const ATTENTION_RULES: &[(&str, &[&str])] = &[
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
pub(crate) const SURFACE_RULES: &[(&str, &[&str])] = &[
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
pub(crate) const REVIEW_STATUSES: &[&str] =
	&["control_plane", "deprecated", "seen", "signal", "skipped", "watch"];
pub(crate) const ARTIFACT_KINDS: &[&str] = &[
	"analysis",
	"bundle",
	"control_plane_upgrade_candidate",
	"release_delta",
	"signal",
	"upstream_impact",
];
pub(crate) const GITHUB_REQUEST_ATTEMPTS: usize = 4;
pub(crate) const GITHUB_REQUEST_BACKOFF: Duration = Duration::from_secs(1);
pub(crate) const GITHUB_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const RETRYABLE_GITHUB_STATUS_CODES: &[StatusCode] = &[
	StatusCode::TOO_MANY_REQUESTS,
	StatusCode::INTERNAL_SERVER_ERROR,
	StatusCode::BAD_GATEWAY,
	StatusCode::SERVICE_UNAVAILABLE,
	StatusCode::GATEWAY_TIMEOUT,
];
