use crate::{
	orchestrator::{OperatorAutonomyExecutionEvidenceStatus, status_autonomy},
	state::{PrivateExecutionEvent, ReviewLifecycleRecord},
};

pub(in crate::orchestrator::status_autonomy::evidence::replay) fn operator_autonomy_pr_evidence_status_from_event(
	event: &PrivateExecutionEvent,
	review: &ReviewLifecycleRecord,
	issue_identifier: Option<&str>,
	summary: String,
	summary_redacted: bool,
) -> OperatorAutonomyExecutionEvidenceStatus {
	let (source_refs, refs_redacted) =
		status_autonomy::public_autonomy_refs(&[review.pr_url().to_owned()]);
	let mut known_gaps = Vec::new();

	if source_refs.is_empty() {
		known_gaps.push(String::from("source_refs_missing_or_redacted"));
	}
	if refs_redacted {
		known_gaps.push(String::from("source_refs_redacted"));
	}
	if summary_redacted {
		known_gaps.push(String::from("summary_redacted"));
	}

	OperatorAutonomyExecutionEvidenceStatus {
		kind: String::from("pr"),
		issue_identifier: issue_identifier.map(str::to_owned),
		source_refs,
		summary,
		updated_at: [review.updated_at(), event.recorded_at()]
			.into_iter()
			.max()
			.unwrap_or_else(|| event.recorded_at())
			.to_owned(),
		completeness: status_autonomy::operator_autonomy_completeness(&known_gaps),
		known_gaps,
	}
}
