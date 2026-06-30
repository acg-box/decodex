use color_eyre::Report;
use serde_json::Value;

use crate::{
	agent::tracker_tool_bridge::{
		self, CLOSEOUT_PUBLIC_SUMMARY_FALLBACK, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, LocalRepoDetails,
		PendingReviewAction, PendingReviewCompletion, PullRequestDetails,
		REVIEW_HANDOFF_PUBLIC_SUMMARY_FALLBACK, REVIEW_POLICY_CONVERGENCE_BUDGET,
		REVIEW_REPAIR_PUBLIC_SUMMARY_FALLBACK, ReviewExecutionMode, ReviewHandoffContext,
		ReviewHandoffWritebackFailed, ReviewPolicyPhase, ReviewPolicyState, ReviewPolicyStatus,
		ReviewPolicyStopReason, ReviewPolicyStopRequested, RunCompletionDisposition, ScopeArgs,
		TrackerToolBridge, tools::REVIEW_COMPLETION_INTENT_EVENT_TYPE,
	},
	prelude::eyre,
	state::{
		ReviewCheckpointArtifactLookup, ReviewHandoffMarker, ReviewOrchestrationMarker,
		ReviewPolicyCheckpoint, ReviewPolicyCheckpointInput,
	},
	tracker::{
		self, TrackerIssue,
		records::{self, LinearExecutionEventPublicProjection},
	},
};

enum CloseoutIssueStateValidation {
	RefreshRequired,
	AlreadyVerified,
}

impl<'a> TrackerToolBridge<'a> {
	fn persist_linear_execution_event(
		&self,
		record: &records::LinearExecutionEventRecord,
	) -> crate::prelude::Result<()> {
		if let Some(state_store) = self.state_store {
			state_store.record_linear_execution_event(record)?;
		}

		Ok(())
	}

	fn persist_review_handoff_marker(
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

	fn persist_review_handoff_marker_for_handoff(
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
		)? && !review_handoff_marker_lineage_matches(&existing, marker)
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

	fn persist_review_orchestration_marker(
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

	pub(super) fn persist_review_policy_state(
		&self,
		review_context: &ReviewHandoffContext,
		review_policy_phase: ReviewPolicyPhase,
		review_policy_status: ReviewPolicyStatus,
		head_sha: &str,
		nonclean_rounds: i64,
		details_json: &str,
	) -> Result<(), String> {
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"Runtime state store is required to persist review checkpoint state for issue `{}`.",
				self.issue.identifier
			)
		})?;

