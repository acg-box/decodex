use std::path::{Path, PathBuf};

use crate::{
	accounts,
	orchestrator::operator_http::{
		self, AccountUseRequest, OperatorAccountRequest, OperatorRequestRoute, Result, eyre,
	},
};

pub(crate) fn operator_request_route_is_account_api(route: &OperatorRequestRoute) -> bool {
	matches!(
		route,
		OperatorRequestRoute::AccountList { .. }
			| OperatorRequestRoute::AccountSelect
			| OperatorRequestRoute::AccountClear
			| OperatorRequestRoute::AccountLogout
			| OperatorRequestRoute::AccountImport
			| OperatorRequestRoute::AccountUse
			| OperatorRequestRoute::AccountRerollName
	)
}

pub(crate) fn build_operator_account_http_response(
	route: OperatorRequestRoute,
	request: &[u8],
) -> Vec<u8> {
	match operator_account_http_response_body(route, request) {
		Ok(body) => operator_http::http_response_bytes("200 OK", "application/json", &body),
		Err(error) => {
			let body = serde_json::to_vec(&operator_http::json!({ "error": error.to_string() }))
				.unwrap_or_else(|_| br#"{"error":"account request failed"}"#.to_vec());

			operator_http::http_response_bytes("400 Bad Request", "application/json", &body)
		},
	}
}

pub(crate) fn operator_account_http_response_body(
	route: OperatorRequestRoute,
	request: &[u8],
) -> Result<Vec<u8>> {
	match route {
		OperatorRequestRoute::AccountList { force_refresh } => {
			serde_json::to_vec(&accounts::account_list_with_cached_usage(force_refresh)?)
				.map_err(Into::into)
		},
		OperatorRequestRoute::AccountSelect => {
			let selector = operator_account_request_selector(request)?;
			let response =
				accounts::hydrate_account_list_usage(accounts::account_select(&selector)?);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountClear => {
			let response = accounts::hydrate_account_list_usage(accounts::account_clear()?);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountLogout => {
			let selector = operator_account_request_selector(request)?;
			let response =
				accounts::hydrate_account_list_usage(accounts::account_logout(&selector)?);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountImport => {
			let body = operator_account_request_body(request)?;
			let auth_json_path = body
				.auth_json_path
				.as_deref()
				.filter(|path| !path.trim().is_empty())
				.ok_or_else(|| eyre::eyre!("Account import requires auth_json_path."))?;
			let response = accounts::hydrate_account_list_usage(accounts::account_import(
				Path::new(auth_json_path),
			)?);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountUse => {
			let body = operator_account_request_body(request)?;
			let selector = body
				.selector
				.as_deref()
				.filter(|selector| !selector.trim().is_empty())
				.ok_or_else(|| eyre::eyre!("Account use requires selector."))?;
			let auth_json_path = body.auth_json_path.as_deref().map(PathBuf::from);
			let response = accounts::account_use(&AccountUseRequest {
				selector: selector.to_owned(),
				auth_json_path,
				json: true,
			})?;

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountRerollName => {
			let body = operator_account_request_body(request)?;
			let selector = body
				.selector
				.as_deref()
				.filter(|selector| !selector.trim().is_empty())
				.ok_or_else(|| eyre::eyre!("Account name reroll requires selector."))?;
			let response = accounts::hydrate_account_list_usage(accounts::account_reroll_name(
				selector,
				body.random_name_offset,
			)?);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		_ => eyre::bail!("Unsupported account API route."),
	}
}

pub(crate) fn operator_account_request_selector(request: &[u8]) -> Result<String> {
	let body = operator_account_request_body(request)?;

	body.selector
		.filter(|selector| !selector.trim().is_empty())
		.ok_or_else(|| eyre::eyre!("Account request requires selector."))
}

pub(crate) fn operator_account_request_body(request: &[u8]) -> Result<OperatorAccountRequest> {
	let body = operator_http::operator_http_request_body(request)?;

	if body.is_empty() {
		return Ok(OperatorAccountRequest {
			selector: None,
			auth_json_path: None,
			random_name_offset: None,
		});
	}

	serde_json::from_slice(body)
		.map_err(|error| eyre::eyre!("Account request body was not valid JSON: {error}"))
}
