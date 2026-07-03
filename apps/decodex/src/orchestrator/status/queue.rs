use crate::{
	orchestrator::{
		kernel::command::{CommandFact, CommandIntent, CommandIntentKind},
		status::{
			self, AccountActivityMode, CodexAccountActivitySummary, CodexAccountPool, HashSet,
			IssueTracker, LOOP_GUARDRAIL_CONVERGENCE_BUDGET, LoopGuardrailCheckpoint,
			LoopGuardrailCheckpointInput, LoopGuardrailReason,
			ORDINARY_DISPATCH_REVIEW_HANDOFF_BLOCK_REASON, OperatorQueuedIssueStatus,
			QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT, ServiceConfig, StateStore, TrackerIssue,
			WorkflowDocument, compare_issue_candidates,
		},
	},
	prelude::Result,
	tracker,
};

#[derive(Clone, Debug)]
pub(crate) struct QueuedCandidateStatusPlan {
	pub(crate) statuses: Vec<OperatorQueuedIssueStatus>,
	pub(crate) guardrail_commands: Vec<QueuedGuardrailCommand>,
}

#[derive(Clone, Debug)]
pub(crate) struct QueuedGuardrailCommand {
	pub(crate) intent: CommandIntent,
	action: QueuedGuardrailCommandAction,
	issue: TrackerIssue,
}

struct QueuedIssueStatusOutcome {
	status: OperatorQueuedIssueStatus,
	guardrail_command: Option<QueuedGuardrailCommand>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QueuedGuardrailCommandAction {
	ObserveDependencyProgramStale,
	ClearDependencyProgramStale,
}

pub(crate) fn codex_account_activity_summaries(
	project: &ServiceConfig,
	warnings: &mut Vec<String>,
	mode: AccountActivityMode,
) -> Vec<CodexAccountActivitySummary> {
	let Some(accounts_config) = project.codex().accounts() else {
		return Vec::new();
	};
	let accounts = CodexAccountPool::from_config(accounts_config).and_then(|pool| match mode {
		AccountActivityMode::Probe => pool.account_activity_summaries_cached(false),
		AccountActivityMode::Snapshot => pool.account_activity_summaries_snapshot(),
	});

	match accounts {
		Ok(accounts) => accounts,
		Err(error) => {
			tracing::warn!(
				project_id = project.service_id(),
				error = %error,
				"Codex accounts snapshot could not be loaded."
			);

			warnings.push(String::from("codex_accounts_unavailable"));

			Vec::new()
		},
	}
}

pub(crate) fn build_queued_candidate_statuses<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<Vec<OperatorQueuedIssueStatus>>
where
	T: IssueTracker,
{
	Ok(build_queued_candidate_status_plan(tracker, project, workflow, state_store)?.statuses)
}

pub(crate) fn build_queued_candidate_status_plan<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<QueuedCandidateStatusPlan>
where
	T: IssueTracker,
{
	let queue_label = tracker::automation_queue_label(project.service_id());
	let retained_post_review_issue_ids = state_store
		.list_worktrees(project.service_id())?
		.into_iter()
		.map(|mapping| mapping.issue_id().to_owned())
		.collect::<HashSet<_>>();
	let success_state = workflow.frontmatter().tracker().success_state();
	let mut issues = tracker.list_issues_with_label(&queue_label)?;

	issues.sort_by(compare_issue_candidates);

	let mut statuses = Vec::new();
	let mut guardrail_commands = Vec::new();

	for issue in issues {
		if status::is_terminal_issue(&issue, workflow)
			|| queued_issue_is_retained_post_review_lane(
				&issue,
				success_state,
				&retained_post_review_issue_ids,
			) {
			continue;
		}

		let outcome = operator_queued_issue_status_with_commands(
			tracker,
			project,
			workflow,
			state_store,
			issue,
		)?;

		if let Some(command) = outcome.guardrail_command {
			guardrail_commands.push(command);
		}

		statuses.push(outcome.status);
	}

	Ok(QueuedCandidateStatusPlan { statuses, guardrail_commands })
}

pub(crate) fn queued_issue_is_retained_post_review_lane(
	issue: &TrackerIssue,
	success_state: &str,
	retained_post_review_issue_ids: &HashSet<String>,
) -> bool {
	issue.state.name == success_state && retained_post_review_issue_ids.contains(&issue.id)
}

#[allow(dead_code)]
pub(crate) fn operator_queued_issue_status<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: TrackerIssue,
) -> Result<OperatorQueuedIssueStatus>
where
	T: IssueTracker,
{
	Ok(operator_queued_issue_status_with_commands(tracker, project, workflow, state_store, issue)?
		.status)
}

