use std::collections::BTreeMap;

use crate::{
	loop_contract::DecisionContract,
	prelude::Result,
	program_intake::{
		goal,
		model::{GoalIssueBriefInput, GoalIssuePlan},
		render,
	},
};

pub(in crate::program_intake) fn goal_issue_plans(
	contract: &DecisionContract,
	program_id: &str,
) -> Result<Vec<GoalIssuePlan>> {
	let mut plans = Vec::new();

	for (index, issue) in contract.execution_readiness().proposed_issues().iter().enumerate() {
		let node_id = goal::goal_node_id(contract.contract_id(), index, issue.key());
		let title = issue.title().to_owned();
		let objective = issue.objective().to_owned();
		let acceptance = issue.acceptance().to_vec();
		let validation = issue.validation().to_vec();
		let risk = issue.risk().to_vec();
		let dependencies = issue.dependencies().to_vec();
		let conflict_domains = goal::goal_proposed_issue_conflict_domains(issue)?;
		let description = render::render_goal_issue_brief(GoalIssueBriefInput {
			contract,
			objective: &objective,
			dependencies: &dependencies,
			conflict_domains: &conflict_domains,
			acceptance: &acceptance,
			validation: &validation,
			risk: &risk,
		})?;
		let private_identifiers =
			render::generated_issue_private_identifiers(contract, program_id, &node_id);
		let private_identifier_refs =
			private_identifiers.iter().map(String::as_str).collect::<Vec<_>>();

		render::validate_generated_issue_text(&title, &description, &private_identifier_refs)?;

		plans.push(GoalIssuePlan {
			key: issue.key().to_owned(),
			node_id,
			title,
			objective,
			stage: goal::parse_goal_stage(issue.stage())?,
			queue_intent: goal::parse_goal_queue_intent(issue.queue_intent())?,
			description,
			dependencies,
			dependency_node_ids: Vec::new(),
			conflict_domains,
			acceptance,
			validation,
			risk,
		});
	}

	bind_goal_dependency_node_ids(&mut plans);

	Ok(plans)
}

pub(in crate::program_intake) fn bind_goal_dependency_node_ids(plans: &mut [GoalIssuePlan]) {
	let node_ids_by_key = plans
		.iter()
		.map(|plan| (plan.key.clone(), plan.node_id.clone()))
		.collect::<BTreeMap<_, _>>();

	for plan in plans {
		plan.dependency_node_ids = plan
			.dependencies
			.iter()
			.map(|dependency| {
				node_ids_by_key.get(dependency).cloned().unwrap_or_else(|| dependency.to_owned())
			})
			.collect();
	}
}
