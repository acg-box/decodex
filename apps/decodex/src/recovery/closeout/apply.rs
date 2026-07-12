use std::path::Path;

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	orchestrator::{
		self, PostReviewLifecycleFactsInput, PullRequestReviewState,
		kernel::{
			lifecycle,
			lifecycle::{
				LifecycleDecisionInput, LifecycleEvidenceKind, LifecycleOutcome,
				PreviousLifecycleAuthority,
			},
		},
	},
	prelude::Result,
	recovery::{
		closeout::{
			LegacyCloseoutValidation, MergedCloseoutValidation,
			apply::lifecycle::decide_lifecycle_transition, events,
		},
		context::RecoveryContext,
		pull_request_inspection,
	},
	tracker::{
		self,
		privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
		records::{self, LinearExecutionEventRecord},
	},
};

pub(super) fn write_legacy_closeout_audit(
	context: &RecoveryContext,
	validation: &LegacyCloseoutValidation,
	event: &LinearExecutionEventRecord,
) -> Result<bool> {
	let audit_body = format!(
		"Decodex legacy manual closeout audit: verified merged PR `{}` for `{}`. Runtime provenance was `{}`, so this records the manual fallback before local cleanup.",
		pull_request_inspection::landing_url(&validation.landing_state),
		validation.issue.identifier,
		validation.worktree.provenance().source()
	);
	let retry_budget_attempt_count = context
		.state_store
		.retry_budget_attempt_count_for_lane(context.config.service_id(), &validation.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body = format!(
		"{audit_body}\n\n{}",
		records::render_linear_execution_event_comment_body(event, retry_budget_attempt_count)
	);
	let privacy_classifier = ConfiguredPublicProjectionPrivacyClassifier::from_config(
		context.config.privacy_classifier(),
	)?;
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, event, &privacy_classifier)?;
	let recorded = context.state_store.record_linear_execution_event(&projection.record)?;

	if !recorded {
		return Ok(false);
	}

	if let Err(error) = tracker::create_linear_execution_event_comment_direct(
		&context.tracker,
		&validation.issue.id,
		&projection,
	) {
		context.state_store.forget_linear_execution_event(&projection.record.idempotency_key)?;

		return Err(error);
	}

	Ok(true)
}

pub(super) fn apply_merged_closeout_recovery(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
) -> Result<(bool, bool)> {
	let closeout_event = events::merged_closeout_event(context, validation);
	let cleanup_event = events::merged_closeout_cleanup_event(context, validation);
	let closeout_recorded = write_merged_closeout_event(
		context,
		validation,
		&closeout_event,
		"Decodex merged closeout recovery: verified the PR was merged into the current default branch and reconciled the stale retained attention closeout ledger.",
	)?;
	let cleanup_recorded = match write_merged_closeout_event(
		context,
		validation,
		&cleanup_event,
		"Decodex merged closeout recovery: verified retained lane cleanup is already complete and recorded cleanup_complete.",
	) {
		Ok(cleanup_recorded) => cleanup_recorded,
		Err(error) => {
			if closeout_recorded {
				context
					.state_store
					.forget_linear_execution_event(&closeout_event.idempotency_key)?;
			}

			return Err(error);
		},
	};

	if validation.worktree_mapping.is_some() {
		context.state_store.clear_worktree(&validation.issue.id)?;

		if validation.issue.identifier != validation.issue.id {
			context.state_store.clear_worktree(&validation.issue.identifier)?;
		}
	}

	record_merged_closeout_lifecycle_authority(context, validation)?;

	context.state_store.update_run_status(&validation.run_id, "succeeded")?;

	Ok((closeout_recorded, cleanup_recorded))
}

fn write_merged_closeout_event(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
	event: &LinearExecutionEventRecord,
	body: &str,
) -> Result<bool> {
	let privacy_classifier = ConfiguredPublicProjectionPrivacyClassifier::from_config(
		context.config.privacy_classifier(),
	)?;
	let retry_budget_attempt_count = context
		.state_store
		.retry_budget_attempt_count_for_lane(context.config.service_id(), &validation.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body = format!(
		"{body}\n\n{}",
		records::render_linear_execution_event_comment_body(event, retry_budget_attempt_count)
	);
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, event, &privacy_classifier)?;
	let recorded = context.state_store.record_linear_execution_event(&projection.record)?;

	if !recorded {
		return Ok(false);
	}

	if let Err(error) = tracker::create_linear_execution_event_comment_direct(
		&context.tracker,
		&validation.issue.id,
		&projection,
	) {
		context.state_store.forget_linear_execution_event(&projection.record.idempotency_key)?;

		return Err(error);
	}

	Ok(true)
}