pub(crate) fn queued_issue_blocker_identifiers(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
	reason: &str,
) -> Vec<String> {
	if reason != "open_tracker_blockers"
		&& reason != LoopGuardrailReason::DependencyProgramStale.error_class()
	{
		return Vec::new();
	}

	issue
		.blockers
		.iter()
		.filter(|blocker| !status::state_name_is_terminal(&blocker.state.name, workflow))
		.map(|blocker| blocker.identifier.clone())
		.collect()
}

pub(crate) fn observe_dependency_program_stale_guardrail(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Result<LoopGuardrailCheckpoint> {
	let blocker_fingerprint = dependency_blocker_fingerprint(issue, workflow);
	let checkpoint =
		state_store.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
			project_id: project.service_id(),
			issue_id: &issue.id,
			reason: LoopGuardrailReason::DependencyProgramStale.error_class(),
			fingerprint: &blocker_fingerprint,
			run_id: "queued-dependency-blocker",
			attempt_number: 0,
			details_json: &status::json!({
				"schema": "decodex.loop_guardrail_checkpoint/1",
				"reason": LoopGuardrailReason::DependencyProgramStale.error_class(),
				"blockers": queued_issue_blocker_identifiers(
					issue,
					workflow,
					"open_tracker_blockers",
				),
				"threshold": LOOP_GUARDRAIL_CONVERGENCE_BUDGET,
			})
			.to_string(),
		})?;

	Ok(checkpoint)
}

pub(crate) fn dependency_blocker_fingerprint(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> String {
	let mut blockers = issue
		.blockers
		.iter()
		.filter(|blocker| !status::state_name_is_terminal(&blocker.state.name, workflow))
		.map(|blocker| format!("{}:{}", blocker.identifier, blocker.state.name))
		.collect::<Vec<_>>();

	blockers.sort();

	status::loop_guardrail_text_hash(&blockers.join("|"))
}

pub(crate) fn apply_queued_candidate_guardrail_commands(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	commands: &[QueuedGuardrailCommand],
) -> Result<()> {
	for command in commands {
		match (command.action, command.intent.kind) {
			(
				QueuedGuardrailCommandAction::ObserveDependencyProgramStale,
				CommandIntentKind::ObserveLoopGuardrailCheckpoint,
			) => {
				observe_dependency_program_stale_guardrail(
					project,
					workflow,
					state_store,
					&command.issue,
				)?;
			},
			(
				QueuedGuardrailCommandAction::ClearDependencyProgramStale,
				CommandIntentKind::ClearLoopGuardrailCheckpoint,
			) => {
				state_store.clear_loop_guardrail_checkpoint(
					project.service_id(),
					&command.issue.id,
					LoopGuardrailReason::DependencyProgramStale.error_class(),
				)?;
			},
			_ => {
				color_eyre::eyre::bail!(
					"queued guardrail command action `{}` does not match intent `{}`",
					match command.action {
						QueuedGuardrailCommandAction::ObserveDependencyProgramStale =>
							"observe_dependency_program_stale",
						QueuedGuardrailCommandAction::ClearDependencyProgramStale =>
							"clear_dependency_program_stale",
					},
					command.intent.kind.as_str()
				);
			},
		}
	}

	Ok(())
}

#[allow(dead_code)]
pub(crate) fn classify_queued_issue<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Result<(&'static str, &'static str)>
where
	T: IssueTracker,
{
	let (classification, reason, _command) =
		classify_queued_issue_with_command(tracker, project, workflow, state_store, issue)?;

	Ok((classification, reason))
}

fn operator_queued_issue_status_with_commands<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: TrackerIssue,
) -> Result<QueuedIssueStatusOutcome>
where
	T: IssueTracker,
{
	let (classification, reason, guardrail_command) =
		classify_queued_issue_with_command(tracker, project, workflow, state_store, &issue)?;
	let blocker_identifiers = queued_issue_blocker_identifiers(&issue, workflow, reason);
	let attention = status::operator_queued_issue_attention_status(
		tracker,
		project,
		workflow,
		state_store,
		&issue,
		reason,
	)?;

	Ok(QueuedIssueStatusOutcome {
		status: OperatorQueuedIssueStatus {
			project_id: project.service_id().to_owned(),
			issue_id: issue.id,
			issue_identifier: issue.identifier,
			title: issue.title,
			author: issue.author,
			state: issue.state.name,
			priority: issue.priority,
			created_at: issue.created_at,
			classification: classification.to_owned(),
			reason: reason.to_owned(),
			attention,
			blocker_identifiers,
		},
		guardrail_command,
	})
}

