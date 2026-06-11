//! Operator issue-batch intake for internal Execution Programs.

use std::{
	collections::{BTreeMap, BTreeSet},
	env,
	path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
	config::ServiceConfig,
	execution_program::{
		ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionDependencySnapshot,
		ExecutionLinearIssueMapping, ExecutionNodeEvaluation, ExecutionProgram,
		ExecutionProgramDependency, ExecutionProgramNode, ExecutionProgramNodeLifecycleState,
		ExecutionProgramNodeStage, ExecutionProgramReadinessContext, ExecutionQueueIntent,
		ExecutionQueueLabelAction, ExecutionWorkflowPolicy,
	},
	orchestrator,
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
	tracker::{self, IssueTracker, TrackerIssue, linear::LinearClient},
	workflow::WorkflowDocument,
};

/// CLI/runtime request for issue-batch Program Intake.
pub(crate) struct IssueBatchIntakeCommandRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) project_id: Option<&'a str>,
	pub(crate) issue_identifiers: Vec<String>,
	pub(crate) dry_run: bool,
	pub(crate) persist: bool,
}

/// Deterministic report for one issue-batch Program Intake run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct IssueBatchIntakeReport {
	/// Registered service id that owns this intake.
	pub(crate) service_id: String,
	/// Internal program id derived from the accepted issue batch.
	pub(crate) program_id: String,
	/// Whether this run was explicitly dry-run only.
	pub(crate) dry_run: bool,
	/// Whether local runtime state was persisted.
	pub(crate) persisted: bool,
	/// Service-scoped queue label that would be used by later reconciliation.
	pub(crate) queue_label: String,
	/// Deterministic classification counts.
	pub(crate) counts: IssueBatchIntakeCounts,
	/// Per-issue classification rows.
	pub(crate) issues: Vec<IssueBatchIntakeIssueReport>,
}

/// Count summary for an issue-batch intake report.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub(crate) struct IssueBatchIntakeCounts {
	/// Issues ready for queue intent.
	pub(crate) ready: usize,
	/// Issues intentionally held from queueing.
	pub(crate) held: usize,
	/// Issues blocked by dependencies, attention, or briefing.
	pub(crate) blocked: usize,
	/// Issues that are stale or terminal for the accepted batch.
	pub(crate) stale: usize,
	/// Supplied identifiers that did not map to Linear issues.
	pub(crate) unmapped: usize,
}

/// Per-issue report row for issue-batch intake.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct IssueBatchIntakeIssueReport {
	/// Linear issue identifier supplied by the operator.
	pub(crate) issue_identifier: String,
	/// Linear issue id, when the identifier resolved.
	pub(crate) issue_id: Option<String>,
	/// Current Linear workflow state, when the identifier resolved.
	pub(crate) issue_state: Option<String>,
	/// Normalized intake classification.
	pub(crate) classification: IssueBatchIntakeClassification,
	/// Queue intent stored on the internal program node, when available.
	pub(crate) queue_intent: Option<String>,
	/// Readiness-derived queue-label action for later reconciliation.
	pub(crate) queue_label_action: Option<String>,
	/// Deterministic local readback reasons.
	pub(crate) reasons: Vec<String>,
	/// Known blocker issue identifiers.
	pub(crate) blockers: Vec<String>,
	/// Coarse conflict-domain hints.
	pub(crate) conflict_domains: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IssueFacts {
	has_queue_label: bool,
	queue_label_program_owned: bool,
	has_active_label: bool,
	has_opt_out_label: bool,
	has_needs_attention_label: bool,
	has_generic_dispatch_briefing: bool,
	has_open_blockers: bool,
	has_human_owned_queue_label: bool,
}

/// Normalized issue-batch intake classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IssueBatchIntakeClassification {
	/// Ready for later queue-label reconciliation.
	Ready,
	/// Intentionally held from queueing.
	Held,
	/// Blocked by issue state, dependency, attention, or briefing evidence.
	Blocked,
	/// Terminal or stale relative to the accepted intake boundary.
	Stale,
	/// Supplied identifier did not map to a tracker issue.
	Unmapped,
}
impl IssueBatchIntakeClassification {
	fn as_str(self) -> &'static str {
		match self {
			Self::Ready => "ready",
			Self::Held => "held",
			Self::Blocked => "blocked",
			Self::Stale => "stale",
			Self::Unmapped => "unmapped",
		}
	}
}

