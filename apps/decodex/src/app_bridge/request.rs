use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub(in crate::app_bridge) enum AppBridgeRequest {
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
	#[serde(rename = "codex_fast_mode_status")]
	FastModeStatus,
	#[serde(rename = "codex_fast_mode_set")]
	FastModeSet { enabled: bool },
}
