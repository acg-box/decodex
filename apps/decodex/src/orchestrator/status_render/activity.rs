use crate::orchestrator::{
	self, ChildAgentActivityBucket, ChildAgentActivitySummary, CodexAccountActivitySummary,
	OperatorLoopStatus, OperatorRunControlCapability, ProtocolActivityEventSummary,
	ProtocolActivitySummary,
};

pub(crate) fn render_child_agent_activity_summary(
	summary: Option<&ChildAgentActivitySummary>,
) -> String {
	let Some(summary) = summary else {
		return String::from("none");
	};
	let current = match (&summary.current_bucket, summary.current_elapsed_seconds) {
		(Some(bucket), Some(seconds)) => format!("{bucket} {}", format_seconds_compact(seconds)),
		(Some(bucket), None) => bucket.clone(),
		(None, _) => String::from("none"),
	};
	let buckets = render_child_agent_bucket_distribution(&summary.buckets);

	format!(
		"current={current}; wall={}; buckets={}; tool_calls={}",
		format_seconds_compact(summary.wall_seconds),
		buckets,
		summary.tool_call_count
	)
}

pub(crate) fn render_protocol_activity_summary(
	summary: Option<&ProtocolActivitySummary>,
) -> String {
	let Some(summary) = summary else {
		return String::from("none");
	};
	let turn = summary.turn_status.as_deref().unwrap_or("none");
	let wait = summary.waiting_reason.as_deref().unwrap_or("none");
	let rate_limit = summary.rate_limit_status.as_deref().unwrap_or("none");
	let recent = if summary.recent_events.is_empty() {
		String::from("none")
	} else {
		summary
			.recent_events
			.iter()
			.rev()
			.take(5)
			.map(render_protocol_activity_event_summary)
			.collect::<Vec<_>>()
			.join(", ")
	};

	format!("turn={turn}; waiting={wait}; rate_limit={rate_limit}; recent={recent}")
}

pub(crate) fn render_loop_status_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(status) = status else {
		return String::from("none");
	};
	let next_action = status.next_action.as_deref().unwrap_or("none");
	let autonomy_objective = status
		.autonomy_objective
		.as_ref()
		.map(|objective| objective.source_ref.as_str())
		.unwrap_or("none");
	let autonomy_report =
		status.autonomy_report.as_ref().map(|report| report.authority.as_str()).unwrap_or("none");

	format!(
		"{}; review_level={}; autonomy={}; autonomy_objective={autonomy_objective}; autonomy_signals={}; autonomy_proposals={}; report={autonomy_report}; next_action={next_action}",
		status.summary,
		status.review_level,
		status.autonomy,
		status.autonomy_signals.len(),
		status.autonomy_proposals.len()
	)
}

pub(crate) fn render_loop_autonomy_signals_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(status) = status else {
		return String::from("none");
	};

	if status.autonomy_signals.is_empty() {
		return String::from("none");
	}

	status
		.autonomy_signals
		.iter()
		.map(|signal| {
			format!(
				"{}:{}@v{} freshness={} confidence={} privacy={} sources={} completeness={} gaps={} contradictions={}",
				signal.kind,
				signal.objective_id,
				signal.objective_version,
				signal.freshness,
				signal.confidence,
				signal.privacy,
				signal.source_refs.len(),
				signal.completeness,
				signal.gaps.len(),
				signal.contradictions.len()
			)
		})
		.collect::<Vec<_>>()
		.join(";")
}

pub(crate) fn render_loop_review_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(review) = status.and_then(|status| status.review.as_ref()) else {
		return String::from("none");
	};
	let checkpoint = review.checkpoint.as_ref().map_or_else(
		|| String::from("checkpoint=none"),
		|checkpoint| {
			format!(
				"checkpoint=head:{} round:{} review_class:{} risk_class:{} compact_eligible:{} fallback:{} updated:{}",
				checkpoint.head_sha,
				checkpoint.round,
				checkpoint.review_class.as_deref().unwrap_or("none"),
				checkpoint.risk_class.as_deref().unwrap_or("none"),
				checkpoint
					.compact_eligible
					.map_or("none", |eligible| if eligible { "true" } else { "false" }),
				checkpoint.fallback_reason.as_deref().unwrap_or("none"),
				checkpoint.updated_at
			)
		},
	);

	format!("phase={} status={} {checkpoint}", review.phase, review.status)
}

