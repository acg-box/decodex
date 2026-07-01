#[allow(clippy::wildcard_imports)] use super::*;

pub(crate) fn run_completion_repo_gate(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	phase: PhaseGoalKind,
) -> Result<()> {
	let selected_repo_gate =
		select_repo_gate_for_worktree(workflow.frontmatter().execution(), &issue_run.worktree.path);

	write_run_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_REPO_GATE,
	);
	if let Err(error) = run_repo_gate_commands(
		selected_repo_gate.canonicalize_commands(),
		selected_repo_gate.verify_commands(),
		&issue_run.worktree.path,
	) {
		if let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() {
			let scope_envelope_violation = repo_gate_failure
				.tracked_rewrite_decision()
				.is_some_and(RepoGateTrackedRewriteDecision::is_scope_envelope_violation);
			let lane_snapshot = LaneDecisionSnapshot::repo_gate_failure(
				issue_run.issue.identifier.clone(),
				issue_run.run_id.clone(),
				issue_run.attempt_number,
				issue_run.dispatch_mode,
				phase,
				repo_gate_failure.disposition(),
				scope_envelope_violation,
			);
			let lane_decision = decide_lane_next_action(&lane_snapshot);

			state_store.append_private_execution_event(
				project.service_id(),
				&issue_run.issue.id,
				&issue_run.run_id,
				issue_run.attempt_number,
				"lane_decision",
				lane_snapshot.to_json(lane_decision.next_action, lane_decision.reason),
			)?;
		}

		return Err(error);
	}
	write_run_operation_marker_best_effort(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		RUN_OPERATION_REVIEW_WRITEBACK,
	);

	Ok(())
}

pub(crate) fn push_retained_review_repair_head(
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
	pr_url: Option<&str>,
) -> Result<()> {
	let token_env_var = project.github().token_env_var();
	let github_token = resolve_configured_env_var("github.token_env_var", Some(token_env_var))
		.map_err(|error| {
			Report::new(RetainedReviewRepairPushFailed {
				issue_identifier: issue_run.issue.identifier.clone(),
				run_id: issue_run.run_id.clone(),
				branch_name: issue_run.worktree.branch_name.clone(),
				pr_url: pr_url.map(ToOwned::to_owned),
				kind: RetainedReviewRepairPushFailureKind::Auth,
				detail: error.to_string(),
			})
		})?;
	let git_credentials =
		GitCredentialSource::new(token_env_var, &github_token).materialize_github_credentials();
	let refspec = format!("HEAD:{}", issue_run.worktree.branch_name);
	let mut command = Command::new("git");

	command.arg("-C").arg(&issue_run.worktree.path).arg("push").arg("origin").arg(&refspec);
	git_credentials.apply_to(&mut command);

	let output = command.output().map_err(|error| {
		Report::new(RetainedReviewRepairPushFailed {
			issue_identifier: issue_run.issue.identifier.clone(),
			run_id: issue_run.run_id.clone(),
			branch_name: issue_run.worktree.branch_name.clone(),
			pr_url: pr_url.map(ToOwned::to_owned),
			kind: RetainedReviewRepairPushFailureKind::Failed,
			detail: error.to_string(),
		})
	})?;

	if output.status.success() {
		return Ok(());
	}

	let detail = repo_gate_output_text(&output);
	let kind = classify_retained_review_repair_push_failure(&detail);

	Err(Report::new(RetainedReviewRepairPushFailed {
		issue_identifier: issue_run.issue.identifier.clone(),
		run_id: issue_run.run_id.clone(),
		branch_name: issue_run.worktree.branch_name.clone(),
		pr_url: pr_url.map(ToOwned::to_owned),
		kind,
		detail,
	}))
}

