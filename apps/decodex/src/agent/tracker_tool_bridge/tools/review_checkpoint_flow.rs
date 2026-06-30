use super::{
	DynamicToolCallResponse, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, LocalRepoDetails,
	NormalizedReviewCheckpointPayload, REVIEW_POLICY_CONVERGENCE_BUDGET, ReviewCheckpointArgs,
	ReviewFindingPolicyUpdate, ReviewHandoffContext, ReviewPolicyPhase, ReviewPolicyStatus,
	TrackerToolBridge, Value, current_review_blocker_findings, normalize_review_checkpoint_payload,
	review_finding_policy_from_previous_state, review_finding_policy_update, serde_json,
	tracker_tool_bridge, validate_review_cost_control_policy_state,
};

struct ReviewCheckpointPayloadCounts {
	evidence: usize,
	accepted_findings: usize,
	rejected_findings: usize,
	finding_routes: usize,
	current_blockers: usize,
}

struct PreparedReviewCheckpoint {
	review_policy_phase: ReviewPolicyPhase,
	review_policy_status: ReviewPolicyStatus,
	head_sha: String,
	checkpoint_payload: NormalizedReviewCheckpointPayload,
	nonclean_rounds: i64,
}

impl<'a> TrackerToolBridge<'a> {
	pub(super) fn handle_review_checkpoint(&self, arguments: Value) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<ReviewCheckpointArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.review_checkpoint` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let Some(review_context) = self.review_context.as_ref() else {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_checkpoint` is unavailable for this run.",
			));
		};

		if !review_context.decodex_review_checkpoint_enabled() {
			return DynamicToolCallResponse::failure(format!(
				"`issue_review_checkpoint` is disabled because `[codex].review = \"{}\"` for this run.",
				review_context.review_level.as_str()
			));
		}

		let prepared = match self.prepare_review_checkpoint(parsed, review_context) {
			Ok(prepared) => prepared,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};
		let details_json = match self.review_checkpoint_details_json(&prepared.checkpoint_payload) {
			Ok(details_json) => details_json,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Err(error) = self.persist_review_policy_state(
			review_context,
			prepared.review_policy_phase,
			prepared.review_policy_status,
			&prepared.head_sha,
			prepared.nonclean_rounds,
			&details_json,
		) {
			return DynamicToolCallResponse::failure(error);
		}
		if let Err(error) = self.append_private_review_checkpoint(
			review_context,
			prepared.review_policy_phase,
			prepared.review_policy_status,
			&prepared.head_sha,
			prepared.nonclean_rounds,
			&prepared.checkpoint_payload,
		) {
			return DynamicToolCallResponse::failure(error);
		}

		let message = self.review_checkpoint_success_message(
			prepared.review_policy_phase,
			prepared.review_policy_status,
			&prepared.head_sha,
			prepared.nonclean_rounds,
			ReviewCheckpointPayloadCounts {
				evidence: prepared.checkpoint_payload.evidence.len(),
				accepted_findings: prepared.checkpoint_payload.accepted_findings.len(),
				rejected_findings: prepared.checkpoint_payload.rejected_findings.len(),
				finding_routes: prepared.checkpoint_payload.finding_routes.len(),
				current_blockers: current_review_blocker_findings(&prepared.checkpoint_payload)
					.count(),
			},
		);

		if let Some(response) = self.review_checkpoint_churn_stop_response(
			prepared.review_policy_status,
			prepared.nonclean_rounds,
			&prepared.checkpoint_payload,
			&message,
		) {
			return response;
		}

		DynamicToolCallResponse::success(message)
	}

	fn prepare_review_checkpoint(
		&self,
		parsed: ReviewCheckpointArgs,
		review_context: &ReviewHandoffContext,
	) -> Result<PreparedReviewCheckpoint, String> {
		let Some(review_policy_phase) = ReviewPolicyPhase::for_mode(review_context.mode) else {
			return Err(String::from(
				"`issue_review_checkpoint` is unavailable for retained closeout runs.",
			));
		};
		let review_policy_status = ReviewPolicyStatus::parse(&parsed.status)?;
		let local_repo = self.current_local_repo_details(review_context)?;
		let head_sha = self.canonicalize_current_lane_head_sha(
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
			parsed.head_sha.as_str(),
			&local_repo.head_oid,
		)?;

		self.ensure_review_checkpoint_committed_head(&local_repo)?;

		let mut checkpoint_payload = normalize_review_checkpoint_payload(
			parsed,
			review_policy_phase,
			review_policy_status,
			&head_sha,
			&local_repo,
		)?;
		let policy_update = self.review_checkpoint_finding_policy_update(
			review_context,
			review_policy_phase,
			review_policy_status,
			&head_sha,
			&checkpoint_payload,
		)?;

		validate_review_cost_control_policy_state(
			&checkpoint_payload.review_cost_control,
			&policy_update,
		)?;

		checkpoint_payload.finding_policy = policy_update.finding_policy;

		Ok(PreparedReviewCheckpoint {
			review_policy_phase,
			review_policy_status,
			head_sha,
			checkpoint_payload,
			nonclean_rounds: policy_update.nonclean_rounds,
		})
	}

	fn review_checkpoint_details_json(
		&self,
		checkpoint_payload: &NormalizedReviewCheckpointPayload,
	) -> Result<String, String> {
		serde_json::to_string(checkpoint_payload).map_err(|error| {
			format!(
				"Failed to serialize the structured review checkpoint for issue `{}`: {error}",
				self.issue.identifier
			)
		})
	}

	fn review_checkpoint_churn_stop_response(
		&self,
		review_policy_status: ReviewPolicyStatus,
		nonclean_rounds: i64,
		checkpoint_payload: &NormalizedReviewCheckpointPayload,
		message: &str,
	) -> Option<DynamicToolCallResponse> {
		if review_policy_status != ReviewPolicyStatus::Findings
			|| nonclean_rounds < REVIEW_POLICY_CONVERGENCE_BUDGET
		{
			return None;
		}

		let fingerprint = checkpoint_payload
			.finding_policy
			.stop_fingerprint
			.as_ref()
			.map_or_else(String::new, |fingerprint| {
				format!(" Finding fingerprint `{fingerprint}` caused the stop.")
			});

		Some(DynamicToolCallResponse::failure(format!(
			"{message} Review churn threshold exceeded.{fingerprint} Stop the current repair strategy now and route through architecture recovery or human attention before making further repair mutations."
		)))
	}

	fn review_checkpoint_finding_policy_update(
		&self,
		review_context: &ReviewHandoffContext,
		review_policy_phase: ReviewPolicyPhase,
		review_policy_status: ReviewPolicyStatus,
		head_sha: &str,
		checkpoint_payload: &NormalizedReviewCheckpointPayload,
	) -> Result<ReviewFindingPolicyUpdate, String> {
		let previous_state = self
			.review_policy_artifact_for_head(review_context, review_policy_phase, head_sha)
			.map_err(|error| error.to_string())?;
		let previous_finding_policy = previous_state
			.as_ref()
			.and_then(|previous_state| {
				review_finding_policy_from_previous_state(previous_state, review_policy_phase)
			})
			.unwrap_or_default();
		let previous_nonclean_rounds = previous_state
			.as_ref()
			.filter(|previous_state| previous_state.phase == review_policy_phase)
			.map_or(0, |previous_state| previous_state.nonclean_rounds);
		let prior_nonclean_rounds_present = self
			.state_store
			.map(|state_store| {
				state_store.has_nonclean_review_checkpoint_artifact(
					&review_context.service_id,
					&self.issue.id,
					review_policy_phase.as_str(),
				)
			})
			.transpose()
			.map_err(|error| error.to_string())?
			.unwrap_or(false);
		let previous_nonclean_rounds = if prior_nonclean_rounds_present {
			previous_nonclean_rounds.max(1)
		} else {
			previous_nonclean_rounds
		};
		let previous_threshold_exceeded = previous_state.as_ref().is_some_and(|previous_state| {
			previous_state.phase == review_policy_phase
				&& previous_state.status == ReviewPolicyStatus::Findings
				&& previous_state.nonclean_rounds >= REVIEW_POLICY_CONVERGENCE_BUDGET
		});

		if review_policy_status == ReviewPolicyStatus::Findings
			&& (previous_finding_policy.stop_fingerprint.is_some() || previous_threshold_exceeded)
		{
			return Err(format!(
				"Review churn threshold already exceeded for issue `{}`; do not record another findings checkpoint. Route through architecture recovery or human attention before making further repair mutations.",
				self.issue.identifier
			));
		}

		Ok(review_finding_policy_update(
			previous_finding_policy,
			previous_nonclean_rounds,
			review_policy_phase,
			review_policy_status,
			head_sha,
			checkpoint_payload,
		))
	}

	fn review_checkpoint_success_message(
		&self,
		review_policy_phase: ReviewPolicyPhase,
		review_policy_status: ReviewPolicyStatus,
		head_sha: &str,
		nonclean_rounds: i64,
		counts: ReviewCheckpointPayloadCounts,
	) -> String {
		let evidence_suffix = format!(
			"{} evidence item(s), {} accepted finding(s), {} rejected finding(s), {} route(s), and {} current blocker(s) recorded",
			counts.evidence,
			counts.accepted_findings,
			counts.rejected_findings,
			counts.finding_routes,
			counts.current_blockers,
		);

		match review_policy_status {
			ReviewPolicyStatus::Clean => format!(
				"Recorded a clean `{}` review checkpoint for issue `{}` at HEAD `{head_sha}`; {evidence_suffix}.",
				review_policy_phase.as_str(),
				self.issue.identifier,
			),
			ReviewPolicyStatus::Findings => format!(
				"Recorded `{}` review findings for issue `{}` at HEAD `{head_sha}`; max unresolved finding repeat count now `{nonclean_rounds}`; {evidence_suffix}.",
				review_policy_phase.as_str(),
				self.issue.identifier,
			),
			ReviewPolicyStatus::NeedsArchitectureReview => format!(
				"Recorded `needs_architecture_review` for issue `{}` at HEAD `{head_sha}`; Decodex will require human architecture review if the turn ends on this checkpoint.",
				self.issue.identifier,
			),
			ReviewPolicyStatus::Blocked => format!(
				"Recorded `blocked` for issue `{}` at HEAD `{head_sha}`; Decodex will require human intervention if the turn ends on this checkpoint.",
				self.issue.identifier,
			),
		}
	}

	fn append_private_review_checkpoint(
		&self,
		review_context: &ReviewHandoffContext,
		review_policy_phase: ReviewPolicyPhase,
		review_policy_status: ReviewPolicyStatus,
		head_sha: &str,
		nonclean_rounds: i64,
		checkpoint_payload: &NormalizedReviewCheckpointPayload,
	) -> Result<(), String> {
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires the Decodex runtime state store for issue `{}`.",
				self.issue.identifier
			)
		})?;
		let private_payload = serde_json::json!({
			"phase": review_policy_phase.as_str(),
			"status": review_policy_status.as_str(),
			"head_sha": head_sha,
			"nonclean_rounds": nonclean_rounds,
			"active_fingerprints": &checkpoint_payload.finding_policy.active_fingerprints,
			"stop_fingerprint": &checkpoint_payload.finding_policy.stop_fingerprint,
			"route_counts": &checkpoint_payload.finding_route_summary.route_counts,
			"route_next_action": &checkpoint_payload.finding_route_summary.next_action,
			"review_class": &checkpoint_payload.review_cost_control.review_class,
			"risk_class": &checkpoint_payload.review_cost_control.risk_class,
			"compact_eligible": checkpoint_payload.review_cost_control.compact_eligible,
			"review_fallback_reason": &checkpoint_payload.review_cost_control.fallback_reason,
			"review": checkpoint_payload,
		});

		state_store
			.append_private_execution_event(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
				"review_checkpoint",
				private_payload,
			)
			.map(|_| ())
			.map_err(|error| {
				format!(
					"Failed to persist the private review checkpoint for issue `{}`: {error}",
					self.issue.identifier
				)
			})
	}

	fn ensure_review_checkpoint_committed_head(
		&self,
		local_repo: &LocalRepoDetails,
	) -> Result<(), String> {
		if local_repo.review_worktree_clean() {
			return Ok(());
		}

		Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires a clean committed lane HEAD before recording formal Decodex Review evidence. Commit or revert review-blocking local changes, rerun required validation, then request review for the committed HEAD. Review-blocking local changes: {}",
			tracker_tool_bridge::summarize_review_blocking_changes(
				&local_repo.review_blocking_changes
			)
		))
	}
}
