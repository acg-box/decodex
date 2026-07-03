use crate::orchestrator::{
	ChildAgentActivityBucket, ChildAgentActivitySummary, status_render::activity,
};

pub(crate) fn render_child_agent_activity_summary(
	summary: Option<&ChildAgentActivitySummary>,
) -> String {
	let Some(summary) = summary else {
		return String::from("none");
	};
	let current = match (&summary.current_bucket, summary.current_elapsed_seconds) {
		(Some(bucket), Some(seconds)) => {
			format!("{bucket} {}", activity::format_seconds_compact(seconds))
		},
		(Some(bucket), None) => bucket.clone(),
		(None, _) => String::from("none"),
	};
	let buckets = render_child_agent_bucket_distribution(&summary.buckets);

	format!(
		"current={current}; wall={}; buckets={}; tool_calls={}",
		activity::format_seconds_compact(summary.wall_seconds),
		buckets,
		summary.tool_call_count
	)
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
		.map(|bucket| {
			format!("{} {}", bucket.name, activity::format_seconds_compact(bucket.wall_seconds))
		})
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