pub(crate) fn render_loop_architecture_recovery_summary(
	status: Option<&OperatorLoopStatus>,
) -> String {
	let Some(recovery) = status.and_then(|status| status.architecture_recovery.as_ref()) else {
		return String::from("none");
	};
	let budget = recovery.budget.as_ref().map_or_else(
		|| String::from("none"),
		|budget| format!("{}/{}", budget.attempt, budget.max_attempts),
	);

	format!(
		"status={} reason={} guardrail={} boundary={} policy={} enhanced_evidence={} blocks_landing={} budget={} next_action={}",
		recovery.status,
		recovery.reason_code,
		recovery.guardrail_reason.as_deref().unwrap_or("none"),
		recovery.boundary_disposition.as_deref().unwrap_or("none"),
		recovery.boundary_policy_decision.as_deref().unwrap_or("none"),
		recovery.requires_enhanced_evidence,
		recovery.blocks_landing,
		budget,
		recovery.next_action
	)
}

pub(crate) fn render_loop_boundary_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(boundary) = status.and_then(|status| status.boundary.as_ref()) else {
		return String::from("none");
	};

	format!(
		"disposition={} policy={} enhanced_evidence={} blocks_landing={} reason={} attempted_recovery={} changed_surfaces={} improvement_signals={}",
		boundary.disposition,
		boundary.policy_decision,
		boundary.requires_enhanced_evidence,
		boundary.blocks_landing,
		boundary.reason.as_deref().unwrap_or("none"),
		boundary.attempted_recovery_reason.as_deref().unwrap_or("none"),
		boundary.changed_surface_count,
		boundary.improvement_signal_count
	)
}

pub(crate) fn render_control_capability_summary(
	capability: Option<&OperatorRunControlCapability>,
) -> String {
	let Some(capability) = capability else {
		return String::from("none");
	};
	let thread_id = capability.thread_id.as_deref().unwrap_or("none");
	let turn_id = capability.turn_id.as_deref().unwrap_or("none");

	format!(
		"status={}; transport={}; channel={}; thread_id={thread_id}; turn_id={turn_id}",
		capability.status, capability.transport, capability.channel_path
	)
}

pub(crate) fn render_account_summary(summary: Option<&CodexAccountActivitySummary>) -> String {
	let Some(summary) = summary else {
		return String::from("none");
	};
	let plan = summary.plan_type.as_deref().unwrap_or("unknown");
	let reached = summary.rate_limit_reached_type.as_deref().unwrap_or("none");
	let credits = render_codex_account_credits(summary);
	let token_status = render_codex_account_token_status(&summary.refresh_status);
	let primary = render_codex_account_window(
		summary.primary_window_seconds,
		summary.primary_remaining_percent,
		summary.primary_resets_at_unix_epoch,
	);
	let secondary = render_codex_account_window(
		summary.secondary_window_seconds,
		summary.secondary_remaining_percent,
		summary.secondary_resets_at_unix_epoch,
	);

	format!(
		"account={}; plan={plan}; status={}; token={token_status}; primary={primary}; secondary={secondary}; credits={credits}; reached={reached}",
		summary.account_fingerprint, summary.status,
	)
}

pub(crate) fn render_accounts_summary(accounts: &[CodexAccountActivitySummary]) -> String {
	if accounts.is_empty() {
		return String::from("none");
	}

	accounts
		.iter()
		.map(|summary| render_account_summary(Some(summary)))
		.collect::<Vec<_>>()
		.join(" | ")
}

pub(crate) fn render_child_agent_context_pressure(
	summary: Option<&ChildAgentActivitySummary>,
) -> String {
	let Some(summary) = summary else {
		return String::from("none");
	};
	let current_input = summary
		.input_tokens_current
		.map(format_count_compact)
		.unwrap_or_else(|| String::from("none"));
	let max_input =
		summary.input_tokens_max.map(format_count_compact).unwrap_or_else(|| String::from("none"));
	let max_input_relation = match (summary.input_tokens_current, summary.input_tokens_max) {
		(Some(current), Some(max)) if current == max => " (same as current)",
		_ => "",
	};
	let largest_output = summary
		.largest_tool_output_bytes
		.map(format_bytes_compact)
		.unwrap_or_else(|| String::from("none"));
	let largest_tool = summary.largest_tool_output_tool.as_deref().unwrap_or("none");
	let warnings = if summary.large_output_warnings.is_empty() {
		String::from("none")
	} else {
		summary.large_output_warnings.join(" | ")
	};

	format!(
		"input=current_window {current_input}, peak_window {max_input}{max_input_relation}, cumulative_input {}; output_tokens={}; largest_output={largest_output} by {largest_tool}; warnings={warnings}",
		format_count_compact(summary.input_tokens_cumulative),
		format_count_compact(summary.output_tokens_cumulative)
	)
}

