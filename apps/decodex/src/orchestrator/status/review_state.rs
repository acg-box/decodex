use crate::{
	orchestrator::status::{
		self, Command, ExternalReviewRequestCiGate, OperatorPostReviewLaneStatus, Path,
		PostReviewLaneClassification, PostReviewLaneDecision, PostReviewLaneSnapshot,
		PostReviewLaneStateLoad, PostReviewReadbackDegradation, PullRequestLandingGateView,
		PullRequestReadbackRootCause, PullRequestReviewState, PullRequestReviewStateInspector,
		RetainedCloseoutPrMergeGate, ReviewHandoffMarker, ServiceConfig, TrackerIssue,
		WorktreeMapping, env, eyre,
	},
	pull_request,
};

pub(crate) fn retained_closeout_pr_merge_gate_with_inspector<I>(
	worktree_path: &Path,
	expected_branch_name: &str,
	pr_url: &str,
	review_state_inspector: &I,
) -> crate::prelude::Result<RetainedCloseoutPrMergeGate>
where
	I: PullRequestReviewStateInspector + ?Sized,
{
	let Some(local_branch_name) = worktree_checkout_branch_name(worktree_path)? else {
		return Ok(RetainedCloseoutPrMergeGate::NotMerged);
	};
	let Some(local_head_oid) = worktree_head_oid(worktree_path)? else {
		return Ok(RetainedCloseoutPrMergeGate::NotMerged);
	};

	if local_branch_name != expected_branch_name {
		return Ok(RetainedCloseoutPrMergeGate::NotMerged);
	}

	let review_state = match review_state_inspector.inspect_review_state(worktree_path, pr_url) {
		Ok(review_state) => review_state,
		Err(_error) => return Ok(RetainedCloseoutPrMergeGate::PullRequestStateReadFailed),
	};

	Ok(
		if matches!(
			status::validate_post_review_lane_review_state(
				review_state,
				expected_branch_name,
				&local_head_oid,
				worktree_path,
			),
			PostReviewLaneStateLoad::ReviewState(PullRequestReviewState {
				state,
				is_draft: false,
				..
			}) if state == "MERGED"
		) {
			RetainedCloseoutPrMergeGate::Merged
		} else {
			RetainedCloseoutPrMergeGate::NotMerged
		},
	)
}

pub(crate) fn validate_post_review_lane_worktree<'a>(
	snapshot: &'a PostReviewLaneSnapshot,
	review_handoff: &ReviewHandoffMarker,
) -> std::result::Result<&'a str, &'static str> {
	if review_handoff.branch_name() != snapshot.worktree.branch_name() {
		return Err("worktree_branch_mismatch");
	}

	let Some(local_branch_name) = snapshot.local_branch_name.as_deref() else {
		return Err("worktree_checkout_branch_missing");
	};

	if local_branch_name != review_handoff.branch_name()
		|| local_branch_name != snapshot.worktree.branch_name()
	{
		return Err("worktree_checkout_branch_mismatch");
	}

	let Some(local_head_oid) = snapshot.local_head_oid.as_deref() else {
		return Err("worktree_head_missing");
	};

	if local_head_oid != review_handoff.pr_head_oid() {
		match worktree_head_descends_from_review_handoff(
			snapshot.worktree.worktree_path(),
			review_handoff.pr_head_oid(),
			local_head_oid,
		) {
			Ok(true) => {},
			Ok(false) => return Err("review_handoff_lineage_mismatch"),
			Err(()) => return Err("review_handoff_lineage_check_failed"),
		}
	}

	Ok(local_head_oid)
}

pub(crate) fn worktree_head_descends_from_review_handoff(
	worktree_path: &Path,
	recorded_head_oid: &str,
	local_head_oid: &str,
) -> std::result::Result<bool, ()> {
	if recorded_head_oid == local_head_oid {
		return Ok(true);
	}

	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["merge-base", "--is-ancestor", recorded_head_oid, local_head_oid])
		.output()
		.map_err(|_| ())?;

	match output.status.code() {
		Some(0) => Ok(true),
		Some(1) => Ok(false),
		_ => Err(()),
	}
}

