use crate::{
	loop_contract::DecisionContract,
	prelude::{Result, eyre},
	program_intake::model::{ApplyGoalIssuesInput, GoalIntakeAnchor, GoalIssuePlan},
	tracker::{IssueTracker, TrackerIssue, TrackerIssueBriefUpdate, TrackerIssueCreate},
	workflow::WorkflowDocument,
};

pub(in crate::program_intake) fn linked_goal_issues<T>(
	tracker: &T,
	contract: &DecisionContract,
	plan_count: usize,
) -> Result<Vec<Option<TrackerIssue>>>
where
	T: IssueTracker + ?Sized,
{
	let mut linked = Vec::with_capacity(plan_count);

	for index in 0..plan_count {
		let issue = match contract.links().generated_issue_identifiers().get(index) {
			Some(identifier) =>
				Some(tracker.get_issue_by_identifier(identifier)?.ok_or_else(|| {
					eyre::eyre!(
						"Generated issue link `{identifier}` for Decision Contract `{}` did not resolve.",
						contract.contract_id()
					)
				})?),
			None => None,
		};

		linked.push(issue);
	}

	Ok(linked)
}

pub(in crate::program_intake) fn goal_intake_anchor<T>(
	tracker: &T,
	workflow: &WorkflowDocument,
	team_issue_identifier: Option<String>,
) -> Result<GoalIntakeAnchor>
where
	T: IssueTracker + ?Sized,
{
	let identifier = team_issue_identifier.ok_or_else(|| {
		eyre::eyre!(
			"Goal intake apply requires a source issue on the Decision Contract or --team-issue <ISSUE>."
		)
	})?;
	let issue = tracker
		.get_issue_by_identifier(&identifier)?
		.ok_or_else(|| eyre::eyre!("Team anchor issue `{identifier}` did not resolve."))?;
	let (state_id, _state_name) = workflow
		.frontmatter()
		.tracker()
		.startable_states()
		.iter()
		.find_map(|state_name| {
			issue
				.state_id_for_name(state_name)
				.map(|state_id| (state_id.to_owned(), state_name.as_str()))
		})
		.ok_or_else(|| {
			eyre::eyre!(
				"Team anchor issue `{}` does not expose any configured startable state.",
				issue.identifier
			)
		})?;

	Ok(GoalIntakeAnchor { team_id: issue.team.id, state_id })
}

pub(in crate::program_intake) fn apply_goal_issues_and_link_contract<T>(
	input: ApplyGoalIssuesInput<'_, T>,
) -> Result<(Vec<TrackerIssue>, DecisionContract)>
where
	T: IssueTracker + ?Sized,
{
	let ApplyGoalIssuesInput {
		state_store,
		service_id,
		source_issue_id,
		tracker,
		contract,
		plans,
		linked_issues,
		anchor,
	} = input;
	let mut issues = Vec::with_capacity(plans.len());
	let mut linked_contract = contract.clone();

	for (plan, linked_issue) in plans.iter().zip(linked_issues) {
		let issue = match linked_issue {
			Some(issue) => tracker.update_issue_brief(
				&issue.id,
				&TrackerIssueBriefUpdate {
					title: plan.title.clone(),
					description: plan.description.clone(),
				},
			)?,
			None => tracker.create_issue(&TrackerIssueCreate {
				team_id: anchor.team_id.clone(),
				title: plan.title.clone(),
				description: plan.description.clone(),
				state_id: Some(anchor.state_id.clone()),
			})?,
		};

		issues.push(issue);

		linked_contract =
			linked_goal_contract_for_apply_progress(contract, plans, linked_issues, &issues)?;

		state_store.upsert_decision_contract(
			service_id,
			source_issue_id,
			linked_contract.clone(),
		)?;
	}

	Ok((issues, linked_contract))
}

pub(in crate::program_intake) fn linked_goal_contract_for_apply_progress(
	contract: &DecisionContract,
	plans: &[GoalIssuePlan],
	linked_issues: &[Option<TrackerIssue>],
	applied_issues: &[TrackerIssue],
) -> Result<DecisionContract> {
	let mut linked_contract = contract.clone();
	let mut issue_ids = Vec::new();
	let mut issue_identifiers = Vec::new();
	let mut node_ids = Vec::new();

	for (index, plan) in plans.iter().enumerate() {
		let issue =
			applied_issues.get(index).or_else(|| linked_issues.get(index).and_then(Option::as_ref));

		if let Some(issue) = issue {
			issue_ids.push(issue.id.clone());
			issue_identifiers.push(issue.identifier.clone());

			let node_id = if applied_issues.get(index).is_some() {
				plan.node_id.clone()
			} else {
				contract
					.links()
					.execution_program_node_ids()
					.get(index)
					.cloned()
					.unwrap_or_else(|| plan.node_id.clone())
			};

			node_ids.push(node_id);
		}
	}

	linked_contract.link_generated_execution_surfaces(issue_ids, issue_identifiers, node_ids)?;

	Ok(linked_contract)
}
