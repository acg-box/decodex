use std::path::{Path, PathBuf};

use crate::{
	accounts::{
		output,
		store::AccountStore,
		types::{AccountImportRequest, AccountListResponse, AccountUseRequest, AccountUseResponse},
	},
	agent::CodexAccountPool,
	config::ServiceConfig,
	prelude::Result,
	runtime,
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
	let store = AccountStore::global()?;
	let mut response = store.list()?;

	hydrate_account_list_usage_from_configured_pool(&mut response, force_refresh);

	Ok(response)
}

pub(crate) fn hydrate_account_list_usage(mut response: AccountListResponse) -> AccountListResponse {
	hydrate_account_list_usage_from_configured_pool(&mut response, false);

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

fn hydrate_account_list_usage_from_configured_pool(
	response: &mut AccountListResponse,
	force_refresh: bool,
) {
	let accounts_path = PathBuf::from(&response.accounts_path);

	match configured_account_pool() {
		Ok(Some(pool)) => response.hydrate_usage_from_pool(&pool, &accounts_path, force_refresh),
		Ok(None) => response.hydrate_usage_from_path(&accounts_path, force_refresh),
		Err(error) => response.usage_probe_error = Some(error.to_string()),
	}
}

fn configured_account_pool() -> Result<Option<CodexAccountPool>> {
	let state_store = runtime::open_runtime_store()?;

	for project in state_store.list_projects()? {
		if !project.enabled() {
			continue;
		}

		let config = ServiceConfig::from_path(project.config_path())?;
		let Some(accounts) = config.codex().accounts() else {
			continue;
		};

		return CodexAccountPool::from_config(accounts).map(Some);
	}

	Ok(None)
}