pub(crate) fn initial_post_review_lane_classification(
	review_state: &PullRequestReviewState,
) -> PostReviewLaneClassification {
	PostReviewLaneClassification {
		decision: PostReviewLaneDecision::WaitForReview,
		reason: String::from("waiting_for_review_or_checks"),
		pr_url: Some(review_state.url.clone()),
		pr_head_sha: Some(review_state.head_ref_oid.clone()),
		pr_state: Some(review_state.state.clone()),
		review_decision: review_state.review_decision.clone(),
		mergeable: Some(review_state.mergeable.clone()),
		check_state: review_state.status_check_rollup_state.clone(),
		unresolved_review_threads: Some(review_state.unresolved_review_threads),
		readback_warning: None,
		readback_root_cause: None,
	}
}

pub(crate) fn blocked_post_review_lane_from_state(
	review_state: &PullRequestReviewState,
	reason: &str,
) -> PostReviewLaneClassification {
	let mut classification = initial_post_review_lane_classification(review_state);

	classification.decision = PostReviewLaneDecision::Block;
	classification.reason = reason.to_owned();
	classification.readback_root_cause = post_review_readback_root_cause_for_reason(reason)
		.map(|root_cause| root_cause.as_str().to_owned());

	classification
}

pub(crate) fn blocked_post_review_lane(reason: &str) -> PostReviewLaneClassification {
	PostReviewLaneClassification {
		decision: PostReviewLaneDecision::Block,
		reason: reason.to_owned(),
		pr_url: None,
		pr_head_sha: None,
		pr_state: None,
		review_decision: None,
		mergeable: None,
		check_state: None,
		unresolved_review_threads: None,
		readback_warning: None,
		readback_root_cause: post_review_readback_root_cause_for_reason(reason)
			.map(|root_cause| root_cause.as_str().to_owned()),
	}
}

pub(crate) fn blocked_post_review_lane_from_handoff(
	review_handoff: &ReviewHandoffMarker,
	reason: &str,
) -> PostReviewLaneClassification {
	let mut classification = blocked_post_review_lane(reason);

	classification.pr_url = Some(review_handoff.pr_url().to_owned());
	classification.pr_head_sha = Some(review_handoff.pr_head_oid().to_owned());

	classification
}

pub(crate) fn readback_degraded_post_review_lane_from_handoff(
	review_handoff: &ReviewHandoffMarker,
	root_cause: PullRequestReadbackRootCause,
) -> PostReviewLaneClassification {
	PostReviewReadbackDegradation::pull_request_state_from_handoff(review_handoff, root_cause)
		.wait_for_review_classification(None)
}

pub(crate) fn blocked_post_review_lane_status(
	project: &ServiceConfig,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	reason: &str,
) -> OperatorPostReviewLaneStatus {
	OperatorPostReviewLaneStatus {
		project_id: project.service_id().to_owned(),
		issue_id: issue.id.clone(),
		issue_identifier: issue.identifier.clone(),
		issue_state: issue.state.name.clone(),
		branch_name: worktree.branch_name().to_owned(),
		worktree_path: status::relative_worktree_path_for_path(project, worktree.worktree_path()),
		classification: String::from("blocked"),
		reason: String::from(reason),
		pr_url: None,
		pr_head_sha: None,
		pr_state: None,
		review_decision: None,
		mergeable: None,
		check_state: None,
		unresolved_review_threads: None,
		shadowed_by_current_lane: false,
		readback_warning: None,
		readback_root_cause: post_review_readback_root_cause_for_reason(reason)
			.map(|root_cause| root_cause.as_str().to_owned()),
		loop_status: None,
	}
}

pub(crate) fn post_review_readback_root_cause_for_reason(
	reason: &str,
) -> Option<PullRequestReadbackRootCause> {
	match reason {
		"pull_request_repository_parse_failed" =>
			Some(PullRequestReadbackRootCause::PullRequestShapeReadFailed),
		"pull_request_branch_mismatch"
		| "pull_request_head_mismatch"
		| "pull_request_head_repository_name_mismatch"
		| "pull_request_head_repository_owner_mismatch"
		| "pull_request_merge_commit_lineage_check_failed"
		| "review_handoff_lineage_check_failed"
		| "review_handoff_lineage_mismatch"
		| "review_orchestration_branch_mismatch"
		| "review_orchestration_head_mismatch"
		| "review_orchestration_pr_mismatch" =>
			Some(PullRequestReadbackRootCause::LineageValidationFailed),
		_ => None,
	}
}

