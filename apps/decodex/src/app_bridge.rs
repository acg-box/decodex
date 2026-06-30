//! Internal JSON bridge used by the bundled Decodex App helper.

use std::{
	io::{self, Read as _, Write as _},
	path::PathBuf,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
	accounts::{self, AccountListResponse, AccountLoginRequest, AccountUseRequest},
	codex_config,
	prelude::{Result, eyre},
};

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum AppBridgeRequest {
	#[serde(rename = "account_list")]
	List {
		#[serde(default)]
		include_usage: bool,
		#[serde(default)]
		force_refresh: bool,
	},
	#[serde(rename = "account_select")]
	Select {
		selector: String,
		#[serde(default)]
		include_usage: bool,
	},
	#[serde(rename = "account_clear")]
	Clear {
		#[serde(default)]
		include_usage: bool,
	},
	#[serde(rename = "account_logout")]
	Logout {
		selector: String,
		#[serde(default)]
		include_usage: bool,
	},
	#[serde(rename = "account_import")]
	Import {
		auth_json_path: String,
		#[serde(default)]
		include_usage: bool,
	},
	#[serde(rename = "account_use")]
	Use { selector: String, auth_json_path: Option<String> },
	#[serde(rename = "account_login")]
	Login {
		#[serde(default = "default_codex_bin")]
		codex_bin: String,
		#[serde(default)]
		keep_temp_home: bool,
		#[serde(default)]
		include_usage: bool,
	},
	#[serde(rename = "codex_fast_mode_status")]
	FastModeStatus,
	#[serde(rename = "codex_fast_mode_set")]
	FastModeSet { enabled: bool },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AppBridgeEvent<'a, T = Value>
where
	T: Serialize,
{
	Output { text: &'a str },
	Result { payload: T },
	Error { message: String },
}

/// Run one Decodex App helper request from stdin and write JSON events to stdout.
pub fn run() -> Result<()> {
	color_eyre::install()?;

	let mut input = String::new();

	io::stdin().read_to_string(&mut input)?;

	let request = serde_json::from_str::<AppBridgeRequest>(&input)
		.map_err(|error| eyre::eyre!("Invalid Decodex App bridge request: {error}"))?;

	match handle_request(request) {
		Ok(()) => Ok(()),
		Err(error) => {
			let event: AppBridgeEvent<'_, ()> =
				AppBridgeEvent::Error { message: error.to_string() };

			emit_event(&event)?;

			Err(error)
		},
	}
}

fn handle_request(request: AppBridgeRequest) -> Result<()> {
	match request {
		AppBridgeRequest::List { include_usage, force_refresh } => {
			if include_usage {
				emit_result(&accounts::account_list_with_cached_usage(force_refresh)?)
			} else {
				emit_result(&accounts::account_list()?)
			}
		},
		AppBridgeRequest::Select { selector, include_usage } => {
			emit_account_list_result(accounts::account_select(&selector)?, include_usage)
		},
		AppBridgeRequest::Clear { include_usage } => {
			emit_account_list_result(accounts::account_clear()?, include_usage)
		},
		AppBridgeRequest::Logout { selector, include_usage } => {
			emit_account_list_result(accounts::account_logout(&selector)?, include_usage)
		},
		AppBridgeRequest::Import { auth_json_path, include_usage } => {
			let auth_json_path = PathBuf::from(auth_json_path);

			emit_account_list_result(accounts::account_import(&auth_json_path)?, include_usage)
		},
		AppBridgeRequest::Use { selector, auth_json_path } => {
			emit_result(&accounts::account_use(&AccountUseRequest {
				selector,
				auth_json_path: auth_json_path.map(Into::into),
				json: true,
			})?)
		},
		AppBridgeRequest::Login { codex_bin, keep_temp_home, include_usage } => {
			let response = accounts::account_login(
				&AccountLoginRequest { codex_bin, keep_temp_home },
				|chunk| {
					let event: AppBridgeEvent<'_, ()> = AppBridgeEvent::Output { text: chunk };

					emit_event(&event)
				},
			)?;

			emit_account_list_result(response, include_usage)
		},
		AppBridgeRequest::FastModeStatus => emit_result(&codex_config::fast_mode_status()?),
		AppBridgeRequest::FastModeSet { enabled } => {
			emit_result(&codex_config::set_fast_mode(enabled)?)
		},
	}
}

fn emit_account_list_result(response: AccountListResponse, include_usage: bool) -> Result<()> {
	let response =
		if include_usage { accounts::hydrate_account_list_usage(response) } else { response };

	emit_result(&response)
}

fn emit_result<T>(payload: &T) -> Result<()>
where
	T: Serialize,
{
	emit_event(&AppBridgeEvent::Result { payload })
}

fn emit_event<T>(event: &AppBridgeEvent<'_, T>) -> Result<()>
where
	T: Serialize,
{
	let mut stdout = io::stdout().lock();

	serde_json::to_writer(&mut stdout, event)?;

	stdout.write_all(b"\n")?;
	stdout.flush()?;

	Ok(())
}

fn default_codex_bin() -> String {
	String::from("codex")
}

#[cfg(test)]
mod tests {
	use crate::app_bridge::AppBridgeRequest;

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
