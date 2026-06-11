use crate::execution_program::{
	ExecutionDependencySnapshot, ExecutionLinearIssueMapping, ExecutionNodeEvaluation,
	ExecutionProgram, ExecutionQueueLabelAction,
};
use crate::execution_program::ExecutionConflictDomain;

#[derive(Clone)]
struct RefreshedExecutionProgram {
	record: ExecutionProgramRecord,
	program: ExecutionProgram,
	issues_by_node: std::collections::BTreeMap<String, ProgramIssueSnapshot>,
}

#[derive(Clone)]
struct ProgramIssueSnapshot {
	issue: TrackerIssue,
	has_queue_label: bool,
	queue_label_owned_by_current_program: bool,
	has_active_label: bool,
	has_opt_out_label: bool,
	has_needs_attention_label: bool,
	has_open_tracker_blockers: bool,
	has_generic_dispatch_briefing: bool,
}
impl ProgramIssueSnapshot {
	fn linear_mapping(
		&self,
		has_queue_label: bool,
		queue_label_program_owned: bool,
	) -> Result<ExecutionLinearIssueMapping> {
		let mut mapping = ExecutionLinearIssueMapping::new(
			&self.issue.id,
			&self.issue.identifier,
			&self.issue.state.name,
		)?;

		mapping = if queue_label_program_owned {
			mapping.with_program_owned_queue_label(true)
		} else {
			mapping.with_queue_label(has_queue_label)
		};

		Ok(mapping
			.with_active_label(self.has_active_label)
			.with_opt_out_label(self.has_opt_out_label)
			.with_needs_attention_label(self.has_needs_attention_label)
			.with_open_tracker_blockers(self.has_open_tracker_blockers)
			.with_generic_dispatch_briefing(self.has_generic_dispatch_briefing))
	}
}

struct ProgramIssueSnapshotInput<'a, T>
where
	T: IssueTracker + ?Sized,
{
	tracker: &'a T,
	service_id: &'a str,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	queue_label: &'a str,
	record: &'a ExecutionProgramRecord,
	node_id: &'a str,
	issue: &'a TrackerIssue,
}

#[derive(Default)]
struct ProgramReconciliationSummary {
	programs_evaluated: usize,
	programs_updated: usize,
	labels_applied: usize,
	labels_removed: usize,
	labels_retained: usize,
}
impl ProgramReconciliationSummary {
	fn label_mutation_count(&self) -> usize {
		self.labels_applied + self.labels_removed
	}
}

struct ProgramQueueLabelState {
	has_queue_label: bool,
	program_owned: bool,
}

fn reconcile_execution_program_queue_labels<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<ProgramReconciliationSummary>
where
	T: IssueTracker + ?Sized,
{
	let records = state_store.list_execution_programs(project.service_id())?;

	if records.is_empty() {
		return Ok(ProgramReconciliationSummary::default());
	}

	let policy = ExecutionWorkflowPolicy::from_workflow(project.service_id(), workflow)?;
	let refreshed_issues = refresh_execution_program_issues(tracker, &records)?;
	let refreshed_programs = records
		.into_iter()
		.map(|record| {
			refresh_execution_program_tracker_facts(
				tracker,
				project.service_id(),
				workflow,
				state_store,
				&policy,
				record,
				&refreshed_issues,
			)
		})
		.collect::<Result<Vec<_>>>()?;
	let context = execution_program_readiness_context(
		project.service_id(),
		workflow,
		state_store,
		&refreshed_programs,
	)?;
	let mut summary = ProgramReconciliationSummary::default();

	for refreshed in refreshed_programs {
		let evaluation = if let Some(source_contract_id) = refreshed.record.source_contract_id() {
			let Some(contract) =
				state_store.decision_contract(project.service_id(), source_contract_id)?
			else {
				continue;
			};

			refreshed.program.evaluate(contract.contract(), &policy, &context)?
		} else {
			refreshed.program.evaluate_issue_batch(&policy, &context)?
		};

		summary.programs_evaluated += 1;

		let final_program = apply_execution_program_queue_actions(
			tracker,
			project.service_id(),
			policy.queue_label(),
			refreshed.program,
			&refreshed.issues_by_node,
			evaluation.nodes(),
			&mut summary,
		)?;

		if final_program != *refreshed.record.program() {
			state_store.upsert_execution_program(project.service_id(), final_program)?;

			summary.programs_updated += 1;
		}
	}

	Ok(summary)
}

fn refresh_execution_program_issues<T>(
	tracker: &T,
	records: &[ExecutionProgramRecord],
) -> Result<std::collections::BTreeMap<String, TrackerIssue>>
where
	T: IssueTracker + ?Sized,
{
	let issue_ids = records
		.iter()
		.flat_map(|record| record.program().nodes())
		.filter_map(|node| node.linear_issue().map(|issue| issue.issue_id().to_owned()))
		.collect::<std::collections::BTreeSet<_>>()
		.into_iter()
		.collect::<Vec<_>>();

	if issue_ids.is_empty() {
		return Ok(std::collections::BTreeMap::new());
	}

	Ok(tracker
		.refresh_issues(&issue_ids)?
		.into_iter()
		.map(|issue| (issue.id.clone(), issue))
		.collect())
}

