use std::{
	path::{Path, PathBuf},
	sync::Mutex,
};

use reqwest::blocking::Client;

use crate::{
	agent::codex_accounts::{
		DEFAULT_REFRESH_ENDPOINT, DEFAULT_RESET_CREDITS_ENDPOINT, DEFAULT_USAGE_ENDPOINT,
		HTTP_TIMEOUT, pool::CodexAccountPool, record, usage,
	},
	config::ProjectCodexAccountsConfig,
	prelude::Result,
	runtime,
};

impl CodexAccountPool {
	pub(crate) fn from_config(config: &ProjectCodexAccountsConfig) -> Result<Self> {
		let fixed_account = runtime::global_fixed_account_selector()?;
		let usage_endpoint = config.usage_endpoint().unwrap_or(DEFAULT_USAGE_ENDPOINT);

		Self::new_with_fixed_account_and_profile_endpoint(
			runtime::accounts_path()?,
			usage_endpoint,
			config.profile_endpoint(),
			config.reset_credits_endpoint().unwrap_or(DEFAULT_RESET_CREDITS_ENDPOINT),
			config.refresh_endpoint().unwrap_or(DEFAULT_REFRESH_ENDPOINT),
			fixed_account.as_deref(),
		)
	}

	pub(crate) fn from_accounts_path(path: impl AsRef<Path>) -> Result<Self> {
		Self::new_with_fixed_account(
			path,
			DEFAULT_USAGE_ENDPOINT,
			DEFAULT_RESET_CREDITS_ENDPOINT,
			DEFAULT_REFRESH_ENDPOINT,
			None,
		)
	}

	pub(in crate::agent::codex_accounts) fn new_with_fixed_account(
		path: impl AsRef<Path>,
		usage_endpoint: impl Into<String>,
		reset_credits_endpoint: impl Into<String>,
		refresh_endpoint: impl Into<String>,
		fixed_account: Option<&str>,
	) -> Result<Self> {
		Self::new_with_fixed_account_and_profile_endpoint(
			path,
			usage_endpoint,
			None,
			reset_credits_endpoint,
			refresh_endpoint,
			fixed_account,
		)
	}

	fn new_with_fixed_account_and_profile_endpoint(
		path: impl AsRef<Path>,
		usage_endpoint: impl Into<String>,
		profile_endpoint: Option<&str>,
		reset_credits_endpoint: impl Into<String>,
		refresh_endpoint: impl Into<String>,
		fixed_account: Option<&str>,
	) -> Result<Self> {
		Self::new_with_fixed_account_profile_and_codex_auth_path(
			path,
			usage_endpoint,
			profile_endpoint,
			reset_credits_endpoint,
			refresh_endpoint,
			fixed_account,
			record::default_codex_auth_json_path()?,
		)
	}

	#[cfg(test)]
	pub(in crate::agent::codex_accounts) fn new_with_fixed_account_and_codex_auth_path(
		path: impl AsRef<Path>,
		usage_endpoint: impl Into<String>,
		reset_credits_endpoint: impl Into<String>,
		refresh_endpoint: impl Into<String>,
		fixed_account: Option<&str>,
		codex_auth_path: impl Into<PathBuf>,
	) -> Result<Self> {
		Self::new_with_fixed_account_profile_and_codex_auth_path(
			path,
			usage_endpoint,
			None,
			reset_credits_endpoint,
			refresh_endpoint,
			fixed_account,
			codex_auth_path,
		)
	}

	fn new_with_fixed_account_profile_and_codex_auth_path(
		path: impl AsRef<Path>,
		usage_endpoint: impl Into<String>,
		profile_endpoint: Option<&str>,
		reset_credits_endpoint: impl Into<String>,
		refresh_endpoint: impl Into<String>,
		fixed_account: Option<&str>,
		codex_auth_path: impl Into<PathBuf>,
	) -> Result<Self> {
		let client = Client::builder().timeout(HTTP_TIMEOUT).build()?;
		let usage_endpoint = usage_endpoint.into();
		let profile_endpoint = profile_endpoint
			.and_then(|endpoint| usage::nonblank_string(Some(endpoint)))
			.or_else(|| record::default_profile_endpoint_for_usage_endpoint(&usage_endpoint));

		Ok(Self {
			path: path.as_ref().to_path_buf(),
			usage_endpoint,
			profile_endpoint,
			reset_credits_endpoint: reset_credits_endpoint.into(),
			refresh_endpoint: refresh_endpoint.into(),
			fixed_account: fixed_account
				.map(str::trim)
				.filter(|selector| !selector.is_empty())
				.map(str::to_owned),
			codex_auth_path: codex_auth_path.into(),
			client,
			selected_account_id: Mutex::new(None),
		})
	}
}
