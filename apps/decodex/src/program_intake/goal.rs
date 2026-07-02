use std::collections::BTreeMap;

use crate::{
	execution_program::{
		ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionDispatchAction,
		ExecutionLinearIssueMapping, ExecutionNodeEvaluation, ExecutionProgram,
		ExecutionProgramDependency, ExecutionProgramEvaluation, ExecutionProgramNode,
		ExecutionProgramNodeStage, ExecutionQueueIntent,
	},
	loop_contract::{DecisionContract, DecisionContractStatus, DecisionProposedIssue},
	prelude::{Result, eyre},
	program_intake::{
		issue_batch,
		model::{
			ApplyGoalIssuesInput, GoalIntakeAnchor, GoalIntakeIssueAction, GoalIntakeIssueReport,
			GoalIssueBriefInput, GoalIssuePlan,
		},
		render,
	},
	tracker::{self, IssueTracker, TrackerIssue, TrackerIssueBriefUpdate, TrackerIssueCreate},
	workflow::WorkflowDocument,
};

pub(super) fn ensure_goal_intake_authority(contract: &DecisionContract) -> Result<()> {
	if contract.status() != DecisionContractStatus::AcceptedPromoted {
		eyre::bail!(
			"Decision Contract `{}` is `{}`; goal intake requires accepted execution authority.",
			contract.contract_id(),
			contract.status().as_str()
		);
	}
	if !contract.execution_readiness().ready_for_issue_shaping() {
		eyre::bail!(
			"Decision Contract `{}` is not ready for issue shaping.",
			contract.contract_id()
		);
	}
	if !contract.execution_readiness().missing_decisions().is_empty() {
		eyre::bail!(
			"Decision Contract `{}` still has unresolved decisions.",
			contract.contract_id()
		);
	}
	if contract.execution_readiness().proposed_issues().is_empty() {
		eyre::bail!(
			"Decision Contract `{}` has no structured proposed issues to materialize.",
			contract.contract_id()
		);
	}

	Ok(())
}

