use crate::prelude::Result;

use super::{AccountListResponse, AccountUseResponse};

pub(super) fn print_list_response(response: &AccountListResponse, json: bool) -> Result<()> {
	if json {
		println!("{}", serde_json::to_string_pretty(response)?);

		return Ok(());
	}

	println!(
		"Codex account pool: {} ({})",
		response.control.mode,
		response.control.account_selector.as_deref().unwrap_or("balanced selection")
	);
	println!("accounts: {}", response.accounts.len());

	for account in &response.accounts {
		let marker = if account.selected { "*" } else { "-" };
		let email = account.email.as_deref().unwrap_or("no email");

		println!("{marker} {email} {} {}", account.account_fingerprint, account.status);
	}

	Ok(())
}

pub(super) fn print_use_response(response: &AccountUseResponse, json: bool) -> Result<()> {
	if json {
		println!("{}", serde_json::to_string_pretty(response)?);

		return Ok(());
	}

	println!(
		"Codex auth now uses {} ({})",
		response.account.email.as_deref().unwrap_or("no email"),
		response.account.account_fingerprint
	);
	println!("auth: {}", response.codex_auth_path);

	Ok(())
}