		state_store
			.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
				project_id: &review_context.service_id,
				issue_id: &self.issue.id,
				run_id: &review_context.run_id,
				attempt_number: review_context.attempt_number,
				phase: review_policy_phase.as_str(),
				review_level: review_context.review_level.as_str(),
				status: review_policy_status.as_str(),
				head_sha,
				nonclean_rounds,
				details_json,
			})
			.map_err(|error| {
				format!(
					"Failed to persist review checkpoint state for issue `{}`: {error}",
					self.issue.identifier
				)
			})?;

		Ok(())
	}

	pub(super) fn ensure_issue_scope(&self, scope: &ScopeArgs) -> Result<(), String> {
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

	pub(super) fn allowed_transition_states(&self) -> Vec<&str> {
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

	pub(super) fn refreshed_issue_snapshot(&self) -> crate::prelude::Result<Option<TrackerIssue>> {
		let issue_ids = [self.issue.id.clone()];
		let mut refreshed_issues = self.tracker.refresh_issues(&issue_ids)?;

		Ok(refreshed_issues.pop())
	}

	pub(super) fn validate_review_action_pr(
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

	pub(super) fn validate_closeout_pr(
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

	pub(super) fn record_continuation_blocking_transition(&self, state: &str) {
		if state != self.workflow.frontmatter().tracker().in_progress_state() {
			self.record_continuation_blocking_write(format!(
				"`{ISSUE_TRANSITION_TOOL_NAME}` to state `{state}`"
			));
		}
	}

	pub(super) fn record_continuation_blocking_write(&self, reason: String) {
		self.continuation_blocking_tracker_write.replace(Some(reason));
	}

	pub(super) fn local_issue_remains_active(&self) -> bool {
		self.local_issue_state_name.borrow().as_str()
			== self.workflow.frontmatter().tracker().in_progress_state()
			&& !*self.local_opt_out_requested.borrow()
			&& !*self.manual_attention_requested.borrow()
	}

	pub(super) fn continuation_blocking_write_reason(
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

	pub(super) fn persist_terminal_review_handoff_marker(
		&self,
		review_context: &ReviewHandoffContext,
	) -> Result<(), String> {
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

		let handoff_marker = review_handoff_marker_from_pull_request(review_context, &pull_request);

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
	) -> Result<(), String> {
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

	pub(crate) fn completion_disposition(
		&self,
	) -> crate::prelude::Result<RunCompletionDisposition> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};
		let manual_attention_requested = *self.manual_attention_requested.borrow();
		let manual_attention_comment_recorded = *self.manual_attention_comment_recorded.borrow();
		let review_completion = self.pending_review_completion.borrow().clone();

		match (manual_attention_requested, manual_attention_comment_recorded, review_completion) {
			(false, false, Some(PendingReviewCompletion::Handoff(_))) => {
				Ok(RunCompletionDisposition::ReviewHandoff)
			},
			(false, false, Some(PendingReviewCompletion::Repair(_))) => {
				Ok(RunCompletionDisposition::ReviewRepair)
			},
			(false, false, Some(PendingReviewCompletion::Closeout(_))) => {
				Ok(RunCompletionDisposition::Closeout)
			},
			(true, true, None) => Ok(RunCompletionDisposition::ManualAttention),
			(true, false, None) => eyre::bail!(
				"Run `{}` requested human attention with label `{}`, but issue `{}` never recorded the required explanatory comment.",
				review_context.run_id,
				self.workflow.frontmatter().tracker().needs_attention_label(),
				self.issue.identifier
			),
			(true, _, Some(_)) => eyre::bail!(
				"Run `{}` recorded both `{}` and label `{}`. Use exactly one final tracker exit path.",
				review_context.run_id,
				self.required_pr_completion_tool_name(),
				self.workflow.frontmatter().tracker().needs_attention_label()
			),
			(false, false, None) => eyre::bail!(
				"Run `{}` completed, but issue `{}` recorded neither `{}` nor label `{}` for human attention.",
				review_context.run_id,
				self.issue.identifier,
				self.required_pr_completion_tool_name(),
				self.workflow.frontmatter().tracker().needs_attention_label()
			),
			(false, true, None) | (false, true, Some(_)) => eyre::bail!(
				"Run `{}` recorded a human-attention comment for issue `{}`, but never recorded label `{}`.",
				review_context.run_id,
				self.issue.identifier,
				self.workflow.frontmatter().tracker().needs_attention_label()
			),
		}
	}

	pub(crate) fn has_tracker_exit_signal(&self) -> bool {
		*self.manual_attention_requested.borrow()
			|| *self.manual_attention_comment_recorded.borrow()
			|| self.pending_review_completion.borrow().is_some()
	}

	pub(crate) fn finalized_completion_disposition(
		&self,
	) -> crate::prelude::Result<Option<RunCompletionDisposition>> {
		let Some(finalized_path) = *self.finalized_completion_path.borrow() else {
			return Ok(None);
		};
		let completion_path = self.completion_disposition()?;

		if finalized_path != completion_path {
			let Some(review_context) = self.review_context.as_ref() else {
				eyre::bail!(
					"Review handoff context is unavailable for issue `{}`.",
					self.issue.identifier
				);
			};

			eyre::bail!(
				"Run `{}` finalized terminal path `{}`, but the recorded terminal path resolved to `{}` after app-server failure.",
				review_context.run_id,
				finalized_path.as_str(),
				completion_path.as_str()
			);
		}

		Ok(Some(finalized_path))
	}

	pub(crate) fn apply_review_handoff(&self) -> crate::prelude::Result<()> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};
		let pending_review_handoff = {
			let pending_review_handoff = self.pending_review_completion.borrow();
			let Some(PendingReviewCompletion::Handoff(pending_review_handoff)) =
				pending_review_handoff.as_ref()
			else {
				eyre::bail!(
					"Run `{}` completed, but issue `{}` never recorded a PR-backed review handoff.",
					review_context.run_id,
					self.issue.identifier
				);
			};

			pending_review_handoff.clone()
		};
		let pull_request = self
			.validate_review_action_pr(review_context, &pending_review_handoff.pr_url)
			.map_err(|error| eyre::eyre!(error))?;
		let success_state = self.workflow.frontmatter().tracker().success_state();
		let success_state_id = self.issue.state_id_for_name(success_state).ok_or_else(|| {
			eyre::eyre!(
				"State `{success_state}` does not exist on issue `{}`.",
				self.issue.identifier
			)
		})?;
		let projection = self.prepare_review_handoff_projection(
			review_context,
			&pending_review_handoff,
			&pull_request,
			success_state,
		)?;
		let handoff_marker = review_handoff_marker_from_pull_request(review_context, &pull_request);
		let orchestration_marker = ReviewOrchestrationMarker::new(
			review_context.run_id.clone(),
			review_context.attempt_number,
			review_context.branch_name.clone(),
			pull_request.url.clone(),
			pull_request.head_ref_oid.clone(),
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);

		self.persist_review_handoff_marker_for_handoff(review_context, &handoff_marker)?;
		self.persist_review_orchestration_marker(review_context, &orchestration_marker)?;

		if let Err(error) = tracker::create_prepared_linear_execution_event_comment(
			self.tracker,
			&self.issue.id,
			&projection,
		) {
			return Err(Report::new(ReviewHandoffWritebackFailed {
				issue_identifier: self.issue.identifier.clone(),
				run_id: review_context.run_id.clone(),
				pr_url: pending_review_handoff.pr_url,
				success_state: success_state.to_owned(),
				source: format!("failed to persist the tracker review handoff record: {error}"),
			}));
		}

		self.persist_linear_execution_event(&projection.record)?;

		if let Err(error) = self.tracker.update_issue_state(&self.issue.id, success_state_id) {
			return Err(Report::new(ReviewHandoffWritebackFailed {
				issue_identifier: self.issue.identifier.clone(),
				run_id: review_context.run_id.clone(),
				pr_url: pull_request.url.clone(),
				success_state: success_state.to_owned(),
				source: format!("failed to move the tracker issue to `{success_state}`: {error}"),
			}));
		}

		self.pending_review_completion.borrow_mut().take();

		Ok(())
	}

	fn prepare_review_handoff_projection(
		&self,
		review_context: &ReviewHandoffContext,
		pending_review_handoff: &PendingReviewAction,
		pull_request: &PullRequestDetails,
		success_state: &str,
	) -> crate::prelude::Result<LinearExecutionEventPublicProjection> {
		let public_summary = tracker_tool_bridge::public_summary_or_fallback(
			&pending_review_handoff.summary,
			REVIEW_HANDOFF_PUBLIC_SUMMARY_FALLBACK,
		);
		let completion_comment = tracker_tool_bridge::format_review_handoff_comment(
			review_context,
			pending_review_handoff,
			public_summary.as_ref(),
		);
		let handoff_record = linear_execution_review_event(
			self.issue,
			review_context,
			pull_request,
			"review_handoff",
			"review_handoff",
			public_summary.as_ref(),
		);

		tracker::prepare_linear_execution_event_comment(
			&completion_comment,
			&handoff_record,
			self.public_projection_privacy_classifier,
		)
		.map_err(|error| {
			Report::new(ReviewHandoffWritebackFailed {
				issue_identifier: self.issue.identifier.clone(),
				run_id: review_context.run_id.clone(),
				pr_url: pull_request.url.clone(),
				success_state: success_state.to_owned(),
				source: format!("failed to prepare the tracker review handoff record: {error}"),
			})
		})
	}

	pub(crate) fn apply_review_repair(&self) -> crate::prelude::Result<()> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};
		let pending_review_repair = {
			let pending_review_repair = self.pending_review_completion.borrow();
			let Some(PendingReviewCompletion::Repair(pending_review_repair)) =
				pending_review_repair.as_ref()
			else {
				eyre::bail!(
					"Run `{}` completed, but issue `{}` never recorded retained review repair completion.",
					review_context.run_id,
					self.issue.identifier
				);
			};

			pending_review_repair.clone()
		};
		let pull_request = self
			.validate_review_action_pr(review_context, &pending_review_repair.pr_url)
			.map_err(|error| eyre::eyre!(error))?;
		let public_summary = tracker_tool_bridge::public_summary_or_fallback(
			&pending_review_repair.summary,
			REVIEW_REPAIR_PUBLIC_SUMMARY_FALLBACK,
		);
		let completion_comment = tracker_tool_bridge::format_review_repair_comment(
			review_context,
			&pending_review_repair,
			public_summary.as_ref(),
		);
		let handoff_record = linear_execution_review_event(
			self.issue,
			review_context,
			&pull_request,
			"repair_handoff",
			"review_repair",
			public_summary.as_ref(),
		);
		let review_handoff = ReviewHandoffMarker::new(
			review_context.run_id.clone(),
			review_context.attempt_number,
			review_context.branch_name.clone(),
			pull_request.url.clone(),
			pull_request.base_ref_name.clone(),
			pull_request.head_ref_name.clone(),
			pull_request.head_ref_oid.clone(),
		);
		let projection = tracker::prepare_linear_execution_event_comment(
			&completion_comment,
			&handoff_record,
			self.public_projection_privacy_classifier,
		)?;
		let state_store = self.state_store.ok_or_else(|| {
			eyre::eyre!(
				"Runtime state store is required to read review orchestration for issue `{}`.",
				self.issue.identifier
			)
		})?;
		let previous_review_handoff = state_store.review_handoff_marker(
			&review_context.service_id,
			&self.issue.id,
			&review_context.branch_name,
		)?;
		let persisted_orchestration = previous_review_handoff
			.as_ref()
			.map(|marker| {
				state_store.review_orchestration_marker(
					&review_context.service_id,
					&self.issue.id,
					marker,
				)
			})
			.transpose()?
			.flatten();
		let external_round_count =
			persisted_orchestration.map_or(0, |marker| marker.external_round_count());

		tracker::create_prepared_linear_execution_event_comment(
			self.tracker,
			&self.issue.id,
			&projection,
		)?;

		self.persist_linear_execution_event(&projection.record)?;
		self.persist_review_handoff_marker(review_context, &review_handoff)?;
		self.persist_review_orchestration_marker(
			review_context,
			&ReviewOrchestrationMarker::new(
				review_context.run_id.clone(),
				review_context.attempt_number,
				review_context.branch_name.clone(),
				pull_request.url.clone(),
				pull_request.head_ref_oid.clone(),
				"request_pending",
				None,
				None,
				None,
				0,
				external_round_count,
				None,
			),
		)?;
		self.pending_review_completion.borrow_mut().take();

		Ok(())
	}

	pub(crate) fn apply_closeout(&self) -> crate::prelude::Result<()> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};
		let pending_closeout = {
			let pending_review_completion = self.pending_review_completion.borrow();
			let Some(PendingReviewCompletion::Closeout(pending_closeout)) =
				pending_review_completion.as_ref()
			else {
				eyre::bail!(
					"Run `{}` completed, but issue `{}` never recorded retained closeout completion.",
					review_context.run_id,
					self.issue.identifier
				);
			};

			pending_closeout.clone()
		};

		self.write_closeout_record(
			review_context,
			&pending_closeout.pr_url,
			CloseoutIssueStateValidation::RefreshRequired,
			&pending_closeout.summary,
		)?;
		self.pending_review_completion.borrow_mut().take();

		Ok(())
	}

	pub(crate) fn validate_deterministic_closeout_pr(
		&self,
		pr_url: &str,
	) -> crate::prelude::Result<PullRequestDetails> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};

		self.validate_closeout_pr(review_context, pr_url).map_err(|error| eyre::eyre!(error))
	}

	pub(crate) fn apply_validated_deterministic_closeout(
		&self,
		pull_request: PullRequestDetails,
	) -> crate::prelude::Result<()> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};

		self.write_validated_closeout_record(
			review_context,
			pull_request,
			CloseoutIssueStateValidation::AlreadyVerified,
			"Validated merged PR lineage and completed retained closeout.",
		)
	}

	fn write_closeout_record(
		&self,
		review_context: &ReviewHandoffContext,
		pr_url: &str,
		issue_state_validation: CloseoutIssueStateValidation,
		summary: &str,
	) -> crate::prelude::Result<()> {
		let pull_request = self
			.validate_closeout_pr(review_context, pr_url)
			.map_err(|error| eyre::eyre!(error))?;

		self.write_validated_closeout_record(
			review_context,
			pull_request,
			issue_state_validation,
			summary,
		)
	}

	fn write_validated_closeout_record(
		&self,
		review_context: &ReviewHandoffContext,
		pull_request: PullRequestDetails,
		issue_state_validation: CloseoutIssueStateValidation,
		summary: &str,
	) -> crate::prelude::Result<()> {
		if matches!(issue_state_validation, CloseoutIssueStateValidation::RefreshRequired) {
			self.validate_closeout_issue_completed_state().map_err(|error| eyre::eyre!(error))?;
		}

		let public_summary = tracker_tool_bridge::public_summary_or_fallback(
			summary,
			CLOSEOUT_PUBLIC_SUMMARY_FALLBACK,
		);
		let closeout_record = linear_execution_closeout_event(
			self.issue,
			review_context,
			&pull_request,
			public_summary.as_ref(),
		);
		let retry_budget_line = self
			.state_store
			.map(|state_store| {
				state_store.retry_budget_attempt_count(&self.issue.id).map(|count| {
					if count > 0 {
						format!("\n- retry_budget_attempts_consumed: `{count}`")
					} else {
						String::new()
					}
				})
			})
			.transpose()?
			.unwrap_or_default();
		let closeout_comment = format!(
			"decodex closeout completed\n\n- run_id: `{}`\n- run_sequence_attempt: `{}` (not retry-budget count){}\n- finished_at: `{}`\n- branch: `{}`\n- pr_url: `{}`\n- worktree_path: `{}`\n- summary: {}",
			review_context.run_id,
			review_context.attempt_number,
			retry_budget_line,
			tracker_tool_bridge::current_timestamp(),
			review_context.branch_name,
			pull_request.url,
			review_context.worktree_path,
			public_summary,
		);
		let projection = tracker::prepare_linear_execution_event_comment(
			&closeout_comment,
			&closeout_record,
			self.public_projection_privacy_classifier,
		)?;

		tracker::create_prepared_linear_execution_event_comment(
			self.tracker,
			&self.issue.id,
			&projection,
		)?;

		self.persist_linear_execution_event(&projection.record)?;

		Ok(())
	}

	pub(crate) fn clear_closeout_issue_scope(&self) -> crate::prelude::Result<()> {
		let Some(review_context) = self.review_context.as_ref() else {
			eyre::bail!(
				"Review handoff context is unavailable for issue `{}`.",
				self.issue.identifier
			);
		};

		tracker::clear_automation_lane_labels(self.tracker, self.issue, &review_context.service_id)
	}

	pub(super) fn canonicalize_current_lane_head_sha(
		&self,
		tool_name: &str,
		head_sha: &str,
		current_head_sha: &str,
	) -> std::result::Result<String, String> {
		let head_sha = head_sha.trim();

		if head_sha.is_empty() {
			return Err(format!("`{tool_name}` requires a non-empty `head_sha`."));
		}
		if head_sha == current_head_sha {
			return Ok(current_head_sha.to_owned());
		}
		if head_sha.len() >= 7 && current_head_sha.starts_with(head_sha) {
			return Ok(current_head_sha.to_owned());
		}

		Err(format!(
			"`{tool_name}` head `{head_sha}` does not match the current lane HEAD `{current_head_sha}`."
		))
	}

	pub(super) fn current_local_repo_details(
		&self,
		review_context: &ReviewHandoffContext,
	) -> std::result::Result<LocalRepoDetails, String> {
		self.local_repo_inspector.inspect_local_repo(&review_context.cwd)
	}

	pub(super) fn resolve_progress_checkpoint_head_sha(
		&self,
		head_sha: Option<String>,
	) -> std::result::Result<Option<String>, String> {
		let normalized_head_sha = tracker_tool_bridge::normalize_optional_progress_field(head_sha);
		let Some(review_context) = self.review_context.as_ref() else {
			return Ok(normalized_head_sha);
		};
		let local_repo = self.current_local_repo_details(review_context)?;

		match normalized_head_sha {
			Some(head_sha) => self
				.canonicalize_current_lane_head_sha(
					ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
					&head_sha,
					&local_repo.head_oid,
				)
				.map(Some),
			None => Ok(Some(local_repo.head_oid)),
		}
	}

	pub(super) fn validate_closeout_issue_completed_state(
		&self,
	) -> std::result::Result<(), String> {
		let completed_state = self.workflow.frontmatter().tracker().resolved_completed_state();
		let current_issue = self.refreshed_issue_snapshot().map_err(|error| error.to_string())?
			.ok_or_else(|| {
				format!(
					"Failed to refresh issue `{}` during closeout validation: tracker returned no current snapshot.",
					self.issue.identifier
				)
			})?;

		if current_issue.state.name != completed_state {
			return Err(format!(
				"Closeout for issue `{}` requires tracker state `{}`, but the refreshed issue is still `{}`. Move the issue to `{}` with `{}` before calling `{}`.",
				self.issue.identifier,
				completed_state,
				current_issue.state.name,
				completed_state,
				ISSUE_TRANSITION_TOOL_NAME,
				ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME
			));
		}

		Ok(())
	}

	fn review_policy_state_from_checkpoint(
		&self,
		checkpoint: ReviewPolicyCheckpoint,
	) -> crate::prelude::Result<ReviewPolicyState> {
		let phase =
			ReviewPolicyPhase::parse(checkpoint.phase()).map_err(|error| eyre::eyre!(error))?;
		let status =
			ReviewPolicyStatus::parse(checkpoint.status()).map_err(|error| eyre::eyre!(error))?;

		Ok(ReviewPolicyState {
			phase,
			status,
			head_sha: checkpoint.head_sha().to_owned(),
			nonclean_rounds: checkpoint.nonclean_rounds(),
			details_json: checkpoint.details_json().to_owned(),
		})
	}

	pub(super) fn review_policy_state_for_current_head(
		&self,
		review_context: &ReviewHandoffContext,
	) -> crate::prelude::Result<Option<ReviewPolicyState>> {
		let Some(current_phase) = ReviewPolicyPhase::for_mode(review_context.mode) else {
			return Ok(None);
		};
		let local_repo =
			self.current_local_repo_details(review_context).map_err(|error| eyre::eyre!(error))?;

		self.review_policy_artifact_for_head(review_context, current_phase, &local_repo.head_oid)
	}

	pub(super) fn review_policy_artifact_for_head(
		&self,
		review_context: &ReviewHandoffContext,
		review_policy_phase: ReviewPolicyPhase,
		head_sha: &str,
	) -> crate::prelude::Result<Option<ReviewPolicyState>> {
		let Some(state_store) = self.state_store else {
			return Ok(None);
		};
		let Some(checkpoint) =
			state_store.review_checkpoint_artifact(ReviewCheckpointArtifactLookup {
				project_id: &review_context.service_id,
				issue_id: &self.issue.id,
				phase: review_policy_phase.as_str(),
				review_level: review_context.review_level.as_str(),
				head_sha,
			})?
		else {
			return Ok(None);
		};

		self.review_policy_state_from_checkpoint(checkpoint).map(Some)
	}

	pub(super) fn require_clean_review_checkpoint(
		&self,
		review_context: &ReviewHandoffContext,
	) -> std::result::Result<(), String> {
		if !review_context.decodex_review_checkpoint_enabled() {
			return Ok(());
		}

		let local_repo = self.current_local_repo_details(review_context)?;

		if !local_repo.review_worktree_clean() {
			return Err(format!(
				"`{}` requires a clean committed lane HEAD before reusing a Decodex Review checkpoint. Commit or revert review-blocking local changes, rerun required validation, and record a fresh clean checkpoint. Review-blocking local changes: {}",
				self.required_pr_completion_tool_name(),
				tracker_tool_bridge::summarize_review_blocking_changes(
					&local_repo.review_blocking_changes
				)
			));
		}

		let Some(checkpoint) = self
			.review_policy_artifact_for_head(
				review_context,
				ReviewPolicyPhase::for_mode(review_context.mode)
					.expect("review completion should only be available during review phases"),
				&local_repo.head_oid,
			)
			.map_err(|error| error.to_string())?
		else {
			return Err(format!(
				"`{}` requires a current `{}` review checkpoint with status `clean` for the current lane HEAD.",
				self.required_pr_completion_tool_name(),
				ReviewPolicyPhase::for_mode(review_context.mode)
					.expect("review completion should only be available during review phases")
					.as_str(),
			));
		};

		if checkpoint.status != ReviewPolicyStatus::Clean {
			return Err(format!(
				"`{}` requires the latest review checkpoint to be `clean`, not `{}`.",
				self.required_pr_completion_tool_name(),
				checkpoint.status.as_str(),
			));
		}

		Ok(())
	}

	pub(super) fn review_policy_stop_requested(
		&self,
		review_context: &ReviewHandoffContext,
	) -> crate::prelude::Result<Option<ReviewPolicyStopRequested>> {
		if !review_context.decodex_review_checkpoint_enabled() {
			return Ok(None);
		}

		let Some(current_phase) = ReviewPolicyPhase::for_mode(review_context.mode) else {
			return Ok(None);
		};
		let Some(state_store) = self.state_store else {
			return Ok(None);
		};

		if !state_store.has_nonclean_review_checkpoint_artifact(
			&review_context.service_id,
			&self.issue.id,
			current_phase.as_str(),
		)? {
			return Ok(None);
		}

		let Some(checkpoint) = self.review_policy_state_for_current_head(review_context)? else {
			return Ok(None);
		};

		Ok(self.review_policy_stop_from_checkpoint(review_context, checkpoint))
	}

	fn review_policy_stop_from_checkpoint(
		&self,
		review_context: &ReviewHandoffContext,
		checkpoint: ReviewPolicyState,
	) -> Option<ReviewPolicyStopRequested> {
		let stop_reason = match checkpoint.status {
			ReviewPolicyStatus::Clean => return None,
			ReviewPolicyStatus::Findings
				if checkpoint.nonclean_rounds < REVIEW_POLICY_CONVERGENCE_BUDGET =>
			{
				return None;
			},
			ReviewPolicyStatus::Findings => ReviewPolicyStopReason::Exhausted,
			ReviewPolicyStatus::NeedsArchitectureReview => {
				ReviewPolicyStopReason::ArchitectureReviewRequired
			},
			ReviewPolicyStatus::Blocked => ReviewPolicyStopReason::Blocked,
		};

		Some(ReviewPolicyStopRequested {
			head_sha: checkpoint.head_sha,
			issue_identifier: self.issue.identifier.clone(),
			fingerprint: review_policy_stop_fingerprint(&checkpoint.details_json),
			nonclean_rounds: Some(checkpoint.nonclean_rounds),
			reason: stop_reason,
			run_id: review_context.run_id.clone(),
		})
	}

	pub(super) fn required_pr_completion_tool_name(&self) -> &'static str {
		match self.review_context.as_ref().map(|context| context.mode) {
			Some(ReviewExecutionMode::Handoff) => ISSUE_REVIEW_HANDOFF_TOOL_NAME,
			Some(ReviewExecutionMode::Repair) => ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
			Some(ReviewExecutionMode::Closeout) => ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
			None => ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		}
	}
}

