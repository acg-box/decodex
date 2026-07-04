mod diagnostics;
mod issue_state;
mod marker;
mod model;
mod pull_request;
mod worktree;

pub(in crate::recovery) use self::model::HandoffDiagnosticRequest;

use crate::recovery::{
	MISSING_HANDOFF_REASON, ORPHANED_REVIEW_HANDOFF_CLASSIFICATION,
	REVIEW_HANDOFF_BOUND_CLASSIFICATION, REVIEW_HANDOFF_UNVERIFIED_CLASSIFICATION,
	review_handoff_diagnosis::{
		actions,
		binding::model::{HandoffBindingDiagnostic, HandoffDiagnosticContext},
	},
};

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

	if let Some(diagnostic) =
		worktree::worktree_binding_mismatch(&context, &pr_base_ref, &pr_head_oid)
	{
		return diagnostic;
	}

	let Some(local_head_oid) = request.local_head_oid else {
		return diagnostics::mismatched_handoff_diagnostic(
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

	if let Some(diagnostic) = pull_request::pull_request_binding_mismatch(
		&context,
		pr_inspection,
		&pr_base_ref,
		&pr_head_oid,
	) {
		return diagnostic;
	}
	if let Some(diagnostic) =
		marker::marker_head_binding_mismatch(&context, local_head_oid, &pr_base_ref, &pr_head_oid)
	{
		return diagnostic;
	}
	if let Some(diagnostic) = issue_state::handoff_issue_state_drift_diagnostic(
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
			request.issue_identifier,
			existing_handoff.pr_url(),
			request.active_label_present,
		),
	}
}
