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
	loop_contract::{DecisionContract, DecisionContractStatus},
	orchestrator,
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
	tracker::{
		self, IssueTracker, TrackerIssue, TrackerIssueBriefUpdate, TrackerIssueCreate,
		linear::LinearClient, public_text,
	},
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
	node_id: String,
	title: String,
	objective: String,
	description: String,
	dependencies: Vec<String>,
	conflict_domains: Vec<ExecutionConflictDomain>,
	acceptance: Vec<String>,
	validation: Vec<String>,
}

struct GoalIntakeAnchor {
	team_id: String,
	state_id: String,
}

struct GoalIssueBriefInput<'a> {
	contract: &'a DecisionContract,
	program_id: &'a str,
	node_id: &'a str,
	objective: &'a str,
	dependencies: &'a [String],
	conflict_domains: &'a [ExecutionConflictDomain],
	acceptance: &'a [String],
	validation: &'a [String],
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

/// Render a compact human-readable intake report.
pub(crate) fn render_issue_batch_intake_report(report: &IssueBatchIntakeReport) -> String {
	let mode = if report.persisted { "apply" } else { "dry-run" };
	let mut output = format!(
		"program intake {mode}: service={} program={} ready={} held={} blocked={} stale={} unmapped={}\n",
		report.service_id,
		report.program_id,
		report.counts.ready,
		report.counts.held,
		report.counts.blocked,
		report.counts.stale,
		report.counts.unmapped,
	);

	for row in &report.issues {
		let state = row.issue_state.as_deref().unwrap_or("unmapped");
		let action = row.dispatch_action.as_deref().unwrap_or("none");
		let reasons =
			if row.reasons.is_empty() { String::from("none") } else { row.reasons.join("; ") };

		output.push_str(&format!(
			"- {} classification={} state={} dispatch_action={} reasons={}\n",
			row.issue_identifier,
			row.classification.as_str(),
			state,
			action,
			reasons
		));
	}

	output
}

/// Render a compact human-readable promoted-goal intake report.
pub(crate) fn render_goal_intake_report(report: &GoalIntakeReport) -> String {
	let mode = if report.applied { "apply" } else { "dry-run" };
	let mut output = format!(
		"goal intake {mode}: service={} contract={} program={} issues={} persisted={}\n",
		report.service_id,
		report.contract_id,
		report.program_id,
		report.issues.len(),
		report.persisted,
	);

	for row in &report.issues {
		let issue = row.issue_identifier.as_deref().unwrap_or("new");
		let dispatch_action = row.dispatch_action.as_deref().unwrap_or("none");
		let dependencies = list_or_none(&row.dependencies);
		let conflicts = list_or_none(&row.conflict_domains);
		let reasons = list_or_none(&row.reasons);

		output.push_str(&format!(
			"- {} action={} issue={} queue_intent={} dispatch_action={} dependencies={} conflict_domains={} reasons={}\n",
			row.node_id,
			row.action.as_str(),
			issue,
			row.queue_intent,
			dispatch_action,
			dependencies,
			conflicts,
			reasons,
		));
	}

	output
}

fn ensure_goal_intake_authority(contract: &DecisionContract) -> Result<()> {
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
	if contract.execution_readiness().proposed_issue_summaries().is_empty() {
		eyre::bail!(
			"Decision Contract `{}` has no proposed issue summaries to materialize.",
			contract.contract_id()
		);
	}

	Ok(())
}

fn goal_issue_plans(contract: &DecisionContract, program_id: &str) -> Result<Vec<GoalIssuePlan>> {
	let conflict_domains = goal_conflict_domains(contract)?;
	let mut plans = Vec::new();

	for (index, objective) in
		contract.execution_readiness().proposed_issue_summaries().iter().enumerate()
	{
		let node_id = goal_node_id(contract.contract_id(), index, objective);
		let title = goal_issue_title(objective);
		let acceptance = goal_acceptance(contract, objective);
		let validation = goal_validation(contract);
		let dependencies = Vec::new();
		let description = render_goal_issue_brief(GoalIssueBriefInput {
			contract,
			program_id,
			node_id: &node_id,
			objective,
			dependencies: &dependencies,
			conflict_domains: &conflict_domains,
			acceptance: &acceptance,
			validation: &validation,
		})?;

		validate_generated_issue_text(&title, &description)?;

		plans.push(GoalIssuePlan {
			node_id,
			title,
			objective: objective.clone(),
			description,
			dependencies,
			conflict_domains: conflict_domains.clone(),
			acceptance,
			validation,
		});
	}

	Ok(plans)
}