/// Run issue-batch intake through the configured Linear tracker.
pub(crate) fn run_issue_batch_intake_command(
	request: IssueBatchIntakeCommandRequest<'_>,
) -> Result<IssueBatchIntakeReport> {
	if request.dry_run == request.persist {
		eyre::bail!("Issue-batch intake requires exactly one of --dry-run or --persist.");
	}

	let state_store = runtime::open_runtime_store()?;
	let config_path =
		resolve_intake_project_config_path(request.config_path, request.project_id, &state_store)?;
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;

	runtime::register_project_config(&state_store, &config_path, true)?;

	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;

	run_issue_batch_intake(
		&state_store,
		&tracker,
		&config,
		&workflow,
		request.issue_identifiers,
		request.dry_run,
		request.persist,
	)
}

/// Build and optionally persist a non-mutating issue-batch intake report.
pub(crate) fn run_issue_batch_intake<T>(
	state_store: &StateStore,
	tracker: &T,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue_identifiers: Vec<String>,
	dry_run: bool,
	persist: bool,
) -> Result<IssueBatchIntakeReport>
where
	T: IssueTracker + ?Sized,
{
	if dry_run == persist {
		eyre::bail!("Issue-batch intake requires exactly one of dry_run or persist.");
	}

	let issue_identifiers = normalize_issue_identifiers(issue_identifiers)?;
	let queue_label = tracker::automation_queue_label(config.service_id());
	let active_label = tracker::automation_active_label(config.service_id());
	let policy = ExecutionWorkflowPolicy::from_workflow(config.service_id(), workflow)?;
	let mut resolved = BTreeMap::new();
	let mut missing = Vec::new();

	for identifier in &issue_identifiers {
		match tracker.get_issue_by_identifier(identifier)? {
			Some(issue) => {
				resolved.insert(identifier.clone(), issue);
			},
			None => missing.push(identifier.clone()),
		}
	}

	let batch_fingerprint =
		issue_batch_fingerprint(config.service_id(), &issue_identifiers, &resolved);
	let program_id = issue_batch_program_id(config.service_id(), &batch_fingerprint);
	let supplied_node_ids = issue_identifiers
		.iter()
		.map(|identifier| (identifier.clone(), node_id_for_issue(identifier)))
		.collect::<BTreeMap<_, _>>();
	let mut nodes = Vec::new();
	let mut dependency_snapshots = Vec::new();
	let mut facts_by_identifier = BTreeMap::new();

	for identifier in &issue_identifiers {
		if let Some(issue) = resolved.get(identifier) {
			let facts = issue_facts(
				tracker,
				state_store,
				config.service_id(),
				workflow,
				issue,
				&queue_label,
				&active_label,
			)?;

			dependency_snapshots.extend(dependency_snapshots_for(issue, &supplied_node_ids)?);
			nodes.push(issue_node(issue, &facts, workflow, &supplied_node_ids)?);
			facts_by_identifier.insert(identifier.clone(), facts);
		} else {
			nodes.push(unmapped_node(identifier)?);
		}
	}

	let program = ExecutionProgram::from_issue_batch_intake(
		&program_id,
		config.service_id(),
		&batch_fingerprint,
		format!("Issue-batch intake for {} issue(s).", issue_identifiers.len()),
		nodes,
	)?;
	let context =
		ExecutionProgramReadinessContext::new().with_dependency_snapshots(dependency_snapshots);
	let evaluation = program.evaluate_issue_batch(&policy, &context)?;
	let evaluation_by_issue = evaluation
		.nodes()
		.iter()
		.filter_map(|node| {
			let issue = node.linear_issue()?;

			Some((issue.issue_identifier().to_owned(), node))
		})
		.collect::<BTreeMap<_, _>>();
	let mut rows = Vec::new();

	for identifier in &issue_identifiers {
		if missing.iter().any(|missing| missing == identifier) {
			rows.push(unmapped_report_row(identifier));

			continue;
		}

		let issue = resolved
			.get(identifier)
			.ok_or_else(|| eyre::eyre!("Resolved issue `{identifier}` disappeared from intake."))?;
		let facts = facts_by_identifier
			.get(identifier)
			.ok_or_else(|| eyre::eyre!("Issue facts for `{identifier}` disappeared."))?;
		let evaluation = evaluation_by_issue
			.get(identifier)
			.ok_or_else(|| eyre::eyre!("Issue evaluation for `{identifier}` disappeared."))?;

		rows.push(issue_report_row(issue, facts, evaluation, workflow));
	}

	let counts = classify_counts(&rows);

	if persist {
		state_store.upsert_execution_program(config.service_id(), program)?;
	}

	Ok(IssueBatchIntakeReport {
		service_id: config.service_id().to_owned(),
		program_id,
		dry_run,
		persisted: persist,
		queue_label,
		counts,
		issues: rows,
	})
}

