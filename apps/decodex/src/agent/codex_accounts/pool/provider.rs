use crate::agent::codex_accounts::{
	CodexAccountLogin,
	pool::{CodexAccountPool, CodexAccountProvider},
};
use crate::prelude::{Result, eyre};

impl CodexAccountPool {
	pub(in crate::agent::codex_accounts::pool) fn remember_selected_account(
		&self,
		account_id: &str,
	) -> Result<()> {
		let mut selected = self
			.selected_account_id
			.lock()
			.map_err(|_| eyre::eyre!("Codex accounts selection lock was poisoned."))?;

		*selected = Some(account_id.to_owned());

		Ok(())
	}

	pub(in crate::agent::codex_accounts::pool) fn selected_account_id(
		&self,
	) -> Result<Option<String>> {
		self.selected_account_id
			.lock()
			.map(|selected| selected.clone())
			.map_err(|_| eyre::eyre!("Codex accounts selection lock was poisoned."))
	}
}

impl CodexAccountProvider for CodexAccountPool {
	fn select_account(&self) -> Result<CodexAccountLogin> {
		let _guard = self.lock_records()?;
		let mut records = self.load_records()?;

		self.select_from_records(&mut records)
	}

	fn refresh_account(&self, previous_account_id: Option<&str>) -> Result<CodexAccountLogin> {
		let _guard = self.lock_records()?;
		let mut records = self.load_records()?;

		self.refresh_from_records(&mut records, previous_account_id)
	}
}