fn refresh_execution_program_tracker_facts<T>(
	tracker: &T,
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	policy: &ExecutionWorkflowPolicy,
	record: ExecutionProgramRecord,
	refreshed_issues: &std::collections::BTreeMap<String, TrackerIssue>,
) -> Result<RefreshedExecutionProgram>
where
	T: IssueTracker + ?Sized,
{
	let mut refreshed_nodes = Vec::with_capacity(record.program().nodes().len());
	let mut issues_by_node = std::collections::BTreeMap::new();

	for node in record.program().nodes() {
		let Some(mapping) = node.linear_issue() else {
			refreshed_nodes.push(node.clone());

			continue;
		};
		let Some(issue) = refreshed_issues.get(mapping.issue_id()) else {
			refreshed_nodes.push(node.clone());

			continue;
		};
		let snapshot = program_issue_snapshot(ProgramIssueSnapshotInput {
			tracker,
			service_id,
			workflow,
			state_store,
			queue_label: policy.queue_label(),
			record: &record,
			node_id: node.node_id(),
			issue,
		})?;
		let mapping =
			snapshot.linear_mapping(snapshot.has_queue_label, snapshot.queue_label_owned_by_current_program)?;

		refreshed_nodes.push(node.clone().with_linear_issue(mapping)?);
		issues_by_node.insert(node.node_id().to_owned(), snapshot);
	}

	let program = record.program().clone().with_nodes(refreshed_nodes)?;

	Ok(RefreshedExecutionProgram { record, program, issues_by_node })
}

fn program_issue_snapshot<T>(input: ProgramIssueSnapshotInput<'_, T>) -> Result<ProgramIssueSnapshot>
where
	T: IssueTracker + ?Sized,
{
	let ProgramIssueSnapshotInput {
		tracker,
		service_id,
		workflow,
		state_store,
		queue_label,
		record,
		node_id,
		issue,
	} = input;
	let tracker_policy = workflow.frontmatter().tracker();
	let has_queue_label =
		tracker::issue_has_label_with_server_confirmation(tracker, issue, queue_label)?;
	let ownership = state_store.program_queue_label_ownership_for_issue(
		service_id,
		&issue.id,
		queue_label,
	)?;
	let queue_label_owned_by_current_program = has_queue_label
		&& ownership.iter().any(|recorded| {
			recorded.service_id() == service_id
				&& recorded.program_id() == record.program_id()
				&& recorded.node_id() == node_id
				&& recorded.issue_id() == issue.id
				&& recorded.issue_identifier() == issue.identifier
				&& recorded.label_name() == queue_label
		});
	let has_active_label = tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		&tracker::automation_active_label(service_id),
	)?;
	let has_opt_out_label =
		tracker::issue_has_label_with_server_confirmation(tracker, issue, tracker_policy.opt_out_label())?;
	let has_needs_attention_label = tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		tracker_policy.needs_attention_label(),
	)?;
	let has_open_tracker_blockers =
		issue.blockers.iter().any(|blocker| !state_name_is_terminal(&blocker.state.name, workflow));

	Ok(ProgramIssueSnapshot {
		issue: issue.clone(),
		has_queue_label,
		queue_label_owned_by_current_program,
		has_active_label,
		has_opt_out_label,
		has_needs_attention_label,
		has_open_tracker_blockers,
		has_generic_dispatch_briefing: issue_has_generic_dispatch_briefing(issue),
	})
}

fn execution_program_readiness_context(
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	programs: &[RefreshedExecutionProgram],
) -> Result<ExecutionProgramReadinessContext> {
	let dependency_snapshots = execution_program_dependency_snapshots(programs)?;
	let occupied_conflict_domains =
		execution_program_occupied_conflict_domains(service_id, workflow, state_store, programs)?;

	Ok(ExecutionProgramReadinessContext::new()
		.with_dependency_snapshots(dependency_snapshots)
		.with_occupied_conflict_domains(occupied_conflict_domains))
}

fn execution_program_dependency_snapshots(
	programs: &[RefreshedExecutionProgram],
) -> Result<Vec<ExecutionDependencySnapshot>> {
	let mut snapshots = std::collections::BTreeMap::new();

	for refreshed in programs {
		for node in refreshed.program.nodes() {
			let Some(snapshot) = refreshed.issues_by_node.get(node.node_id()) else {
				continue;
			};

			insert_dependency_snapshot(&mut snapshots, node.node_id(), &snapshot.issue.state.name)?;
			insert_dependency_snapshot(
				&mut snapshots,
				&snapshot.issue.identifier,
				&snapshot.issue.state.name,
			)?;

			for blocker in &snapshot.issue.blockers {
				insert_dependency_snapshot(&mut snapshots, &blocker.identifier, &blocker.state.name)?;
			}
		}
	}

	Ok(snapshots.into_values().collect())
}