fn record_merged_closeout_lifecycle_authority(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
) -> Result<()> {
	record_merged_closeout_lifecycle_decision(
		context,
		validation,
		LifecycleEvidenceKind::LandingReadback,
		LifecycleOutcome::Succeeded,
		"landed",
		"not_started",
		"not_started",
		"merged_closeout_recovery_landed_readback",
	)?;

	record_merged_closeout_lifecycle_decision(
		context,
		validation,
		LifecycleEvidenceKind::CloseoutCompletion,
		LifecycleOutcome::Succeeded,
		"landed",
		"completed",
		"completed",
		"merged_closeout_recovery_closeout_complete",
	)
}

#[allow(clippy::too_many_arguments)]
fn record_merged_closeout_lifecycle_decision(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
	evidence_kind: LifecycleEvidenceKind,
	outcome: LifecycleOutcome,
	landing_state: &str,
	closeout_state: &str,
	cleanup_state: &str,
	causation_id: &str,
) -> Result<()> {
	let review_level = context.config.codex().review_level();
	let checkpoint = orchestrator::runtime_review_checkpoint_status_for_head(
		&context.state_store,
		context.config.service_id(),
		&validation.issue.id,
		review_level,
		&validation.landing_state.head_ref_oid,
	)?;
	let review_state = merged_closeout_review_state(validation);
	let facts = orchestrator::build_post_review_lifecycle_facts(PostReviewLifecycleFactsInput {
		project_id: context.config.service_id(),
		issue_id: &validation.issue.id,
		review_lifecycle: None,
		review_state: &review_state,
		worktree_path: Path::new(&validation.worktree_path_for_event),
		review_level,
		phase: "merged_closeout_recovery",
		landing_state: Some(landing_state),
		closeout_state: Some(closeout_state),
		validated_head_sha: Some(&validation.landing_state.head_ref_oid),
		review_checkpoint_phase: checkpoint.as_ref().map(|checkpoint| checkpoint.phase),
		review_checkpoint_status: checkpoint.as_ref().map(|checkpoint| checkpoint.status.as_str()),
	});
	let previous_record = context.state_store.review_lifecycle_record(
		context.config.service_id(),
		&validation.issue.id,
		&validation.branch_name,
	)?;
	let previous = previous_record.as_ref().map(|record| PreviousLifecycleAuthority {
		sequence: record.sequence(),
		next_state: record.next_state(),
	});
	let idempotency_key = format!(
		"{}:{}:{}:{}:{}",
		context.config.service_id(),
		validation.issue.id,
		validation.landing_state.head_ref_oid,
		evidence_kind.as_str(),
		causation_id
	);
	let decided_at = current_timestamp();
	let decision = self::decide_lifecycle_transition(LifecycleDecisionInput {
		facts: &facts,
		previous,
		evidence_kind,
		outcome,
		merge_commit: Some(&validation.merge_commit),
		cleanup_state: Some(cleanup_state),
		authority: "issue_authority",
		actor: "merged_closeout_recovery",
		idempotency_key: &idempotency_key,
		correlation_id: &validation.run_id,
		causation_id: Some(causation_id),
		decided_at: &decided_at,
	});

	context.state_store.record_lifecycle_decision(
		&validation.run_id,
		validation.attempt_number,
		&decision,
	)?;

	Ok(())
}

fn merged_closeout_review_state(validation: &MergedCloseoutValidation) -> PullRequestReviewState {
	PullRequestReviewState {
		url: pull_request_inspection::landing_url(&validation.landing_state).to_owned(),
		state: validation.landing_state.state.clone(),
		is_draft: validation.landing_state.is_draft,
		review_decision: validation.landing_state.review_decision.clone(),
		merge_commit_allowed: false,
		pending_review_requests: validation.landing_state.pending_review_requests,
		mergeable: validation.landing_state.mergeable.clone(),
		merge_state_status: validation.landing_state.merge_state_status.clone(),
		base_ref_oid: validation.landing_state.base_ref_oid.clone(),
		head_ref_name: validation.landing_state.head_ref_name.clone(),
		head_ref_oid: validation.landing_state.head_ref_oid.clone(),
		merge_commit_oid: Some(validation.merge_commit.clone()),
		head_repository_name: None,
		head_repository_owner: None,
		status_check_rollup_state: validation.landing_state.status_check_rollup_state.clone(),
		required_status_contexts: validation.landing_state.required_status_contexts.clone(),
		unresolved_review_threads: validation.landing_state.unresolved_review_threads,
		issue_description_external_review_thumbs_up_count: 0,
		issue_comments: Vec::new(),
		reviews: Vec::new(),
	}
}

fn current_timestamp() -> String {
	OffsetDateTime::now_utc().format(&Rfc3339).expect("timestamp formatting should succeed")
}
