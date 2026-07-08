use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::orchestrator::{
	self, PostReviewLifecycleFacts, PostReviewLifecycleFactsInput, RuntimeReviewGateState,
	build_post_review_lifecycle_facts,
	kernel::lifecycle::{
		LifecycleDecisionInput, LifecycleEvidenceKind, LifecycleOutcome,
		PreviousLifecycleAuthority, decide_lifecycle_transition,
	},
	latest_runtime_review_checkpoint_status,
	retained_review_orchestration::{
		CommandIntentKind, IssueTracker, PassiveRetainedAttentionRuntime, Result,
		RetainedAdminMergeReasons, RetainedReviewLane, RetainedReviewLifecycleAuthorityFields,
		RetainedReviewRuntime, ServiceConfig, StateStore, WorkflowDocument, admin_merge,
		lifecycle_authority,
		phases::{merge, result},
	},
	runtime_review_checkpoint_status_for_head_phase,
	runtime_standard_review::{RuntimeStandardReviewRunner, runtime_review_execution_mode},
	worktree_has_review_blocking_changes,
};

const RUNTIME_STANDARD_REVIEW_PRODUCER_FAILURE_BUDGET: i64 = 3;

pub(in crate::orchestrator::retained_review_orchestration::phases) fn handle_non_github_review_lane<
	T,
>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	github_token: &mut Option<String>,
	now_unix_epoch: i64,
	runtime_review_runner: &impl RuntimeStandardReviewRunner,
) -> Result<()>
where
	T: IssueTracker,
{
	let action =
		super::RetainedReviewLifecycleAction::parse(lane.lifecycle_record().next_action())?;

	if matches!(
		action,
		super::RetainedReviewLifecycleAction::PollLandingReadback
			| super::RetainedReviewLifecycleAction::RunCloseoutAdapter
	) {
		return merge::handle_waiting_for_merge_phase(
			tracker,
			project,
			workflow,
			state_store,
			lane,
			now_unix_epoch,
			"non_github_review_merge_visibility_timeout",
		);
	}
	if result::external_review_requires_repair(&lane.review_state, lane.lifecycle_record())
		|| orchestrator::failed_checks_require_repair(
			lane.review_state.status_check_rollup_state.as_deref(),
			&lane.review_state.merge_state_status,
		) || orchestrator::merge_state_requires_review_repair(
		&lane.review_state.mergeable,
		&lane.review_state.merge_state_status,
	)
	.is_some()
	{
		return lifecycle_authority::write_retained_review_lifecycle_authority_for_command(
			state_store,
			lane,
			CommandIntentKind::StartReviewRepair,
			"non_github_review_repair_required",
			"repair_required",
			RetainedReviewLifecycleAuthorityFields::from_lifecycle_record(lane.lifecycle_record()),
		);
	}
	if orchestrator::review_state_landing_requires_agent_fallback(&lane.review_state) {
		return lifecycle_authority::write_retained_review_lifecycle_authority_for_command(
			state_store,
			lane,
			CommandIntentKind::StartReviewRepair,
			"retained_landing_agent_fallback_required",
			"repair_required",
			RetainedReviewLifecycleAuthorityFields::from_lifecycle_record(lane.lifecycle_record()),
		);
	}
	if !orchestrator::review_state_landing_gates_satisfied(&lane.review_state) {
		return Ok(());
	}
	if runtime_standard_review_gate_requires_wait_or_repair(
		tracker,
		project,
		workflow,
		state_store,
		lane,
		runtime_review_runner,
	)? {
		return Ok(());
	}

	let mut runtime =
		RetainedReviewRuntime { tracker, project, workflow, state_store, github_token };

	admin_merge::start_retained_admin_merge(
		&mut runtime,
		lane,
		RetainedAdminMergeReasons {
			start_landing: "non_github_review_ready_to_land",
			admin_merge_unavailable: "non_github_review_admin_merge_unavailable",
			admin_merge_failed: "non_github_review_admin_merge_failed",
		},
	)
}

