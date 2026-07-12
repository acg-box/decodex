use crate::{
	execution_program::{ExecutionWorkflowPolicy, decision_contract_fingerprint},
	lane_authority::{IntakeAuthority, IntakeAuthorityKind, ProjectBindingAttestation},
	prelude::{Result, eyre},
	program_intake::{
		GoalIntakeReport, GoalIntakeRunRequest, goal, model::ApplyGoalIssuesInput, readiness,
	},
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
	let accepted_contract_fingerprint = decision_contract_fingerprint(&contract)?;

	let program_id = goal::goal_program_id(config.service_id(), contract.contract_id());
	let plans = goal::goal_issue_plans(&contract, &program_id)?;
	let linked_issues = goal::linked_goal_issues(tracker, &contract, plans.len())?;
	let binding = if apply {
		Some(match state_store.registered_project_binding(config.service_id())? {
			Some(binding) => binding,
			None => {
				#[cfg(not(test))]
				eyre::bail!("Project is not registered; goal intake is forbidden.");
				#[cfg(test)]
				config.project_binding("test-config-fingerprint")
			},
		})
	} else {
		None
	};
	let (issues, linked_contract) = if apply {
		let binding = binding.as_ref().expect("apply binding should exist");
		let anchor = goal::goal_intake_anchor(
			tracker,
			workflow,
			team_issue_identifier
				.or_else(|| record.source_issue_id().map(str::to_owned))
				.or_else(|| contract.source_intent().source_issue_identifier().map(str::to_owned)),
		)?;
		if anchor.team_id != binding.tracker_team_id() {
			eyre::bail!("Goal intake anchor is outside the registered project binding.");
		}

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
			&readiness::intake_readiness_context(
				config.service_id(),
				workflow,
				state_store,
				&program,
				Vec::new(),
			)?,
		)?;
		let rows = goal::applied_goal_issue_rows(&plans, &issues, &linked_issues, &evaluation);

		let plan = program
			.program_intake_plan()
			.ok_or_else(|| eyre::eyre!("Goal Execution Program is missing its intake plan."))?;
		let authority = if let Some(existing) =
			state_store.intake_authority_for_program(config.service_id(), program.program_id())?
		{
			existing
		} else {
			let promotion = linked_contract.promotion().ok_or_else(|| {
				eyre::eyre!("Accepted Decision Contract has no promotion provenance.")
			})?;
			let accepted_at = time::OffsetDateTime::parse(
				promotion.accepted_at(),
				&time::format_description::well_known::Rfc3339,
			)?;
			IntakeAuthority::new(
				&format!("intake-authority-contract-{}", linked_contract.contract_id()),
				config.service_id(),
				ProjectBindingAttestation::new(
					binding.as_ref().expect("apply binding should exist"),
				),
				plan.plan_id(),
				program.program_id(),
				promotion.accepted_by(),
				promotion.acceptance_source(),
				&format!("decision-contract:{}", linked_contract.contract_id()),
				promotion.accepted_at(),
				accepted_at.unix_timestamp(),
				IntakeAuthorityKind::DecisionContract {
					accepted_contract_id: linked_contract.contract_id().to_owned(),
					contract_fingerprint: accepted_contract_fingerprint,
				},
			)?
		};
		state_store.upsert_execution_program_with_intake_authority(
			config.service_id(),
			program,
			authority,
		)?;

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
