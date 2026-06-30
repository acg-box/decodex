use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
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
impl ChildAgentActivitySummary {
	pub(crate) fn sealed_durable(mut self) -> Self {
		self.seal_open_interval();

		self
	}

	pub(crate) fn live_projection(mut self, now_unix_epoch: i64) -> Self {
		let observed_elapsed_seconds =
			self.current_elapsed_seconds.filter(|elapsed| *elapsed >= 0).unwrap_or(0);
		let current_elapsed_seconds = self.current_started_unix_epoch.and_then(|started_at| {
			now_unix_epoch.checked_sub(started_at).filter(|elapsed| *elapsed >= 0)
		});
		let open_delta_seconds = current_elapsed_seconds.and_then(|elapsed| {
			elapsed.checked_sub(observed_elapsed_seconds).filter(|delta| *delta > 0)
		});

		self.current_elapsed_seconds = current_elapsed_seconds;

		let current_bucket = self.current_bucket.clone();

		if let (Some(current_bucket), Some(open_delta_seconds)) =
			(current_bucket, open_delta_seconds)
		{
			let bucket = self.bucket_mut(&current_bucket);

			bucket.wall_seconds = bucket.wall_seconds.saturating_add(open_delta_seconds);
		}

		self
	}

	fn seal_open_interval(&mut self) {
		self.current_bucket = None;
		self.current_detail = None;
		self.current_started_unix_epoch = None;
		self.current_elapsed_seconds = None;
	}

	fn bucket_mut(&mut self, name: &str) -> &mut ChildAgentActivityBucket {
		if let Some(index) = self.buckets.iter().position(|bucket| bucket.name == name) {
			return &mut self.buckets[index];
		}

		self.buckets.push(ChildAgentActivityBucket {
			name: name.to_owned(),
			..ChildAgentActivityBucket::default()
		});

		let last_index = self.buckets.len().saturating_sub(1);

		&mut self.buckets[last_index]
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ChildAgentActivityBucket {
	pub(crate) name: String,
	pub(crate) wall_seconds: i64,
	pub(crate) event_count: i64,
	pub(crate) tool_call_count: i64,
	pub(crate) input_tokens: i64,
	pub(crate) output_tokens: i64,
	pub(crate) output_bytes: i64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProtocolActivitySummary {
	pub(crate) turn_status: Option<String>,
	pub(crate) waiting_reason: Option<String>,
	pub(crate) rate_limit_status: Option<String>,
	pub(crate) recent_events: Vec<ProtocolActivityEventSummary>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProtocolActivityEventSummary {
	pub(crate) event_type: String,
	pub(crate) category: String,
	pub(crate) detail: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
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
impl RunActivityMarker {
	pub(crate) fn run_id(&self) -> &str {
		&self.run_id
	}

	pub(crate) fn attempt_number(&self) -> i64 {
		self.attempt_number
	}

	pub(crate) fn process_id(&self) -> Option<u32> {
		self.process_id
	}

	pub(crate) fn host_boot_id(&self) -> Option<&str> {
		self.host_boot_id.as_deref()
	}

	pub(crate) fn process_start_identity(&self) -> Option<&str> {
		self.process_start_identity.as_deref()
	}

	pub(crate) fn last_activity_unix_epoch(&self) -> Option<i64> {
		self.last_activity_unix_epoch
	}

	pub(crate) fn last_protocol_activity_unix_epoch(&self) -> Option<i64> {
		self.last_protocol_activity_unix_epoch
	}

	pub(crate) fn last_progress_unix_epoch(&self) -> Option<i64> {
		self.last_progress_unix_epoch
	}

	pub(crate) fn current_operation(&self) -> Option<&str> {
		self.current_operation.as_deref()
	}

	pub(crate) fn thread_id(&self) -> Option<&str> {
		self.thread_id.as_deref()
	}

	pub(crate) fn turn_id(&self) -> Option<&str> {
		self.turn_id.as_deref()
	}

	pub(crate) fn thread_status(&self) -> Option<&str> {
		self.thread_status.as_deref()
	}

	pub(crate) fn thread_active_flags(&self) -> &[String] {
		&self.thread_active_flags
	}

	pub(crate) fn event_count(&self) -> i64 {
		self.event_count.unwrap_or(0)
	}

	pub(crate) fn last_event_type(&self) -> Option<&str> {
		self.last_event_type.as_deref()
	}

	pub(crate) fn effective_model(&self) -> Option<&str> {
		self.effective_model.as_deref()
	}

	pub(crate) fn effective_model_provider(&self) -> Option<&str> {
		self.effective_model_provider.as_deref()
	}

	pub(crate) fn effective_cwd(&self) -> Option<&str> {
		self.effective_cwd.as_deref()
	}

	pub(crate) fn effective_approval_policy(&self) -> Option<&str> {
		self.effective_approval_policy.as_deref()
	}

	pub(crate) fn effective_approvals_reviewer(&self) -> Option<&str> {
		self.effective_approvals_reviewer.as_deref()
	}

	pub(crate) fn effective_sandbox_mode(&self) -> Option<&str> {
		self.effective_sandbox_mode.as_deref()
	}

	pub(crate) fn child_agent_activity(&self) -> Option<&ChildAgentActivitySummary> {
		self.child_agent_activity.as_ref()
	}

	pub(crate) fn protocol_activity(&self) -> Option<&ProtocolActivitySummary> {
		self.protocol_activity.as_ref()
	}

	pub(crate) fn account(&self) -> Option<&CodexAccountActivitySummary> {
		self.account.as_ref()
	}

	pub(crate) fn accounts(&self) -> &[CodexAccountActivitySummary] {
		&self.accounts
	}

	pub(crate) fn retry_kind(&self) -> Option<&str> {
		self.retry_kind.as_deref()
	}

	pub(crate) fn retry_ready_at_unix_epoch(&self) -> Option<i64> {
		self.retry_ready_at_unix_epoch
	}

	pub(crate) fn retry_budget_attempt_count(&self) -> Option<i64> {
		self.retry_budget_attempt_count
	}
}
