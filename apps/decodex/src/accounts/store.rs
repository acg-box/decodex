mod config;
mod records;
mod selection;

use std::path::PathBuf;

use crate::{
	accounts::{auth_json, types::AccountListResponse},
	prelude::Result,
	runtime,
};

pub(crate) struct AccountStore {
	pub(super) accounts_path: PathBuf,
	pub(super) global_config_path: PathBuf,
	pub(in crate::accounts) codex_auth_path: PathBuf,
}
impl AccountStore {
	pub(crate) fn global() -> Result<Self> {
		Ok(Self {
			accounts_path: runtime::accounts_path()?,
			global_config_path: runtime::global_config_path()?,
			codex_auth_path: auth_json::default_codex_auth_json_path()?,
		})
	}

	#[cfg(test)]
	pub(super) fn new(accounts_path: PathBuf, global_config_path: PathBuf) -> Self {
		let codex_auth_path = accounts_path
			.parent()
			.map(|parent| parent.join("auth.json"))
			.unwrap_or_else(|| PathBuf::from("auth.json"));

		Self { accounts_path, global_config_path, codex_auth_path }
	}

	#[cfg(test)]
	pub(super) fn new_with_codex_auth_path(
		accounts_path: PathBuf,
		global_config_path: PathBuf,
		codex_auth_path: PathBuf,
	) -> Self {
		Self { accounts_path, global_config_path, codex_auth_path }
	}

	pub(super) fn list(&self) -> Result<AccountListResponse> {
		let records = self.load_records()?;

		self.response_from_records(&records)
	}
}
