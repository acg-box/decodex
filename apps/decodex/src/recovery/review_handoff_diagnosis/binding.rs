use crate::{
	pull_request::PullRequestLandingState,
	recovery::{
		MISSING_HANDOFF_REASON, ORPHANED_REVIEW_HANDOFF_CLASSIFICATION,
		REVIEW_HANDOFF_BOUND_CLASSIFICATION, REVIEW_HANDOFF_MISMATCH_CLASSIFICATION,
		REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION,
		REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION, REVIEW_HANDOFF_UNVERIFIED_CLASSIFICATION,
		git_worktree::{self, ReviewHandoffLineage},
		review_handoff_diagnosis::actions,
	},
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker, WorktreeMapping},
};

pub(in crate::recovery) struct HandoffBindingDiagnostic {
	pub(in crate::recovery) classification: String,
	pub(in crate::recovery) reason: String,
	pub(in crate::recovery) pr_base_ref: Option<String>,
	pub(in crate::recovery) pr_head_oid: Option<String>,
	pub(in crate::recovery) mismatched_field: Option<String>,
	pub(in crate::recovery) next_action: String,
}

pub(in crate::recovery) struct HandoffDiagnosticRequest<'a> {
	pub(in crate::recovery) service_id: &'a str,
	pub(in crate::recovery) issue_identifier: &'a str,
	pub(in crate::recovery) issue_state_name: &'a str,
	pub(in crate::recovery) success_state: &'a str,
	pub(in crate::recovery) in_progress_state: &'a str,
	pub(in crate::recovery) failure_state: &'a str,
	pub(in crate::recovery) worktree: &'a WorktreeMapping,
	pub(in crate::recovery) existing_handoff: Option<&'a ReviewHandoffMarker>,
	pub(in crate::recovery) existing_orchestration: Option<&'a ReviewOrchestrationMarker>,
	pub(in crate::recovery) local_branch_name: Option<&'a str>,
	pub(in crate::recovery) local_head_oid: Option<&'a str>,
	pub(in crate::recovery) worktree_clean: Option<bool>,
	pub(in crate::recovery) pr_inspection: Option<&'a PullRequestLandingState>,
	pub(in crate::recovery) active_label_present: Option<bool>,
}

struct HandoffDiagnosticContext<'a> {
	issue_identifier: &'a str,
	worktree: &'a WorktreeMapping,
	existing_handoff: &'a ReviewHandoffMarker,
	existing_orchestration: Option<&'a ReviewOrchestrationMarker>,
	local_branch_name: Option<&'a str>,
	local_head_oid: Option<&'a str>,
	worktree_clean: Option<bool>,
}

pub(in crate::recovery) fn diagnostic_binding(
	request: HandoffDiagnosticRequest<'_>,
) -> HandoffBindingDiagnostic {
	let Some(existing_handoff) = request.existing_handoff else {
		return HandoffBindingDiagnostic {
			classification: String::from(ORPHANED_REVIEW_HANDOFF_CLASSIFICATION),
			reason: String::from(MISSING_HANDOFF_REASON),
			pr_base_ref: None,
			pr_head_oid: None,
			mismatched_field: None,
			next_action: actions::missing_handoff_next_action(
				request.service_id,
				request.issue_identifier,
			),
		};
	};
	let context = HandoffDiagnosticContext {
		issue_identifier: request.issue_identifier,
		worktree: request.worktree,
		existing_handoff,
		existing_orchestration: request.existing_orchestration,
		local_branch_name: request.local_branch_name,
		local_head_oid: request.local_head_oid,
		worktree_clean: request.worktree_clean,
	};
	let pr_base_ref = request.pr_inspection.map(|pr| pr.base_ref_name.clone());
	let pr_head_oid = request.pr_inspection.map(|pr| pr.head_ref_oid.clone());

	if let Some(diagnostic) = worktree_binding_mismatch(&context, &pr_base_ref, &pr_head_oid) {
		return diagnostic;
	}

	let Some(local_head_oid) = request.local_head_oid else {
		return mismatched_handoff_diagnostic(
			"worktree_head_missing",
			"worktree.local_head",
			pr_base_ref,
			pr_head_oid,
			actions::inspect_handoff_next_action(
				request.issue_identifier,
				existing_handoff.pr_url(),
			),
		);
	};
	let Some(pr_inspection) = request.pr_inspection else {
		return HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_UNVERIFIED_CLASSIFICATION),
			reason: String::from("pull_request_state_read_failed"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("pr_url")),
			next_action: actions::inspect_handoff_next_action(
				request.issue_identifier,
				existing_handoff.pr_url(),
			),
		};
	};

	if let Some(diagnostic) =
		pull_request_binding_mismatch(&context, pr_inspection, &pr_base_ref, &pr_head_oid)
	{
		return diagnostic;
	}
	if let Some(diagnostic) =
		marker_head_binding_mismatch(&context, local_head_oid, &pr_base_ref, &pr_head_oid)
	{
		return diagnostic;
	}
	if let Some(diagnostic) = handoff_issue_state_drift_diagnostic(
		&request,
		existing_handoff,
		pr_base_ref.clone(),
		pr_head_oid.clone(),
	) {
		return diagnostic;
	}

	HandoffBindingDiagnostic {
		classification: String::from(REVIEW_HANDOFF_BOUND_CLASSIFICATION),
		reason: String::from("review_handoff_record_present"),
		pr_base_ref,
		pr_head_oid,
		mismatched_field: None,
		next_action: actions::bound_handoff_next_action(
			request.service_id,
			request.active_label_present,
		),
	}
}

