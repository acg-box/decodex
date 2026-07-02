mod auth;
mod catalog;
mod core;
mod enums;
mod error;
mod helpers;
mod notifications;
mod runtime;
mod thread;
mod tools;

pub(in crate::agent::app_server) use auth::{
	ChatgptAuthTokensRefreshParams, ChatgptAuthTokensRefreshResponse, LoginAccountParams,
	LoginAccountResponse,
};
pub(in crate::agent::app_server) use catalog::{
	ListMcpServerStatusParams, ListMcpServerStatusResponse, McpServerStatusSummary,
	PluginListParams, PluginListResponse, SkillsListParams, SkillsListResponse,
};
#[cfg(test)]
pub(in crate::agent::app_server) use catalog::{
	MarketplaceLoadErrorInfo, PluginMarketplaceEntry, PluginSummary, SkillErrorInfo,
	SkillMetadata, SkillsListEntry,
};
pub(in crate::agent::app_server) use core::{
	ClientInfo, InitializeCapabilities, InitializeParams, InitializeResponse, RunOutcome,
};
pub(in crate::agent::app_server) use enums::{
	CommandExecutionApprovalDecision, FileChangeApprovalDecision, McpServerElicitationAction,
	PermissionGrantScope, ThreadGoalStatus, UserInput,
};
pub(in crate::agent::app_server) use error::{TurnError, TurnStatusPayload};
use helpers::{externally_tagged_value_name, string_like_json_value};
pub(in crate::agent::app_server) use notifications::{
	AgentMessageDeltaNotification, ErrorNotification, ItemCompletedNotification,
	ThreadGoalUpdatedNotification, ThreadStatusChangedNotification, TurnCompletedNotification,
};
pub(in crate::agent::app_server) use runtime::{
	CommandExecParams, CommandExecResponse, ConfigReadParams, ConfigReadResponse, ModelListParams,
	ModelListResponse, ModelProviderCapabilitiesReadParams, ModelProviderCapabilitiesReadResponse,
	ModelSummary, RuntimeConfigSummary, TurnInterruptRequest, TurnStartRequest, TurnStartResponse,
	TurnSteerRequest, TurnSteerResponse,
};
pub(in crate::agent::app_server) use thread::{
	EffectiveThreadConfig, ThreadArchiveRequest, ThreadArchiveResponse, ThreadGoal,
	ThreadGoalClearParams, ThreadGoalClearResponse, ThreadGoalGetParams, ThreadGoalGetResponse,
	ThreadGoalSetParams, ThreadGoalSetResponse, ThreadResumeRequest, ThreadSessionResponse,
	ThreadStartRequest,
};
pub(in crate::agent::app_server) use tools::{
	AppServerDynamicToolSpec, CommandExecutionRequestApprovalResponse, DynamicToolCallParams,
	FileChangeRequestApprovalResponse,
	McpServerElicitationRequestResponse, PermissionsRequestApprovalResponse,
	ProbeDynamicToolHandler, ToolRequestUserInputResponse, app_server_dynamic_tool_specs,
};

#[cfg(test)] use crate::agent::app_server::REQUEST_TIMEOUT;

#[cfg(test)]
mod tests {
	#[test]
	fn externally_tagged_values_prefer_explicit_type_field() {
		assert_eq!(
			super::externally_tagged_value_name(&serde_json::json!({ "type": "dangerFullAccess" })),
			Some(String::from("dangerFullAccess"))
		);
	}

	#[test]
	fn error_notifications_keep_codex_error_info_without_retry_flag() {
		let notification: super::ErrorNotification = serde_json::from_value(serde_json::json!({
			"error": {
				"message": "usage limit exceeded",
				"codexErrorInfo": "usageLimitExceeded"
			},
			"threadId": "thread-1",
			"turnId": "turn-1"
		}))
		.expect("error notification should parse");

		assert_eq!(notification.error.codex_error_info.as_deref(), Some("usageLimitExceeded"));
		assert_eq!(notification.will_retry, None);
	}

	#[test]
	fn error_notifications_stringify_structured_error_fields() {
		let notification: super::ErrorNotification = serde_json::from_value(serde_json::json!({
			"error": {
				"message": {
					"kind": "protocolFailure",
					"detail": "unexpected response"
				},
				"codexErrorInfo": {
					"type": "appServerProtocolMismatch"
				}
			},
			"threadId": "thread-1",
			"turnId": "turn-1",
			"willRetry": false
		}))
		.expect("structured error notification should parse");

		assert!(notification.error.message.contains("protocolFailure"));
		assert!(
			notification
				.error
				.codex_error_info
				.as_deref()
				.is_some_and(|value| value.contains("appServerProtocolMismatch"))
		);
		assert_eq!(notification.will_retry, Some(false));
	}

	#[test]
	fn error_notifications_accept_structured_string_fields() {
		let notification: super::ErrorNotification = serde_json::from_value(serde_json::json!({
			"error": {
				"message": {
					"type": "streamDisconnected",
					"message": "stream disconnected"
				},
				"codexErrorInfo": {
					"type": "transientNetworkError"
				}
			},
			"threadId": { "id": "thread-1" },
			"turnId": { "id": "turn-1" },
			"willRetry": true
		}))
		.expect("structured error notification should parse");

		assert_eq!(notification.error.message, "stream disconnected");
		assert_eq!(notification.error.codex_error_info.as_deref(), Some("transientNetworkError"));
		assert_eq!(notification.thread_id.as_deref(), Some("thread-1"));
		assert_eq!(notification.turn_id.as_deref(), Some("turn-1"));
		assert_eq!(notification.will_retry, Some(true));
	}

	#[test]
	fn chatgpt_auth_tokens_login_uses_app_server_protocol_shape() {
		let value = serde_json::to_value(super::LoginAccountParams::ChatgptAuthTokens {
			access_token: String::from("access"),
			chatgpt_account_id: String::from("acct_1"),
			chatgpt_plan_type: Some(String::from("pro")),
		})
		.expect("login params should serialize");

		assert_eq!(
			value,
			serde_json::json!({
				"type": "chatgptAuthTokens",
				"accessToken": "access",
				"chatgptAccountId": "acct_1",
				"chatgptPlanType": "pro"
			})
		);
	}

	#[test]
	fn command_exec_request_timeout_includes_process_timeout() {
		let params = super::CommandExecParams {
			command: vec![String::from("/bin/sh")],
			cwd: None,
			timeout_ms: Some(1_000),
			output_bytes_cap: Some(128),
		};

		assert_eq!(
			params.request_timeout(),
			std::time::Duration::from_millis(1_000) + super::REQUEST_TIMEOUT
		);
	}
}
