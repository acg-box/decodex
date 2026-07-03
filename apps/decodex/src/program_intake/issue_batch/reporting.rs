use crate::{
	execution_program::{
		ExecutionDispatchAction, ExecutionNodeEvaluation, ExecutionProgramNodeLifecycleState,
		ExecutionQueueIntent,
	},
	program_intake::{
		IssueBatchIntakeClassification, IssueBatchIntakeCounts, IssueBatchIntakeIssueReport,
		issue_batch::nodes, model::IssueFacts,
	},
	tracker::TrackerIssue,
	workflow::WorkflowDocument,
};

pub(in crate::program_intake) fn issue_report_row(
	issue: &TrackerIssue,
	facts: &IssueFacts,
	evaluation: &ExecutionNodeEvaluation,
	workflow: &WorkflowDocument,
) -> IssueBatchIntakeIssueReport {
	let classification = classify_issue(issue, facts, evaluation, workflow);
	let mut reasons = evaluation.reasons().to_vec();

	reasons.sort();
	reasons.dedup();

	let mut blockers =
		issue.blockers.iter().map(|blocker| blocker.identifier.clone()).collect::<Vec<_>>();

	blockers.sort();
	blockers.dedup();

	let mut conflict_domains = nodes::issue_conflict_domains(issue)
		.unwrap_or_default()
		.into_iter()
		.map(|domain| format!("{}:{}", domain.kind().as_str(), domain.key()))
		.collect::<Vec<_>>();

	conflict_domains.sort();
	conflict_domains.dedup();
	IssueBatchIntakeIssueReport {
		issue_identifier: issue.identifier.clone(),
		issue_id: Some(issue.id.clone()),
		issue_state: Some(issue.state.name.clone()),
		classification,
		queue_intent: Some(
			evaluation
				.linear_issue()
				.map_or(ExecutionQueueIntent::NotReady, |_| {
					nodes::issue_queue_intent(issue, facts, workflow)
				})
				.as_str()
				.to_owned(),
		),
		dispatch_action: evaluation.dispatch_action().map(dispatch_action_name),
		reasons,
		blockers,
		conflict_domains,
	}
}

pub(in crate::program_intake) fn classify_issue(
	issue: &TrackerIssue,
	facts: &IssueFacts,
	evaluation: &ExecutionNodeEvaluation,
	workflow: &WorkflowDocument,
) -> IssueBatchIntakeClassification {
	if nodes::state_name_is_terminal(&issue.state.name, workflow) {
		return IssueBatchIntakeClassification::Stale;
	}
	if facts.has_active_label || facts.has_opt_out_label {
		return IssueBatchIntakeClassification::Held;
	}
	if facts.has_needs_attention_label
		|| facts.has_open_blockers
		|| !facts.has_generic_dispatch_briefing
	{
		return IssueBatchIntakeClassification::Blocked;
	}

	match evaluation.lifecycle_state() {
		ExecutionProgramNodeLifecycleState::Ready | ExecutionProgramNodeLifecycleState::Queued => {
			IssueBatchIntakeClassification::Ready
		},
		ExecutionProgramNodeLifecycleState::Planned
		| ExecutionProgramNodeLifecycleState::Mapped
		| ExecutionProgramNodeLifecycleState::Active
		| ExecutionProgramNodeLifecycleState::PostReview => IssueBatchIntakeClassification::Held,
		ExecutionProgramNodeLifecycleState::Blocked
		| ExecutionProgramNodeLifecycleState::NeedsAttention => IssueBatchIntakeClassification::Blocked,
		ExecutionProgramNodeLifecycleState::Completed
		| ExecutionProgramNodeLifecycleState::Stale
		| ExecutionProgramNodeLifecycleState::Superseded => IssueBatchIntakeClassification::Stale,
	}
}

pub(in crate::program_intake) fn dispatch_action_name(action: ExecutionDispatchAction) -> String {
	match action {
		ExecutionDispatchAction::Dispatch => "dispatch",
	}
	.to_owned()
}

pub(in crate::program_intake) fn unmapped_report_row(
	identifier: &str,
) -> IssueBatchIntakeIssueReport {
	IssueBatchIntakeIssueReport {
		issue_identifier: identifier.to_owned(),
		issue_id: None,
		issue_state: None,
		classification: IssueBatchIntakeClassification::Unmapped,
		queue_intent: None,
		dispatch_action: None,
		reasons: vec![String::from("tracker issue identifier did not resolve")],
		blockers: Vec::new(),
		conflict_domains: Vec::new(),
	}
}

pub(in crate::program_intake) fn classify_counts(
	rows: &[IssueBatchIntakeIssueReport],
) -> IssueBatchIntakeCounts {
	let mut counts = IssueBatchIntakeCounts::default();

	for row in rows {
		match row.classification {
			IssueBatchIntakeClassification::Ready => counts.ready += 1,
			IssueBatchIntakeClassification::Held => counts.held += 1,
			IssueBatchIntakeClassification::Blocked => counts.blocked += 1,
			IssueBatchIntakeClassification::Stale => counts.stale += 1,
			IssueBatchIntakeClassification::Unmapped => counts.unmapped += 1,
		}
	}

	counts
}