fn handoff_issue_state_drift_diagnostic(
	request: &HandoffDiagnosticRequest<'_>,
	existing_handoff: &ReviewHandoffMarker,
	pr_base_ref: Option<String>,
	pr_head_oid: Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	if request.active_label_present == Some(false) {
		let next_action = if request.issue_state_name == request.in_progress_state
			|| request.issue_state_name == request.failure_state
		{
			actions::rebind_state_transition_next_action(
				request.issue_identifier,
				existing_handoff.pr_url(),
			)
		} else if request.issue_state_name == request.success_state {
			actions::bound_handoff_next_action(request.service_id, request.active_label_present)
		} else {
			actions::issue_state_mismatch_next_action(
				request.success_state,
				request.in_progress_state,
			)
		};

		return Some(HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION),
			reason: String::from("active_ownership_label_missing"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("issue.labels")),
			next_action,
		});
	}
	if request.issue_state_name == request.in_progress_state {
		return Some(HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION),
			reason: String::from("review_handoff_state_transition_pending"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("issue.state")),
			next_action: actions::rebind_state_transition_next_action(
				request.issue_identifier,
				existing_handoff.pr_url(),
			),
		});
	}
	if request.issue_state_name == request.failure_state {
		return Some(HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION),
			reason: String::from("review_handoff_failure_state_drift"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("issue.state")),
			next_action: actions::rebind_state_transition_next_action(
				request.issue_identifier,
				existing_handoff.pr_url(),
			),
		});
	}

	(request.issue_state_name != request.success_state).then(|| HandoffBindingDiagnostic {
		classification: String::from(REVIEW_HANDOFF_MISMATCH_CLASSIFICATION),
		reason: String::from("review_handoff_issue_state_mismatch"),
		pr_base_ref,
		pr_head_oid,
		mismatched_field: Some(String::from("issue.state")),
		next_action: actions::issue_state_mismatch_next_action(
			request.success_state,
			request.in_progress_state,
		),
	})
}

fn worktree_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	pr_base_ref: &Option<String>,
	pr_head_oid: &Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	let mismatch = if context.existing_handoff.branch_name() != context.worktree.branch_name() {
		Some(("review_handoff_branch_mismatch", "review_handoff.branch_name"))
	} else if context.local_branch_name.is_none() {
		Some(("worktree_checkout_branch_missing", "worktree.local_branch"))
	} else if context.local_branch_name != Some(context.worktree.branch_name()) {
		Some(("worktree_checkout_branch_mismatch", "worktree.local_branch"))
	} else if context.worktree_clean == Some(false) {
		Some(("worktree_dirty", "worktree.clean"))
	} else if context.local_head_oid.is_none() {
		Some(("worktree_head_missing", "worktree.local_head"))
	} else {
		None
	};

	mismatch.map(|(reason, field)| {
		mismatched_handoff_diagnostic(
			reason,
			field,
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			actions::inspect_handoff_next_action(
				context.issue_identifier,
				context.existing_handoff.pr_url(),
			),
		)
	})
}

