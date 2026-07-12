pub(crate) mod lifecycle;

mod closeout;
mod description;
mod retry_budget;
pub(crate) use self::{
	description::render_issue_description_for_prompt,
	lifecycle::{
		cleanup_completed_post_review_lane, cleanup_terminal_worktree, cleanup_worktree_mapping,
		clear_recovered_issue_lease, clear_worktree_retry_schedule, is_issue_eligible,
		is_issue_in_progress_for_run, is_issue_not_dispatchable_for_current_dispatch,
		is_terminal_issue, mark_run_attempt_if_active, refresh_issue, state_name_is_terminal,
		todo_blocker_rule_passes,
	},
	retry_budget::{
		clear_terminal_guard_marker, issue_has_service_ownership,
		issue_passes_retry_dispatch_policy, issue_passes_retry_retention_policy,
		issue_retry_budget_exhausted, issue_retry_budget_exhausted_for_worktree,
		retry_budget_base_for_dispatch_mode, retry_budget_base_for_issue_worktree,
		write_retry_budget_marker, write_terminal_guard_marker,
	},
};
pub(crate) use closeout::{
	closeout_dispatch_block_reason, evaluate_closeout_dispatch_policy_with_inspector,
	issue_passes_closeout_dispatch_policy, issue_passes_review_repair_dispatch_policy,
};
#[cfg(test)]
pub(crate) use closeout::{
	closeout_dispatch_block_reason_with_inspector,
	issue_passes_closeout_dispatch_policy_with_inspector,
};
pub(crate) use description::description_is_machine_only_fenced_block;

use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	lane_authority::{
		BindingAttestation, IntakeAuthority, IntakeAuthorityKind, LaneCommand, ProjectBinding,
	},
	orchestrator::{
		ErrorKind, GhPullRequestReviewStateInspector, GitCredentialSource, IssueDispatchMode,
		IssueRunPlan, IssueTracker, Path, PathBuf, PullRequestReviewStateInspector, Result,
		RetainedCloseoutPrMergeGate, RetryIssueStateHint, ServiceConfig, StateStore,
		TERMINAL_GUARD_MARKER_FILE, TERMINAL_GUARDED_RUN_STATUS, TrackerIssue, Value,
		WorkflowDocument, WorktreeManager, WorktreeMapping, WorktreeSpec, default_branch_sync,
		delete_local_branch_if_present, detach_worktree_head_from_branch_if_checked_out, eyre, fs,
		github, retained_closeout_pr_merge_gate_with_inspector,
	},
	tracker,
};

pub(crate) const REVIEW_HANDOFF_BLOCK_REASON: &str = "review_handoff_state_transition_pending";

pub(crate) fn issue_matches_project_tracker_scope(
	issue: &TrackerIssue,
	project: &ServiceConfig,
) -> bool {
	issue.team.id == project.tracker().team_id()
}

pub(crate) fn attest_issue_project_binding(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue: &TrackerIssue,
) -> Result<BindingAttestation> {
	let binding = attest_project_binding(state_store, project)?;
	let mut repository_selectors = issue
		.labels
		.iter()
		.filter_map(|label| label.name.strip_prefix("repo:"))
		.collect::<Vec<_>>();
	repository_selectors.sort_unstable();
	repository_selectors.dedup();
	if repository_selectors.iter().any(|selector| selector.is_empty())
		|| repository_selectors.len() > 1
	{
		eyre::bail!("Issue repository selector is ambiguous; binding attestation rejected.");
	}
	if let Some(selector) = repository_selectors.first()
		&& *selector != binding.github_repository()
	{
		eyre::bail!(
			"Issue repository selector does not match the immutable project binding."
		);
	}
	BindingAttestation::new(&binding, &issue.id, &issue.team.id)
}

pub(crate) fn admit_normal_queue_lane(
	state_store: &StateStore,
	attestation: &BindingAttestation,
	issue: &TrackerIssue,
) -> Result<()> {
	let lane_id = attestation.lane_id().clone();
	if let Some(lane) = state_store.lane(&lane_id)? {
		if lane.intake_authority_id().is_some() {
			return Ok(());
		}
	}

	let mut digest = Sha256::new();
	for value in [
		lane_id.project_key(),
		lane_id.tracker_issue_id(),
		&issue.team.id,
		&issue.updated_at,
		attestation.routing_label(),
		attestation.binding_fingerprint(),
	] {
		digest.update((value.len() as u64).to_be_bytes());
		digest.update(value.as_bytes());
	}
	let snapshot_fingerprint =
		digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>();
	let program_id =
		format!("normal-queue-{}-{}", lane_id.project_key(), &snapshot_fingerprint[..16]);
	let authority = if let Some(existing) =
		state_store.intake_authority_for_program(lane_id.project_key(), &program_id)?
	{
		existing
	} else {
		let now = OffsetDateTime::now_utc();
		let accepted_at = now.format(&Rfc3339)?;
		IntakeAuthority::new(
			&format!("intake-authority-{program_id}"),
			lane_id.project_key(),
			attestation.project().clone(),
			&format!("plan-{program_id}"),
			&program_id,
			"decodex_runtime",
			"normal_queue_label",
			&format!("normal-queue:{}", lane_id.tracker_issue_id()),
			&accepted_at,
			now.unix_timestamp(),
			IntakeAuthorityKind::IssueBatch {
				accepted_intake_id: format!("normal-queue:{}", lane_id.tracker_issue_id()),
				batch_fingerprint: snapshot_fingerprint,
			},
		)?
	};
	let authority = state_store.persist_intake_authority(authority)?;
	state_store.apply_lane_command(
		lane_id,
		attestation.binding_fingerprint(),
		LaneCommand::Admit { intake_authority_id: authority.authority_id().to_owned() },
	)?;
	Ok(())
}