fn current_dependency_program_stale_count(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Result<i64> {
	let blocker_fingerprint = dependency_blocker_fingerprint(issue, workflow);

	Ok(state_store
		.loop_guardrail_checkpoint(
			project.service_id(),
			&issue.id,
			LoopGuardrailReason::DependencyProgramStale.error_class(),
		)?
		.filter(|checkpoint| checkpoint.fingerprint() == blocker_fingerprint)
		.map_or(0, |checkpoint| checkpoint.consecutive_count()))
}

fn dependency_program_stale_checkpoint_exists(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Result<bool> {
	Ok(state_store
		.loop_guardrail_checkpoint(
			project.service_id(),
			&issue.id,
			LoopGuardrailReason::DependencyProgramStale.error_class(),
		)?
		.is_some())
}

fn observe_dependency_program_stale_guardrail_command(
	issue: &TrackerIssue,
) -> QueuedGuardrailCommand {
	QueuedGuardrailCommand {
		intent: CommandIntent::new(
			CommandIntentKind::ObserveLoopGuardrailCheckpoint,
			format!("{}:dependency_program_stale:observe", issue.id),
			vec![CommandFact::OpenTrackerBlockersPresent],
			vec![CommandFact::LoopGuardrailCheckpointObserved],
		),
		action: QueuedGuardrailCommandAction::ObserveDependencyProgramStale,
		issue: issue.clone(),
	}
}

fn clear_dependency_program_stale_guardrail_command(
	issue: &TrackerIssue,
) -> QueuedGuardrailCommand {
	QueuedGuardrailCommand {
		intent: CommandIntent::new(
			CommandIntentKind::ClearLoopGuardrailCheckpoint,
			format!("{}:dependency_program_stale:clear", issue.id),
			vec![CommandFact::OpenTrackerBlockersResolved],
			vec![CommandFact::LoopGuardrailCheckpointCleared],
		),
		action: QueuedGuardrailCommandAction::ClearDependencyProgramStale,
		issue: issue.clone(),
	}
}

fn classify_queued_issue_with_command<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Result<(&'static str, &'static str, Option<QueuedGuardrailCommand>)>
where
	T: IssueTracker,
{
	let tracker_policy = workflow.frontmatter().tracker();

	if tracker_policy.terminal_states().iter().any(|state| state == &issue.state.name) {
		return Ok(("closed", "terminal_state", None));
	}
	if issue.has_label(tracker_policy.needs_attention_label()) {
		return Ok(("blocked", "issue_needs_attention", None));
	}
	if state_store.issue_has_active_shared_claim(project.service_id(), &issue.id)? {
		return Ok(("claimed", "shared_claim_present", None));
	}
	if (issue.state.name == tracker_policy.in_progress_state()
		|| tracker_policy.startable_states().iter().any(|state| state == &issue.state.name))
		&& status::ordinary_dispatch_blocked_by_retained_review_handoff(
			project.service_id(),
			issue,
			state_store,
		)? {
		return Ok(("blocked", ORDINARY_DISPATCH_REVIEW_HANDOFF_BLOCK_REASON, None));
	}
	if tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		&tracker::automation_active_label(project.service_id()),
	)? {
		return Ok(("blocked", QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT, None));
	}
	if !tracker_policy.startable_states().iter().any(|state| state == &issue.state.name) {
		return Ok(("blocked", "non_startable_state", None));
	}
	if issue.has_label(tracker_policy.opt_out_label()) {
		return Ok(("blocked", "issue_opted_out", None));
	}
	if !status::todo_blocker_rule_passes(issue, workflow) {
		let checkpoint_count =
			current_dependency_program_stale_count(project, workflow, state_store, issue)?;
		let reason = if checkpoint_count >= LOOP_GUARDRAIL_CONVERGENCE_BUDGET {
			LoopGuardrailReason::DependencyProgramStale.error_class()
		} else {
			"open_tracker_blockers"
		};

		return Ok((
			"blocked",
			reason,
			Some(observe_dependency_program_stale_guardrail_command(issue)),
		));
	}

	let clear_guardrail_command =
		dependency_program_stale_checkpoint_exists(project, state_store, issue)?
			.then(|| clear_dependency_program_stale_guardrail_command(issue));

	if !status::issue_has_generic_dispatch_briefing(issue) {
		return Ok(("blocked", "missing_dispatch_briefing", clear_guardrail_command));
	}

	let queue_label = tracker::automation_queue_label(project.service_id());

	if !status::issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, true)? {
		return Ok(("blocked", "dispatch_policy_rejected", clear_guardrail_command));
	}

	Ok(("ready", "eligible_for_dispatch", clear_guardrail_command))
}