pub(super) fn runtime_standard_review_gate_requires_wait_or_repair(
	tracker: &impl IssueTracker,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	runtime_review_runner: &impl RuntimeStandardReviewRunner,
) -> Result<bool> {
	if !project.codex().review_level().requires_review_checkpoint() {
		return Ok(false);
	}

	let local_head_oid = lane.snapshot.local_head_oid.as_deref();
	let checkpoint = if let Some(local_head_oid) = local_head_oid {
		runtime_standard_review_checkpoint_for_gate(project, state_store, lane, local_head_oid)?
	} else {
		None
	};
	let facts = build_post_review_lifecycle_facts(PostReviewLifecycleFactsInput {
		project_id: project.service_id(),
		issue_id: &lane.snapshot.issue.id,
		review_lifecycle: Some(lane.lifecycle_record()),
		review_state: &lane.review_state,
		worktree_path: lane.snapshot.worktree.worktree_path(),
		review_level: project.codex().review_level(),
		phase: lane.lifecycle_record().phase(),
		landing_state: None,
		closeout_state: None,
		validated_head_sha: local_head_oid,
		review_checkpoint_phase: checkpoint.as_ref().map(|checkpoint| checkpoint.phase),
		review_checkpoint_status: checkpoint.as_ref().map(|checkpoint| checkpoint.status.as_str()),
	});

	match facts.review_gate_state {
		RuntimeReviewGateState::NotRequired => Ok(false),
		RuntimeReviewGateState::Clean =>
			Ok(worktree_has_review_blocking_changes(lane.snapshot.worktree.worktree_path())?),
		RuntimeReviewGateState::Findings => {
			lifecycle_authority::write_retained_review_lifecycle_authority_for_command(
				state_store,
				lane,
				CommandIntentKind::StartReviewRepair,
				"runtime_standard_review_repair_required",
				"repair_required",
				RetainedReviewLifecycleAuthorityFields::from_lifecycle_record(
					lane.lifecycle_record(),
				),
			)?;
			Ok(true)
		},
		RuntimeReviewGateState::WorktreeHeadMissing => Ok(true),
		RuntimeReviewGateState::NeedsArchitectureReview => {
			apply_runtime_standard_review_manual_attention(
				tracker,
				project,
				workflow,
				state_store,
				lane,
				&facts,
				"runtime_standard_review_needs_architecture_review",
			)?;
			Ok(true)
		},
		RuntimeReviewGateState::Blocked => {
			apply_runtime_standard_review_manual_attention(
				tracker,
				project,
				workflow,
				state_store,
				lane,
				&facts,
				"runtime_standard_review_blocked",
			)?;
			Ok(true)
		},
		RuntimeReviewGateState::Unknown(_) => {
			apply_runtime_standard_review_manual_attention(
				tracker,
				project,
				workflow,
				state_store,
				lane,
				&facts,
				"runtime_standard_review_unknown_checkpoint_status",
			)?;
			Ok(true)
		},
		RuntimeReviewGateState::Pending => {
			match orchestrator::runtime_standard_review::ensure_runtime_standard_review_checkpoint_with_runner(
				tracker,
				project,
				workflow,
				state_store,
				lane,
				runtime_review_runner,
			) {
				Ok(()) => {},
				Err(error) => {
					let retry_count = lane.lifecycle_record().request_retry_count() + 1;
					tracing::warn!(
						?error,
						project_id = project.service_id(),
						issue_id = lane.snapshot.issue.id.as_str(),
						retry_count,
						"Runtime-owned Decodex Review checkpoint producer failed."
					);
					write_runtime_standard_review_producer_retry_count(
						state_store,
						lane,
						retry_count,
					)?;
					if retry_count >= RUNTIME_STANDARD_REVIEW_PRODUCER_FAILURE_BUDGET {
						apply_runtime_standard_review_manual_attention(
							tracker,
							project,
							workflow,
							state_store,
							lane,
							&facts,
							"runtime_standard_review_checkpoint_producer_failed",
						)?;
					}
				},
			}

			Ok(true)
		},
	}
}

