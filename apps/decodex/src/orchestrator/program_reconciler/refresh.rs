use std::collections::{BTreeMap, BTreeSet};

use crate::{
	execution_program::{ExecutionProgramNode, ExecutionQueueIntent},
	orchestrator::{
		self, ExecutionProgramRecord, IssueTracker, ProgramIssueSnapshot,
		ProgramIssueSnapshotInput, RefreshedExecutionProgram, Result, StateStore, TrackerIssue,
		WorkflowDocument,
	},
	tracker,
};

pub(crate) fn refresh_execution_program_issues<T>(
	tracker: &T,
	records: &[ExecutionProgramRecord],
) -> Result<BTreeMap<String, TrackerIssue>>
where
	T: IssueTracker + ?Sized,
{
	let issue_ids = records
		.iter()
		.flat_map(|record| record.program().nodes())
		.filter_map(|node| node.linear_issue().map(|issue| issue.issue_id().to_owned()))
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect::<Vec<_>>();

	if issue_ids.is_empty() {
		return Ok(BTreeMap::new());
	}

	Ok(tracker
		.refresh_issues(&issue_ids)?
		.into_iter()
		.map(|issue| (issue.id.clone(), issue))
		.collect())
}

pub(crate) fn refresh_execution_program_tracker_facts<T>(
	tracker: &T,
	state_store: &StateStore,
	service_id: &str,
	workflow: &WorkflowDocument,
	record: ExecutionProgramRecord,
	refreshed_issues: &BTreeMap<String, TrackerIssue>,
) -> Result<RefreshedExecutionProgram>
where
	T: IssueTracker + ?Sized,
{
	let refresh_queue_intent = record
		.program()
		.program_intake_plan()
		.is_some_and(|plan| plan.intake_kind().as_str() == "issue_batch_intake");
	let mut refreshed_nodes = Vec::with_capacity(record.program().nodes().len());
	let mut issues_by_node = BTreeMap::new();

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
		let mut refreshed_node = node.clone().with_linear_issue(mapping)?;

		if refresh_queue_intent && node.queue_intent() == ExecutionQueueIntent::Active {
			refreshed_node =
				refreshed_node.with_queue_intent(refreshed_active_issue_batch_queue_intent(
					state_store,
					service_id,
					&snapshot,
					workflow,
				)?)?;
		}

		refreshed_nodes.push(refreshed_node);
		issues_by_node.insert(node.node_id().to_owned(), snapshot);
	}

	let program = record.program().clone().with_nodes(refreshed_nodes)?;

	Ok(RefreshedExecutionProgram { record, program, issues_by_node })
}

pub(crate) fn refresh_execution_program_local_lifecycle_facts(
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

	node.clone()
		.with_linear_issue(issue.clone().with_post_review_lifecycle(has_post_review_lifecycle))
}

pub(crate) fn program_issue_snapshot<T>(
	input: ProgramIssueSnapshotInput<'_, T>,
) -> Result<ProgramIssueSnapshot>
where
	T: IssueTracker + ?Sized,
{
	let ProgramIssueSnapshotInput { tracker, state_store, service_id, workflow, issue } = input;
	let tracker_policy = workflow.frontmatter().tracker();
	let has_active_label = tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		&tracker::automation_active_label(service_id),
	)?;
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
	let has_open_tracker_blockers = issue
		.blockers
		.iter()
		.any(|blocker| !orchestrator::state_name_is_terminal(&blocker.state.name, workflow));
	let has_post_review_lifecycle =
		state_store.issue_has_review_lifecycle_record(service_id, &issue.id)?;

	Ok(ProgramIssueSnapshot {
		issue: issue.clone(),
		has_active_label,
		has_opt_out_label,
		has_needs_attention_label,
		has_open_tracker_blockers,
		has_generic_dispatch_briefing: orchestrator::issue_has_generic_dispatch_briefing(issue),
		has_post_review_lifecycle,
	})
}

fn refreshed_active_issue_batch_queue_intent(
	state_store: &StateStore,
	service_id: &str,
	snapshot: &ProgramIssueSnapshot,
	workflow: &WorkflowDocument,
) -> Result<ExecutionQueueIntent> {
	if orchestrator::state_name_is_terminal(&snapshot.issue.state.name, workflow) {
		return Ok(ExecutionQueueIntent::Done);
	}
	if snapshot.has_active_label
		|| snapshot.has_post_review_lifecycle
		|| state_store.worktree_for_issue(&snapshot.issue.id)?.is_some()
		|| state_store.issue_has_active_shared_claim(service_id, &snapshot.issue.id)?
	{
		return Ok(ExecutionQueueIntent::Active);
	}
	if snapshot.has_opt_out_label
		|| !workflow
			.frontmatter()
			.tracker()
			.startable_states()
			.iter()
			.any(|state| state == &snapshot.issue.state.name)
	{
		return Ok(ExecutionQueueIntent::NotReady);
	}

	Ok(ExecutionQueueIntent::ReadyToQueue)
}