pub(crate) fn admit_program_lane(
	state_store: &StateStore,
	attestation: &BindingAttestation,
	program_id: &str,
) -> Result<()> {
	let authority = state_store
		.intake_authority_for_program(attestation.lane_id().project_key(), program_id)?;
	let Some(authority) = authority else {
		#[cfg(not(test))]
		eyre::bail!("Program dispatch has no typed Intake Authority.");
		#[cfg(test)]
		return Ok(());
	};
	if authority.project_key() != attestation.lane_id().project_key()
		|| authority.binding_attestation().binding_fingerprint()
			!= attestation.binding_fingerprint()
	{
		eyre::bail!("Program Intake Authority does not match lane binding attestation.");
	}
	state_store.apply_lane_command(
		attestation.lane_id().clone(),
		attestation.binding_fingerprint(),
		LaneCommand::Admit { intake_authority_id: authority.authority_id().to_owned() },
	)?;
	Ok(())
}

pub(crate) fn attest_project_binding(
	state_store: &StateStore,
	project: &ServiceConfig,
) -> Result<ProjectBinding> {
	let binding = match state_store.registered_project_binding(project.service_id())? {
		Some(binding) => binding,
		None => {
			#[cfg(not(test))]
			eyre::bail!("Project is not registered; lane admission is forbidden.");
			#[cfg(test)]
			ProjectBinding::new(
				project.service_id(),
				project.github().owner(),
				project.github().repository(),
				project.tracker().team_id(),
				&tracker::automation_queue_label(project.service_id()),
				"test-config-fingerprint",
			)?
		},
	};

	if binding.project_key() != project.service_id()
		|| binding.github_owner() != project.github().owner()
		|| binding.github_repository() != project.github().repository()
		|| binding.tracker_team_id() != project.tracker().team_id()
		|| binding.routing_label() != tracker::automation_queue_label(project.service_id())
	{
		eyre::bail!("Current project config does not match its registered immutable binding.");
	}

	Ok(binding)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CloseoutDispatchEligibility {
	Eligible,
	Ineligible,
	Blocked(&'static str),
}

pub(crate) fn issue_has_generic_dispatch_briefing(issue: &TrackerIssue) -> bool {
	!description_is_machine_only_fenced_block(&issue.description)
}

pub(crate) fn ordinary_dispatch_blocked_by_retained_review_handoff(
	project_id: &str,
	issue: &TrackerIssue,
	state_store: &StateStore,
) -> Result<bool> {
	let Some(worktree) = state_store.worktree_for_issue(&issue.id)? else {
		return Ok(false);
	};

	if worktree.project_id() != project_id || !worktree.worktree_path().try_exists()? {
		return Ok(false);
	}

	let Some(lifecycle_record) =
		state_store.review_lifecycle_record(project_id, &issue.id, worktree.branch_name())?
	else {
		return Ok(false);
	};

	Ok(lifecycle_record.branch_name() == worktree.branch_name())
}

pub(crate) fn issue_passes_dispatch_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
	queue_label: &str,
	queue_membership_confirmed_by_source: bool,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();

	if tracker_policy.terminal_states().iter().any(|state| state == &issue.state.name) {
		return Ok(false);
	}
	if !tracker_policy.startable_states().iter().any(|state| state == &issue.state.name) {
		return Ok(false);
	}
	if issue.has_label(tracker_policy.opt_out_label()) {
		return Ok(false);
	}
	if issue.has_label(tracker_policy.needs_attention_label()) {
		return Ok(false);
	}
	if !queue_membership_confirmed_by_source {
		if issue.labels_complete {
			if !issue.has_label(queue_label) {
				return Ok(false);
			}
		} else if !tracker::issue_has_label_with_server_confirmation(tracker, issue, queue_label)? {
			return Ok(false);
		}
	}
	if !todo_blocker_rule_passes(issue, workflow) {
		return Ok(false);
	}
	if !issue_has_generic_dispatch_briefing(issue) {
		return Ok(false);
	}

	Ok(true)
}

pub(crate) fn issue_passes_current_dispatch_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dispatch_mode: IssueDispatchMode,
	hint: RetryIssueStateHint<'_>,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	if !issue_matches_project_tracker_scope(issue, project) {
		return Ok(false);
	}

	match dispatch_mode {
		IssueDispatchMode::Normal => {
			let queue_label = tracker::automation_queue_label(project.service_id());

			Ok(issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, false)?
				&& !ordinary_dispatch_blocked_by_retained_review_handoff(
					project.service_id(),
					issue,
					state_store,
				)?)
		},
		IssueDispatchMode::Program => {
			let queue_label = tracker::automation_queue_label(project.service_id());

			Ok(issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, true)?
				&& !ordinary_dispatch_blocked_by_retained_review_handoff(
					project.service_id(),
					issue,
					state_store,
				)?)
		},
		IssueDispatchMode::Retry => self::retry_budget::issue_passes_retry_dispatch_policy(
			tracker,
			issue,
			project,
			workflow,
			state_store,
			hint,
		),
		IssueDispatchMode::ReviewRepair => {
			Ok(issue_passes_review_repair_dispatch_policy(tracker, issue, project, workflow)?
				&& !self::retry_budget::issue_retry_budget_exhausted(
					workflow,
					state_store,
					project.service_id(),
					&issue.id,
				)?)
		},
		IssueDispatchMode::Closeout => {
			issue_passes_closeout_dispatch_policy(tracker, issue, project, workflow, state_store)
		},
	}
}
