//! Decodex auxiliary publishing handoff tooling.

mod cli;
mod filesystem;
mod social_browser_lease;
mod social_contracts;
mod social_publish;
mod social_skip;
mod social_validation;
mod prelude {
	pub use color_eyre::{Result, eyre};
}

pub(crate) use self::{
	filesystem::{
		collect_json_files, load_json, path_arg, repo_root, resolve_against, write_new_json,
	},
	social_contracts::{
		SocialBrowserLeaseReport, SocialReservePublishReport, SocialReservePublishRequest,
		SocialTerminalizeSkipReport, SocialTerminalizeSkipRequest, SocialValidationReport,
	},
};

use std::path::PathBuf;

use clap::Parser as _;
use serde_json::Value;

use cli::Cli;
use prelude::{Result, eyre};
use social_validation::SocialValidationState;

pub(crate) const SOCIAL_CANDIDATE_SCHEMA: &str = "social_candidate/v1";
pub(crate) const SOCIAL_OUTCOME_SCHEMA: &str = "social_outcome/v1";
pub(crate) const SOCIAL_POST_SCHEMA: &str = "social_post/v1";
pub(crate) const SOCIAL_PUBLISH_RESERVATION_SCHEMA: &str = "social_publish_reservation/v1";
pub(crate) const SOCIAL_STRATEGY_SCHEMA: &str = "social_strategy/v1";
pub(crate) const DEFAULT_SOCIAL_CANDIDATES_DIR: &str =
	".agent/automations/decodex/cache/social/x/candidates";
pub(crate) const DEFAULT_SOCIAL_RESERVATIONS_DIR: &str =
	".agent/automations/decodex/cache/social/x/reservations";
pub(crate) const DEFAULT_SOCIAL_POSTS_DIR: &str = ".agent/automations/decodex/cache/social/x/posts";
pub(crate) const DEFAULT_SOCIAL_OUTCOMES_DIR: &str =
	".agent/automations/decodex/cache/social/x/outcomes";
pub(crate) const DEFAULT_SOCIAL_LOCKS_DIR: &str = ".agent/automations/decodex/cache/social/x/locks";
pub(crate) const DEFAULT_SOCIAL_STRATEGIES_DIR: &str =
	".agent/automations/decodex/cache/manager/strategy";

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

pub(crate) fn terminalize_social_skip(
	request: &SocialTerminalizeSkipRequest,
) -> Result<SocialTerminalizeSkipReport> {
	social_skip::terminalize_social_skip(request)
}

pub(crate) fn acquire_social_browser_lease(
	out_dir: &std::path::Path,
	ttl_seconds: u64,
) -> Result<SocialBrowserLeaseReport> {
	social_browser_lease::acquire(out_dir, ttl_seconds)
}

pub(crate) fn verify_social_browser_lease(
	out_dir: &std::path::Path,
	lease_token: &str,
) -> Result<SocialBrowserLeaseReport> {
	social_browser_lease::verify(out_dir, lease_token)
}

pub(crate) fn renew_social_browser_lease(
	out_dir: &std::path::Path,
	lease_token: &str,
	ttl_seconds: u64,
) -> Result<SocialBrowserLeaseReport> {
	social_browser_lease::renew(out_dir, lease_token, ttl_seconds)
}

pub(crate) fn release_social_browser_lease(
	out_dir: &std::path::Path,
	lease_token: &str,
) -> Result<SocialBrowserLeaseReport> {
	social_browser_lease::release(out_dir, lease_token)
}

pub(crate) fn validate_social(paths: &[PathBuf]) -> Result<SocialValidationReport> {
	let root = repo_root()?;
	let paths = if paths.is_empty() {
		vec![
			PathBuf::from(DEFAULT_SOCIAL_CANDIDATES_DIR),
			PathBuf::from(DEFAULT_SOCIAL_OUTCOMES_DIR),
			PathBuf::from(DEFAULT_SOCIAL_RESERVATIONS_DIR),
			PathBuf::from(DEFAULT_SOCIAL_POSTS_DIR),
			PathBuf::from(DEFAULT_SOCIAL_STRATEGIES_DIR),
		]
	} else {
		paths.to_vec()
	};
	let files = collect_json_files(
		&paths.iter().map(|path| resolve_against(&root, path)).collect::<Vec<_>>(),
	)?;
	let mut state = SocialValidationState::new();
	let mut errors = Vec::new();

	for path in &files {
		let payload = load_json(path)?;
		let validation = social_validation::validate_social_artifact_for_path(path, &payload);

		for error in validation.errors {
			errors.push(format!("{}: {error}", path_arg(&root, path)));
		}

		social_validation::validate_social_cross_file_constraints(
			path,
			&payload,
			&mut state,
			&mut errors,
		);
	}
	state.finish(&mut errors);

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

	Ok(())
}

#[cfg(test)] mod tests;
