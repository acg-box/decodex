use serde_json::Value;

use crate::agent::tracker_tool_bridge::{
	DocsImpact, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
	PullRequestDetails, ReviewHandoffContext, RunCompletionDisposition, TrackerToolBridge,
	tools::{REVIEW_COMPLETION_INTENT_EVENT_TYPE, TERMINAL_FINALIZE_EVENT_TYPE},
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge::tools) fn append_review_completion_intent(
		&self,
		review_context: &ReviewHandoffContext,
		path: RunCompletionDisposition,
		pull_request: &PullRequestDetails,
		summary: &str,
	) -> Result<(), String> {
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"`{}` requires the Decodex runtime state store for issue `{}`.",
				self.required_pr_completion_tool_name(),
				self.issue.identifier
			)
		})?;

		state_store
			.append_private_execution_event(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
				REVIEW_COMPLETION_INTENT_EVENT_TYPE,
				serde_json::json!({
					"path": path.as_str(),
					"mode": review_context.mode.as_str(),
					"branch": review_context.branch_name.as_str(),
					"worktree_path": review_context.worktree_path.as_str(),
					"pr_url": pull_request.url.as_str(),
					"pr_base_ref": pull_request.base_ref_name.as_str(),
					"pr_head_ref": pull_request.head_ref_name.as_str(),
					"pr_head_oid": pull_request.head_ref_oid.as_str(),
					"summary": summary,
				}),
			)
			.map(|_| ())
			.map_err(|error| {
				format!(
					"Failed to persist review completion intent for issue `{}`: {error}",
					self.issue.identifier
				)
			})
	}

	pub(in crate::agent::tracker_tool_bridge::tools) fn append_terminal_finalize_event(
		&self,
		review_context: &ReviewHandoffContext,
		path: RunCompletionDisposition,
	) -> Result<(), String> {
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"`{ISSUE_TERMINAL_FINALIZE_TOOL_NAME}` requires the Decodex runtime state store for issue `{}`.",
				self.issue.identifier
			)
		})?;

		state_store
			.append_private_execution_event(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
				TERMINAL_FINALIZE_EVENT_TYPE,
				serde_json::json!({
					"path": path.as_str(),
					"mode": review_context.mode.as_str(),
					"branch": review_context.branch_name.as_str(),
					"worktree_path": review_context.worktree_path.as_str(),
				}),
			)
			.map(|_| ())
			.map_err(|error| {
				format!(
					"Failed to persist terminal finalize intent for issue `{}`: {error}",
					self.issue.identifier
				)
			})
	}

	pub(in crate::agent::tracker_tool_bridge::tools) fn ensure_docs_impact_checkpoint(
		&self,
		review_context: &ReviewHandoffContext,
		path: RunCompletionDisposition,
	) -> Result<(), String> {
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"`{ISSUE_TERMINAL_FINALIZE_TOOL_NAME}` requires the Decodex runtime state store for issue `{}`.",
				self.issue.identifier
			)
		})?;
		let local_repo = self.current_local_repo_details(review_context)?;
		let events = state_store
			.list_private_execution_events(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
			)
			.map_err(|error| {
				format!(
					"Failed to inspect docs-impact checkpoints for issue `{}`: {error}",
					self.issue.identifier
				)
			})?;
		let Some(checkpoint) =
			events.iter().rev().find(|event| event.event_type() == "progress_checkpoint")
		else {
			return Err(format!(
				"`{ISSUE_TERMINAL_FINALIZE_TOOL_NAME}` path `{}` requires a prior `{ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME}` with `docs_impact` for the current lane HEAD `{}`.",
				path.as_str(),
				local_repo.head_oid
			));
		};
		let has_docs_impact = checkpoint
			.payload()
			.get("docs_impact")
			.and_then(Value::as_str)
			.is_some_and(|value| DocsImpact::parse(value).is_ok());
		let matches_current_head = checkpoint
			.payload()
			.get("head_sha")
			.and_then(Value::as_str)
			.is_some_and(|head_sha| head_sha == local_repo.head_oid);

		if has_docs_impact && matches_current_head {
			return Ok(());
		}

		Err(format!(
			"`{ISSUE_TERMINAL_FINALIZE_TOOL_NAME}` path `{}` requires the latest `{ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME}` to record `docs_impact` for the current lane HEAD `{}`.",
			path.as_str(),
			local_repo.head_oid
		))
	}

	pub(in crate::agent::tracker_tool_bridge::tools) fn clear_review_policy_state_after_completion(
		&self,
		review_context: &ReviewHandoffContext,
		tool_name: &str,
	) -> Result<(), String> {
		if let Some(state_store) = self.state_store {
			state_store
				.clear_review_policy_checkpoints_for_run_attempt(
					&review_context.service_id,
					&self.issue.id,
					&review_context.run_id,
					review_context.attempt_number,
				)
				.map_err(|error| {
					format!(
						"Failed to clear review policy state for issue `{}` after recording `{tool_name}`: {error}",
						self.issue.identifier
					)
				})?;
		} else if review_context.decodex_review_checkpoint_enabled() {
			return Err(format!(
				"Runtime state store is required to clear review policy state for issue `{}` after recording `{tool_name}`.",
				self.issue.identifier
			));
		}

		Ok(())
	}
}