fn runtime_standard_review_checkpoint_for_gate(
	project: &ServiceConfig,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	local_head_oid: &str,
) -> Result<Option<crate::orchestrator::RuntimeReviewCheckpointStatus>> {
	let expected_phase = runtime_review_execution_mode(project, state_store, lane)?.as_str();
	let handoff = runtime_review_checkpoint_status_for_head_phase(
		state_store,
		project.service_id(),
		&lane.snapshot.issue.id,
		project.codex().review_level(),
		local_head_oid,
		"handoff",
	)?;
	let repair = runtime_review_checkpoint_status_for_head_phase(
		state_store,
		project.service_id(),
		&lane.snapshot.issue.id,
		project.codex().review_level(),
		local_head_oid,
		"repair",
	)?;

	let latest = latest_runtime_review_checkpoint_status(vec![handoff, repair])?;

	Ok(match latest {
		Some(checkpoint) => Some(checkpoint),
		None => runtime_review_checkpoint_status_for_head_phase(
			state_store,
			project.service_id(),
			&lane.snapshot.issue.id,
			project.codex().review_level(),
			local_head_oid,
			expected_phase,
		)?,
	})
}

fn write_runtime_standard_review_producer_retry_count(
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	retry_count: i64,
) -> Result<()> {
	lifecycle_authority::write_retained_review_lifecycle_authority_for_current_action(
		state_store,
		lane,
		CommandIntentKind::SyncReviewLifecycleAuthority,
		"runtime_standard_review_checkpoint_producer_retry",
		RetainedReviewLifecycleAuthorityFields {
			request_retry_count: retry_count,
			..RetainedReviewLifecycleAuthorityFields::from_lifecycle_record(lane.lifecycle_record())
		},
	)
}

fn apply_runtime_standard_review_manual_attention(
	tracker: &impl IssueTracker,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	facts: &PostReviewLifecycleFacts,
	reason: &str,
) -> Result<()> {
	record_runtime_standard_review_manual_attention_authority(state_store, lane, facts, reason)?;

	orchestrator::retained_review_orchestration::apply_passive_retained_manual_attention(
		PassiveRetainedAttentionRuntime { tracker, project, workflow, state_store },
		&lane.snapshot.issue,
		&lane.snapshot.worktree,
		lane.lifecycle_record(),
		reason,
	)
}

fn record_runtime_standard_review_manual_attention_authority(
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	facts: &PostReviewLifecycleFacts,
	reason: &str,
) -> Result<()> {
	let previous_record = state_store.review_lifecycle_record(
		facts.project_id.as_str(),
		&lane.snapshot.issue.id,
		lane.snapshot.worktree.branch_name(),
	)?;
	let previous = previous_record.as_ref().map(|record| PreviousLifecycleAuthority {
		sequence: record.sequence(),
		next_state: record.next_state(),
	});
	let idempotency_key = format!(
		"{}:{}:{}:{}:{}",
		facts.project_id,
		lane.snapshot.issue.id,
		facts.validated_head_sha,
		LifecycleEvidenceKind::LandingReadback.as_str(),
		reason
	);
	let decision = decide_lifecycle_transition(LifecycleDecisionInput {
		facts,
		previous,
		evidence_kind: LifecycleEvidenceKind::LandingReadback,
		outcome: LifecycleOutcome::NeedsManualAttention,
		merge_commit: None,
		cleanup_state: Some("not_started"),
		authority: "issue_authority",
		actor: "runtime_standard_review_gate",
		idempotency_key: &idempotency_key,
		correlation_id: lane.lifecycle_record().run_id(),
		causation_id: Some(reason),
		decided_at: &current_timestamp(),
	});

	state_store.record_lifecycle_decision(
		lane.lifecycle_record().run_id(),
		lane.lifecycle_record().attempt_number(),
		&decision,
	)?;

	Ok(())
}

fn current_timestamp() -> String {
	OffsetDateTime::now_utc().format(&Rfc3339).expect("timestamp formatting should succeed")
}
