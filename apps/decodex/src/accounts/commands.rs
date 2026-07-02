use std::path::{Path, PathBuf};

use crate::{
	accounts::{
		output,
		store::AccountStore,
		types::{AccountImportRequest, AccountListResponse, AccountUseRequest, AccountUseResponse},
	},
	prelude::Result,
};

pub(crate) fn run_account_list(json: bool) -> Result<()> {
	output::print_list_response(&account_list()?, json)
}

pub(crate) fn run_account_select(selector: &str, json: bool) -> Result<()> {
	output::print_list_response(&account_select(selector)?, json)
}

pub(crate) fn run_account_clear(json: bool) -> Result<()> {
	output::print_list_response(&account_clear()?, json)
}

pub(crate) fn run_account_logout(selector: &str, json: bool) -> Result<()> {
	output::print_list_response(&account_logout(selector)?, json)
}

pub(crate) fn run_account_import(request: &AccountImportRequest) -> Result<()> {
	output::print_list_response(&account_import(&request.auth_json_path)?, request.json)
}

pub(crate) fn run_account_use(request: &AccountUseRequest) -> Result<()> {
	output::print_use_response(&account_use(request)?, request.json)
}

pub(crate) fn account_list() -> Result<AccountListResponse> {
	AccountStore::global()?.list()
}

pub(crate) fn account_list_with_cached_usage(force_refresh: bool) -> Result<AccountListResponse> {
	AccountStore::global()?.list_with_cached_usage(force_refresh)
}

pub(crate) fn hydrate_account_list_usage(mut response: AccountListResponse) -> AccountListResponse {
	let accounts_path = PathBuf::from(&response.accounts_path);

	response.hydrate_usage_from_path(&accounts_path, false);

	response
}

pub(crate) fn account_select(selector: &str) -> Result<AccountListResponse> {
	AccountStore::global()?.select(selector)
}

pub(crate) fn account_clear() -> Result<AccountListResponse> {
	AccountStore::global()?.clear_selection()
}

pub(crate) fn account_logout(selector: &str) -> Result<AccountListResponse> {
	AccountStore::global()?.logout(selector)
}

pub(crate) fn account_reroll_name(
	selector: &str,
	offset: Option<i64>,
) -> Result<AccountListResponse> {
	AccountStore::global()?.reroll_name(selector, offset)
}

pub(crate) fn account_import(auth_json_path: &Path) -> Result<AccountListResponse> {
	AccountStore::global()?.import_auth_json(auth_json_path)
}

pub(crate) fn account_use(request: &AccountUseRequest) -> Result<AccountUseResponse> {
	AccountStore::global()?.use_for_codex(&request.selector, request.auth_json_path.as_deref())
}
