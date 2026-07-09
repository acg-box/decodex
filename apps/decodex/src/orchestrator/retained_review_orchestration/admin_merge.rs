use std::time::Duration;

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	commit_message,
	orchestrator::{
		self, EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS, PostReviewLifecycleFactsInput,
		build_post_review_lifecycle_facts,
		kernel::lifecycle::{
			LifecycleDecisionInput, LifecycleEvidenceKind, LifecycleOutcome,
			PreviousLifecycleAuthority, decide_lifecycle_transition,
		},
		retained_review_orchestration,
		retained_review_orchestration::{
			Command, CommandIntentKind, IssueTracker, Path, PostReviewRuntimeState, Result,
			RetainedAdminMergeReasons, RetainedReviewLane, RetainedReviewRuntime, ServiceConfig,
			attention, eyre, github,
		},
		runtime_review_checkpoint_status_for_head,
	},
};

#[allow(clippy::too_many_lines)]
pub(super) fn start_retained_admin_merge<T>(
	runtime: &mut RetainedReviewRuntime<'_, T>,
	lane: &RetainedReviewLane,
	reasons: RetainedAdminMergeReasons,
) -> Result<()>
where
	T: IssueTracker,
{
	retained_review_orchestration::retained_review_command_adapter(
		retained_review_orchestration::retained_review_command_intent(
			lane,
			CommandIntentKind::StartRetainedLanding,
			reasons.start_landing,
		),
		CommandIntentKind::StartRetainedLanding,
	)?;

	if let Some(reason) = orchestrator::authority_boundary_landing_requirement(
		&lane.snapshot,
		Some(PostReviewRuntimeState {
			state_store: runtime.state_store,
			project_id: runtime.project.service_id(),
			review_level: runtime.project.codex().review_level(),
		}),
	)? {
		tracing::info!(
			project_id = runtime.project.service_id(),
			issue_id = lane.snapshot.issue.id,
			issue = lane.snapshot.issue.identifier,
			reason,
			"Retained admin merge is waiting for authority-boundary landing clearance."
		);

		return Ok(());
	}

	if !lane.review_state.merge_commit_allowed {
		record_retained_landing_decision(
			runtime,
			lane,
			LifecycleEvidenceKind::LandingReadback,
			LifecycleOutcome::NeedsManualAttention,
			None,
			"manual_attention_required",
			"not_started",
			reasons.admin_merge_unavailable,
		)?;

		return retained_review_orchestration::apply_passive_retained_manual_attention(
			attention::passive_attention_runtime(runtime),
			&lane.snapshot.issue,
			&lane.snapshot.worktree,
			lane.lifecycle_record(),
			reasons.admin_merge_unavailable,
		);
	}

	let merge_subject = match retained_review_merge_subject(lane) {
		Ok(subject) => subject,
		Err(error) => {
			tracing::warn!(
				issue_id = lane.snapshot.issue.id,
				issue = lane.snapshot.issue.identifier,
				branch = lane.snapshot.worktree.branch_name(),
				?error,
				"Retained admin merge could not derive a compliant landed change record."
			);

			record_retained_landing_decision(
				runtime,
				lane,
				LifecycleEvidenceKind::LandingReadback,
				LifecycleOutcome::NeedsManualAttention,
				None,
				"manual_attention_required",
				"not_started",
				"retained_admin_merge_subject_unavailable",
			)?;

			return retained_review_orchestration::apply_passive_retained_manual_attention(
				attention::passive_attention_runtime(runtime),
				&lane.snapshot.issue,
				&lane.snapshot.worktree,
				lane.lifecycle_record(),
				"retained_admin_merge_subject_unavailable",
			);
		},
	};
	record_retained_landing_decision(
		runtime,
		lane,
		LifecycleEvidenceKind::LandingIntent,
		LifecycleOutcome::Intent,
		None,
		"intent",
		"not_started",
		reasons.start_landing,
	)?;
	let github_token = retained_review_github_token(runtime.project, &mut *runtime.github_token)?;
	let merge_succeeded = match github::admin_merge_pull_request(
		lane.snapshot.worktree.worktree_path(),
		lane.review_state.url.as_str(),
		lane.lifecycle_record().head_sha(),
		Some(merge_subject.as_str()),
		github_token,
		runtime.project.github().command_path(),
	) {
		Ok(()) => true,
		Err(_error) => matches!(
			github::pull_request_is_merged_at_head(
				lane.snapshot.worktree.worktree_path(),
				lane.review_state.url.as_str(),
				lane.lifecycle_record().head_sha(),
				github_token,
				runtime.project.github().command_path(),
			),
			Ok(true)
		),
	};

	if merge_succeeded {
		let merge_commit = match github::wait_for_pull_request_merge_commit(
			lane.snapshot.worktree.worktree_path(),
			lane.review_state.url.as_str(),
			github_token,
			Duration::from_secs(EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS as u64),
			runtime.project.github().command_path(),
		) {
			Ok(merge_commit) => merge_commit,
			Err(error) => {
				record_retained_landing_decision(
					runtime,
					lane,
					LifecycleEvidenceKind::LandingReadback,
					LifecycleOutcome::NeedsManualAttention,
					None,
					"readback_unavailable",
					"not_started",
					"retained_admin_merge_readback_unavailable",
				)?;
				tracing::warn!(
					issue_id = lane.snapshot.issue.id,
					issue = lane.snapshot.issue.identifier,
					pr_url = lane.review_state.url,
					?error,
					"Retained admin merge succeeded, but merge commit readback is unavailable."
				);

				return retained_review_orchestration::apply_passive_retained_manual_attention(
					attention::passive_attention_runtime(runtime),
					&lane.snapshot.issue,
					&lane.snapshot.worktree,
					lane.lifecycle_record(),
					"retained_admin_merge_readback_unavailable",
				);
			},
		};
		record_retained_landing_decision(
			runtime,
			lane,
			LifecycleEvidenceKind::LandingReadback,
			LifecycleOutcome::Succeeded,
			Some(merge_commit.as_str()),
			"landed",
			"not_started",
			"retained_admin_merge_readback",
		)?;

		return Ok(());
	}

	record_retained_landing_decision(
		runtime,
		lane,
		LifecycleEvidenceKind::LandingReadback,
		LifecycleOutcome::Failed,
		None,
		"failed",
		"not_started",
		reasons.admin_merge_failed,
	)?;

	retained_review_orchestration::apply_passive_retained_manual_attention(
		attention::passive_attention_runtime(runtime),
		&lane.snapshot.issue,
		&lane.snapshot.worktree,
		lane.lifecycle_record(),
		reasons.admin_merge_failed,
	)
}

