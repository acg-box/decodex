use super::{
	DocsImpact, DynamicToolCallResponse, ExecutionProgressPhase, LinearExecutionEventIdentity,
	LinearExecutionEventRecord, NormalizedProgressCheckpoint, ProgressCheckpointArgs,
	ReviewHandoffContext, StateStore, TrackerToolBridge, Value, records, serde_json, tracker,
	tracker_tool_bridge,
};

impl<'a> TrackerToolBridge<'a> {
	pub(super) fn handle_progress_checkpoint(&self, arguments: Value) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<ProgressCheckpointArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.progress_checkpoint` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let checkpoint = match self.normalize_progress_checkpoint(parsed) {
			Ok(checkpoint) => checkpoint,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};
		let (review_context, state_store) = match self.progress_checkpoint_context() {
			Ok(context) => context,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Err(error) =
			self.append_private_progress_checkpoint(review_context, state_store, &checkpoint)
		{
			return DynamicToolCallResponse::failure(error);
		}

		let public_projection =
			self.render_progress_checkpoint_projection(review_context, &checkpoint);

		match self.publish_progress_checkpoint_projection(state_store, &public_projection) {
			Ok(true) => DynamicToolCallResponse::success(format!(
				"Recorded private `{}` execution state for issue `{}` and published the public Linear projection.",
				checkpoint.phase.as_str(),
				self.issue.identifier
			)),
			Ok(false) => DynamicToolCallResponse::success(format!(
				"Recorded private `{}` execution state for issue `{}`; public Linear projection is unchanged.",
				checkpoint.phase.as_str(),
				self.issue.identifier
			)),
			Err(error) => DynamicToolCallResponse::failure(error),
		}
	}

	fn normalize_progress_checkpoint(
		&self,
		parsed: ProgressCheckpointArgs,
	) -> Result<NormalizedProgressCheckpoint, String> {
		let phase = ExecutionProgressPhase::parse(&parsed.phase)?;
		let docs_impact = DocsImpact::parse(&parsed.docs_impact)?;
		let focus = tracker_tool_bridge::normalize_summary(&parsed.focus);
		let next_action = tracker_tool_bridge::normalize_summary(&parsed.next_action);
		let blockers = tracker_tool_bridge::normalize_progress_list(parsed.blockers);
		let evidence = tracker_tool_bridge::normalize_progress_list(parsed.evidence);
		let verification = tracker_tool_bridge::normalize_progress_list(parsed.verification);
		let head_sha = self.resolve_progress_checkpoint_head_sha(parsed.head_sha)?;
		let branch = tracker_tool_bridge::normalize_optional_progress_field(parsed.branch);
		let pr_url = tracker_tool_bridge::normalize_optional_progress_field(parsed.pr_url);

		if focus.is_empty() {
			return Err(String::from("`issue_progress_checkpoint` requires a non-empty `focus`."));
		}
		if next_action.is_empty() {
			return Err(String::from(
				"`issue_progress_checkpoint` requires a non-empty `next_action`.",
			));
		}
		if phase == ExecutionProgressPhase::Blocked && blockers.is_empty() {
			return Err(String::from(
				"`issue_progress_checkpoint` phase `blocked` requires at least one blocker.",
			));
		}

		Ok(NormalizedProgressCheckpoint {
			phase,
			docs_impact,
			focus,
			next_action,
			blockers,
			evidence,
			verification,
			head_sha,
			branch,
			pr_url,
		})
	}

	fn progress_checkpoint_context(&self) -> Result<(&ReviewHandoffContext, &StateStore), String> {
		let review_context = self.review_context.as_ref().ok_or_else(|| {
			String::from("`issue_progress_checkpoint` requires an active Decodex run context.")
		})?;
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"`issue_progress_checkpoint` requires the Decodex runtime state store for issue `{}`.",
				self.issue.identifier
			)
		})?;

		Ok((review_context, state_store))
	}

	fn append_private_progress_checkpoint(
		&self,
		review_context: &ReviewHandoffContext,
		state_store: &StateStore,
		checkpoint: &NormalizedProgressCheckpoint,
	) -> Result<(), String> {
		let branch = checkpoint.public_branch(review_context);
		let private_payload = serde_json::json!({
				"phase": checkpoint.phase.as_str(),
				"docs_impact": checkpoint.docs_impact.as_str(),
				"focus": checkpoint.focus.as_str(),
			"next_action": checkpoint.next_action.as_str(),
			"blockers": &checkpoint.blockers,
			"evidence": &checkpoint.evidence,
			"verification": &checkpoint.verification,
			"head_sha": checkpoint.head_sha.as_deref(),
			"branch": branch.as_str(),
			"worktree_path": review_context.worktree_path.as_str(),
			"pr_url": checkpoint.pr_url.as_deref(),
		});

		state_store
			.append_private_execution_event(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
				"progress_checkpoint",
				private_payload,
			)
			.map(|_| ())
			.map_err(|error| {
				format!(
					"Failed to persist the private execution-state checkpoint for issue `{}`: {error}",
					self.issue.identifier
				)
			})
	}

	fn render_progress_checkpoint_projection(
		&self,
		review_context: &ReviewHandoffContext,
		checkpoint: &NormalizedProgressCheckpoint,
	) -> LinearExecutionEventRecord {
		let branch = checkpoint.public_branch(review_context);

		records::render_progress_checkpoint_public_projection(
			LinearExecutionEventIdentity {
				service_id: &review_context.service_id,
				issue_id: &self.issue.id,
				issue_identifier: &self.issue.identifier,
				run_id: &review_context.run_id,
				attempt_number: review_context.attempt_number,
			},
			tracker_tool_bridge::current_timestamp(),
			checkpoint.phase.as_str(),
			Some(branch.as_str()),
			Some(review_context.worktree_path.as_str()),
			checkpoint.pr_url.as_deref(),
		)
	}

	fn publish_progress_checkpoint_projection(
		&self,
		state_store: &StateStore,
		public_projection: &LinearExecutionEventRecord,
	) -> Result<bool, String> {
		let projection = tracker::prepare_linear_execution_event_comment(
			"",
			public_projection,
			self.public_projection_privacy_classifier,
		)
		.map_err(|error| {
			format!(
				"Failed to prepare the public progress projection for issue `{}`: {error}",
				self.issue.identifier
			)
		})?;

		if self.progress_checkpoint_projection_cached(state_store, &projection.record)? {
			return Ok(false);
		}

		let comment_created = match tracker::create_prepared_linear_execution_event_comment(
			self.tracker,
			&self.issue.id,
			&projection,
		) {
			Ok(comment_created) => comment_created,
			Err(error) => {
				return Err(format!(
					"Failed to record an execution-state checkpoint for issue `{}`: {error}",
					self.issue.identifier
				));
			},
		};

		state_store.record_linear_execution_event(&projection.record).map_err(|error| {
			format!(
				"Failed to persist the public progress projection cache for issue `{}`: {error}",
				self.issue.identifier
			)
		})?;

		Ok(comment_created)
	}

	fn progress_checkpoint_projection_cached(
		&self,
		state_store: &StateStore,
		public_projection: &LinearExecutionEventRecord,
	) -> Result<bool, String> {
		let records = state_store
			.list_linear_execution_events(
				&public_projection.service_id,
				&public_projection.issue_id,
			)
			.map_err(|error| {
				format!(
					"Failed to read the public progress projection cache for issue `{}`: {error}",
					self.issue.identifier
				)
			})?;

		Ok(records.iter().any(|record| record.idempotency_key == public_projection.idempotency_key))
	}
}