fn insert_dependency_snapshot(
	snapshots: &mut std::collections::BTreeMap<String, ExecutionDependencySnapshot>,
	dependency_id: &str,
	state: &str,
) -> Result<()> {
	if snapshots.contains_key(dependency_id) {
		return Ok(());
	}

	snapshots.insert(
		dependency_id.to_owned(),
		ExecutionDependencySnapshot::tracker_state(dependency_id, state)?,
	);

	Ok(())
}

fn execution_program_occupied_conflict_domains(
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	programs: &[RefreshedExecutionProgram],
) -> Result<Vec<ExecutionConflictDomain>> {
	let retained_issue_ids = state_store
		.list_worktrees(service_id)?
		.into_iter()
		.map(|worktree| worktree.issue_id().to_owned())
		.collect::<std::collections::BTreeSet<_>>();
	let mut occupied = Vec::new();
	let mut seen = std::collections::BTreeSet::new();

	for refreshed in programs {
		for node in refreshed.program.nodes() {
			let Some(snapshot) = refreshed.issues_by_node.get(node.node_id()) else {
				continue;
			};

			if !program_issue_occupies_conflict_domain(
				service_id,
				workflow,
				state_store,
				&retained_issue_ids,
				snapshot,
			)? {
				continue;
			}

			for domain in node.conflict_domains() {
				let key = format!("{}:{}", domain.kind().as_str(), domain.key());

				if seen.insert(key) {
					occupied.push(domain.clone());
				}
			}
		}
	}

	Ok(occupied)
}

fn program_issue_occupies_conflict_domain(
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	retained_issue_ids: &std::collections::BTreeSet<String>,
	snapshot: &ProgramIssueSnapshot,
) -> Result<bool> {
	let issue = &snapshot.issue;
	let retained_nonterminal =
		retained_issue_ids.contains(&issue.id) && !state_name_is_terminal(&issue.state.name, workflow);

	Ok(snapshot.has_active_label
		|| snapshot.has_needs_attention_label
		|| retained_nonterminal
		|| state_store.issue_has_active_shared_claim(service_id, &issue.id)?)
}

fn apply_execution_program_queue_actions<T>(
	tracker: &T,
	service_id: &str,
	queue_label: &str,
	program: ExecutionProgram,
	issues_by_node: &std::collections::BTreeMap<String, ProgramIssueSnapshot>,
	evaluations: &[ExecutionNodeEvaluation],
	summary: &mut ProgramReconciliationSummary,
) -> Result<ExecutionProgram>
where
	T: IssueTracker + ?Sized,
{
	let evaluations_by_node = evaluations
		.iter()
		.map(|evaluation| (evaluation.node_id().to_owned(), evaluation))
		.collect::<std::collections::BTreeMap<_, _>>();
	let mut final_nodes = Vec::with_capacity(program.nodes().len());

	for node in program.nodes() {
		let (Some(snapshot), Some(evaluation)) = (
			issues_by_node.get(node.node_id()),
			evaluations_by_node.get(node.node_id()),
		) else {
			final_nodes.push(node.clone());

			continue;
		};
		let label_state = apply_execution_program_node_queue_action(
			tracker,
			service_id,
			queue_label,
			snapshot,
			evaluation.queue_label_action(),
			summary,
		)?;
		let mapping =
			snapshot.linear_mapping(label_state.has_queue_label, label_state.program_owned)?;

		final_nodes.push(node.clone().with_linear_issue(mapping)?);
	}

	program.with_nodes(final_nodes)
}

fn apply_execution_program_node_queue_action<T>(
	tracker: &T,
	service_id: &str,
	queue_label: &str,
	snapshot: &ProgramIssueSnapshot,
	action: Option<ExecutionQueueLabelAction>,
	summary: &mut ProgramReconciliationSummary,
) -> Result<ProgramQueueLabelState>
where
	T: IssueTracker + ?Sized,
{
	match action {
		Some(ExecutionQueueLabelAction::Apply) => {
			if tracker::set_issue_label_presence(tracker, &snapshot.issue, queue_label, true)? {
				summary.labels_applied += 1;
			}

			Ok(ProgramQueueLabelState { has_queue_label: true, program_owned: true })
		},
		Some(ExecutionQueueLabelAction::Retain) => {
			summary.labels_retained += 1;

			Ok(ProgramQueueLabelState { has_queue_label: true, program_owned: true })
		},
		Some(ExecutionQueueLabelAction::Remove) => {
			if snapshot.queue_label_owned_by_current_program
				&& tracker::set_issue_label_presence(tracker, &snapshot.issue, queue_label, false)?
			{
				summary.labels_removed += 1;
			}

			Ok(ProgramQueueLabelState { has_queue_label: false, program_owned: false })
		},
		None => {
			if snapshot.has_queue_label && snapshot.queue_label_owned_by_current_program {
				tracing::debug!(
					project_id = service_id,
					issue = snapshot.issue.identifier,
					"Retained program-owned queue label without a readiness action."
				);
			}

			Ok(ProgramQueueLabelState {
				has_queue_label: snapshot.has_queue_label,
				program_owned: snapshot.queue_label_owned_by_current_program,
			})
		},
	}
}
