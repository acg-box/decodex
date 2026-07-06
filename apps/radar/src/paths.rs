//! Canonical Radar-owned repository paths.

pub(crate) const DEFAULT_CONFIG_PATH: &str = "automations/radar/radar.toml";
pub(crate) const DEFAULT_LEDGER_PATH: &str = ".agent/automations/radar/cache/github/radar.sqlite3";
pub(crate) const DEFAULT_QUEUE_OUT: &str =
	".agent/automations/radar/cache/github/review-queue/openai-codex-latest.json";
pub(crate) const DEFAULT_RELEASE_DELTA_OUT: &str =
	".agent/automations/radar/cache/site-content/release-deltas/openai-codex-latest.json";
pub(crate) const DEFAULT_SIGNALS_DIR: &str = ".agent/automations/radar/cache/site-content/signals";
pub(crate) const CONFIG_FEATURE_CATALOG_PATH: &str =
	".agent/automations/radar/cache/generated/codex-config-features.json";
pub(crate) const RUN_CODEX_ANALYSIS_SCRIPT: &str =
	"automations/radar/scripts/github/run_codex_analysis.py";
pub(crate) const DEFAULT_BUNDLES_DIR: &str = ".agent/automations/radar/cache/github/bundles";
pub(crate) const DEFAULT_ANALYSIS_DIR: &str = ".agent/automations/radar/cache/generated/analysis";
pub(crate) const DEFAULT_VALIDATION_PATHS: &[&str] = &[
	".agent/automations/radar/cache/github/bundles",
	".agent/automations/radar/cache/github/review-queue",
	".agent/automations/radar/cache/github/reviews",
	".agent/automations/radar/cache/github/impact",
	".agent/automations/radar/cache/github/control-plane-upgrades",
	".agent/automations/radar/cache/site-content/signals",
	".agent/automations/radar/cache/site-content/release-deltas",
	".agent/automations/radar/cache/generated",
];