fn pull_request_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	pr_inspection: &PullRequestLandingState,
	pr_base_ref: &Option<String>,
	pr_head_oid: &Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	if context
		.existing_handoff
		.target_base_ref_name()
		.is_some_and(|base_ref| base_ref != pr_inspection.base_ref_name.as_str())
	{
		return Some(rebind_required_diagnostic(
			"review_handoff_base_mismatch",
			"review_handoff.target_base_ref_name",
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			context.issue_identifier,
			context.existing_handoff.pr_url(),
		));
	}

	let mismatch = if pr_inspection.head_ref_name != context.worktree.branch_name() {
		Some(("pull_request_branch_mismatch", "pull_request.head_ref_name"))
	} else if context.local_head_oid != Some(pr_inspection.head_ref_oid.as_str()) {
		Some(("pull_request_head_mismatch", "pull_request.head_ref_oid"))
	} else {
		None
	};

	mismatch.map(|(reason, field)| {
		mismatched_handoff_diagnostic(
			reason,
			field,
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			actions::inspect_handoff_next_action(
				context.issue_identifier,
				context.existing_handoff.pr_url(),
			),
		)
	})
}

fn marker_head_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	local_head_oid: &str,
	pr_base_ref: &Option<String>,
	pr_head_oid: &Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	let mismatch = if context.existing_handoff.pr_head_oid() != local_head_oid {
		match git_worktree::worktree_head_descends_from_review_handoff(
			context.worktree.worktree_path(),
			context.existing_handoff.pr_head_oid(),
			local_head_oid,
		) {
			ReviewHandoffLineage::Descends => None,
			ReviewHandoffLineage::Diverged => {
				Some(("review_handoff_lineage_mismatch", "review_handoff.pr_head_oid"))
			},
			ReviewHandoffLineage::Unknown => {
				Some(("review_handoff_lineage_check_failed", "review_handoff.pr_head_oid"))
			},
		}
	} else if let Some(orchestration) = context.existing_orchestration {
		orchestration_binding_mismatch(context, orchestration, local_head_oid)
	} else {
		None
	};

	mismatch.map(|(reason, field)| {
		rebind_required_diagnostic(
			reason,
			field,
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			context.issue_identifier,
			context.existing_handoff.pr_url(),
		)
	})
}

fn orchestration_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	orchestration: &ReviewOrchestrationMarker,
	local_head_oid: &str,
) -> Option<(&'static str, &'static str)> {
	if orchestration.branch_name() != context.worktree.branch_name() {
		Some(("review_orchestration_branch_mismatch", "review_orchestration.branch_name"))
	} else if orchestration.pr_url() != context.existing_handoff.pr_url() {
		Some(("review_orchestration_pr_mismatch", "review_orchestration.pr_url"))
	} else if orchestration.head_sha() != local_head_oid {
		Some(("review_orchestration_head_mismatch", "review_orchestration.head_sha"))
	} else {
		None
	}
}

fn mismatched_handoff_diagnostic(
	reason: &str,
	mismatched_field: &str,
	pr_base_ref: Option<String>,
	pr_head_oid: Option<String>,
	next_action: String,
) -> HandoffBindingDiagnostic {
	HandoffBindingDiagnostic {
		classification: String::from(REVIEW_HANDOFF_MISMATCH_CLASSIFICATION),
		reason: reason.to_owned(),
		pr_base_ref,
		pr_head_oid,
		mismatched_field: Some(mismatched_field.to_owned()),
		next_action,
	}
}

fn rebind_required_diagnostic(
	reason: &str,
	mismatched_field: &str,
	pr_base_ref: Option<String>,
	pr_head_oid: Option<String>,
	issue_identifier: &str,
	pr_url: &str,
) -> HandoffBindingDiagnostic {
	HandoffBindingDiagnostic {
		classification: String::from(REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION),
		reason: reason.to_owned(),
		pr_base_ref,
		pr_head_oid,
		mismatched_field: Some(mismatched_field.to_owned()),
		next_action: actions::rebind_refresh_next_action(issue_identifier, pr_url),
	}
}