fn classify_retained_review_repair_push_failure(
	detail: &str,
) -> RetainedReviewRepairPushFailureKind {
	let normalized = detail.to_ascii_lowercase();

	if normalized.contains("authentication failed")
		|| normalized.contains("could not read username")
		|| normalized.contains("permission denied")
		|| normalized.contains("repository not found")
		|| normalized.contains("403")
		|| normalized.contains("401")
	{
		return RetainedReviewRepairPushFailureKind::Auth;
	}
	if normalized.contains("src refspec")
		|| normalized.contains("dst refspec")
		|| normalized.contains("invalid refspec")
	{
		return RetainedReviewRepairPushFailureKind::Refspec;
	}
	if normalized.contains("rejected")
		|| normalized.contains("non-fast-forward")
		|| normalized.contains("fetch first")
		|| normalized.contains("protected branch hook declined")
	{
		return RetainedReviewRepairPushFailureKind::RemoteRejected;
	}

	RetainedReviewRepairPushFailureKind::Failed
}

pub(super) fn apply_run_completion_disposition<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	tracker_tool_bridge: &TrackerToolBridge<'_>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	match tracker_tool_bridge.completion_disposition()? {
		RunCompletionDisposition::ReviewHandoff => {
			validate_review_handoff_runtime(project, false)?;
			run_completion_repo_gate(
				project,
				workflow,
				state_store,
				issue_run,
				PhaseGoalKind::HandoffEvidence,
			)?;

			tracker_tool_bridge.apply_review_handoff().map_err(|error| {
				if let Some(writeback_error) = error.downcast_ref::<ReviewHandoffWritebackFailed>()
				{
					Report::new(ReviewHandoffNeedsAttention {
						issue_identifier: writeback_error.issue_identifier.clone(),
						pr_url: writeback_error.pr_url.clone(),
						run_id: writeback_error.run_id.clone(),
					})
					.wrap_err(error)
				} else {
					error
				}
			})?;

			record_harness_outcome_best_effort(
				state_store,
				project.service_id(),
				issue_run,
				HarnessOutcomeKind::ReviewHandoff,
				None,
				Some("passed"),
				tracker_tool_bridge
					.review_context()
					.and_then(|context| context.recorded_pr_url.as_deref()),
			);
		},
		RunCompletionDisposition::ManualAttention => {
			return Err(Report::new(ManualAttentionRequested {
				issue_identifier: issue_run.issue.identifier.clone(),
				label: workflow.frontmatter().tracker().needs_attention_label().to_owned(),
				run_id: issue_run.run_id.clone(),
				error_class: tracker_tool_bridge.manual_attention_error_class(),
			}));
		},
		RunCompletionDisposition::ReviewRepair => {
			validate_review_repair_runtime(project, false)?;
			run_completion_repo_gate(
				project,
				workflow,
				state_store,
				issue_run,
				PhaseGoalKind::ReviewRepairEvidence,
			)?;
			push_retained_review_repair_head(
				project,
				issue_run,
				tracker_tool_bridge
					.review_context()
					.and_then(|context| context.recorded_pr_url.as_deref()),
			)?;

			tracker_tool_bridge.apply_review_repair()?;

			record_harness_outcome_best_effort(
				state_store,
				project.service_id(),
				issue_run,
				HarnessOutcomeKind::ReviewRepair,
				None,
				Some("passed"),
				tracker_tool_bridge
					.review_context()
					.and_then(|context| context.recorded_pr_url.as_deref()),
			);
		},
		RunCompletionDisposition::Closeout => {
			write_run_operation_marker_best_effort(
				&issue_run.worktree.path,
				&issue_run.run_id,
				issue_run.attempt_number,
				RUN_OPERATION_REVIEW_WRITEBACK,
			);

			let cleanup_commit_sha = worktree_head_oid(&issue_run.worktree.path)?;

			tracker_tool_bridge.apply_closeout()?;

			cleanup_completed_post_review_lane(project, workflow, state_store, issue_run)?;
			write_cleanup_complete_lifecycle_event(
				tracker,
				project,
				state_store,
				issue_run,
				tracker_tool_bridge
					.review_context()
					.and_then(|context| context.recorded_pr_url.as_deref()),
				cleanup_commit_sha.as_deref(),
			)?;

			tracker_tool_bridge.clear_closeout_issue_scope()?;

			record_harness_outcome_best_effort(
				state_store,
				project.service_id(),
				issue_run,
				HarnessOutcomeKind::Closeout,
				None,
				Some("passed"),
				tracker_tool_bridge
					.review_context()
					.and_then(|context| context.recorded_pr_url.as_deref()),
			);
		},
	}

	Ok(())
}