fn review_policy_stop_fingerprint(details_json: &str) -> Option<String> {
	serde_json::from_str::<Value>(details_json)
		.ok()?
		.get("finding_policy")?
		.get("stop_fingerprint")?
		.as_str()
		.map(str::to_owned)
}

fn review_handoff_marker_from_pull_request(
	review_context: &ReviewHandoffContext,
	pull_request: &PullRequestDetails,
) -> ReviewHandoffMarker {
	ReviewHandoffMarker::new(
		review_context.run_id.clone(),
		review_context.attempt_number,
		review_context.branch_name.clone(),
		pull_request.url.clone(),
		pull_request.base_ref_name.clone(),
		pull_request.head_ref_name.clone(),
		pull_request.head_ref_oid.clone(),
	)
}

fn review_handoff_marker_lineage_matches(
	existing: &ReviewHandoffMarker,
	marker: &ReviewHandoffMarker,
) -> bool {
	existing.branch_name() == marker.branch_name()
		&& existing.pr_url() == marker.pr_url()
		&& existing.target_base_ref_name() == marker.target_base_ref_name()
		&& existing.pr_head_ref_name() == marker.pr_head_ref_name()
		&& existing.pr_head_oid() == marker.pr_head_oid()
}

fn linear_execution_identity<'a>(
	issue: &'a TrackerIssue,
	review_context: &'a ReviewHandoffContext,
) -> records::LinearExecutionEventIdentity<'a> {
	records::LinearExecutionEventIdentity {
		service_id: &review_context.service_id,
		issue_id: &issue.id,
		issue_identifier: &issue.identifier,
		run_id: &review_context.run_id,
		attempt_number: review_context.attempt_number,
	}
}

