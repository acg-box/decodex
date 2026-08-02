//! Hard publication boundaries for the Decodex content agents.

mod cli;
mod filesystem;
mod social_clock;
mod social_contracts;
mod social_evidence;
mod social_outcome_store;
mod social_publish;
mod social_record;
mod social_skip;
mod social_validation;
mod social_workflow;
mod social_xurl;
mod prelude {
	pub use color_eyre::{Result, eyre};
}

#[cfg(test)] pub(crate) use self::filesystem::repo_local_test_directory;
pub(crate) use self::{
	filesystem::{
		collect_json_files, ensure_private_directory, load_json, load_json_with_sha256,
		load_json_with_sha256_bounded, open_or_create_private_lock, path_arg,
		replace_existing_json, repo_root, require_contained_regular_file, resolve_against,
		write_new_json,
	},
	social_clock::SocialClock,
	social_contracts::{
		SocialObserveDueReport, SocialObserveDueRequest, SocialObserveXurlReport,
		SocialObserveXurlRequest, SocialProbeXurlReport, SocialPublishNextReport,
		SocialPublishNextRequest, SocialPublishXurlReport, SocialPublishXurlRequest,
		SocialReconcileXurlReport, SocialReconcileXurlRequest, SocialRefreshPricingReport,
		SocialReservePublishReport, SocialReservePublishRequest, SocialSealXurlAuthReport,
		SocialSealXurlAuthRequest, SocialTerminalizeSkipReport, SocialTerminalizeSkipRequest,
		SocialValidationReport, SocialXurlCostReport, XPricingPolicyReport, XPricingRatesReport,
		XurlAuthorizationContractReport,
	},
	social_record::{SocialRecordCandidateRequest, record_social_candidate},
};

use std::path::PathBuf;

use clap::Parser as _;
use serde_json::Value;

use cli::Cli;
use prelude::{Result, eyre};

pub(crate) const SOCIAL_CANDIDATE_SCHEMA: &str = "decodex/content-evidence/1";
pub(crate) const SOCIAL_OUTCOME_SCHEMA: &str = "social_outcome/v1";
pub(crate) const SOCIAL_POST_SCHEMA: &str = "social_post/v1";
pub(crate) const SOCIAL_PUBLISH_RESERVATION_SCHEMA: &str = "social_publish_reservation/v1";
pub(crate) const DEFAULT_SOCIAL_CANDIDATES_DIR: &str =
	".agent/automations/decodex/cache/social/x/candidates";
pub(crate) const DEFAULT_SOCIAL_ATTEMPTS_DIR: &str =
	".agent/automations/decodex/cache/social/x/xurl-attempts";
pub(crate) const DEFAULT_SOCIAL_RESERVATIONS_DIR: &str =
	".agent/automations/decodex/cache/social/x/reservations";
pub(crate) const DEFAULT_SOCIAL_POSTS_DIR: &str = ".agent/automations/decodex/cache/social/x/posts";
pub(crate) const DEFAULT_SOCIAL_OUTCOMES_DIR: &str =
	".agent/automations/decodex/cache/social/x/outcomes";
pub(crate) const DEFAULT_SOCIAL_LOCKS_DIR: &str = ".agent/automations/decodex/cache/social/x/locks";
pub(crate) const DEFAULT_XURL_AUTH_CONTRACT_PATH: &str =
	".agent/automations/decodex/cache/social/x/xurl-authorization-contract.json";
pub(crate) const DEFAULT_SOCIAL_STAGING_DIR: &str =
	".agent/automations/decodex/cache/manager/staging";
pub(crate) const SOCIAL_DAILY_LIMIT: usize = 1;
pub(crate) const SOCIAL_MONTHLY_BUDGET_MICROUSD: u64 = 1_250_000;
pub(crate) const SOCIAL_TIMEZONE: &str = "UTC";

/// Run the Decodex Publisher CLI.
pub fn run() -> Result<()> {
	color_eyre::install()?;

	Cli::parse().run()
}

