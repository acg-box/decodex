pub(in crate::program_intake) mod identity;
pub(in crate::program_intake) mod nodes;
pub(in crate::program_intake) mod reporting;

mod config;

pub(crate) use self::config::{
	register_intake_project_config_for_persist, resolve_intake_project_config_path,
};

use sha2::{Digest, Sha256};

use crate::{
	config::ServiceConfig,
	execution_program::{
		ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionDependencySnapshot,
		ExecutionDispatchAction, ExecutionLinearIssueMapping, ExecutionNodeEvaluation,
		ExecutionProgramDependency, ExecutionProgramNode, ExecutionProgramNodeLifecycleState,
		ExecutionProgramNodeStage, ExecutionQueueIntent,
	},
	orchestrator,
	prelude::{Result, eyre},
	program_intake::{
		IssueBatchIntakeClassification, IssueBatchIntakeCounts, IssueBatchIntakeIssueReport,
		model::IssueFacts,
	},
	runtime,
	state::StateStore,
	tracker::{self, IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
};

pub(crate) fn resolve_intake_project_config_path(
	config_path: Option<&Path>,
	project_id: Option<&str>,
	state_store: &StateStore,
) -> Result<PathBuf> {
	if let Some(config_path) = config_path {
		return ServiceConfig::resolve_project_config_path(config_path);
	}
	if let Some(project_id) = project_id {
		let Some(project) = state_store
			.list_projects()?
			.into_iter()
			.find(|project| project.service_id() == project_id)
		else {
			eyre::bail!(
				"Decodex project `{project_id}` is not registered; pass --config <PROJECT_DIR>."
			);
		};

		return Ok(project.config_path().to_path_buf());
	}

	runtime::registered_config_path_for_cwd(state_store, &env::current_dir()?)?.ok_or_else(|| {
		eyre::eyre!(
			"Current directory is not registered to a Decodex project; pass --config <PROJECT_DIR> or --project <SERVICE_ID>."
		)
	})
}

pub(crate) fn register_intake_project_config_for_persist(
	state_store: &StateStore,
	config_path: &Path,
	persist: bool,
) -> Result<()> {
	if persist {
		runtime::register_project_config(state_store, config_path, true)?;
	}

	Ok(())
}

pub(super) fn normalize_issue_identifiers(issue_identifiers: Vec<String>) -> Result<Vec<String>> {
	let mut normalized = issue_identifiers
		.into_iter()
		.map(|identifier| identifier.trim().to_owned())
		.filter(|identifier| !identifier.is_empty())
		.collect::<Vec<_>>();

	normalized.sort();
	normalized.dedup();

	if normalized.is_empty() {
		eyre::bail!("Issue-batch intake requires at least one issue identifier.");
	}

	Ok(normalized)
}

pub(super) fn issue_batch_fingerprint(
	service_id: &str,
	issue_identifiers: &[String],
	resolved: &BTreeMap<String, TrackerIssue>,
) -> String {
	let mut digest = Sha256::new();

	digest.update(service_id.as_bytes());

	for identifier in issue_identifiers {
		digest.update(b"\0identifier:");
		digest.update(identifier.as_bytes());

		if let Some(issue) = resolved.get(identifier) {
			digest.update(b"\0issue:");
			digest.update(issue.id.as_bytes());
			digest.update(b"\0state:");
			digest.update(issue.state.name.as_bytes());
			digest.update(b"\0updated:");
			digest.update(issue.updated_at.as_bytes());
		}
	}

	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn issue_batch_program_id(service_id: &str, fingerprint: &str) -> String {
	format!("issue-batch-{service_id}-{}", &fingerprint[..16])
}

pub(super) fn node_id_for_issue(identifier: &str) -> String {
	format!("issue:{identifier}")
}

pub(super) fn unmapped_node(identifier: &str) -> Result<ExecutionProgramNode> {
	ExecutionProgramNode::new(
		format!("unmapped:{identifier}"),
		ExecutionProgramNodeStage::Runtime,
		format!("Resolve supplied Linear issue identifier `{identifier}` before dispatch."),
		ExecutionQueueIntent::NotReady,
	)?
	.with_acceptance_expectations([format!(
		"`{identifier}` maps to a normal Linear issue before execution."
	)])?
	.with_validation_expectations([String::from("Tracker lookup succeeds before queue intent.")])
}

pub(super) fn issue_facts<T>(
	tracker: &T,
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
	active_label: &str,
) -> Result<IssueFacts>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();
	let has_active_label =
		tracker::issue_has_label_with_server_confirmation(tracker, issue, active_label)?;
	let has_opt_out_label = tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		tracker_policy.opt_out_label(),
	)?;
	let has_needs_attention_label = tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		tracker_policy.needs_attention_label(),
	)?;
	let has_open_blockers =
		issue.blockers.iter().any(|blocker| !state_name_is_terminal(&blocker.state.name, workflow));

	Ok(IssueFacts {
		has_active_label,
		has_opt_out_label,
		has_needs_attention_label,
		has_generic_dispatch_briefing: issue_has_generic_dispatch_briefing(issue),
		has_open_blockers,
	})
}