pub(crate) fn resolve_configured_env_var(
	field_name: &str,
	env_var: Option<&str>,
) -> crate::prelude::Result<String> {
	let env_var = env_var.ok_or_else(|| {
		eyre::eyre!("`{field_name}` must be configured for this GitHub-backed operation.")
	})?;
	let value = env::var(env_var).map_err(|error| {
		eyre::eyre!(
			"Failed to read environment variable `{env_var}` referenced by `{field_name}`: {error}"
		)
	})?;

	if value.trim().is_empty() {
		eyre::bail!(
			"Environment variable `{env_var}` referenced by `{field_name}` must not be blank."
		);
	}

	Ok(value)
}

pub(crate) fn external_review_request_ci_gate(
	review_state: &PullRequestReviewState,
) -> ExternalReviewRequestCiGate {
	match review_state.status_check_rollup_state.as_deref() {
		None | Some("SUCCESS") => ExternalReviewRequestCiGate::Ready,
		Some("EXPECTED" | "PENDING") => ExternalReviewRequestCiGate::WaitForGreenChecks,
		Some("ERROR" | "FAILURE") => ExternalReviewRequestCiGate::RepairRequired,
		Some(_) => ExternalReviewRequestCiGate::WaitForGreenChecks,
	}
}

pub(crate) fn failed_checks_require_repair(
	check_state: Option<&str>,
	merge_state_status: &str,
) -> bool {
	pull_request::failed_checks_require_repair(check_state, merge_state_status)
}

pub(crate) fn merge_state_requires_review_repair(
	mergeable: &str,
	merge_state_status: &str,
) -> Option<&'static str> {
	pull_request::merge_state_requires_review_repair(mergeable, merge_state_status)
}

pub(crate) fn review_state_landing_gates_satisfied(review_state: &PullRequestReviewState) -> bool {
	pull_request::retained_landing_gates_satisfied(review_state_landing_gate_view(review_state))
}

pub(crate) fn review_state_clean_path_landing_gates_satisfied(
	review_state: &PullRequestReviewState,
) -> bool {
	pull_request::retained_clean_path_landing_gates_satisfied(review_state_landing_gate_view(
		review_state,
	))
}

pub(crate) fn review_state_landing_requires_agent_fallback(
	review_state: &PullRequestReviewState,
) -> bool {
	pull_request::retained_landing_requires_agent_fallback(review_state_landing_gate_view(
		review_state,
	))
}

pub(crate) fn review_state_landing_gate_view(
	review_state: &PullRequestReviewState,
) -> PullRequestLandingGateView<'_> {
	PullRequestLandingGateView {
		state: review_state.state.as_str(),
		is_draft: review_state.is_draft,
		review_decision: review_state.review_decision.as_deref(),
		pending_review_requests: review_state.pending_review_requests,
		mergeable: review_state.mergeable.as_str(),
		merge_state_status: review_state.merge_state_status.as_str(),
		status_check_rollup_state: review_state.status_check_rollup_state.as_deref(),
		unresolved_review_threads: review_state.unresolved_review_threads,
	}
}

pub(crate) fn worktree_head_oid(worktree_path: &Path) -> crate::prelude::Result<Option<String>> {
	let output =
		Command::new("git").arg("-C").arg(worktree_path).args(["rev-parse", "HEAD"]).output()?;

	if !output.status.success() {
		if !worktree_path.exists() {
			return Ok(None);
		}

		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect worktree HEAD in `{}`: {}",
			worktree_path.display(),
			stderr.trim()
		);
	}

	Ok(Some(String::from_utf8_lossy(&output.stdout).trim().to_owned()))
}

pub(crate) fn worktree_checkout_branch_name(
	worktree_path: &Path,
) -> crate::prelude::Result<Option<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["branch", "--show-current"])
		.output()?;

	if !output.status.success() {
		if !worktree_path.exists() {
			return Ok(None);
		}

		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect worktree checkout branch in `{}`: {}",
			worktree_path.display(),
			stderr.trim()
		);
	}

	let branch_name = String::from_utf8_lossy(&output.stdout).trim().to_owned();

	if branch_name.is_empty() {
		return Ok(None);
	}

	Ok(Some(branch_name))
}
