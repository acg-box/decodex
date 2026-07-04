use crate::{
	recovery::{
		REVIEW_HANDOFF_MISMATCH_CLASSIFICATION, REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION,
		REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION,
		review_handoff_diagnosis::{
			actions,
			binding::model::{HandoffBindingDiagnostic, HandoffDiagnosticRequest},
		},
	},
	state::ReviewHandoffMarker,
};

pub(in crate::recovery) fn handoff_issue_state_drift_diagnostic(
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
			actions::bound_handoff_next_action(
				request.service_id,
				request.issue_identifier,
				existing_handoff.pr_url(),
				request.active_label_present,
			)
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