pub(super) fn goal_issue_plans(
	contract: &DecisionContract,
	program_id: &str,
) -> Result<Vec<GoalIssuePlan>> {
	let mut plans = Vec::new();

	for (index, issue) in contract.execution_readiness().proposed_issues().iter().enumerate() {
		let node_id = goal_node_id(contract.contract_id(), index, issue.key());
		let title = issue.title().to_owned();
		let objective = issue.objective().to_owned();
		let acceptance = issue.acceptance().to_vec();
		let validation = issue.validation().to_vec();
		let risk = issue.risk().to_vec();
		let dependencies = issue.dependencies().to_vec();
		let conflict_domains = goal_proposed_issue_conflict_domains(issue)?;
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
			stage: parse_goal_stage(issue.stage())?,
			queue_intent: parse_goal_queue_intent(issue.queue_intent())?,
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

pub(super) fn bind_goal_dependency_node_ids(plans: &mut [GoalIssuePlan]) {
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

pub(super) fn linked_goal_issues<T>(
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
			Some(identifier) => {
				Some(tracker.get_issue_by_identifier(identifier)?.ok_or_else(|| {
					eyre::eyre!(
						"Generated issue link `{identifier}` for Decision Contract `{}` did not resolve.",
						contract.contract_id()
					)
				})?)
			},
			None => None,
		};

		linked.push(issue);
	}

	Ok(linked)
}

pub(super) fn goal_intake_anchor<T>(
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

pub(super) fn apply_goal_issues_and_link_contract<T>(
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

pub(super) fn linked_goal_contract_for_apply_progress(
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

pub(super) fn goal_execution_program(
	service_id: &str,
	program_id: &str,
	contract: &DecisionContract,
	plans: &[GoalIssuePlan],
	issues: &[TrackerIssue],
	workflow: &WorkflowDocument,
) -> Result<ExecutionProgram> {
	let nodes = plans
		.iter()
		.zip(issues)
		.map(|(plan, issue)| goal_program_node(service_id, contract, plan, issue, workflow))
		.collect::<Result<Vec<_>>>()?;

	ExecutionProgram::from_accepted_contract(program_id, service_id, contract, nodes)
}

pub(super) fn goal_program_node(
	service_id: &str,
	contract: &DecisionContract,
	plan: &GoalIssuePlan,
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> Result<ExecutionProgramNode> {
	let dependencies = plan
		.dependency_node_ids
		.iter()
		.map(ExecutionProgramDependency::new)
		.collect::<Result<Vec<_>>>()?;
	let mapping = goal_issue_mapping(service_id, issue, workflow)?;

	ExecutionProgramNode::new(
		plan.node_id.clone(),
		plan.stage,
		plan.objective.clone(),
		plan.queue_intent,
	)?
	.with_objective_lineage(goal_objective_lineage(contract))?
	.with_dependencies(dependencies)?
	.with_conflict_domains(plan.conflict_domains.clone())?
	.with_acceptance_expectations(plan.acceptance.clone())?
	.with_validation_expectations(plan.validation.clone())?
	.with_linear_issue(mapping)
}

pub(super) fn goal_issue_mapping(
	service_id: &str,
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> Result<ExecutionLinearIssueMapping> {
	let active_label = tracker::automation_active_label(service_id);
	let tracker_policy = workflow.frontmatter().tracker();

	Ok(ExecutionLinearIssueMapping::new(&issue.id, &issue.identifier, &issue.state.name)?
		.with_active_label(issue.has_label(&active_label))
		.with_opt_out_label(issue.has_label(tracker_policy.opt_out_label()))
		.with_needs_attention_label(issue.has_label(tracker_policy.needs_attention_label()))
		.with_generic_dispatch_briefing(issue_batch::issue_has_generic_dispatch_briefing(issue)))
}

pub(super) fn applied_goal_issue_rows(
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

pub(super) fn dry_run_goal_issue_rows(
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

pub(super) fn goal_issue_report_row(
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
		dispatch_action: dispatch_action.map(issue_batch::dispatch_action_name),
		dependencies: plan.dependencies.clone(),
		conflict_domains: conflict_domain_labels(&plan.conflict_domains),
		acceptance: plan.acceptance.clone(),
		validation: plan.validation.clone(),
		reasons,
	}
}

pub(super) fn goal_objective_lineage(contract: &DecisionContract) -> Vec<String> {
	let mut lineage = vec![
		format!("Accepted Decision Contract `{}`.", contract.contract_id()),
		format!("Source intent: {}", contract.source_intent().summary()),
	];

	lineage.extend(contract.accepted_authority().accepted_objectives().iter().cloned());

	lineage
}

pub(super) fn goal_proposed_issue_conflict_domains(
	issue: &DecisionProposedIssue,
) -> Result<Vec<ExecutionConflictDomain>> {
	issue.conflict_domains().iter().map(|domain| parse_goal_conflict_domain(domain)).collect()
}

pub(super) fn parse_goal_conflict_domain(domain: &str) -> Result<ExecutionConflictDomain> {
	let domain = domain.trim();
	let (kind, key) = domain.split_once(':').unwrap_or(("module", domain));
	let kind = match kind {
		"file" => ExecutionConflictDomainKind::File,
		"module" => ExecutionConflictDomainKind::Module,
		"state" => ExecutionConflictDomainKind::State,
		"credentials" => ExecutionConflictDomainKind::Credentials,
		"tracker_ownership" => ExecutionConflictDomainKind::TrackerOwnership,
		"review_surface" => ExecutionConflictDomainKind::ReviewSurface,
		_ => return ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, domain),
	};

	ExecutionConflictDomain::new(kind, key)
}

pub(super) fn parse_goal_stage(stage: &str) -> Result<ExecutionProgramNodeStage> {
	match stage {
		"research" => Ok(ExecutionProgramNodeStage::Research),
		"design" => Ok(ExecutionProgramNodeStage::Design),
		"spec" => Ok(ExecutionProgramNodeStage::Spec),
		"schema" => Ok(ExecutionProgramNodeStage::Schema),
		"runtime" => Ok(ExecutionProgramNodeStage::Runtime),
		"plugin" => Ok(ExecutionProgramNodeStage::Plugin),
		"eval" => Ok(ExecutionProgramNodeStage::Eval),
		"handoff" => Ok(ExecutionProgramNodeStage::Handoff),
		_ => eyre::bail!("Unsupported proposed issue stage `{stage}`."),
	}
}

pub(super) fn parse_goal_queue_intent(queue_intent: &str) -> Result<ExecutionQueueIntent> {
	match queue_intent {
		"not_ready" => Ok(ExecutionQueueIntent::NotReady),
		"ready_to_queue" => Ok(ExecutionQueueIntent::ReadyToQueue),
		"queued" => Ok(ExecutionQueueIntent::Queued),
		"active" => Ok(ExecutionQueueIntent::Active),
		"paused" => Ok(ExecutionQueueIntent::Paused),
		"done" => Ok(ExecutionQueueIntent::Done),
		"canceled" => Ok(ExecutionQueueIntent::Canceled),
		_ => eyre::bail!("Unsupported proposed issue queue_intent `{queue_intent}`."),
	}
}

pub(super) fn conflict_domain_labels(domains: &[ExecutionConflictDomain]) -> Vec<String> {
	let mut labels = domains
		.iter()
		.map(|domain| format!("{}:{}", domain.kind().as_str(), domain.key()))
		.collect::<Vec<_>>();

	labels.sort();
	labels.dedup();

	labels
}

pub(super) fn goal_program_id(service_id: &str, contract_id: &str) -> String {
	format!("goal-{service_id}-{}", stable_slug(contract_id, 48))
}

pub(super) fn goal_node_id(contract_id: &str, index: usize, objective: &str) -> String {
	format!("goal:{}:{:02}-{}", stable_slug(contract_id, 32), index + 1, stable_slug(objective, 32))
}

pub(super) fn stable_slug(value: &str, max_len: usize) -> String {
	let mut slug = String::new();
	let mut previous_dash = false;

	for character in value.chars() {
		if character.is_ascii_alphanumeric() {
			slug.push(character.to_ascii_lowercase());

			previous_dash = false;
		} else if !previous_dash && !slug.is_empty() {
			slug.push('-');

			previous_dash = true;
		}
		if slug.len() >= max_len {
			break;
		}
	}

	while slug.ends_with('-') {
		slug.pop();
	}

	if slug.is_empty() { String::from("goal") } else { slug }
}
