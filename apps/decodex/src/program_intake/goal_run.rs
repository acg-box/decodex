use crate::{
	execution_program::{ExecutionProgramReadinessContext, ExecutionWorkflowPolicy},
	prelude::{Result, eyre},
	program_intake::model::ApplyGoalIssuesInput,
	program_intake::{GoalIntakeReport, GoalIntakeRunRequest, goal},
	tracker::IssueTracker,
};

/// Build and optionally apply a promoted-goal materialization plan.
pub(crate) fn run_goal_intake<T>(request: GoalIntakeRunRequest<'_, T>) -> Result<GoalIntakeReport>
where
	T: IssueTracker + ?Sized,
{
	let GoalIntakeRunRequest {
		state_store,
		tracker,
		config,
		workflow,
		contract_id,
		team_issue_identifier,
		dry_run,
		apply,
	} = request;

	if dry_run == apply {
		eyre::bail!("Goal intake requires exactly one of dry_run or apply.");
	}

	let record = state_store
		.decision_contract(config.service_id(), contract_id)?
		.ok_or_else(|| eyre::eyre!("Decision Contract `{contract_id}` does not exist."))?;
	let contract = record.contract().clone();

	goal::ensure_goal_intake_authority(&contract)?;

	let program_id = goal::goal_program_id(config.service_id(), contract.contract_id());
	let plans = goal::goal_issue_plans(&contract, &program_id)?;
	let linked_issues = goal::linked_goal_issues(tracker, &contract, plans.len())?;
	let (issues, linked_contract) = if apply {
		let anchor = goal::goal_intake_anchor(
			tracker,
			workflow,
			team_issue_identifier
				.or_else(|| record.source_issue_id().map(str::to_owned))
				.or_else(|| contract.source_intent().source_issue_identifier().map(str::to_owned)),
		)?;

		goal::apply_goal_issues_and_link_contract(ApplyGoalIssuesInput {
			state_store,
			service_id: config.service_id(),
			source_issue_id: record.source_issue_id(),
			tracker,
			contract: &contract,
			plans: &plans,
			linked_issues: &linked_issues,
			anchor: &anchor,
		})?
	} else {
		(Vec::new(), contract.clone())
	};
	let report_issues = if apply {
		let program = goal::goal_execution_program(
			config.service_id(),
			&program_id,
			&linked_contract,
			&plans,
			&issues,
			workflow,
		)?;
		let evaluation = program.evaluate(
			&linked_contract,
			&ExecutionWorkflowPolicy::from_workflow(config.service_id(), workflow)?,
			&ExecutionProgramReadinessContext::new(),
		)?;
		let rows = goal::applied_goal_issue_rows(&plans, &issues, &linked_issues, &evaluation);

		state_store.upsert_execution_program(config.service_id(), program)?;

		rows
	} else {
		goal::dry_run_goal_issue_rows(&plans, &linked_issues)
	};

	Ok(GoalIntakeReport {
		service_id: config.service_id().to_owned(),
		contract_id: contract.contract_id().to_owned(),
		program_id,
		dry_run,
		applied: apply,
		persisted: apply,
		issues: report_issues,
	})
}
