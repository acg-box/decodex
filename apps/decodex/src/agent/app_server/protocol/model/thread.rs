use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{AppServerDynamicToolSpec, ThreadGoalStatus, externally_tagged_value_name};

#[derive(Debug, Default, Serialize)]
pub(in crate::agent::app_server) struct ThreadStartRequest {
	#[serde(rename = "baseInstructions", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) base_instructions: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) config: Option<Value>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) cwd: Option<String>,
	#[serde(rename = "dynamicTools", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) dynamic_tools: Option<Vec<AppServerDynamicToolSpec>>,
	#[serde(rename = "developerInstructions", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) developer_instructions: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) ephemeral: Option<bool>,
	#[serde(rename = "modelProvider", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) model_provider: Option<String>,
	#[serde(rename = "serviceName", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) service_name: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub(in crate::agent::app_server) struct ThreadResumeRequest {
	#[serde(rename = "threadId")]
	pub(in crate::agent::app_server) thread_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) model: Option<String>,
	#[serde(rename = "modelProvider", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) model_provider: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) cwd: Option<String>,
	#[serde(rename = "approvalPolicy", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) approval_policy: Option<Value>,
	#[serde(rename = "approvalsReviewer", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) approvals_reviewer: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) sandbox: Option<Value>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) config: Option<Value>,
	#[serde(rename = "baseInstructions", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) base_instructions: Option<String>,
	#[serde(rename = "developerInstructions", skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) developer_instructions: Option<String>,
}

#[derive(Debug, Serialize)]
pub(in crate::agent::app_server) struct ThreadArchiveRequest {
	#[serde(rename = "threadId")]
	pub(in crate::agent::app_server) thread_id: String,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ThreadArchiveResponse {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ThreadGoalSetParams {
	pub(in crate::agent::app_server) thread_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) objective: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) status: Option<ThreadGoalStatus>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(in crate::agent::app_server) token_budget: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ThreadGoalGetParams {
	pub(in crate::agent::app_server) thread_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ThreadGoalClearParams {
	pub(in crate::agent::app_server) thread_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::agent::app_server) struct ThreadGoal {
	#[allow(dead_code)]
	pub(in crate::agent::app_server) created_at: i64,
	#[allow(dead_code)]
	pub(in crate::agent::app_server) objective: String,
	pub(in crate::agent::app_server) status: ThreadGoalStatus,
	pub(in crate::agent::app_server) thread_id: String,
	pub(in crate::agent::app_server) time_used_seconds: i64,
	pub(in crate::agent::app_server) token_budget: Option<i64>,
	pub(in crate::agent::app_server) tokens_used: i64,
	#[allow(dead_code)]
	pub(in crate::agent::app_server) updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ThreadGoalSetResponse {
	pub(in crate::agent::app_server) goal: ThreadGoal,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ThreadGoalGetResponse {
	pub(in crate::agent::app_server) goal: Option<ThreadGoal>,
}

#[derive(Debug, Deserialize)]
pub(in crate::agent::app_server) struct ThreadGoalClearResponse {
	pub(in crate::agent::app_server) cleared: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub(in crate::agent::app_server) struct ThreadSessionResponse {
	pub(in crate::agent::app_server) thread: Thread,
	pub(in crate::agent::app_server) model: String,
	#[serde(rename = "modelProvider")]
	pub(in crate::agent::app_server) model_provider: String,
	#[serde(rename = "serviceTier")]
	pub(in crate::agent::app_server) _service_tier: Option<Value>,
	pub(in crate::agent::app_server) cwd: String,
	#[serde(default, rename = "instructionSources")]
	pub(in crate::agent::app_server) _instruction_sources: Vec<String>,
	#[serde(rename = "approvalPolicy")]
	pub(in crate::agent::app_server) approval_policy: Value,
	#[serde(rename = "approvalsReviewer")]
	pub(in crate::agent::app_server) approvals_reviewer: String,
	pub(in crate::agent::app_server) sandbox: Value,
	#[serde(rename = "reasoningEffort")]
	pub(in crate::agent::app_server) _reasoning_effort: Option<String>,
}
impl ThreadSessionResponse {
	pub(in crate::agent::app_server) fn effective_config(&self) -> EffectiveThreadConfig {
		EffectiveThreadConfig {
			model: self.model.clone(),
			model_provider: self.model_provider.clone(),
			cwd: self.cwd.clone(),
			approval_policy: externally_tagged_value_name(&self.approval_policy)
				.unwrap_or_else(|| String::from("unknown")),
			approvals_reviewer: self.approvals_reviewer.clone(),
			sandbox_mode: externally_tagged_value_name(&self.sandbox)
				.unwrap_or_else(|| String::from("unknown")),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::agent::app_server) struct EffectiveThreadConfig {
	pub(in crate::agent::app_server) model: String,
	pub(in crate::agent::app_server) model_provider: String,
	pub(in crate::agent::app_server) cwd: String,
	pub(in crate::agent::app_server) approval_policy: String,
	pub(in crate::agent::app_server) approvals_reviewer: String,
	pub(in crate::agent::app_server) sandbox_mode: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(in crate::agent::app_server) struct Thread {
	pub(in crate::agent::app_server) id: String,
}
