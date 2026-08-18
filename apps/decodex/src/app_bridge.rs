//! Internal JSON bridge used by the bundled Decodex App helper.

mod event;
mod request;

use std::{io, path::PathBuf};

use crate::{
	accounts::{self, AccountListResponse, AccountUseRequest},
	app_bridge::{event::AppBridgeEvent, request::AppBridgeRequest},
	codex_config,
	prelude::{Result, eyre},
};

/// Run one Decodex App helper request from stdin and write JSON events to stdout.
pub fn run() -> Result<()> {
	color_eyre::install()?;

	let input = io::read_to_string(io::stdin())?;
	let request = serde_json::from_str::<AppBridgeRequest>(&input)
		.map_err(|error| eyre::eyre!("Invalid Decodex App bridge request: {error}"))?;

	match handle_request(request) {
		Ok(()) => Ok(()),
		Err(error) => {
			let event: AppBridgeEvent<()> =
				AppBridgeEvent::Error { message: error.to_string() };

			event::emit_event(&event)?;

			Err(error)
		},
	}
}

fn handle_request(request: AppBridgeRequest) -> Result<()> {
	match request {
		AppBridgeRequest::List { include_usage, force_refresh } =>
			if include_usage {
				event::emit_result(&accounts::account_list_with_cached_usage(force_refresh)?)
			} else {
				event::emit_result(&accounts::account_list()?)
			},
		AppBridgeRequest::Select { selector, include_usage } =>
			emit_account_list_result(accounts::account_select(&selector)?, include_usage),
		AppBridgeRequest::Clear { include_usage } =>
			emit_account_list_result(accounts::account_clear()?, include_usage),
		AppBridgeRequest::Logout { selector, include_usage } =>
			emit_account_list_result(accounts::account_logout(&selector)?, include_usage),
		AppBridgeRequest::Import { auth_json_path, include_usage } => {
			let auth_json_path = PathBuf::from(auth_json_path);

			emit_account_list_result(accounts::account_import(&auth_json_path)?, include_usage)
		},
		AppBridgeRequest::Use { selector, auth_json_path } =>
			event::emit_result(&accounts::account_use(&AccountUseRequest {
				selector,
				auth_json_path: auth_json_path.map(Into::into),
				json: true,
			})?),
		AppBridgeRequest::FastModeStatus => event::emit_result(&codex_config::fast_mode_status()?),
		AppBridgeRequest::FastModeSet { enabled } =>
			event::emit_result(&codex_config::set_fast_mode(enabled)?),
	}
}

fn emit_account_list_result(response: AccountListResponse, include_usage: bool) -> Result<()> {
	let response =
		if include_usage { accounts::hydrate_account_list_usage(response) } else { response };

	event::emit_result(&response)
}

#[cfg(test)]
mod tests {
	use crate::app_bridge::request::AppBridgeRequest;

	#[test]
	fn parses_account_use_bridge_request() {
		let request = serde_json::from_value::<AppBridgeRequest>(serde_json::json!({
			"operation": "account_use",
			"selector": "copy@example.com",
			"auth_json_path": "/tmp/auth.json"
		}))
		.expect("bridge request should parse");

		assert!(matches!(request, AppBridgeRequest::Use { .. }));
	}

	#[test]
	fn parses_account_list_bridge_request_with_force_refresh() {
		let request = serde_json::from_value::<AppBridgeRequest>(serde_json::json!({
			"operation": "account_list",
			"include_usage": true,
			"force_refresh": true
		}))
		.expect("bridge request should parse");

		assert!(matches!(
			request,
			AppBridgeRequest::List { include_usage: true, force_refresh: true }
		));
	}

	#[test]
	fn parses_fast_mode_set_bridge_request() {
		let request = serde_json::from_value::<AppBridgeRequest>(serde_json::json!({
			"operation": "codex_fast_mode_set",
			"enabled": true
		}))
		.expect("bridge request should parse");

		assert!(matches!(request, AppBridgeRequest::FastModeSet { enabled: true }));
	}
}
