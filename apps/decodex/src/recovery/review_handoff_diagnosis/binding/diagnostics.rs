use crate::recovery::{
	REVIEW_HANDOFF_MISMATCH_CLASSIFICATION, REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION,
	review_handoff_diagnosis::{actions, binding::model::HandoffBindingDiagnostic},
};

pub(in crate::recovery) fn mismatched_handoff_diagnostic(
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

pub(in crate::recovery) fn rebind_required_diagnostic(
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
