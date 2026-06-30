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
		ExecutionDispatchAction, ExecutionLinearIssueMapping, ExecutionNodeEvaluation,
		ExecutionProgram, ExecutionProgramDependency, ExecutionProgramEvaluation,
		ExecutionProgramNode, ExecutionProgramNodeLifecycleState, ExecutionProgramNodeStage,
		ExecutionProgramReadinessContext, ExecutionQueueIntent, ExecutionWorkflowPolicy,
	},
	loop_contract::{DecisionContract, DecisionContractStatus, DecisionProposedIssue},
	orchestrator,
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
	tracker::{
		self, IssueTracker, TrackerIssue, TrackerIssueBriefUpdate, TrackerIssueCreate,
		linear::LinearClient,
	},
	workflow::WorkflowDocument,
};

mod goal;
mod issue_batch;
mod render;
#[cfg(test)]
mod tests;

use self::render::{
	generated_issue_private_identifiers, render_goal_issue_brief, validate_generated_issue_text,
};
pub(crate) use self::render::{render_goal_intake_report, render_issue_batch_intake_report};
#[allow(clippy::wildcard_imports)]
use goal::*;
#[allow(clippy::wildcard_imports)]
use issue_batch::*;

/// CLI/runtime request for issue-batch Program Intake.
pub(crate) struct IssueBatchIntakeCommandRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) project_id: Option<&'a str>,
	pub(crate) issue_identifiers: Vec<String>,
	pub(crate) dry_run: bool,
	pub(crate) persist: bool,
}

/// CLI/runtime request for promoted-goal Program Intake.
pub(crate) struct GoalIntakeCommandRequest<'a> {
	/// Project config override, if supplied.
	pub(crate) config_path: Option<&'a Path>,
	/// Registered service id to intake against.
	pub(crate) project_id: Option<&'a str>,
	/// Accepted Decision Contract id to materialize.
	pub(crate) contract_id: &'a str,
	/// Optional Linear issue whose team/startable state should anchor generated issues.
	pub(crate) team_issue_identifier: Option<&'a str>,
	/// Read and render the proposed materialization without mutating Linear or local intake rows.
	pub(crate) dry_run: bool,
	/// Create or update generated Linear issues and persist the Execution Program.
	pub(crate) apply: bool,
}

/// In-process request for promoted-goal Program Intake.
pub(crate) struct GoalIntakeRunRequest<'a, T>
where
	T: IssueTracker + ?Sized,
{
	/// Runtime state store used for Decision Contract and program persistence.
	pub(crate) state_store: &'a StateStore,
	/// Tracker adapter used for Linear reads and apply-mode writes.
	pub(crate) tracker: &'a T,
	/// Registered service config that owns this intake.
	pub(crate) config: &'a ServiceConfig,
	/// Registered workflow document for queue and startable-state policy.
	pub(crate) workflow: &'a WorkflowDocument,
	/// Accepted Decision Contract id to materialize.
	pub(crate) contract_id: &'a str,
	/// Optional Linear issue whose team/startable state should anchor generated issues.
	pub(crate) team_issue_identifier: Option<String>,
	/// Read and render without mutation.
	pub(crate) dry_run: bool,
	/// Create/update Linear issues and persist local intake rows.
	pub(crate) apply: bool,
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
	/// Readiness-derived direct dispatch action.
	pub(crate) dispatch_action: Option<String>,
	/// Deterministic local readback reasons.
	pub(crate) reasons: Vec<String>,
	/// Known blocker issue identifiers.
	pub(crate) blockers: Vec<String>,
	/// Coarse conflict-domain hints.
	pub(crate) conflict_domains: Vec<String>,
}

/// Deterministic report for one promoted-goal Program Intake run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct GoalIntakeReport {
	/// Registered service id that owns this intake.
	pub(crate) service_id: String,
	/// Accepted Decision Contract that authorized the materialization.
	pub(crate) contract_id: String,
	/// Internal program id derived from the accepted goal.
	pub(crate) program_id: String,
	/// Whether this run was explicitly dry-run only.
	pub(crate) dry_run: bool,
	/// Whether Linear issues were created or updated.
	pub(crate) applied: bool,
	/// Whether local runtime Program Intake records were persisted.
	pub(crate) persisted: bool,
	/// Per-issue materialization rows.
	pub(crate) issues: Vec<GoalIntakeIssueReport>,
}

