use serde_json::Value;

use crate::{
	agent::tracker_tool_bridge::{
		self, ISSUE_TRANSITION_TOOL_NAME, PendingReviewCompletion, PullRequestDetails,
		ReviewExecutionMode, ReviewHandoffContext, RunCompletionDisposition, ScopeArgs,
		TrackerToolBridge, review, tools::REVIEW_COMPLETION_INTENT_EVENT_TYPE,
	},
	prelude::eyre,
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker},
	tracker::{TrackerIssue, records::LinearExecutionEventRecord},
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn persist_terminal_review_handoff_marker(
		&self,
		review_context: &ReviewHandoffContext,
	) -> std::result::Result<(), String> {
		let pending_review_handoff = {
			let pending_review_handoff = self.pending_review_completion.borrow();
			let Some(PendingReviewCompletion::Handoff(pending_review_handoff)) =
				pending_review_handoff.as_ref()
			else {
				return Err(format!(
					"`issue_terminal_finalize` cannot persist review handoff lifecycle state for issue `{}` because no PR-backed review handoff is pending.",
					self.issue.identifier
				));
			};

			pending_review_handoff.clone()
		};
		let pull_request =
			self.validate_review_action_pr(review_context, &pending_review_handoff.pr_url)?;

		self.require_matching_review_completion_intent(
			review_context,
			RunCompletionDisposition::ReviewHandoff,
			&pull_request,
		)?;

		let handoff_marker =
			review::review_handoff_marker_from_pull_request(review_context, &pull_request);

		self.persist_review_handoff_marker_for_handoff(review_context, &handoff_marker).map_err(
			|error| {
				format!(
					"Failed to persist durable review handoff lifecycle marker for issue `{}`: {error}",
					self.issue.identifier
				)
			},
		)
	}

	fn require_matching_review_completion_intent(
		&self,
		review_context: &ReviewHandoffContext,
		path: RunCompletionDisposition,
		pull_request: &PullRequestDetails,
	) -> std::result::Result<(), String> {
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"`{}` requires the Decodex runtime state store for issue `{}`.",
				self.required_pr_completion_tool_name(),
				self.issue.identifier
			)
		})?;
		let events = state_store
			.list_private_execution_events(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
			)
			.map_err(|error| {
				format!(
					"Failed to read private review completion intent for issue `{}`: {error}",
					self.issue.identifier
				)
			})?;
		let exact_intent_exists = events.iter().rev().any(|event| {
			let payload = event.payload();

			event.event_type() == REVIEW_COMPLETION_INTENT_EVENT_TYPE
				&& payload.get("path").and_then(Value::as_str) == Some(path.as_str())
				&& payload.get("mode").and_then(Value::as_str) == Some(review_context.mode.as_str())
				&& payload.get("branch").and_then(Value::as_str)
					== Some(review_context.branch_name.as_str())
				&& payload.get("worktree_path").and_then(Value::as_str)
					== Some(review_context.worktree_path.as_str())
				&& payload.get("pr_url").and_then(Value::as_str) == Some(pull_request.url.as_str())
				&& payload.get("pr_base_ref").and_then(Value::as_str)
					== Some(pull_request.base_ref_name.as_str())
				&& payload.get("pr_head_ref").and_then(Value::as_str)
					== Some(pull_request.head_ref_name.as_str())
				&& payload.get("pr_head_oid").and_then(Value::as_str)
					== Some(pull_request.head_ref_oid.as_str())
		});

		if exact_intent_exists {
			return Ok(());
		}

		Err(format!(
			"`issue_terminal_finalize` requires an exact private review completion intent for issue `{}` before writing local review handoff lifecycle state.",
			self.issue.identifier
		))
	}
}

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge::review) fn persist_linear_execution_event(
		&self,
		record: &LinearExecutionEventRecord,
	) -> crate::prelude::Result<()> {
		if let Some(state_store) = self.state_store {
			state_store.record_linear_execution_event(record)?;
		}

		Ok(())
	}

	pub(in crate::agent::tracker_tool_bridge::review) fn persist_review_handoff_marker(
		&self,
		review_context: &ReviewHandoffContext,
		marker: &ReviewHandoffMarker,
	) -> crate::prelude::Result<()> {
		let state_store = self.state_store.ok_or_else(|| {
			eyre::eyre!(
				"Runtime state store is required to persist review handoff for issue `{}`.",
				self.issue.identifier
			)
		})?;

		state_store.upsert_review_handoff_marker(&review_context.service_id, &self.issue.id, marker)
	}

	pub(in crate::agent::tracker_tool_bridge::review) fn persist_review_handoff_marker_for_handoff(
		&self,
		review_context: &ReviewHandoffContext,
		marker: &ReviewHandoffMarker,
	) -> crate::prelude::Result<()> {
		let state_store = self.state_store.ok_or_else(|| {
			eyre::eyre!(
				"Runtime state store is required to persist review handoff for issue `{}`.",
				self.issue.identifier
			)
		})?;

		if let Some(existing) = state_store.review_handoff_marker(
			&review_context.service_id,
			&self.issue.id,
			&review_context.branch_name,
		)? && !review::review_handoff_marker_lineage_matches(&existing, marker)
		{
			eyre::bail!(
				"Existing review lifecycle record for issue `{}` branch `{}` points at PR `{}` head `{}`, but the current review handoff intent points at PR `{}` head `{}`. Use explicit review-handoff recovery before rebinding this lane.",
				self.issue.identifier,
				review_context.branch_name,
				existing.pr_url(),
				existing.pr_head_oid(),
				marker.pr_url(),
				marker.pr_head_oid()
			);
		}

		self.persist_review_handoff_marker(review_context, marker)
	}

	pub(in crate::agent::tracker_tool_bridge::review) fn persist_review_orchestration_marker(
		&self,
		review_context: &ReviewHandoffContext,
		marker: &ReviewOrchestrationMarker,
	) -> crate::prelude::Result<()> {
		let state_store = self.state_store.ok_or_else(|| {
			eyre::eyre!(
				"Runtime state store is required to persist review orchestration for issue `{}`.",
				self.issue.identifier
			)
		})?;

		state_store.upsert_review_orchestration_marker(
			&review_context.service_id,
			&self.issue.id,
			marker,
		)
	}
}

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn validate_review_action_pr(
		&self,
		review_context: &ReviewHandoffContext,
		pr_url: &str,
	) -> std::result::Result<PullRequestDetails, String> {
		let github_token =
			tracker_tool_bridge::resolve_review_handoff_github_token(review_context)?;
		let pull_request = self.pull_request_inspector.inspect_pull_request(
			&review_context.cwd,
			pr_url,
			github_token.as_str(),
			review_context.github_command_path.as_deref(),
		)?;
		let local_repo = self.local_repo_inspector.inspect_local_repo(&review_context.cwd)?;

		if pull_request.head_repository_owner != local_repo.repository_owner
			|| pull_request.head_repository_name != local_repo.repository_name
		{
			return Err(format!(
				"Pull request `{}` belongs to repository `{}/{}`, but the current lane repository is `{}/{}`.",
				pull_request.url,
				pull_request.head_repository_owner,
				pull_request.head_repository_name,
				local_repo.repository_owner,
				local_repo.repository_name
			));
		}
		if pull_request.url != pr_url {
			return Err(format!(
				"Pull request readback returned `{}` while validating requested PR `{}`.",
				pull_request.url, pr_url
			));
		}
		if pull_request.base_ref_name != local_repo.default_branch {
			return Err(format!(
				"Pull request `{}` targets base branch `{}`, but retained review lanes must target the repository default branch `{}`.",
				pull_request.url, pull_request.base_ref_name, local_repo.default_branch
			));
		}
		if pull_request.head_ref_name != review_context.branch_name {
			return Err(format!(
				"Pull request `{}` is for branch `{}`, but the current lane branch is `{}`.",
				pull_request.url, pull_request.head_ref_name, review_context.branch_name
			));
		}
		if pull_request.head_ref_oid != local_repo.head_oid {
			return Err(format!(
				"Pull request `{}` points at commit `{}`, but the current lane HEAD is `{}`. Push the latest lane commit before review handoff.",
				pull_request.url, pull_request.head_ref_oid, local_repo.head_oid
			));
		}
		if pull_request.state != "OPEN" {
			return Err(format!(
				"Pull request `{}` is `{}`; it must be open for review handoff.",
				pull_request.url, pull_request.state
			));
		}
		if pull_request.is_draft {
			return Err(format!(
				"Pull request `{}` is still draft; mark it ready for review before handoff.",
				pull_request.url
			));
		}

		if let Some(recorded_pr_url) = review_context.recorded_pr_url.as_deref()
			&& pull_request.url != recorded_pr_url
		{
			return Err(format!(
				"Pull request `{}` does not match the retained lane PR `{}`.",
				pull_request.url, recorded_pr_url
			));
		}

		Ok(pull_request)
	}

	pub(in crate::agent::tracker_tool_bridge) fn validate_closeout_pr(
		&self,
		review_context: &ReviewHandoffContext,
		pr_url: &str,
	) -> std::result::Result<PullRequestDetails, String> {
		let github_token =
			tracker_tool_bridge::resolve_review_handoff_github_token(review_context)?;
		let pull_request = self.pull_request_inspector.inspect_pull_request(
			&review_context.cwd,
			pr_url,
			github_token.as_str(),
			review_context.github_command_path.as_deref(),
		)?;
		let local_repo = self.local_repo_inspector.inspect_local_repo(&review_context.cwd)?;

		if pull_request.head_repository_owner != local_repo.repository_owner
			|| pull_request.head_repository_name != local_repo.repository_name
		{
			return Err(format!(
				"Pull request `{}` belongs to repository `{}/{}`, but the current lane repository is `{}/{}`.",
				pull_request.url,
				pull_request.head_repository_owner,
				pull_request.head_repository_name,
				local_repo.repository_owner,
				local_repo.repository_name
			));
		}
		if pull_request.base_ref_name != local_repo.default_branch {
			return Err(format!(
				"Pull request `{}` targets base branch `{}`, but retained closeout requires the repository default branch `{}`.",
				pull_request.url, pull_request.base_ref_name, local_repo.default_branch
			));
		}
		if pull_request.head_ref_name != review_context.branch_name {
			return Err(format!(
				"Pull request `{}` is for branch `{}`, but the current lane branch is `{}`.",
				pull_request.url, pull_request.head_ref_name, review_context.branch_name
			));
		}
		if pull_request.head_ref_oid != local_repo.head_oid {
			return Err(format!(
				"Pull request `{}` points at commit `{}`, but the current lane HEAD is `{}`. Finish closeout from the merged lane head.",
				pull_request.url, pull_request.head_ref_oid, local_repo.head_oid
			));
		}
		if pull_request.state != "MERGED" {
			return Err(format!(
				"Pull request `{}` is `{}`; it must be merged before closeout completes.",
				pull_request.url, pull_request.state
			));
		}
		if pull_request.is_draft {
			return Err(format!(
				"Pull request `{}` is still draft; closeout requires a merged non-draft PR lineage.",
				pull_request.url
			));
		}

		if let Some(recorded_pr_url) = review_context.recorded_pr_url.as_deref()
			&& pull_request.url != recorded_pr_url
		{
			return Err(format!(
				"Pull request `{}` does not match the retained lane PR `{}`.",
				pull_request.url, recorded_pr_url
			));
		}

		Ok(pull_request)
	}
}

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn ensure_issue_scope(
		&self,
		scope: &ScopeArgs,
	) -> std::result::Result<(), String> {
		if let Some(issue_id) = scope.issue_id.as_deref()
			&& issue_id != self.issue.id
		{
			return Err(format!(
				"Tool call targeted issue id `{issue_id}`, but the leased issue id is `{}`.",
				self.issue.id
			));
		}
		if let Some(issue_identifier) = scope.issue_identifier.as_deref()
			&& issue_identifier != self.issue.identifier
		{
			return Err(format!(
				"Tool call targeted issue identifier `{issue_identifier}`, but the leased issue identifier is `{}`.",
				self.issue.identifier
			));
		}

		Ok(())
	}

	pub(in crate::agent::tracker_tool_bridge) fn allowed_transition_states(&self) -> Vec<&str> {
		let tracker = self.workflow.frontmatter().tracker();

		if matches!(
			self.review_context.as_ref().map(|context| context.mode),
			Some(ReviewExecutionMode::Closeout)
		) {
			return vec![tracker.resolved_completed_state()];
		}

		let success_state = tracker.success_state();
		let mut states = tracker
			.startable_states()
			.iter()
			.map(String::as_str)
			.filter(|state| *state != success_state)
			.collect::<Vec<_>>();

		for state in [tracker.in_progress_state(), tracker.failure_state()] {
			if state != success_state && !states.iter().any(|existing| existing == &state) {
				states.push(state);
			}
		}

		states
	}

	pub(in crate::agent::tracker_tool_bridge) fn refreshed_issue_snapshot(
		&self,
	) -> crate::prelude::Result<Option<TrackerIssue>> {
		let issue_ids = [self.issue.id.clone()];
		let mut refreshed_issues = self.tracker.refresh_issues(&issue_ids)?;

		Ok(refreshed_issues.pop())
	}

	pub(in crate::agent::tracker_tool_bridge) fn record_continuation_blocking_transition(
		&self,
		state: &str,
	) {
		if state != self.workflow.frontmatter().tracker().in_progress_state() {
			self.record_continuation_blocking_write(format!(
				"`{ISSUE_TRANSITION_TOOL_NAME}` to state `{state}`"
			));
		}
	}

	pub(in crate::agent::tracker_tool_bridge) fn record_continuation_blocking_write(
		&self,
		reason: String,
	) {
		self.continuation_blocking_tracker_write.replace(Some(reason));
	}

	pub(in crate::agent::tracker_tool_bridge) fn local_issue_remains_active(&self) -> bool {
		self.local_issue_state_name.borrow().as_str()
			== self.workflow.frontmatter().tracker().in_progress_state()
			&& !*self.local_opt_out_requested.borrow()
			&& !*self.manual_attention_requested.borrow()
	}

	pub(in crate::agent::tracker_tool_bridge) fn continuation_blocking_write_reason(
		&self,
	) -> crate::prelude::Result<Option<String>> {
		let Some(reason) = self.continuation_blocking_tracker_write.borrow().clone() else {
			return Ok(None);
		};
		let tracker_policy = self.workflow.frontmatter().tracker();
		let run_started_active = self.issue.state.name == tracker_policy.in_progress_state();

		if run_started_active && !self.local_issue_remains_active() {
			return Ok(Some(reason));
		}

		let issue = match self.refreshed_issue_snapshot()? {
			Some(issue) => issue,
			None => return Ok(Some(reason)),
		};
		let issue_still_active = issue.state.name == tracker_policy.in_progress_state()
			&& !issue.has_label(tracker_policy.opt_out_label())
			&& !issue.has_label(tracker_policy.needs_attention_label());

		if issue_still_active {
			return Ok(None);
		}

		Ok(Some(reason))
	}

	pub(crate) fn startup_transition_succeeded_locally(&self) -> bool {
		self.local_issue_state_name.borrow().as_str()
			== self.workflow.frontmatter().tracker().in_progress_state()
	}
}
