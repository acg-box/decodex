use crate::{
	commit_message,
	orchestrator::{
		self, retained_review_orchestration,
		retained_review_orchestration::{
			Command, CommandIntentKind, IssueTracker, Path, PostReviewRuntimeState, Result,
			RetainedAdminMergeReasons, RetainedReviewLane, RetainedReviewOrchestrationMarkerFields,
			RetainedReviewRuntime, ReviewOrchestrationPhase, ServiceConfig, attention, eyre,
			github, markers,
		},
	},
};

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
		return retained_review_orchestration::apply_passive_retained_manual_attention(
			attention::passive_attention_runtime(runtime),
			&lane.snapshot.issue,
			&lane.snapshot.worktree,
			&lane.orchestration_marker,
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

			return retained_review_orchestration::apply_passive_retained_manual_attention(
				attention::passive_attention_runtime(runtime),
				&lane.snapshot.issue,
				&lane.snapshot.worktree,
				&lane.orchestration_marker,
				"retained_admin_merge_subject_unavailable",
			);
		},
	};
	let github_token = retained_review_github_token(runtime.project, &mut *runtime.github_token)?;
	let merge_succeeded = match github::admin_merge_pull_request(
		lane.snapshot.worktree.worktree_path(),
		lane.review_state.url.as_str(),
		lane.orchestration_marker.head_sha(),
		Some(merge_subject.as_str()),
		github_token,
		runtime.project.github().command_path(),
	) {
		Ok(()) => true,
		Err(_error) => matches!(
			github::pull_request_is_merged_at_head(
				lane.snapshot.worktree.worktree_path(),
				lane.review_state.url.as_str(),
				lane.orchestration_marker.head_sha(),
				github_token,
				runtime.project.github().command_path(),
			),
			Ok(true)
		),
	};

	if merge_succeeded {
		return markers::write_retained_review_orchestration_marker_for_command(
			runtime.state_store,
			lane,
			CommandIntentKind::StartRetainedLanding,
			reasons.start_landing,
			ReviewOrchestrationPhase::WaitingForMerge,
			RetainedReviewOrchestrationMarkerFields {
				auto_merge_enabled_at_unix_epoch: Some(runtime.now_unix_epoch),
				..RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker)
			},
		);
	}

	retained_review_orchestration::apply_passive_retained_manual_attention(
		attention::passive_attention_runtime(runtime),
		&lane.snapshot.issue,
		&lane.snapshot.worktree,
		&lane.orchestration_marker,
		reasons.admin_merge_failed,
	)
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
	let review_handoff = lane.snapshot.review_handoff.as_ref().ok_or_else(|| {
		eyre::eyre!(
			"Retained admin merge for `{}` requires a matching runtime review handoff on branch `{}`.",
			lane.snapshot.issue.identifier,
			lane.snapshot.worktree.branch_name(),
		)
	})?;

	if review_handoff.pr_head_oid() != lane.orchestration_marker.head_sha() {
		eyre::bail!(
			"Retained admin merge for `{}` requires review handoff head `{}` to match orchestration head `{}`.",
			lane.snapshot.issue.identifier,
			review_handoff.pr_head_oid(),
			lane.orchestration_marker.head_sha(),
		);
	}

	let head_subject = retained_review_head_commit_subject(
		lane.snapshot.worktree.worktree_path(),
		lane.orchestration_marker.head_sha(),
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