pub(crate) fn format_seconds_compact(seconds: i64) -> String {
	if seconds >= 3_600 {
		return format!("{}h{}m", seconds / 3_600, (seconds % 3_600) / 60);
	}
	if seconds >= 60 {
		return format!("{}m{}s", seconds / 60, seconds % 60);
	}

	format!("{seconds}s")
}

fn render_protocol_activity_event_summary(event: &ProtocolActivityEventSummary) -> String {
	event.detail.as_ref().map_or_else(
		|| event.event_type.clone(),
		|detail| format!("{}:{}", event.event_type, render_protocol_activity_detail(detail)),
	)
}

fn render_protocol_activity_detail(detail: &str) -> &str {
	if orchestrator::operator_protocol_activity_detail_is_public(detail) {
		detail
	} else {
		"redacted_sensitive_detail"
	}
}

fn render_codex_account_window(
	window_seconds: Option<i64>,
	remaining_percent: Option<i64>,
	resets_at_unix_epoch: Option<i64>,
) -> String {
	let label = window_seconds.map(codex_window_label).unwrap_or_else(|| String::from("window"));
	let remaining =
		remaining_percent.map_or_else(|| String::from("unknown"), |value| format!("{value}%"));
	let reset = orchestrator::format_optional_unix_timestamp(resets_at_unix_epoch)
		.unwrap_or_else(|| String::from("unknown"));

	format!("{label} remaining={remaining} reset={reset}")
}

fn render_codex_account_credits(summary: &CodexAccountActivitySummary) -> String {
	if summary.credits_unlimited == Some(true) {
		return String::from("unlimited");
	}

	match (summary.credits_has_credits, summary.credits_balance.as_deref()) {
		(Some(false), Some(balance)) => format!("depleted balance={balance}"),
		(Some(false), None) => String::from("depleted"),
		(_, Some(balance)) => format!("balance={balance}"),
		(Some(true), None) => String::from("available"),
		(None, None) => String::from("unknown"),
	}
}

fn render_codex_account_token_status(refresh_status: &str) -> &'static str {
	match refresh_status {
		"not_needed" | "none" => "ok",
		"succeeded" | "refreshed" => "refreshed",
		"failed" => "refresh_failed",
		_ => "unknown",
	}
}

fn codex_window_label(window_seconds: i64) -> String {
	match window_seconds {
		18_000 => String::from("5h"),
		604_800 => String::from("7d"),
		seconds => format_seconds_compact(seconds),
	}
}

fn render_child_agent_bucket_distribution(buckets: &[ChildAgentActivityBucket]) -> String {
	if buckets.is_empty() {
		return String::from("none");
	}

	let mut buckets = buckets.iter().collect::<Vec<_>>();

	buckets.sort_by(|left, right| {
		right
			.wall_seconds
			.cmp(&left.wall_seconds)
			.then_with(|| right.event_count.cmp(&left.event_count))
			.then_with(|| left.name.cmp(&right.name))
	});

	buckets
		.into_iter()
		.take(5)
		.map(|bucket| format!("{} {}", bucket.name, format_seconds_compact(bucket.wall_seconds)))
		.collect::<Vec<_>>()
		.join(", ")
}

fn format_count_compact(count: i64) -> String {
	if count >= 1_000_000 {
		return format!("{:.2}M", count as f64 / 1_000_000.0);
	}
	if count >= 1_000 {
		return format!("{:.1}k", count as f64 / 1_000.0);
	}

	count.to_string()
}

fn format_bytes_compact(bytes: i64) -> String {
	if bytes >= 1_048_576 {
		return format!("{:.1}MiB", bytes as f64 / 1_048_576.0);
	}
	if bytes >= 1_024 {
		return format!("{:.1}KiB", bytes as f64 / 1_024.0);
	}

	format!("{bytes}B")
}
