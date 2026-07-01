mod construction;
mod provider;
mod record_selection;
mod refresh_flow;
mod usage_probe;

use std::{path::PathBuf, sync::Mutex};

use reqwest::blocking::Client;

use crate::{agent::codex_accounts::CodexAccountLogin, prelude::Result};

pub(crate) trait CodexAccountProvider {
	fn select_account(&self) -> Result<CodexAccountLogin>;
	fn refresh_account(&self, previous_account_id: Option<&str>) -> Result<CodexAccountLogin>;
}

pub(crate) struct CodexAccountPool {
	pub(super) path: PathBuf,
	pub(super) usage_endpoint: String,
	pub(super) profile_endpoint: Option<String>,
	pub(super) refresh_endpoint: String,
	pub(super) fixed_account: Option<String>,
	pub(super) codex_auth_path: PathBuf,
	pub(super) client: Client,
	pub(super) selected_account_id: Mutex<Option<String>>,
}