/// Render a compact human-readable intake report.
pub(crate) fn render_issue_batch_intake_report(report: &IssueBatchIntakeReport) -> String {
	let mode = if report.persisted { "persist" } else { "dry-run" };
	let mut output = format!(
		"program intake {mode}: service={} program={} queue_label={} ready={} held={} blocked={} stale={} unmapped={}\n",
		report.service_id,
		report.program_id,
		report.queue_label,
		report.counts.ready,
		report.counts.held,
		report.counts.blocked,
		report.counts.stale,
		report.counts.unmapped,
	);

	for row in &report.issues {
		let state = row.issue_state.as_deref().unwrap_or("unmapped");
		let action = row.queue_label_action.as_deref().unwrap_or("none");
		let reasons =
			if row.reasons.is_empty() { String::from("none") } else { row.reasons.join("; ") };

		output.push_str(&format!(
			"- {} classification={} state={} queue_action={} reasons={}\n",
			row.issue_identifier,
			row.classification.as_str(),
			state,
			action,
			reasons
		));
	}

	output
}

fn resolve_intake_project_config_path(
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

fn normalize_issue_identifiers(issue_identifiers: Vec<String>) -> Result<Vec<String>> {
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

fn issue_batch_fingerprint(
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

fn issue_batch_program_id(service_id: &str, fingerprint: &str) -> String {
	format!("issue-batch-{service_id}-{}", &fingerprint[..16])
}

fn node_id_for_issue(identifier: &str) -> String {
	format!("issue:{identifier}")
}

fn unmapped_node(identifier: &str) -> Result<ExecutionProgramNode> {
	ExecutionProgramNode::new(
		format!("unmapped:{identifier}"),
		ExecutionProgramNodeStage::Runtime,
		format!("Resolve supplied Linear issue identifier `{identifier}` before queueing."),
		ExecutionQueueIntent::NotReady,
	)?
	.with_acceptance_expectations([format!(
		"`{identifier}` maps to a normal Linear issue before execution."
	)])?
	.with_validation_expectations([String::from("Tracker lookup succeeds before queue intent.")])
}

fn issue_facts<T>(
	tracker: &T,
	state_store: &StateStore,
	service_id: &str,
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
	queue_label: &str,
	active_label: &str,
) -> Result<IssueFacts>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();
	let has_queue_label =
		tracker::issue_has_label_with_server_confirmation(tracker, issue, queue_label)?;
	let queue_label_program_owned = has_queue_label
		&& !state_store
			.program_queue_label_ownership_for_issue(service_id, &issue.id, queue_label)?
			.is_empty();
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
	let has_human_owned_queue_label = has_queue_label && !queue_label_program_owned;

	Ok(IssueFacts {
		has_queue_label,
		queue_label_program_owned,
		has_active_label,
		has_opt_out_label,
		has_needs_attention_label,
		has_generic_dispatch_briefing: issue_has_generic_dispatch_briefing(issue),
		has_open_blockers,
		has_human_owned_queue_label,
	})
}

fn issue_node(
	issue: &TrackerIssue,
	facts: &IssueFacts,
	workflow: &WorkflowDocument,
	supplied_node_ids: &BTreeMap<String, String>,
) -> Result<ExecutionProgramNode> {
	let queue_intent = issue_queue_intent(issue, facts, workflow);
	let mut mapping =
		ExecutionLinearIssueMapping::new(&issue.id, &issue.identifier, &issue.state.name)?;

	mapping = if facts.queue_label_program_owned {
		mapping.with_program_owned_queue_label(true)
	} else {
		mapping.with_queue_label(facts.has_queue_label)
	};
	mapping = mapping
		.with_active_label(facts.has_active_label)
		.with_opt_out_label(facts.has_opt_out_label)
		.with_needs_attention_label(facts.has_needs_attention_label)
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

fn issue_queue_intent(
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
	if facts.has_opt_out_label || facts.has_human_owned_queue_label {
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

fn issue_dependencies(
	issue: &TrackerIssue,
	supplied_node_ids: &BTreeMap<String, String>,
) -> Result<Vec<ExecutionProgramDependency>> {
	issue
		.blockers
		.iter()
		.map(|blocker| {
			let dependency_id = supplied_node_ids
				.get(&blocker.identifier)
				.cloned()
				.unwrap_or_else(|| blocker.identifier.clone());

			ExecutionProgramDependency::new(dependency_id)
		})
		.collect()
}

fn dependency_snapshots_for(
	issue: &TrackerIssue,
	supplied_node_ids: &BTreeMap<String, String>,
) -> Result<Vec<ExecutionDependencySnapshot>> {
	issue
		.blockers
		.iter()
		.map(|blocker| {
			let dependency_id = supplied_node_ids
				.get(&blocker.identifier)
				.cloned()
				.unwrap_or_else(|| blocker.identifier.clone());

			ExecutionDependencySnapshot::tracker_state(dependency_id, blocker.state.name.clone())
		})
		.collect()
}

fn issue_conflict_domains(issue: &TrackerIssue) -> Result<Vec<ExecutionConflictDomain>> {
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

	Ok(domains)
}

fn issue_report_row(
	issue: &TrackerIssue,
	facts: &IssueFacts,
	evaluation: &ExecutionNodeEvaluation,
	workflow: &WorkflowDocument,
) -> IssueBatchIntakeIssueReport {
	let classification = classify_issue(issue, facts, evaluation, workflow);
	let mut reasons = evaluation.reasons().to_vec();

	if facts.has_human_owned_queue_label {
		reasons.push(String::from("service queue label is present without program-owned evidence"));
	}

	reasons.sort();
	reasons.dedup();
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
		queue_label_action: evaluation.queue_label_action().map(queue_label_action_name),
		reasons,
		blockers: issue.blockers.iter().map(|blocker| blocker.identifier.clone()).collect(),
		conflict_domains: issue_conflict_domains(issue)
			.unwrap_or_default()
			.into_iter()
			.map(|domain| format!("{}:{}", domain.kind().as_str(), domain.key()))
			.collect(),
	}
}

fn classify_issue(
	issue: &TrackerIssue,
	facts: &IssueFacts,
	evaluation: &ExecutionNodeEvaluation,
	workflow: &WorkflowDocument,
) -> IssueBatchIntakeClassification {
	if state_name_is_terminal(&issue.state.name, workflow) {
		return IssueBatchIntakeClassification::Stale;
	}
	if facts.has_active_label || facts.has_opt_out_label || facts.has_human_owned_queue_label {
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
		| ExecutionProgramNodeLifecycleState::Active => IssueBatchIntakeClassification::Held,
		ExecutionProgramNodeLifecycleState::Blocked
		| ExecutionProgramNodeLifecycleState::NeedsAttention => IssueBatchIntakeClassification::Blocked,
		ExecutionProgramNodeLifecycleState::Completed
		| ExecutionProgramNodeLifecycleState::Stale
		| ExecutionProgramNodeLifecycleState::Superseded => IssueBatchIntakeClassification::Stale,
	}
}

fn queue_label_action_name(action: ExecutionQueueLabelAction) -> String {
	match action {
		ExecutionQueueLabelAction::Apply => "apply",
		ExecutionQueueLabelAction::Retain => "retain",
		ExecutionQueueLabelAction::Remove => "remove",
	}
	.to_owned()
}

fn unmapped_report_row(identifier: &str) -> IssueBatchIntakeIssueReport {
	IssueBatchIntakeIssueReport {
		issue_identifier: identifier.to_owned(),
		issue_id: None,
		issue_state: None,
		classification: IssueBatchIntakeClassification::Unmapped,
		queue_intent: None,
		queue_label_action: None,
		reasons: vec![String::from("tracker issue identifier did not resolve")],
		blockers: Vec::new(),
		conflict_domains: Vec::new(),
	}
}

fn classify_counts(rows: &[IssueBatchIntakeIssueReport]) -> IssueBatchIntakeCounts {
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

fn state_name_is_terminal(state_name: &str, workflow: &WorkflowDocument) -> bool {
	workflow.frontmatter().tracker().terminal_states().iter().any(|state| state == state_name)
}

fn issue_has_generic_dispatch_briefing(issue: &TrackerIssue) -> bool {
	orchestrator::issue_has_generic_dispatch_briefing(issue)
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;

	use crate::{
		program_intake::{self, IssueBatchIntakeClassification},
		state::StateStore,
		tracker::{
			IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBlocker, TrackerState,
			TrackerTeam,
		},
		workflow::WorkflowDocument,
	};

	trait TestIssueExt {
		fn with_blocker(self, identifier: &str, state: &str) -> Self;
	}

	#[derive(Default)]
	struct FakeTracker {
		issues: HashMap<String, TrackerIssue>,
	}

	impl FakeTracker {
		fn with_issues(mut self, issues: impl IntoIterator<Item = TrackerIssue>) -> Self {
			for issue in issues {
				self.issues.insert(issue.identifier.clone(), issue);
			}

			self
		}
	}

	impl IssueTracker for FakeTracker {
		fn list_issues_with_label(
			&self,
			label_name: &str,
		) -> crate::prelude::Result<Vec<TrackerIssue>> {
			Ok(self.issues.values().filter(|issue| issue.has_label(label_name)).cloned().collect())
		}

		fn find_team_label_id(
			&self,
			_team_id: &str,
			label_name: &str,
		) -> crate::prelude::Result<Option<String>> {
			Ok(Some(format!("label-{label_name}")))
		}

		fn get_issue_by_identifier(
			&self,
			issue_identifier: &str,
		) -> crate::prelude::Result<Option<TrackerIssue>> {
			Ok(self.issues.get(issue_identifier).cloned())
		}

		fn refresh_issues(
			&self,
			_issue_ids: &[String],
		) -> crate::prelude::Result<Vec<TrackerIssue>> {
			Ok(Vec::new())
		}

		fn list_comments(&self, _issue_id: &str) -> crate::prelude::Result<Vec<TrackerComment>> {
			Ok(Vec::new())
		}

		fn update_issue_state(
			&self,
			_issue_id: &str,
			_state_id: &str,
		) -> crate::prelude::Result<()> {
			Ok(())
		}

		fn add_issue_labels(
			&self,
			_issue_id: &str,
			_label_ids: &[String],
		) -> crate::prelude::Result<()> {
			Ok(())
		}

		fn remove_issue_labels(
			&self,
			_issue_id: &str,
			_label_ids: &[String],
		) -> crate::prelude::Result<()> {
			Ok(())
		}

		fn create_comment(&self, _issue_id: &str, _body: &str) -> crate::prelude::Result<()> {
			Ok(())
		}
	}

	impl TestIssueExt for TrackerIssue {
		fn with_blocker(mut self, identifier: &str, state: &str) -> Self {
			self.blockers.push(TrackerIssueBlocker {
				id: format!("id-{identifier}"),
				identifier: identifier.to_owned(),
				state: TrackerState { id: format!("state-{state}"), name: state.to_owned() },
			});

			self
		}
	}

	#[test]
	fn issue_batch_dry_run_classifies_without_persisting() {
		let store = StateStore::open_in_memory().expect("store should open");
		let workflow = workflow();
		let config = test_config();
		let tracker = FakeTracker::default().with_issues([
			issue("XY-1", "Todo"),
			issue("XY-2", "In Progress"),
			issue("XY-3", "Done"),
			issue("XY-4", "Todo").with_blocker("XY-10", "Todo"),
		]);
		let report = program_intake::run_issue_batch_intake(
			&store,
			&tracker,
			&config,
			&workflow,
			vec![
				String::from("XY-4"),
				String::from("XY-2"),
				String::from("XY-404"),
				String::from("XY-1"),
				String::from("XY-3"),
			],
			true,
			false,
		)
		.expect("dry-run should classify");

		assert_eq!(report.counts.ready, 1);
		assert_eq!(report.counts.held, 1);
		assert_eq!(report.counts.blocked, 1);
		assert_eq!(report.counts.stale, 1);
		assert_eq!(report.counts.unmapped, 1);
		assert_eq!(report.issues[0].issue_identifier, "XY-1");
		assert_eq!(report.issues[0].classification, IssueBatchIntakeClassification::Ready);
		assert!(
			store.list_execution_programs("decodex").expect("program list should read").is_empty()
		);
	}

	#[test]
	fn issue_batch_persist_writes_program_and_adjacent_intake_state() {
		let store = StateStore::open_in_memory().expect("store should open");
		let workflow = workflow();
		let config = test_config();
		let tracker = FakeTracker::default().with_issues([issue("XY-1", "Todo")]);
		let report = program_intake::run_issue_batch_intake(
			&store,
			&tracker,
			&config,
			&workflow,
			vec![String::from("XY-1")],
			false,
			true,
		)
		.expect("persist should write local state");

		assert!(report.persisted);
		assert_eq!(store.list_execution_programs("decodex").expect("programs").len(), 1);
		assert_eq!(store.list_program_intake_plans("decodex").expect("plans").len(), 1);
		assert_eq!(
			store
				.list_program_issue_mappings("decodex", &report.program_id)
				.expect("mappings")
				.len(),
			1
		);
		assert_eq!(
			store.list_program_intake_plans("decodex").expect("plans")[0].intake_kind(),
			"issue_batch_intake"
		);
	}

	fn workflow() -> WorkflowDocument {
		WorkflowDocument::parse_markdown(
			r#"+++
version = 1
[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"
[agent]
transport = "stdio://"
[execution]
max_attempts = 3
max_turns = 3
max_retry_backoff_ms = 300000
max_concurrent_agents = 0
gate_profiles = {}
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]
[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60
[context]
read_first = []
+++
"#,
		)
		.expect("workflow should parse")
	}

	fn test_config() -> crate::config::ServiceConfig {
		crate::config::ServiceConfig::parse_toml(
			r#"
service_id = "decodex"
[tracker]
api_key_env_var = "HOME"
[github]
token_env_var = "HOME"
[codex]
review = "standard"
[paths]
repo_root = "."
worktree_root = ".worktrees"
"#,
		)
		.expect("config should parse")
	}

	fn issue(identifier: &str, state: &str) -> TrackerIssue {
		TrackerIssue {
			id: format!("id-{identifier}"),
			identifier: identifier.to_owned(),
			project_slug: None,
			title: format!("Issue {identifier}"),
			author: None,
			description: format!("Implement {identifier}."),
			priority: None,
			created_at: String::from("2026-06-01T00:00:00Z"),
			updated_at: String::from("2026-06-01T00:00:00Z"),
			state: TrackerState { id: format!("state-{state}"), name: state.to_owned() },
			team: TrackerTeam {
				id: String::from("team"),
				name: String::from("Team"),
				states: Vec::new(),
				labels: Vec::new(),
			},
			labels_complete: true,
			labels: Vec::new(),
			blockers: Vec::new(),
		}
	}
}
