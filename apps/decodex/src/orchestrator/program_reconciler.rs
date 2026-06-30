use crate::execution_program::{
	ExecutionDependencySnapshot, ExecutionLinearIssueMapping, ExecutionProgram,
	ExecutionProgramNode, ExecutionReadinessState,
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
	has_active_label: bool,
	has_opt_out_label: bool,
	has_needs_attention_label: bool,
	has_open_tracker_blockers: bool,
	has_generic_dispatch_briefing: bool,
	has_post_review_lifecycle: bool,
}
impl ProgramIssueSnapshot {
	fn linear_mapping(&self) -> Result<ExecutionLinearIssueMapping> {
		Ok(ExecutionLinearIssueMapping::new(
			&self.issue.id,
			&self.issue.identifier,
			&self.issue.state.name,
		)?
		.with_active_label(self.has_active_label)
			.with_opt_out_label(self.has_opt_out_label)
			.with_needs_attention_label(self.has_needs_attention_label)
			.with_open_tracker_blockers(self.has_open_tracker_blockers)
			.with_generic_dispatch_briefing(self.has_generic_dispatch_briefing)
			.with_post_review_lifecycle(self.has_post_review_lifecycle))
	}
}

struct ProgramIssueSnapshotInput<'a, T>
where
	T: IssueTracker + ?Sized,
{
	tracker: &'a T,
	state_store: &'a StateStore,
	service_id: &'a str,
	workflow: &'a WorkflowDocument,
	issue: &'a TrackerIssue,
}

#[derive(Default)]
struct ProgramSchedulerSummary {
	programs_evaluated: usize,
	programs_updated: usize,
	dispatchable_nodes: usize,
}

struct ProgramSchedulerSelection {
	selected: Option<SelectedIssueRunCandidate>,
	summary: ProgramSchedulerSummary,
}

fn select_execution_program_run_candidate<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	excluded_issue_ids: &[&str],
) -> Result<Option<SelectedIssueRunCandidate>>
where
	T: IssueTracker + ?Sized,
{
	let ProgramSchedulerSelection { selected, summary } =
		select_execution_program_run_candidate_with_summary(
			tracker,
			project,
			workflow,
			state_store,
			excluded_issue_ids,
		)?;

	if summary.dispatchable_nodes > 0 || summary.programs_updated > 0 {
		tracing::info!(
			project_id = project.service_id(),
			programs_evaluated = summary.programs_evaluated,
			programs_updated = summary.programs_updated,
			dispatchable_nodes = summary.dispatchable_nodes,
			"Evaluated Execution Programs for direct graph dispatch."
		);
	}

	Ok(selected)
}

