use super::{
	AccountActivityMode, CodexAccountActivitySummary, CodexAccountPool, HashSet, IssueTracker,
	LOOP_GUARDRAIL_CONVERGENCE_BUDGET, LoopGuardrailCheckpoint, LoopGuardrailCheckpointInput,
	LoopGuardrailReason, ORDINARY_DISPATCH_REVIEW_HANDOFF_BLOCK_REASON, OperatorQueuedIssueStatus,
	QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT, ServiceConfig, StateStore, TrackerIssue,
	WorkflowDocument, compare_issue_candidates, is_terminal_issue,
	issue_has_generic_dispatch_briefing, issue_passes_dispatch_policy, json,
	loop_guardrail_text_hash, operator_queued_issue_attention_status,
	ordinary_dispatch_blocked_by_retained_review_handoff, state_name_is_terminal,
	todo_blocker_rule_passes, tracker,
};

pub(in crate::orchestrator) fn codex_account_activity_summaries(
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

pub(in crate::orchestrator) fn build_queued_candidate_statuses<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> crate::prelude::Result<Vec<OperatorQueuedIssueStatus>>
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

	issues
		.into_iter()
		.filter(|issue| !is_terminal_issue(issue, workflow))
		.filter(|issue| {
			!queued_issue_is_retained_post_review_lane(
				issue,
				success_state,
				&retained_post_review_issue_ids,
			)
		})
		.map(|issue| operator_queued_issue_status(tracker, project, workflow, state_store, issue))
		.collect()
}

pub(in crate::orchestrator) fn queued_issue_is_retained_post_review_lane(
	issue: &TrackerIssue,
	success_state: &str,
	retained_post_review_issue_ids: &HashSet<String>,
) -> bool {
	issue.state.name == success_state && retained_post_review_issue_ids.contains(&issue.id)
}

pub(in crate::orchestrator) fn operator_queued_issue_status<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: TrackerIssue,
) -> crate::prelude::Result<OperatorQueuedIssueStatus>
where
	T: IssueTracker,
{
	let (classification, reason) =
		classify_queued_issue(tracker, project, workflow, state_store, &issue)?;
	let blocker_identifiers = queued_issue_blocker_identifiers(&issue, workflow, reason);
	let attention = operator_queued_issue_attention_status(
		tracker,
		project,
		workflow,
		state_store,
		&issue,
		reason,
	)?;

	Ok(OperatorQueuedIssueStatus {
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
	})
}

pub(in crate::orchestrator) fn queued_issue_blocker_identifiers(
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
		.filter(|blocker| !state_name_is_terminal(&blocker.state.name, workflow))
		.map(|blocker| blocker.identifier.clone())
		.collect()
}

pub(in crate::orchestrator) fn observe_dependency_program_stale_guardrail(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> crate::prelude::Result<LoopGuardrailCheckpoint> {
	let blocker_fingerprint = dependency_blocker_fingerprint(issue, workflow);
	let checkpoint =
		state_store.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
			project_id: project.service_id(),
			issue_id: &issue.id,
			reason: LoopGuardrailReason::DependencyProgramStale.error_class(),
			fingerprint: &blocker_fingerprint,
			run_id: "queued-dependency-blocker",
			attempt_number: 0,
			details_json: &json!({
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

pub(in crate::orchestrator) fn dependency_blocker_fingerprint(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> String {
	let mut blockers = issue
		.blockers
		.iter()
		.filter(|blocker| !state_name_is_terminal(&blocker.state.name, workflow))
		.map(|blocker| format!("{}:{}", blocker.identifier, blocker.state.name))
		.collect::<Vec<_>>();

	blockers.sort();

	loop_guardrail_text_hash(&blockers.join("|"))
}

pub(in crate::orchestrator) fn classify_queued_issue<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> crate::prelude::Result<(&'static str, &'static str)>
where
	T: IssueTracker,
{
	let tracker_policy = workflow.frontmatter().tracker();

	if tracker_policy.terminal_states().iter().any(|state| state == &issue.state.name) {
		return Ok(("closed", "terminal_state"));
	}
	if issue.has_label(tracker_policy.needs_attention_label()) {
		return Ok(("blocked", "issue_needs_attention"));
	}
	if state_store.issue_has_active_shared_claim(project.service_id(), &issue.id)? {
		return Ok(("claimed", "shared_claim_present"));
	}
	if (issue.state.name == tracker_policy.in_progress_state()
		|| tracker_policy.startable_states().iter().any(|state| state == &issue.state.name))
		&& ordinary_dispatch_blocked_by_retained_review_handoff(
			project.service_id(),
			issue,
			state_store,
		)? {
		return Ok(("blocked", ORDINARY_DISPATCH_REVIEW_HANDOFF_BLOCK_REASON));
	}
	if tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		&tracker::automation_active_label(project.service_id()),
	)? {
		return Ok(("blocked", QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT));
	}
	if !tracker_policy.startable_states().iter().any(|state| state == &issue.state.name) {
		return Ok(("blocked", "non_startable_state"));
	}
	if issue.has_label(tracker_policy.opt_out_label()) {
		return Ok(("blocked", "issue_opted_out"));
	}
	if !todo_blocker_rule_passes(issue, workflow) {
		let checkpoint =
			observe_dependency_program_stale_guardrail(project, workflow, state_store, issue)?;
		let reason = if checkpoint.consecutive_count() >= LOOP_GUARDRAIL_CONVERGENCE_BUDGET {
			LoopGuardrailReason::DependencyProgramStale.error_class()
		} else {
			"open_tracker_blockers"
		};

		return Ok(("blocked", reason));
	}

	state_store.clear_loop_guardrail_checkpoint(
		project.service_id(),
		&issue.id,
		LoopGuardrailReason::DependencyProgramStale.error_class(),
	)?;

	if !issue_has_generic_dispatch_briefing(issue) {
		return Ok(("blocked", "missing_dispatch_briefing"));
	}
	let queue_label = tracker::automation_queue_label(project.service_id());

	if !issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, true)? {
		return Ok(("blocked", "dispatch_policy_rejected"));
	}

	Ok(("ready", "eligible_for_dispatch"))
}
