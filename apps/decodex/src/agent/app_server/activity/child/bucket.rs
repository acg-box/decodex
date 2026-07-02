use serde_json::Value;

use crate::{
	agent::app_server::activity::{
		CHILD_BUCKET_BROWSER_IMAGE, CHILD_BUCKET_PR_LAND, CHILD_BUCKET_SHELL, CHILD_BUCKET_TOOL,
		CHILD_BUCKET_TRACKER, payload,
	},
	state::{ChildAgentActivityBucket, ChildAgentActivitySummary},
};

pub(in crate::agent::app_server::activity::child) fn child_tool_bucket(
	tool_name: &str,
	arguments: Option<&Value>,
) -> (String, String) {
	let normalized_tool = tool_name.to_ascii_lowercase();

	if is_tracker_tool_name(&normalized_tool) {
		return (CHILD_BUCKET_TRACKER.to_owned(), tool_name.to_owned());
	}
	if normalized_tool.contains("view_image")
		|| normalized_tool.contains("screenshot")
		|| normalized_tool.contains("image_query")
		|| normalized_tool.contains("browser")
	{
		return (CHILD_BUCKET_BROWSER_IMAGE.to_owned(), tool_name.to_owned());
	}
	if normalized_tool.contains("exec_command") {
		let command_category = arguments
			.and_then(payload::extract_command_text)
			.map(|command| shell_command_category(&command))
			.unwrap_or_else(|| String::from("shell"));

		if command_category == "pr_land" {
			return (CHILD_BUCKET_PR_LAND.to_owned(), String::from("exec_command: pr_land"));
		}

		return (CHILD_BUCKET_SHELL.to_owned(), format!("exec_command: {command_category}"));
	}

	(CHILD_BUCKET_TOOL.to_owned(), tool_name.to_owned())
}

pub(in crate::agent::app_server::activity::child) fn child_activity_bucket_mut<'a>(
	summary: &'a mut ChildAgentActivitySummary,
	name: &str,
) -> &'a mut ChildAgentActivityBucket {
	if let Some(index) = summary.buckets.iter().position(|bucket| bucket.name == name) {
		return &mut summary.buckets[index];
	}

	summary.buckets.push(ChildAgentActivityBucket {
		name: name.to_owned(),
		..ChildAgentActivityBucket::default()
	});

	let last_index = summary.buckets.len().saturating_sub(1);

	&mut summary.buckets[last_index]
}

fn is_tracker_tool_name(normalized_tool: &str) -> bool {
	matches!(
		normalized_tool,
		"issue_transition"
			| "issue_comment"
			| "issue_progress_checkpoint"
			| "issue_review_checkpoint"
			| "issue_review_handoff"
			| "issue_review_repair_complete"
			| "issue_delivery_closeout_complete"
			| "issue_terminal_finalize"
			| "issue_label_add"
	) || normalized_tool.ends_with(".issue_transition")
		|| normalized_tool.ends_with(".issue_comment")
		|| normalized_tool.ends_with(".issue_progress_checkpoint")
		|| normalized_tool.ends_with(".issue_review_checkpoint")
		|| normalized_tool.ends_with(".issue_review_handoff")
		|| normalized_tool.ends_with(".issue_review_repair_complete")
		|| normalized_tool.ends_with(".issue_delivery_closeout_complete")
		|| normalized_tool.ends_with(".issue_terminal_finalize")
		|| normalized_tool.ends_with(".issue_label_add")
}

fn shell_command_category(command: &str) -> String {
	let trimmed = command.trim();
	let lowered = trimmed.to_ascii_lowercase();

	if lowered.starts_with("git push")
		|| lowered.starts_with("gh pr")
		|| lowered.contains(" gh pr ")
		|| lowered.contains("decodex land")
		|| lowered.contains("issue_terminal_finalize")
	{
		return String::from("pr_land");
	}
	if lowered.starts_with("cargo make")
		|| lowered.starts_with("cargo test")
		|| lowered.starts_with("npm run check")
		|| lowered.contains(" nextest ")
	{
		return String::from("checks");
	}
	if lowered.starts_with("git ") {
		return String::from("git");
	}
	if lowered.starts_with("gh ") {
		return String::from("gh");
	}
	if lowered.contains("vite") || lowered.contains("dev server") || lowered.contains("localhost") {
		return String::from("dev_server");
	}
	if lowered.contains("playwright") || lowered.contains("browser") {
		return String::from("browser_smoke");
	}

	String::from("shell")
}
