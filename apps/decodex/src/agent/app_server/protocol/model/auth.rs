use serde::{Deserialize, Serialize};

#[derive(Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ChatgptAuthTokensRefreshParams {
	pub(in crate::agent::app_server) reason: Option<String>,
	pub(in crate::agent::app_server) previous_account_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ChatgptAuthTokensRefreshResponse {
	pub(in crate::agent::app_server) access_token: String,
	pub(in crate::agent::app_server) chatgpt_account_id: String,
	pub(in crate::agent::app_server) chatgpt_plan_type: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
pub(in crate::agent::app_server) enum LoginAccountParams {
	#[serde(rename = "chatgptAuthTokens", rename_all = "camelCase")]
	ChatgptAuthTokens {
		access_token: String,
		chatgpt_account_id: String,
		#[serde(skip_serializing_if = "Option::is_none")]
		chatgpt_plan_type: Option<String>,
	},
}

#[derive(Debug, Eq, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub(in crate::agent::app_server) enum LoginAccountResponse {
	#[serde(rename = "chatgptAuthTokens")]
	ChatgptAuthTokens {},
}