fn linear_execution_review_event(
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
	pull_request: &PullRequestDetails,
	event_type: &str,
	terminal_path: &str,
	summary: &str,
) -> records::LinearExecutionEventRecord {
	let anchor = records::stable_event_anchor(&[&pull_request.url, &pull_request.head_ref_oid]);
	let mut record = records::LinearExecutionEventRecord::new(
		linear_execution_identity(issue, review_context),
		event_type,
		tracker_tool_bridge::current_timestamp(),
		&anchor,
	);

	record.branch = Some(review_context.branch_name.clone());
	record.worktree_path = Some(review_context.worktree_path.clone());
	record.pr_url = Some(pull_request.url.clone());
	record.pr_head_sha = Some(pull_request.head_ref_oid.clone());
	record.pr_base_ref = Some(pull_request.base_ref_name.clone());
	record.commit_sha = Some(pull_request.head_ref_oid.clone());
	record.validation_result = Some(String::from("passed"));
	record.summary = Some(summary.to_owned());
	record.terminal_path = Some(terminal_path.to_owned());
	record.verification = Some(vec![String::from("repo gate passed before tracker writeback")]);

	record
}

fn linear_execution_closeout_event(
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
	pull_request: &PullRequestDetails,
	summary: &str,
) -> records::LinearExecutionEventRecord {
	let anchor = records::stable_event_anchor(&[&pull_request.url, &pull_request.head_ref_oid]);
	let mut record = records::LinearExecutionEventRecord::new(
		linear_execution_identity(issue, review_context),
		"closeout",
		tracker_tool_bridge::current_timestamp(),
		&anchor,
	);

	record.branch = Some(review_context.branch_name.clone());
	record.worktree_path = Some(review_context.worktree_path.clone());
	record.pr_url = Some(pull_request.url.clone());
	record.commit_sha = Some(pull_request.head_ref_oid.clone());
	record.summary = Some(summary.to_owned());
	record.validation_result = Some(String::from("passed"));

	record
}