#[allow(clippy::too_many_arguments)]
fn record_retained_landing_decision<T>(
	runtime: &RetainedReviewRuntime<'_, T>,
	lane: &RetainedReviewLane,
	evidence_kind: LifecycleEvidenceKind,
	outcome: LifecycleOutcome,
	merge_commit: Option<&str>,
	landing_state: &str,
	cleanup_state: &str,
	causation_id: &str,
) -> Result<()>
where
	T: IssueTracker,
{
	let review_checkpoint = runtime_review_checkpoint_status_for_head(
		runtime.state_store,
		runtime.project.service_id(),
		&lane.snapshot.issue.id,
		runtime.project.codex().review_level(),
		lane.lifecycle_record().head_sha(),
	)?;
	let facts = build_post_review_lifecycle_facts(PostReviewLifecycleFactsInput {
		project_id: runtime.project.service_id(),
		issue_id: &lane.snapshot.issue.id,
		review_lifecycle: Some(lane.lifecycle_record()),
		review_state: &lane.review_state,
		worktree_path: lane.snapshot.worktree.worktree_path(),
		review_level: runtime.project.codex().review_level(),
		phase: lane.lifecycle_record().phase(),
		landing_state: Some(landing_state),
		closeout_state: None,
		validated_head_sha: Some(lane.lifecycle_record().head_sha()),
		review_checkpoint_phase: review_checkpoint.as_ref().map(|checkpoint| checkpoint.phase),
		review_checkpoint_status: review_checkpoint
			.as_ref()
			.map(|checkpoint| checkpoint.status.as_str()),
	});
	let previous_record = runtime.state_store.review_lifecycle_record(
		runtime.project.service_id(),
		&lane.snapshot.issue.id,
		lane.snapshot.worktree.branch_name(),
	)?;
	let previous = previous_record.as_ref().map(|record| PreviousLifecycleAuthority {
		sequence: record.sequence(),
		next_state: record.next_state(),
	});
	let idempotency_key = format!(
		"{}:{}:{}:{}:{}",
		runtime.project.service_id(),
		lane.snapshot.issue.id,
		lane.lifecycle_record().head_sha(),
		evidence_kind.as_str(),
		causation_id
	);
	let decision = decide_lifecycle_transition(LifecycleDecisionInput {
		facts: &facts,
		previous,
		evidence_kind,
		outcome,
		merge_commit,
		cleanup_state: Some(cleanup_state),
		authority: "issue_authority",
		actor: "runtime",
		idempotency_key: &idempotency_key,
		correlation_id: lane.lifecycle_record().run_id(),
		causation_id: Some(causation_id),
		decided_at: &current_timestamp(),
	});

	runtime.state_store.record_lifecycle_decision(
		lane.lifecycle_record().run_id(),
		lane.lifecycle_record().attempt_number(),
		&decision,
	)?;

	Ok(())
}

fn current_timestamp() -> String {
	OffsetDateTime::now_utc().format(&Rfc3339).expect("timestamp formatting should succeed")
}

pub(super) fn retained_review_github_token<'a>(
	project: &ServiceConfig,
	github_token: &'a mut Option<String>,
) -> Result<&'a str> {
	if github_token.is_none() {
		*github_token = Some(orchestrator::resolve_configured_env_var(
			"github.token_env_var",
			Some(project.github().token_env_var()),
		)?);
	}

	github_token.as_deref().ok_or_else(|| {
		eyre::eyre!("Retained review orchestration requires a configured GitHub token.")
	})
}

fn retained_review_merge_subject(lane: &RetainedReviewLane) -> Result<String> {
	if lane.lifecycle_record().pr_head_oid() != lane.lifecycle_record().head_sha() {
		eyre::bail!(
			"Retained admin merge for `{}` requires lifecycle PR head `{}` to match lifecycle validated head `{}`.",
			lane.snapshot.issue.identifier,
			lane.lifecycle_record().pr_head_oid(),
			lane.lifecycle_record().head_sha(),
		);
	}

	let head_subject = retained_review_head_commit_subject(
		lane.snapshot.worktree.worktree_path(),
		lane.lifecycle_record().head_sha(),
	)?;

	commit_message::build_landed_merge_commit_message(
		&head_subject,
		&lane.snapshot.issue.identifier,
	)
}

fn retained_review_head_commit_subject(worktree_path: &Path, head_sha: &str) -> Result<String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["log", "-1", "--format=%s"])
		.arg(head_sha)
		.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to read retained review head commit subject `{}` in `{}`: {}",
			head_sha,
			worktree_path.display(),
			stderr.trim()
		);
	}

	String::from_utf8(output.stdout)
		.map(|stdout| stdout.trim_end_matches(['\n', '\r']).to_owned())
		.map_err(Into::into)
}