fn linked_goal_issues<T>(
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

fn goal_intake_anchor<T>(
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

fn apply_goal_issues_and_link_contract<T>(
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

fn linked_goal_contract_for_apply_progress(
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

fn goal_execution_program(
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

fn goal_program_node(
	service_id: &str,
	contract: &DecisionContract,
	plan: &GoalIssuePlan,
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> Result<ExecutionProgramNode> {
	let dependencies = plan
		.dependencies
		.iter()
		.map(ExecutionProgramDependency::new)
		.collect::<Result<Vec<_>>>()?;
	let mapping = goal_issue_mapping(service_id, issue, workflow)?;

	ExecutionProgramNode::new(
		plan.node_id.clone(),
		ExecutionProgramNodeStage::Runtime,
		plan.objective.clone(),
		ExecutionQueueIntent::ReadyToQueue,
	)?
	.with_objective_lineage(goal_objective_lineage(contract))?
	.with_dependencies(dependencies)?
	.with_conflict_domains(plan.conflict_domains.clone())?
	.with_acceptance_expectations(plan.acceptance.clone())?
	.with_validation_expectations(plan.validation.clone())?
	.with_linear_issue(mapping)
}

fn goal_issue_mapping(
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
		.with_generic_dispatch_briefing(issue_has_generic_dispatch_briefing(issue)))
}

fn applied_goal_issue_rows(
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

fn dry_run_goal_issue_rows(
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
				GoalIntakeIssueAction::WouldCreate =>
					"apply will create a normal Linear issue and persist a mapped program node",
				GoalIntakeIssueAction::WouldUpdate =>
					"apply will update the linked normal Linear issue and persist a mapped program node",
				GoalIntakeIssueAction::Created | GoalIntakeIssueAction::Updated =>
					"apply already materialized this issue",
			};

			goal_issue_report_row(plan, linked.as_ref(), action, None, vec![reason.to_owned()])
		})
		.collect()
}

fn goal_issue_report_row(
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
		queue_intent: ExecutionQueueIntent::ReadyToQueue.as_str().to_owned(),
		dispatch_action: dispatch_action.map(dispatch_action_name),
		dependencies: plan.dependencies.clone(),
		conflict_domains: conflict_domain_labels(&plan.conflict_domains),
		acceptance: plan.acceptance.clone(),
		validation: plan.validation.clone(),
		reasons,
	}
}

fn render_goal_issue_brief(input: GoalIssueBriefInput<'_>) -> Result<String> {
	let mut output = String::new();

	append_heading(&mut output, "Objective");

	output.push_str(input.objective.trim());
	output.push('\n');

	append_heading(&mut output, "Authority");
	append_item(
		&mut output,
		&format!("Accepted Decision Contract: `{}`", input.contract.contract_id()),
	);
	append_item(&mut output, &format!("Execution Program: `{}`", input.program_id));
	append_item(&mut output, &format!("Execution Program node: `{}`", input.node_id));
	append_optional_item(
		&mut output,
		"Source issue",
		input.contract.source_intent().source_issue_identifier(),
	);
	append_heading(&mut output, "Scope");
	append_item(&mut output, input.objective.trim());
	append_items(&mut output, input.contract.accepted_authority().accepted_objectives());
	append_items(&mut output, input.contract.accepted_authority().constraints());
	append_items(&mut output, input.contract.accepted_authority().assumptions());
	append_heading(&mut output, "Non-goals");
	append_items_or_none(&mut output, input.contract.accepted_authority().non_goals());
	append_heading(&mut output, "Dependencies");
	append_items_or_none(&mut output, input.dependencies);
	append_heading(&mut output, "Conflict Domains");
	append_items_or_none(&mut output, &conflict_domain_labels(input.conflict_domains));
	append_heading(&mut output, "Acceptance");
	append_items(&mut output, input.acceptance);
	append_heading(&mut output, "Validation");
	append_items(&mut output, input.validation);
	append_heading(&mut output, "Stop Conditions");
	append_items_or_none(&mut output, input.contract.accepted_authority().stop_conditions());
	validate_public_issue_description(&output)?;

	Ok(output)
}

fn append_heading(output: &mut String, heading: &str) {
	if !output.is_empty() {
		output.push('\n');
	}

	output.push_str("## ");
	output.push_str(heading);
	output.push('\n');
}

fn append_item(output: &mut String, item: &str) {
	output.push_str("- ");
	output.push_str(item);
	output.push('\n');
}

fn append_optional_item(output: &mut String, label: &str, value: Option<&str>) {
	if let Some(value) = value {
		append_item(output, &format!("{label}: `{value}`"));
	}
}

fn append_items(output: &mut String, items: &[String]) {
	for item in items {
		append_item(output, item);
	}
}

fn append_items_or_none(output: &mut String, items: &[String]) {
	if items.is_empty() {
		append_item(output, "None declared by the accepted Decision Contract.");
	} else {
		append_items(output, items);
	}
}

fn validate_generated_issue_text(title: &str, description: &str) -> Result<()> {
	public_text::validate_public_text_field("generated issue title", title)
		.map_err(|error| eyre::eyre!(error))?;

	validate_public_issue_description(description)
}

fn validate_public_issue_description(description: &str) -> Result<()> {
	public_text::validate_public_text_field("generated issue description", description)
		.map_err(|error| eyre::eyre!(error))
}

fn goal_acceptance(contract: &DecisionContract, objective: &str) -> Vec<String> {
	let mut acceptance = vec![format!("Deliver this generated issue objective: {objective}")];

	acceptance.extend(contract.accepted_authority().accepted_objectives().iter().cloned());

	acceptance
}

fn goal_validation(contract: &DecisionContract) -> Vec<String> {
	if contract.execution_readiness().validation_expectations().is_empty() {
		return vec![String::from("Run the registered project validation before review handoff.")];
	}

	contract.execution_readiness().validation_expectations().to_vec()
}

fn goal_objective_lineage(contract: &DecisionContract) -> Vec<String> {
	let mut lineage = vec![
		format!("Accepted Decision Contract `{}`.", contract.contract_id()),
		format!("Source intent: {}", contract.source_intent().summary()),
	];

	lineage.extend(contract.accepted_authority().accepted_objectives().iter().cloned());

	lineage
}

fn goal_conflict_domains(contract: &DecisionContract) -> Result<Vec<ExecutionConflictDomain>> {
	contract
		.execution_readiness()
		.conflict_domains()
		.iter()
		.map(|domain| parse_goal_conflict_domain(domain))
		.collect()
}

fn parse_goal_conflict_domain(domain: &str) -> Result<ExecutionConflictDomain> {
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

fn conflict_domain_labels(domains: &[ExecutionConflictDomain]) -> Vec<String> {
	let mut labels = domains
		.iter()
		.map(|domain| format!("{}:{}", domain.kind().as_str(), domain.key()))
		.collect::<Vec<_>>();

	labels.sort();
	labels.dedup();

	labels
}

fn goal_program_id(service_id: &str, contract_id: &str) -> String {
	format!("goal-{service_id}-{}", stable_slug(contract_id, 48))
}

fn goal_node_id(contract_id: &str, index: usize, objective: &str) -> String {
	format!("goal:{}:{:02}-{}", stable_slug(contract_id, 32), index + 1, stable_slug(objective, 32))
}

fn goal_issue_title(objective: &str) -> String {
	let objective = objective.trim();

	if objective.chars().count() <= 120 {
		return objective.to_owned();
	}

	let mut title = objective.chars().take(117).collect::<String>();

	title.push_str("...");

	title
}

fn stable_slug(value: &str, max_len: usize) -> String {
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

fn list_or_none(values: &[String]) -> String {
	if values.is_empty() { String::from("none") } else { values.join(", ") }
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

fn register_intake_project_config_for_persist(
	state_store: &StateStore,
	config_path: &Path,
	persist: bool,
) -> Result<()> {
	if persist {
		runtime::register_project_config(state_store, config_path, true)?;
	}

	Ok(())
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
		format!("Resolve supplied Linear issue identifier `{identifier}` before dispatch."),
		ExecutionQueueIntent::NotReady,
	)?
	.with_acceptance_expectations([format!(
		"`{identifier}` maps to a normal Linear issue before execution."
	)])?
	.with_validation_expectations([String::from("Tracker lookup succeeds before queue intent.")])
}

fn issue_facts<T>(
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

fn issue_node(
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

fn issue_dependencies(
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

fn dependency_snapshots_for(
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

	domains.sort_by(|left, right| {
		left.kind().as_str().cmp(right.kind().as_str()).then_with(|| left.key().cmp(right.key()))
	});

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

fn classify_issue(
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
		| ExecutionProgramNodeLifecycleState::Active => IssueBatchIntakeClassification::Held,
		ExecutionProgramNodeLifecycleState::Blocked
		| ExecutionProgramNodeLifecycleState::NeedsAttention => IssueBatchIntakeClassification::Blocked,
		ExecutionProgramNodeLifecycleState::Completed
		| ExecutionProgramNodeLifecycleState::Stale
		| ExecutionProgramNodeLifecycleState::Superseded => IssueBatchIntakeClassification::Stale,
	}
}

fn dispatch_action_name(action: ExecutionDispatchAction) -> String {
	match action {
		ExecutionDispatchAction::Dispatch => "dispatch",
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
		dispatch_action: None,
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
	use std::{cell::RefCell, collections::HashMap, fs, path::Path};

	use tempfile::TempDir;

	use crate::{
		loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind},
		prelude::eyre,
		program_intake::{
			self, GoalIntakeIssueAction, GoalIntakeRunRequest, IssueBatchIntakeClassification,
		},
		state::StateStore,
		tracker::{
			IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBlocker,
			TrackerIssueBriefUpdate, TrackerIssueCreate, TrackerLabel, TrackerState, TrackerTeam,
		},
		workflow::WorkflowDocument,
	};

	trait TestIssueExt {
		fn with_blocker(self, identifier: &str, state: &str) -> Self;
		fn with_label(self, name: &str) -> Self;
	}

	#[derive(Default)]
	struct FakeTracker {
		issues: RefCell<HashMap<String, TrackerIssue>>,
		next_issue_number: RefCell<usize>,
		created_issues: RefCell<Vec<TrackerIssue>>,
		updated_issues: RefCell<Vec<TrackerIssue>>,
		fail_create_after_successes: RefCell<Option<usize>>,
		fail_update_after_successes: RefCell<Option<usize>>,
	}

	impl FakeTracker {
		fn with_issues(self, issues: impl IntoIterator<Item = TrackerIssue>) -> Self {
			for issue in issues {
				self.issues.borrow_mut().insert(issue.identifier.clone(), issue);
			}

			self
		}

		fn with_create_failure_after_successes(self, successes: usize) -> Self {
			*self.fail_create_after_successes.borrow_mut() = Some(successes);

			self
		}

		fn with_update_failure_after_successes(self, successes: usize) -> Self {
			*self.fail_update_after_successes.borrow_mut() = Some(successes);

			self
		}

		fn created_issue_count(&self) -> usize {
			self.created_issues.borrow().len()
		}

		fn updated_issue_count(&self) -> usize {
			self.updated_issues.borrow().len()
		}

		fn generated_issue_identifier(&self, index: usize) -> String {
			format!("XY-G{}", index + 1)
		}
	}

	impl IssueTracker for FakeTracker {
		fn list_issues_with_label(
			&self,
			label_name: &str,
		) -> crate::prelude::Result<Vec<TrackerIssue>> {
			Ok(self
				.issues
				.borrow()
				.values()
				.filter(|issue| issue.has_label(label_name))
				.cloned()
				.collect())
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
			Ok(self.issues.borrow().get(issue_identifier).cloned())
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

		fn create_issue(
			&self,
			request: &TrackerIssueCreate,
		) -> crate::prelude::Result<TrackerIssue> {
			if let Some(success_limit) = *self.fail_create_after_successes.borrow()
				&& self.created_issues.borrow().len() >= success_limit
			{
				eyre::bail!("injected create failure after {success_limit} successes");
			}

			let identifier = loop {
				let mut next_issue_number = self.next_issue_number.borrow_mut();

				*next_issue_number += 1;

				let candidate = self.generated_issue_identifier(*next_issue_number - 1);

				if !self.issues.borrow().contains_key(&candidate) {
					break candidate;
				}
			};
			let state_name = request
				.state_id
				.as_deref()
				.and_then(|state_id| state_id.strip_prefix("state-"))
				.unwrap_or("Todo");
			let mut issue = issue(&identifier, state_name);

			issue.id = format!("id-{identifier}");

			issue.title.clone_from(&request.title);
			issue.description.clone_from(&request.description);
			issue.team.id.clone_from(&request.team_id);
			self.issues.borrow_mut().insert(identifier, issue.clone());
			self.created_issues.borrow_mut().push(issue.clone());

			Ok(issue)
		}

		fn update_issue_brief(
			&self,
			issue_id: &str,
			request: &TrackerIssueBriefUpdate,
		) -> crate::prelude::Result<TrackerIssue> {
			if let Some(success_limit) = *self.fail_update_after_successes.borrow()
				&& self.updated_issues.borrow().len() >= success_limit
			{
				eyre::bail!("injected update failure after {success_limit} successes");
			}

			let mut issues = self.issues.borrow_mut();
			let issue = issues
				.values_mut()
				.find(|issue| issue.id == issue_id)
				.ok_or_else(|| eyre::eyre!("issue `{issue_id}` not found"))?;

			issue.title.clone_from(&request.title);
			issue.description.clone_from(&request.description);

			let issue = issue.clone();

			self.updated_issues.borrow_mut().push(issue.clone());

			Ok(issue)
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

		fn with_label(mut self, name: &str) -> Self {
			self.labels.push(TrackerLabel { id: format!("label-{name}"), name: name.to_owned() });

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
			issue("XY-4", "Todo")
				.with_blocker("XY-20", "Todo")
				.with_blocker("XY-10", "Todo")
				.with_label("repo:zeta")
				.with_label("repo:alpha"),
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

		let blocked = report
			.issues
			.iter()
			.find(|issue| issue.issue_identifier == "XY-4")
			.expect("blocked issue should be reported");

		assert_eq!(blocked.blockers, vec![String::from("XY-10"), String::from("XY-20")]);
		assert_eq!(
			blocked.conflict_domains,
			vec![
				String::from("module:alpha"),
				String::from("module:zeta"),
				String::from("tracker_ownership:XY-4"),
			]
		);
		assert!(
			store.list_execution_programs("decodex").expect("program list should read").is_empty()
		);
	}

	#[test]
	fn project_registration_is_persist_only_for_command_path() {
		let store = StateStore::open_in_memory().expect("store should open");
		let temp_dir = TempDir::new().expect("temp dir should create");
		let config_path = write_project_files(temp_dir.path());

		program_intake::register_intake_project_config_for_persist(&store, &config_path, false)
			.expect("dry-run registration should no-op");

		assert!(store.list_projects().expect("projects should list").is_empty());

		program_intake::register_intake_project_config_for_persist(&store, &config_path, true)
			.expect("persist registration should write");

		let projects = store.list_projects().expect("projects should list");

		assert_eq!(projects.len(), 1);
		assert_eq!(projects[0].service_id(), "decodex");
		assert!(projects[0].enabled());
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

	#[test]
	fn goal_intake_dry_run_shows_issue_split_without_mutation() {
		let store = StateStore::open_in_memory().expect("store should open");
		let contract = accepted_goal_contract();

		store
			.upsert_decision_contract("decodex", Some("XY-852"), contract)
			.expect("contract should persist");

		let tracker = FakeTracker::default().with_issues([issue("XY-852", "Todo")]);
		let config = test_config();
		let workflow = workflow();
		let report = program_intake::run_goal_intake(GoalIntakeRunRequest {
			state_store: &store,
			tracker: &tracker,
			config: &config,
			workflow: &workflow,
			contract_id: "goal-intake-contract",
			team_issue_identifier: None,
			dry_run: true,
			apply: false,
		})
		.expect("dry-run should produce materialization plan");

		assert!(report.dry_run);
		assert!(!report.persisted);
		assert_eq!(report.issues.len(), 2);
		assert_eq!(report.issues[0].action, GoalIntakeIssueAction::WouldCreate);
		assert_eq!(report.issues[0].dependencies, Vec::<String>::new());
		assert_eq!(
			report.issues[0].conflict_domains,
			vec![String::from("file:docs/spec/loop-runtime.md"), String::from("module:runtime"),]
		);
		assert_eq!(tracker.created_issue_count(), 0);
		assert!(store.list_execution_programs("decodex").expect("programs").is_empty());

		let rendered = program_intake::render_goal_intake_report(&report);

		assert!(rendered.contains("dependencies=none"));
		assert!(
			rendered.contains("conflict_domains=file:docs/spec/loop-runtime.md, module:runtime")
		);
	}

	#[test]
	fn goal_intake_refuses_latent_or_missing_decision_authority() {
		let store = StateStore::open_in_memory().expect("store should open");
		let tracker = FakeTracker::default().with_issues([issue("XY-852", "Todo")]);
		let latent = latent_goal_contract();

		store
			.upsert_decision_contract("decodex", Some("XY-852"), latent)
			.expect("latent contract should persist");

		let config = test_config();
		let workflow = workflow();
		let latent_error = program_intake::run_goal_intake(GoalIntakeRunRequest {
			state_store: &store,
			tracker: &tracker,
			config: &config,
			workflow: &workflow,
			contract_id: "goal-intake-contract",
			team_issue_identifier: None,
			dry_run: false,
			apply: true,
		})
		.expect_err("latent contract must not materialize");

		assert!(latent_error.to_string().contains("requires accepted execution authority"));

		let mut needs_decision = latent_goal_contract();

		needs_decision
			.require_human_decision("Choose the public issue split before apply.")
			.expect("contract should record missing decision");
		store
			.upsert_decision_contract("decodex", Some("XY-852"), needs_decision)
			.expect("needs-decision contract should persist");

		let missing_decision_error = program_intake::run_goal_intake(GoalIntakeRunRequest {
			state_store: &store,
			tracker: &tracker,
			config: &config,
			workflow: &workflow,
			contract_id: "goal-intake-contract",
			team_issue_identifier: None,
			dry_run: false,
			apply: true,
		})
		.expect_err("missing decision must stop apply");

		assert!(missing_decision_error.to_string().contains("needs_human_decision"));
		assert_eq!(tracker.created_issue_count(), 0);
		assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
	}

	#[test]
	fn goal_intake_apply_creates_updates_and_persists_links() {
		let store = StateStore::open_in_memory().expect("store should open");
		let mut contract = accepted_goal_contract();

		contract
			.link_generated_execution_surfaces(["id-XY-G1"], ["XY-G1"], ["old-node"])
			.expect("existing generated link should attach");
		store
			.upsert_decision_contract("decodex", Some("XY-852"), contract)
			.expect("contract should persist");

		let tracker =
			FakeTracker::default().with_issues([issue("XY-852", "Todo"), issue("XY-G1", "Todo")]);
		let config = test_config();
		let workflow = workflow();
		let report = program_intake::run_goal_intake(GoalIntakeRunRequest {
			state_store: &store,
			tracker: &tracker,
			config: &config,
			workflow: &workflow,
			contract_id: "goal-intake-contract",
			team_issue_identifier: None,
			dry_run: false,
			apply: true,
		})
		.expect("apply should materialize issues and program");

		assert!(report.applied);
		assert!(report.persisted);
		assert_eq!(tracker.updated_issue_count(), 1);
		assert_eq!(tracker.created_issue_count(), 1);
		assert_eq!(report.issues[0].action, GoalIntakeIssueAction::Updated);
		assert_eq!(report.issues[1].action, GoalIntakeIssueAction::Created);
		assert_eq!(report.issues[0].dispatch_action.as_deref(), Some("dispatch"));
		assert_eq!(report.issues[1].dispatch_action.as_deref(), Some("dispatch"));

		let linked_contract = store
			.decision_contract("decodex", "goal-intake-contract")
			.expect("contract lookup should read")
			.expect("contract should exist");

		assert_eq!(
			linked_contract.contract().links().generated_issue_identifiers(),
			&[String::from("XY-G1"), String::from("XY-G2")]
		);
		assert_eq!(
			linked_contract.contract().links().execution_program_node_ids(),
			&report.issues.iter().map(|issue| issue.node_id.clone()).collect::<Vec<_>>()
		);

		let programs = store
			.list_execution_programs_for_contract("decodex", "goal-intake-contract")
			.expect("programs should list");

		assert_eq!(programs.len(), 1);
		assert_eq!(programs[0].program_id(), report.program_id);

		let intake_plans =
			store.list_program_intake_plans("decodex").expect("intake plans should list");

		assert_eq!(intake_plans.len(), 1);
		assert_eq!(intake_plans[0].intake_kind(), "goal_intake");
		assert_eq!(intake_plans[0].source_contract_id(), Some("goal-intake-contract"));

		let mappings = store
			.list_program_issue_mappings("decodex", &report.program_id)
			.expect("mappings should list");

		assert_eq!(mappings.len(), 2);

		let updated = tracker
			.get_issue_by_identifier("XY-G1")
			.expect("issue lookup should work")
			.expect("updated issue should exist");

		assert!(updated.description.contains("## Objective"));
		assert!(updated.description.contains("Accepted Decision Contract: `goal-intake-contract`"));
		assert!(updated.description.contains("Execution Program node:"));
		assert!(updated.description.contains("## Dependencies"));
		assert!(!updated.description.contains("```"));
		assert!(!updated.description.contains("private_evidence_refs"));
	}

	#[test]
	fn goal_intake_apply_persists_links_after_each_successful_issue_mutation() {
		let store = StateStore::open_in_memory().expect("store should open");
		let contract = accepted_goal_contract();

		store
			.upsert_decision_contract("decodex", Some("XY-852"), contract)
			.expect("contract should persist");

		let tracker = FakeTracker::default()
			.with_issues([issue("XY-852", "Todo")])
			.with_create_failure_after_successes(1);
		let config = test_config();
		let workflow = workflow();
		let error = program_intake::run_goal_intake(GoalIntakeRunRequest {
			state_store: &store,
			tracker: &tracker,
			config: &config,
			workflow: &workflow,
			contract_id: "goal-intake-contract",
			team_issue_identifier: None,
			dry_run: false,
			apply: true,
		})
		.expect_err("second issue create should fail");

		assert!(error.to_string().contains("injected create failure"));
		assert_eq!(tracker.created_issue_count(), 1);

		let linked_contract = store
			.decision_contract("decodex", "goal-intake-contract")
			.expect("contract lookup should read")
			.expect("contract should exist");

		assert_eq!(
			linked_contract.contract().links().generated_issue_identifiers(),
			&[String::from("XY-G1")]
		);
		assert_eq!(linked_contract.contract().links().generated_issue_ids().len(), 1);
		assert_eq!(linked_contract.contract().links().execution_program_node_ids().len(), 1);
		assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
	}

	#[test]
	fn goal_intake_apply_preserves_later_existing_links_after_update_failure() {
		let store = StateStore::open_in_memory().expect("store should open");
		let mut contract = accepted_goal_contract();

		contract
			.link_generated_execution_surfaces(
				["id-XY-G1", "id-XY-G2"],
				["XY-G1", "XY-G2"],
				["old-node-1", "old-node-2"],
			)
			.expect("existing generated links should attach");
		store
			.upsert_decision_contract("decodex", Some("XY-852"), contract)
			.expect("contract should persist");

		let tracker = FakeTracker::default()
			.with_issues([issue("XY-852", "Todo"), issue("XY-G1", "Todo"), issue("XY-G2", "Todo")])
			.with_update_failure_after_successes(1);
		let config = test_config();
		let workflow = workflow();
		let error = program_intake::run_goal_intake(GoalIntakeRunRequest {
			state_store: &store,
			tracker: &tracker,
			config: &config,
			workflow: &workflow,
			contract_id: "goal-intake-contract",
			team_issue_identifier: None,
			dry_run: false,
			apply: true,
		})
		.expect_err("second issue update should fail");

		assert!(error.to_string().contains("injected update failure"));
		assert_eq!(tracker.updated_issue_count(), 1);

		let linked_contract = store
			.decision_contract("decodex", "goal-intake-contract")
			.expect("contract lookup should read")
			.expect("contract should exist");

		assert_eq!(
			linked_contract.contract().links().generated_issue_identifiers(),
			&[String::from("XY-G1"), String::from("XY-G2")]
		);
		assert_eq!(linked_contract.contract().links().execution_program_node_ids().len(), 2);
		assert_eq!(
			linked_contract.contract().links().execution_program_node_ids()[1],
			"old-node-2"
		);
		assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
	}

	#[test]
	fn goal_intake_apply_fails_closed_when_existing_generated_link_is_missing() {
		let store = StateStore::open_in_memory().expect("store should open");
		let mut contract = accepted_goal_contract();

		contract
			.link_generated_execution_surfaces(["id-XY-G1"], ["XY-G1"], ["old-node"])
			.expect("existing generated link should attach");
		store
			.upsert_decision_contract("decodex", Some("XY-852"), contract)
			.expect("contract should persist");

		let tracker = FakeTracker::default().with_issues([issue("XY-852", "Todo")]);
		let config = test_config();
		let workflow = workflow();
		let error = program_intake::run_goal_intake(GoalIntakeRunRequest {
			state_store: &store,
			tracker: &tracker,
			config: &config,
			workflow: &workflow,
			contract_id: "goal-intake-contract",
			team_issue_identifier: None,
			dry_run: false,
			apply: true,
		})
		.expect_err("missing generated issue link should block apply");

		assert!(error.to_string().contains("Generated issue link `XY-G1`"));
		assert_eq!(tracker.created_issue_count(), 0);
		assert_eq!(tracker.updated_issue_count(), 0);
		assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
	}

	fn workflow() -> WorkflowDocument {
		WorkflowDocument::parse_markdown(workflow_markdown()).expect("workflow should parse")
	}

	fn latent_goal_contract() -> DecisionContract {
		serde_json::from_value(serde_json::json!({
			"schema": crate::loop_contract::DECISION_CONTRACT_SCHEMA,
			"record_version": crate::loop_contract::DECISION_CONTRACT_RECORD_VERSION,
			"contract_id": "goal-intake-contract",
			"status": "draft_latent",
			"source_intent": {
				"summary": "Ship promoted goal intake.",
				"user_utterance": "arrange this goal",
				"source_issue_identifier": "XY-852",
			},
			"research_provenance": [
				{
					"kind": "spec",
					"reference": "docs/spec/loop-runtime.md",
					"summary": "Promoted contracts can shape normal Linear issues."
				}
			],
			"research_evidence": [
				{
					"claim": "Goal intake needs generated issues and an internal program.",
					"support": "The loop-runtime spec defines Program Intake and Execution Program records.",
					"source_ref": "docs/spec/loop-runtime.md"
				}
			],
			"research_options": [],
			"accepted_authority": {
				"accepted_objectives": [
					"Materialize accepted goal intake into normal Linear issues.",
					"Persist the internal Execution Program without exposing graph mechanics."
				],
				"non_goals": [
					"Do not run implementation from goal intake."
				],
				"constraints": [
					"Linear receives only public-safe issue briefs and sparse links."
				],
				"assumptions": [
					"The source issue anchors the generated issue team."
				],
				"objections": [],
				"stop_conditions": [
					"Stop when promotion authority or required decisions are missing."
				]
			},
			"execution_readiness": {
				"summary": "Ready for issue shaping after promotion.",
				"ready_for_issue_shaping": true,
				"missing_decisions": [],
				"validation_expectations": [
					"Run cargo make test before handoff."
				],
				"risk_notes": [
					"Generated issue descriptions must stay natural-language."
				],
				"proposed_issue_summaries": [
					"Implement goal intake CLI/API behavior.",
					"Persist Execution Program links for generated issues."
				],
				"conflict_domains": [
					"module:runtime",
					"file:docs/spec/loop-runtime.md"
				],
				"queue_intent": [
					"ready_to_queue_after_apply"
				]
			},
			"links": {
				"generated_issue_ids": [],
				"generated_issue_identifiers": [],
				"execution_program_node_ids": []
			},
			"evidence_boundary": {
				"private_evidence_refs": [],
				"public_projection_refs": [],
				"public_summary": "Goal intake contract ready for issue shaping."
			}
		}))
		.expect("goal contract should deserialize")
	}

	fn accepted_goal_contract() -> DecisionContract {
		let mut contract = latent_goal_contract();

		contract
			.promote(
				DecisionPromotion::new(
					"operator",
					DecisionPromotionActorKind::User,
					"2026-06-10T00:00:00Z",
					"conversation",
					Some(String::from("User asked Decodex to arrange this goal.")),
				)
				.expect("promotion should build"),
			)
			.expect("contract should promote");

		contract
	}

	fn workflow_markdown() -> &'static str {
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
"#
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

	fn write_project_files(project_dir: &Path) -> std::path::PathBuf {
		fs::write(project_dir.join("WORKFLOW.md"), workflow_markdown())
			.expect("workflow should write");
		fs::write(
			project_dir.join("project.toml"),
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
		.expect("project config should write");

		project_dir.join("project.toml")
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
				states: vec![
					TrackerState { id: String::from("state-Todo"), name: String::from("Todo") },
					TrackerState {
						id: String::from("state-In Progress"),
						name: String::from("In Progress"),
					},
					TrackerState { id: String::from("state-Done"), name: String::from("Done") },
				],
				labels: Vec::new(),
			},
			labels_complete: true,
			labels: Vec::new(),
			blockers: Vec::new(),
		}
	}
}
