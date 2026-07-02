use crate::{
	execution_program::{
		ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionProgramNodeStage,
		ExecutionQueueIntent,
	},
	loop_contract::{DecisionContract, DecisionProposedIssue},
	prelude::{Result, eyre},
};

pub(in crate::program_intake) fn goal_objective_lineage(
	contract: &DecisionContract,
) -> Vec<String> {
	let mut lineage = vec![
		format!("Accepted Decision Contract `{}`.", contract.contract_id()),
		format!("Source intent: {}", contract.source_intent().summary()),
	];

	lineage.extend(contract.accepted_authority().accepted_objectives().iter().cloned());

	lineage
}

pub(in crate::program_intake) fn goal_proposed_issue_conflict_domains(
	issue: &DecisionProposedIssue,
) -> Result<Vec<ExecutionConflictDomain>> {
	issue.conflict_domains().iter().map(|domain| parse_goal_conflict_domain(domain)).collect()
}

pub(in crate::program_intake) fn parse_goal_conflict_domain(
	domain: &str,
) -> Result<ExecutionConflictDomain> {
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

pub(in crate::program_intake) fn parse_goal_stage(
	stage: &str,
) -> Result<ExecutionProgramNodeStage> {
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

pub(in crate::program_intake) fn parse_goal_queue_intent(
	queue_intent: &str,
) -> Result<ExecutionQueueIntent> {
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

pub(in crate::program_intake) fn conflict_domain_labels(
	domains: &[ExecutionConflictDomain],
) -> Vec<String> {
	let mut labels = domains
		.iter()
		.map(|domain| format!("{}:{}", domain.kind().as_str(), domain.key()))
		.collect::<Vec<_>>();

	labels.sort();
	labels.dedup();

	labels
}

pub(in crate::program_intake) fn goal_program_id(service_id: &str, contract_id: &str) -> String {
	format!("goal-{service_id}-{}", stable_slug(contract_id, 48))
}

pub(in crate::program_intake) fn goal_node_id(
	contract_id: &str,
	index: usize,
	objective: &str,
) -> String {
	format!("goal:{}:{:02}-{}", stable_slug(contract_id, 32), index + 1, stable_slug(objective, 32))
}

pub(in crate::program_intake) fn stable_slug(value: &str, max_len: usize) -> String {
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