pub(super) fn issue_node(
	issue: &TrackerIssue,
	facts: &IssueFacts,
	workflow: &WorkflowDocument,
	supplied_node_ids: &BTreeMap<String, String>,
) -> Result<ExecutionProgramNode> {
	let queue_intent = issue_queue_intent(issue, facts, workflow);
	let mut mapping =
		ExecutionLinearIssueMapping::new(&issue.id, &issue.identifier, &issue.state.name)?;

	mapping = mapping
		.with_active_label(facts.has_active_label)
		.with_opt_out_label(facts.has_opt_out_label)
		.with_needs_attention_label(facts.has_needs_attention_label)
		.with_open_tracker_blockers(facts.has_open_blockers)
		.with_generic_dispatch_briefing(facts.has_generic_dispatch_briefing);

	ExecutionProgramNode::new(
		node_id_for_issue(&issue.identifier),
		ExecutionProgramNodeStage::Runtime,
		issue.title.clone(),
		queue_intent,
	)?
	.with_objective_lineage([format!("Issue-batch intake supplied `{}`.", issue.identifier)])?
	.with_dependencies(issue_dependencies(issue, supplied_node_ids)?)?
	.with_conflict_domains(issue_conflict_domains(issue)?)?
	.with_acceptance_expectations([format!(
		"`{}` remains a normal Linear issue with an executable brief.",
		issue.identifier
	)])?
	.with_validation_expectations([String::from(
		"Run the issue-specific repository validation before review handoff.",
	)])?
	.with_linear_issue(mapping)
}

pub(super) fn issue_queue_intent(
	issue: &TrackerIssue,
	facts: &IssueFacts,
	workflow: &WorkflowDocument,
) -> ExecutionQueueIntent {
	if state_name_is_terminal(&issue.state.name, workflow) {
		return ExecutionQueueIntent::Done;
	}
	if facts.has_active_label {
		return ExecutionQueueIntent::Active;
	}
	if facts.has_opt_out_label {
		return ExecutionQueueIntent::NotReady;
	}
	if !workflow
		.frontmatter()
		.tracker()
		.startable_states()
		.iter()
		.any(|state| state == &issue.state.name)
	{
		return ExecutionQueueIntent::NotReady;
	}

	ExecutionQueueIntent::ReadyToQueue
}

pub(super) fn issue_dependencies(
	issue: &TrackerIssue,
	supplied_node_ids: &BTreeMap<String, String>,
) -> Result<Vec<ExecutionProgramDependency>> {
	let mut dependencies = BTreeMap::new();

	for blocker in &issue.blockers {
		let dependency_id = supplied_node_ids
			.get(&blocker.identifier)
			.cloned()
			.unwrap_or_else(|| blocker.identifier.clone());

		dependencies
			.entry(dependency_id.clone())
			.or_insert(ExecutionProgramDependency::new(dependency_id)?);
	}

	Ok(dependencies.into_values().collect())
}

pub(super) fn dependency_snapshots_for(
	issue: &TrackerIssue,
	supplied_node_ids: &BTreeMap<String, String>,
) -> Result<Vec<ExecutionDependencySnapshot>> {
	let mut snapshots = BTreeMap::new();

	for blocker in &issue.blockers {
		let dependency_id = supplied_node_ids
			.get(&blocker.identifier)
			.cloned()
			.unwrap_or_else(|| blocker.identifier.clone());
		let snapshot = ExecutionDependencySnapshot::tracker_state(
			dependency_id.clone(),
			blocker.state.name.clone(),
		)?;

		snapshots.entry(dependency_id).or_insert(snapshot);
	}

	Ok(snapshots.into_values().collect())
}

pub(super) fn issue_conflict_domains(issue: &TrackerIssue) -> Result<Vec<ExecutionConflictDomain>> {
	let mut domains = vec![ExecutionConflictDomain::new(
		ExecutionConflictDomainKind::TrackerOwnership,
		issue.identifier.clone(),
	)?];
	let mut seen = BTreeSet::from([format!(
		"{}:{}",
		ExecutionConflictDomainKind::TrackerOwnership.as_str(),
		issue.identifier
	)]);

	for label in &issue.labels {
		if let Some(module) = label.name.strip_prefix("repo:")
			&& !module.trim().is_empty()
		{
			let key = module.trim().to_owned();
			let seen_key = format!("{}:{key}", ExecutionConflictDomainKind::Module.as_str());

			if seen.insert(seen_key) {
				domains
					.push(ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, key)?);
			}
		}
	}

	domains.sort_by(|left, right| {
		left.kind().as_str().cmp(right.kind().as_str()).then_with(|| left.key().cmp(right.key()))
	});

	Ok(domains)
}

pub(super) fn issue_report_row(
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

	let mut conflict_domains = issue_conflict_domains(issue)
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
					issue_queue_intent(issue, facts, workflow)
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

pub(super) fn classify_issue(
	issue: &TrackerIssue,
	facts: &IssueFacts,
	evaluation: &ExecutionNodeEvaluation,
	workflow: &WorkflowDocument,
) -> IssueBatchIntakeClassification {
	if state_name_is_terminal(&issue.state.name, workflow) {
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
		ExecutionProgramNodeLifecycleState::Ready | ExecutionProgramNodeLifecycleState::Queued =>
			IssueBatchIntakeClassification::Ready,
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

pub(super) fn dispatch_action_name(action: ExecutionDispatchAction) -> String {
	match action {
		ExecutionDispatchAction::Dispatch => "dispatch",
	}
	.to_owned()
}

pub(super) fn unmapped_report_row(identifier: &str) -> IssueBatchIntakeIssueReport {
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

pub(super) fn classify_counts(rows: &[IssueBatchIntakeIssueReport]) -> IssueBatchIntakeCounts {
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

pub(super) fn state_name_is_terminal(state_name: &str, workflow: &WorkflowDocument) -> bool {
	workflow.frontmatter().tracker().terminal_states().iter().any(|state| state == state_name)
}

pub(super) fn issue_has_generic_dispatch_briefing(issue: &TrackerIssue) -> bool {
	orchestrator::issue_has_generic_dispatch_briefing(issue)
}