fn select_execution_program_run_candidate_with_summary<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	excluded_issue_ids: &[&str],
) -> Result<ProgramSchedulerSelection>
where
	T: IssueTracker + ?Sized,
{
	let records = state_store.list_execution_programs(project.service_id())?;

	if records.is_empty() {
		return Ok(ProgramSchedulerSelection {
			selected: None,
			summary: ProgramSchedulerSummary::default(),
		});
	}

	let policy = ExecutionWorkflowPolicy::from_workflow(project.service_id(), workflow)?;
	let refreshed_issues = refresh_execution_program_issues(tracker, &records)?;
	let refreshed_programs = records
		.into_iter()
		.map(|record| {
			refresh_execution_program_tracker_facts(
				tracker,
				state_store,
				project.service_id(),
				workflow,
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
	let mut summary = ProgramSchedulerSummary::default();
	let mut candidates = Vec::new();

	for refreshed in refreshed_programs {
		let evaluation = if let Some(source_contract_id) = refreshed.record.source_contract_id() {
				let Some(contract) = state_store.decision_contract(project.service_id(), source_contract_id)?
				else {
				continue;
			};

			refreshed.program.evaluate(contract.contract(), &policy, &context)?
		} else {
			refreshed.program.evaluate_issue_batch(&policy, &context)?
		};

		summary.programs_evaluated += 1;

		for node in evaluation.nodes() {
			if node.state() != ExecutionReadinessState::Ready {
				continue;
			}

			let Some(mapping) = node.linear_issue() else {
				continue;
			};

			if excluded_issue_ids.contains(&mapping.issue_id()) {
				continue;
			}

			let Some(snapshot) = refreshed.issues_by_node.get(node.node_id()) else {
				continue;
			};

			summary.dispatchable_nodes += 1;

			candidates.push(snapshot.issue.clone());
		}

		if refreshed.program != *refreshed.record.program() {
			state_store.upsert_execution_program(project.service_id(), refreshed.program)?;

			summary.programs_updated += 1;
		}
	}

	candidates.sort_by(compare_issue_candidates);

	Ok(ProgramSchedulerSelection {
		selected: candidates
			.into_iter()
			.next()
			.map(|issue| SelectedIssueRunCandidate::new(issue, IssueDispatchMode::Program)),
		summary,
	})
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
	state_store: &StateStore,
	service_id: &str,
	workflow: &WorkflowDocument,
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
			refreshed_nodes.push(refresh_execution_program_local_lifecycle_facts(
				state_store,
				service_id,
				node,
			)?);

			continue;
		};
		let snapshot = program_issue_snapshot(ProgramIssueSnapshotInput {
			tracker,
			state_store,
			service_id,
			workflow,
			issue,
		})?;
		let mapping = snapshot.linear_mapping()?;

		refreshed_nodes.push(node.clone().with_linear_issue(mapping)?);
		issues_by_node.insert(node.node_id().to_owned(), snapshot);
	}

	let program = record.program().clone().with_nodes(refreshed_nodes)?;

	Ok(RefreshedExecutionProgram { record, program, issues_by_node })
}

fn refresh_execution_program_local_lifecycle_facts(
	state_store: &StateStore,
	service_id: &str,
	node: &ExecutionProgramNode,
) -> Result<ExecutionProgramNode> {
	let Some(issue) = node.linear_issue() else {
		return Ok(node.clone());
	};
	let has_post_review_lifecycle =
		state_store.issue_has_review_lifecycle_record(service_id, issue.issue_id())?;

	if issue.has_post_review_lifecycle() == has_post_review_lifecycle {
		return Ok(node.clone());
	}

	node.clone().with_linear_issue(issue.clone().with_post_review_lifecycle(has_post_review_lifecycle))
}

fn program_issue_snapshot<T>(input: ProgramIssueSnapshotInput<'_, T>) -> Result<ProgramIssueSnapshot>
where
	T: IssueTracker + ?Sized,
{
	let ProgramIssueSnapshotInput {
		tracker,
		state_store,
		service_id,
		workflow,
		issue,
	} = input;
	let tracker_policy = workflow.frontmatter().tracker();
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
	let has_post_review_lifecycle =
		state_store.issue_has_review_lifecycle_record(service_id, &issue.id)?;

	Ok(ProgramIssueSnapshot {
		issue: issue.clone(),
		has_active_label,
		has_opt_out_label,
		has_needs_attention_label,
		has_open_tracker_blockers,
		has_generic_dispatch_briefing: issue_has_generic_dispatch_briefing(issue),
		has_post_review_lifecycle,
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
	let active_issue_ids = state_store
		.list_active_shared_leases(service_id)?
		.into_iter()
		.map(|lease| lease.issue_id().to_owned())
		.collect::<Vec<_>>();

	Ok(ExecutionProgramReadinessContext::new()
		.with_dependency_snapshots(dependency_snapshots)
		.with_occupied_conflict_domains(occupied_conflict_domains)
		.with_active_issue_ids(active_issue_ids))
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
		|| snapshot.has_post_review_lifecycle
		|| retained_nonterminal
		|| state_store.issue_has_active_shared_claim(service_id, &issue.id)?)
}