pub(crate) fn reserve_social_publish(
	request: &SocialReservePublishRequest,
) -> Result<SocialReservePublishReport> {
	social_publish::reserve_social_publish(request)
}

pub(crate) fn publish_social_xurl(
	request: &SocialPublishXurlRequest,
) -> Result<SocialPublishXurlReport> {
	social_xurl::publish(request)
}

pub(crate) fn observe_social_xurl(
	request: &SocialObserveXurlRequest,
) -> Result<SocialObserveXurlReport> {
	social_xurl::observe(request)
}

pub(crate) fn probe_social_xurl(now: &str) -> Result<SocialProbeXurlReport> {
	social_xurl::probe(now)
}

pub(crate) fn refresh_social_x_pricing(now: &str) -> Result<SocialRefreshPricingReport> {
	social_xurl::refresh_pricing(now)
}

pub(crate) fn report_social_xurl_cost(billing_month: &str) -> Result<SocialXurlCostReport> {
	social_xurl::cost_report(billing_month)
}

pub(crate) fn seal_social_xurl_auth(
	request: &SocialSealXurlAuthRequest,
) -> Result<SocialSealXurlAuthReport> {
	social_xurl::seal_auth(request)
}

pub(crate) fn reconcile_social_xurl(
	request: &SocialReconcileXurlRequest,
) -> Result<SocialReconcileXurlReport> {
	social_xurl::reconcile(request)
}

pub(crate) fn terminalize_social_skip(
	request: &SocialTerminalizeSkipRequest,
) -> Result<SocialTerminalizeSkipReport> {
	social_skip::terminalize_social_skip(request)
}

pub(crate) fn validate_social(paths: &[PathBuf]) -> Result<SocialValidationReport> {
	let root = repo_root()?;
	validate_social_at(&root, paths)
}

fn validate_social_at(root: &std::path::Path, paths: &[PathBuf]) -> Result<SocialValidationReport> {
	let default_scope = paths.is_empty();
	let paths = if default_scope {
		vec![
			PathBuf::from(DEFAULT_SOCIAL_CANDIDATES_DIR),
			PathBuf::from(DEFAULT_SOCIAL_OUTCOMES_DIR),
			PathBuf::from(DEFAULT_SOCIAL_RESERVATIONS_DIR),
			PathBuf::from(DEFAULT_SOCIAL_POSTS_DIR),
		]
	} else {
		paths.to_vec()
	};
	let files = collect_json_files(
		&paths.iter().map(|path| resolve_against(root, path)).collect::<Vec<_>>(),
	)?;
	let mut errors = Vec::new();

	for path in &files {
		let payload = load_json(path)?;
		let validation = social_validation::validate_social_artifact_for_path(path, &payload);

		for error in validation.errors {
			errors.push(format!("{}: {error}", path_arg(root, path)));
		}
	}
	if default_scope
		&& errors.is_empty()
		&& let Err(error) = social_outcome_store::validated_observed_windows(
			root,
			&PathBuf::from(DEFAULT_SOCIAL_OUTCOMES_DIR),
			&PathBuf::from(DEFAULT_SOCIAL_POSTS_DIR),
		) {
		errors.push(error.to_string());
	}

	if !errors.is_empty() {
		return Err(eyre::eyre!("Social artifact validation failed:\n- {}", errors.join("\n- ")));
	}
	Ok(SocialValidationReport { checked_files: files.len(), errors })
}

pub(crate) fn validate_generated_social_artifact(payload: &Value) -> Result<()> {
	let validation = social_validation::validate_social_artifact(payload);

	if !validation.errors.is_empty() {
		eyre::bail!("Social artifact validation failed:\n- {}", validation.errors.join("\n- "));
	}
	social_record::validate_publication_identity(payload)?;

	Ok(())
}

pub(crate) fn publish_next(request: &SocialPublishNextRequest) -> Result<SocialPublishNextReport> {
	social_workflow::publish_next(request)
}

pub(crate) fn observe_due(request: &SocialObserveDueRequest) -> Result<SocialObserveDueReport> {
	social_workflow::observe_due(request)
}

#[cfg(test)] mod tests;