/// Per-issue promoted-goal materialization row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct GoalIntakeIssueReport {
	/// Internal Execution Program node id.
	pub(crate) node_id: String,
	/// Public issue title.
	pub(crate) title: String,
	/// Natural-language objective for this generated issue.
	pub(crate) objective: String,
	/// Linear issue id after apply, or the linked id found during dry-run.
	pub(crate) issue_id: Option<String>,
	/// Linear issue identifier after apply, or the linked identifier found during dry-run.
	pub(crate) issue_identifier: Option<String>,
	/// Whether apply would create/update or did create/update the issue.
	pub(crate) action: GoalIntakeIssueAction,
	/// Queue intent stored on the internal program node.
	pub(crate) queue_intent: String,
	/// Direct dispatch action derived for the mapped node, when known.
	pub(crate) dispatch_action: Option<String>,
	/// Dependency ids or public issue identifiers required before this node can run.
	pub(crate) dependencies: Vec<String>,
	/// Coarse conflict domains retained in the internal program.
	pub(crate) conflict_domains: Vec<String>,
	/// Acceptance expectations rendered into the public issue brief.
	pub(crate) acceptance: Vec<String>,
	/// Validation expectations rendered into the public issue brief.
	pub(crate) validation: Vec<String>,
	/// Deterministic local readback reasons.
	pub(crate) reasons: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IssueFacts {
	has_active_label: bool,
	has_opt_out_label: bool,
	has_needs_attention_label: bool,
	has_generic_dispatch_briefing: bool,
	has_open_blockers: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GoalIssuePlan {
	key: String,
	node_id: String,
	title: String,
	objective: String,
	stage: ExecutionProgramNodeStage,
	queue_intent: ExecutionQueueIntent,
	description: String,
	dependencies: Vec<String>,
	dependency_node_ids: Vec<String>,
	conflict_domains: Vec<ExecutionConflictDomain>,
	acceptance: Vec<String>,
	validation: Vec<String>,
	risk: Vec<String>,
}

struct GoalIntakeAnchor {
	team_id: String,
	state_id: String,
}

struct GoalIssueBriefInput<'a> {
	contract: &'a DecisionContract,
	objective: &'a str,
	dependencies: &'a [String],
	conflict_domains: &'a [ExecutionConflictDomain],
	acceptance: &'a [String],
	validation: &'a [String],
	risk: &'a [String],
}

struct ApplyGoalIssuesInput<'a, T>
where
	T: IssueTracker + ?Sized,
{
	state_store: &'a StateStore,
	service_id: &'a str,
	source_issue_id: Option<&'a str>,
	tracker: &'a T,
	contract: &'a DecisionContract,
	plans: &'a [GoalIssuePlan],
	linked_issues: &'a [Option<TrackerIssue>],
	anchor: &'a GoalIntakeAnchor,
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

/// Promoted-goal materialization action.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GoalIntakeIssueAction {
	/// Dry-run would create a new normal Linear issue.
	WouldCreate,
	/// Dry-run would update an already linked normal Linear issue.
	WouldUpdate,
	/// Apply created a new normal Linear issue.
	Created,
	/// Apply updated an already linked normal Linear issue.
	Updated,
}
impl GoalIntakeIssueAction {
	fn as_str(self) -> &'static str {
		match self {
			Self::WouldCreate => "would_create",
			Self::WouldUpdate => "would_update",
			Self::Created => "created",
			Self::Updated => "updated",
		}
	}
}

/// Run issue-batch intake through the configured Linear tracker.
pub(crate) fn run_issue_batch_intake_command(
	request: IssueBatchIntakeCommandRequest<'_>,
) -> Result<IssueBatchIntakeReport> {
	if request.dry_run == request.persist {
		eyre::bail!("Issue-batch intake requires exactly one of --dry-run or --apply.");
	}

	let state_store = runtime::open_runtime_store()?;
	let config_path =
		resolve_intake_project_config_path(request.config_path, request.project_id, &state_store)?;
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;

	register_intake_project_config_for_persist(&state_store, &config_path, request.persist)?;

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

/// Run promoted-goal intake through the configured Linear tracker.
pub(crate) fn run_goal_intake_command(
	request: GoalIntakeCommandRequest<'_>,
) -> Result<GoalIntakeReport> {
	if request.dry_run == request.apply {
		eyre::bail!("Goal intake requires exactly one of --dry-run or --apply.");
	}

	let state_store = runtime::open_runtime_store()?;
	let config_path =
		resolve_intake_project_config_path(request.config_path, request.project_id, &state_store)?;
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;

	register_intake_project_config_for_persist(&state_store, &config_path, request.apply)?;

	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;

	run_goal_intake(GoalIntakeRunRequest {
		state_store: &state_store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: request.contract_id,
		team_issue_identifier: request.team_issue_identifier.map(str::to_owned),
		dry_run: request.dry_run,
		apply: request.apply,
	})
}

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

	ensure_goal_intake_authority(&contract)?;

	let program_id = goal_program_id(config.service_id(), contract.contract_id());
	let plans = goal_issue_plans(&contract, &program_id)?;
	let linked_issues = linked_goal_issues(tracker, &contract, plans.len())?;
	let (issues, linked_contract) = if apply {
		let anchor = goal_intake_anchor(
			tracker,
			workflow,
			team_issue_identifier
				.or_else(|| record.source_issue_id().map(str::to_owned))
				.or_else(|| contract.source_intent().source_issue_identifier().map(str::to_owned)),
		)?;

		apply_goal_issues_and_link_contract(ApplyGoalIssuesInput {
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
		let program = goal_execution_program(
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
		let rows = applied_goal_issue_rows(&plans, &issues, &linked_issues, &evaluation);

		state_store.upsert_execution_program(config.service_id(), program)?;

		rows
	} else {
		dry_run_goal_issue_rows(&plans, &linked_issues)
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
			let facts = issue_facts(tracker, workflow, issue, &active_label)?;

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
		counts,
		issues: rows,
	})
}
