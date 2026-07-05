mod child_agent;
mod run_marker;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ChildAgentActivitySummary {
	pub(crate) buckets: Vec<ChildAgentActivityBucket>,
	pub(crate) current_bucket: Option<String>,
	pub(crate) current_detail: Option<String>,
	pub(crate) current_started_unix_epoch: Option<i64>,
	pub(crate) current_elapsed_seconds: Option<i64>,
	pub(crate) wall_seconds: i64,
	pub(crate) event_count: i64,
	pub(crate) tool_call_count: i64,
	pub(crate) input_tokens_current: Option<i64>,
	pub(crate) input_tokens_max: Option<i64>,
	pub(crate) input_tokens_cumulative: i64,
	pub(crate) output_tokens_cumulative: i64,
	pub(crate) largest_tool_output_bytes: Option<i64>,
	pub(crate) largest_tool_output_tool: Option<String>,
	pub(crate) large_output_warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ChildAgentActivityBucket {
	pub(crate) name: String,
	pub(crate) wall_seconds: i64,
	pub(crate) event_count: i64,
	pub(crate) tool_call_count: i64,
	pub(crate) input_tokens: i64,
	pub(crate) output_tokens: i64,
	pub(crate) output_bytes: i64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ProtocolActivitySummary {
	pub(crate) turn_status: Option<String>,
	pub(crate) waiting_reason: Option<String>,
	pub(crate) rate_limit_status: Option<String>,
	pub(crate) recent_events: Vec<ProtocolActivityEventSummary>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ProtocolActivityEventSummary {
	pub(crate) event_type: String,
	pub(crate) category: String,
	pub(crate) detail: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct CodexAccountActivitySummary {
	pub(crate) account_fingerprint: String,
	pub(crate) email: Option<String>,
	pub(crate) plan_type: Option<String>,
	pub(crate) status: String,
	pub(crate) refresh_status: String,
	pub(crate) checked_at_unix_epoch: Option<i64>,
	pub(crate) selected_at_unix_epoch: Option<i64>,
	pub(crate) primary_window_seconds: Option<i64>,
	pub(crate) primary_remaining_percent: Option<i64>,
	pub(crate) primary_resets_at_unix_epoch: Option<i64>,
	pub(crate) secondary_window_seconds: Option<i64>,
	pub(crate) secondary_remaining_percent: Option<i64>,
	pub(crate) secondary_resets_at_unix_epoch: Option<i64>,
	pub(crate) credits_has_credits: Option<bool>,
	pub(crate) credits_unlimited: Option<bool>,
	pub(crate) credits_balance: Option<String>,
	pub(crate) reset_credits_available_count: Option<i64>,
	pub(crate) reset_credits_total_earned_count: Option<i64>,
	pub(crate) reset_credits_checked_at_unix_epoch: Option<i64>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) reset_credits: Vec<CodexAccountResetCreditSummary>,
	pub(crate) rate_limit_reached_type: Option<String>,
	pub(crate) cooldown_until_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_display_name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_username: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_checked_at_unix_epoch: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_lifetime_tokens: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_peak_daily_tokens: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_longest_task_seconds: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_current_streak_days: Option<i64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(crate) profile_longest_streak_days: Option<i64>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub(crate) profile_daily_usage: Vec<CodexAccountProfileDailyUsageSummary>,
	pub(crate) note: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct CodexAccountResetCreditSummary {
	pub(crate) granted_at_unix_epoch: Option<i64>,
	pub(crate) expires_at_unix_epoch: Option<i64>,
	pub(crate) status: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct CodexAccountProfileDailyUsageSummary {
	pub(crate) date: String,
	pub(crate) tokens: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RunActivityMarker {
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) process_id: Option<u32>,
	pub(in crate::state) host_boot_id: Option<String>,
	pub(in crate::state) process_start_identity: Option<String>,
	pub(in crate::state) last_activity_unix_epoch: Option<i64>,
	pub(in crate::state) last_protocol_activity_unix_epoch: Option<i64>,
	pub(in crate::state) last_progress_unix_epoch: Option<i64>,
	pub(in crate::state) current_operation: Option<String>,
	pub(in crate::state) thread_id: Option<String>,
	pub(in crate::state) turn_id: Option<String>,
	pub(in crate::state) thread_status: Option<String>,
	pub(in crate::state) thread_active_flags: Vec<String>,
	pub(in crate::state) event_count: Option<i64>,
	pub(in crate::state) last_event_type: Option<String>,
	pub(in crate::state) effective_model: Option<String>,
	pub(in crate::state) effective_model_provider: Option<String>,
	pub(in crate::state) effective_cwd: Option<String>,
	pub(in crate::state) effective_approval_policy: Option<String>,
	pub(in crate::state) effective_approvals_reviewer: Option<String>,
	pub(in crate::state) effective_sandbox_mode: Option<String>,
	pub(in crate::state) child_agent_activity: Option<ChildAgentActivitySummary>,
	pub(in crate::state) protocol_activity: Option<ProtocolActivitySummary>,
	pub(in crate::state) account: Option<CodexAccountActivitySummary>,
	pub(in crate::state) accounts: Vec<CodexAccountActivitySummary>,
	pub(in crate::state) retry_budget_attempt_count: Option<i64>,
	pub(in crate::state) retry_kind: Option<String>,
	pub(in crate::state) retry_ready_at_unix_epoch: Option<i64>,
}
