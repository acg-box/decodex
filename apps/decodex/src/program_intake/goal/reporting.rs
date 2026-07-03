use crate::{
	execution_program::{
		ExecutionDispatchAction, ExecutionNodeEvaluation, ExecutionProgramEvaluation,
	},
	program_intake::{
		goal, issue_batch,
		model::{GoalIntakeIssueAction, GoalIntakeIssueReport, GoalIssuePlan},
	},
	tracker::TrackerIssue,
};

pub(in crate::program_intake) fn applied_goal_issue_rows(
	plans: &[GoalIssuePlan],
	issues: &[TrackerIssue],
	linked_issues: &[Option<TrackerIssue>],
	evaluation: &ExecutionProgramEvaluation,
) -> Vec<GoalIntakeIssueReport> {
	plans
		.iter()
		.zip(issues)
		.zip(linked_issues)
		.map(|((plan, issue), linked)| {
			let evaluation = evaluation.nodes().iter().find(|node| node.node_id() == plan.node_id);

			goal_issue_report_row(
				plan,
				Some(issue),
				if linked.is_some() {
					GoalIntakeIssueAction::Updated
				} else {
					GoalIntakeIssueAction::Created
				},
				evaluation.and_then(ExecutionNodeEvaluation::dispatch_action),
				evaluation.map_or_else(Vec::new, |node| node.reasons().to_vec()),
			)
		})
		.collect()
}

pub(in crate::program_intake) fn dry_run_goal_issue_rows(
	plans: &[GoalIssuePlan],
	linked_issues: &[Option<TrackerIssue>],
) -> Vec<GoalIntakeIssueReport> {
	plans
		.iter()
		.zip(linked_issues)
		.map(|(plan, linked)| {
			let action = if linked.is_some() {
				GoalIntakeIssueAction::WouldUpdate
			} else {
				GoalIntakeIssueAction::WouldCreate
			};
			let reason = match action {
				GoalIntakeIssueAction::WouldCreate => {
					"apply will create a normal Linear issue and persist a mapped program node"
				},
				GoalIntakeIssueAction::WouldUpdate => {
					"apply will update the linked normal Linear issue and persist a mapped program node"
				},
				GoalIntakeIssueAction::Created | GoalIntakeIssueAction::Updated => {
					"apply already materialized this issue"
				},
			};

			goal_issue_report_row(plan, linked.as_ref(), action, None, vec![reason.to_owned()])
		})
		.collect()
}

pub(in crate::program_intake) fn goal_issue_report_row(
	plan: &GoalIssuePlan,
	issue: Option<&TrackerIssue>,
	action: GoalIntakeIssueAction,
	dispatch_action: Option<ExecutionDispatchAction>,
	reasons: Vec<String>,
) -> GoalIntakeIssueReport {
	GoalIntakeIssueReport {
		node_id: plan.node_id.clone(),
		title: plan.title.clone(),
		objective: plan.objective.clone(),
		issue_id: issue.map(|issue| issue.id.clone()),
		issue_identifier: issue.map(|issue| issue.identifier.clone()),
		action,
		queue_intent: plan.queue_intent.as_str().to_owned(),
		dispatch_action: dispatch_action.map(issue_batch::reporting::dispatch_action_name),
		dependencies: plan.dependencies.clone(),
		conflict_domains: goal::conflict_domain_labels(&plan.conflict_domains),
		acceptance: plan.acceptance.clone(),
		validation: plan.validation.clone(),
		reasons,
	}
}
