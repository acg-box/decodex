#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CloseoutDispatchEligibility {
	Eligible,
	Ineligible,
	Blocked(&'static str),
}

pub(crate) fn issue_has_generic_dispatch_briefing(issue: &TrackerIssue) -> bool {
	!description_is_machine_only_fenced_block(&issue.description)
}

fn issue_passes_dispatch_policy<T>(
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

fn description_is_machine_only_fenced_block(description: &str) -> bool {
	let trimmed = description.trim();

	if trimmed.is_empty() {
		return false;
	}

	let mut saw_fence = false;
	let mut inside_fence = false;
	let mut current_fence_marker = b'`';
	let mut current_fence_ticks = 0;
	let mut current_fence_info = String::new();
	let mut current_fence_body = String::new();

	for line in trimmed.lines() {
		let trimmed_line = line.trim();

		if let Some((fence_marker, fence_ticks, fence_tail)) = parse_code_fence(trimmed_line) {
			if inside_fence {
				if fence_marker == current_fence_marker
					&& fence_ticks >= current_fence_ticks
					&& fence_tail.is_empty()
				{
					if !fenced_block_is_machine_readable(&current_fence_info, &current_fence_body) {
						return false;
					}

					inside_fence = false;
					current_fence_marker = b'`';
					current_fence_ticks = 0;

					current_fence_info.clear();
					current_fence_body.clear();

					continue;
				}
			} else {
				saw_fence = true;
				inside_fence = true;
				current_fence_marker = fence_marker;
				current_fence_ticks = fence_ticks;
				current_fence_info = fence_tail.to_ascii_lowercase();

				current_fence_body.clear();

				continue;
			}
		}

		if inside_fence {
			current_fence_body.push_str(line);
			current_fence_body.push('\n');

			continue;
		}
		if !inside_fence && !trimmed_line.is_empty() {
			return false;
		}
	}

	saw_fence && !inside_fence
}

fn parse_code_fence(line: &str) -> Option<(u8, usize, &str)> {
	let first_byte = *line.as_bytes().first()?;

	if first_byte != b'`' && first_byte != b'~' {
		return None;
	}

	let fence_ticks = line.bytes().take_while(|byte| *byte == first_byte).count();

	if fence_ticks < 3 {
		return None;
	}

	Some((first_byte, fence_ticks, line[fence_ticks..].trim()))
}

fn fenced_block_is_machine_readable(fence_info: &str, fence_body: &str) -> bool {
	if !fence_info.is_empty() && fence_info != "json" {
		return false;
	}

	match serde_json::from_str::<Value>(fence_body.trim()) {
		Ok(payload) => payload.is_object() || payload.is_array(),
		Err(_) => false,
	}
}

fn render_issue_description_for_prompt(issue: &TrackerIssue) -> String {
	if issue.description.trim().is_empty() {
		return String::from("(no description)");
	}
	if description_is_machine_only_fenced_block(&issue.description) {
		return String::from(
			"(machine-only tracker description omitted; this lane requires a separate generic issue briefing surface)",
		);
	}

	issue.description.clone()
}

fn issue_passes_review_repair_dispatch_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();

	Ok(issue_has_service_ownership(tracker, issue, project.service_id())?
		&& issue.state.name == tracker_policy.success_state()
		&& !issue.has_label(tracker_policy.opt_out_label())
		&& !issue.has_label(tracker_policy.needs_attention_label()))
}

fn issue_passes_closeout_dispatch_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};

	issue_passes_closeout_dispatch_policy_with_inspector(
		tracker,
		issue,
		project,
		workflow,
		state_store,
		&review_state_inspector,
	)
}

fn issue_passes_closeout_dispatch_policy_with_inspector<T, I>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
	I: PullRequestReviewStateInspector + ?Sized,
{
	Ok(matches!(
		evaluate_closeout_dispatch_policy_with_inspector(
			tracker,
				issue,
				project,
				workflow,
				state_store,
				review_state_inspector,
			)?,
		CloseoutDispatchEligibility::Eligible
	))
}

fn closeout_dispatch_block_reason<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<Option<&'static str>>
where
	T: IssueTracker + ?Sized,
{
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};

	closeout_dispatch_block_reason_with_inspector(
		tracker,
		issue,
		project,
		workflow,
		state_store,
		&review_state_inspector,
	)
}

fn closeout_dispatch_block_reason_with_inspector<T, I>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> Result<Option<&'static str>>
where
	T: IssueTracker + ?Sized,
	I: PullRequestReviewStateInspector + ?Sized,
{
	Ok(match evaluate_closeout_dispatch_policy_with_inspector(
		tracker,
		issue,
		project,
		workflow,
		state_store,
		review_state_inspector,
	)? {
		CloseoutDispatchEligibility::Blocked(reason) => Some(reason),
		CloseoutDispatchEligibility::Eligible | CloseoutDispatchEligibility::Ineligible =>
			None,
	})
}

fn evaluate_closeout_dispatch_policy_with_inspector<T, I>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> Result<CloseoutDispatchEligibility>
where
	T: IssueTracker + ?Sized,
	I: PullRequestReviewStateInspector + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();
	let completed_state = tracker_policy.resolved_completed_state();
	let issue_state = issue.state.name.as_str();

	if issue.has_label(tracker_policy.opt_out_label())
		|| issue.has_label(tracker_policy.needs_attention_label())
	{
		return Ok(CloseoutDispatchEligibility::Ineligible);
	}
	if !issue_has_service_ownership(tracker, issue, project.service_id())? {
		return Ok(CloseoutDispatchEligibility::Ineligible);
	}
	if issue_state != tracker_policy.success_state() && issue_state != completed_state {
		return Ok(CloseoutDispatchEligibility::Ineligible);
	}

	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());
	let worktree = worktree_manager.plan_for_issue(&issue.identifier);

	if !worktree.path.exists() {
		return Ok(CloseoutDispatchEligibility::Ineligible);
	}

	let Some(review_handoff) =
		state_store.review_handoff_marker(project.service_id(), &issue.id, &worktree.branch_name)?
	else {
		return Ok(CloseoutDispatchEligibility::Blocked(
			"missing_review_handoff_record",
		));
	};

	if review_handoff.branch_name() != worktree.branch_name {
		return Ok(CloseoutDispatchEligibility::Ineligible);
	}

	Ok(match retained_closeout_pr_merge_gate_with_inspector(
		&worktree.path,
		&worktree.branch_name,
		review_handoff.pr_url(),
		review_state_inspector,
	)? {
		RetainedCloseoutPrMergeGate::Merged =>
			CloseoutDispatchEligibility::Eligible,
		RetainedCloseoutPrMergeGate::NotMerged =>
			CloseoutDispatchEligibility::Blocked("pull_request_not_merged"),
		RetainedCloseoutPrMergeGate::PullRequestStateReadFailed =>
			CloseoutDispatchEligibility::Blocked("pull_request_state_read_failed"),
	})
}

fn issue_passes_retry_dispatch_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	hint: RetryIssueStateHint<'_>,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	issue_passes_retry_retention_policy(
		tracker,
		issue,
		project,
		workflow,
		state_store,
		hint,
	)
}

fn issue_passes_retry_retention_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	hint: RetryIssueStateHint<'_>,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();
	let continuation_startable_snapshot = hint
		.preferred_issue_state
		.is_some_and(|state| state == tracker_policy.in_progress_state())
		&& hint.preferred_initial_issue_state.is_some_and(|state| state == issue.state.name)
		&& tracker_policy.startable_states().iter().any(|candidate| candidate == &issue.state.name);

	Ok(issue_has_service_ownership(tracker, issue, project.service_id())?
		&& (issue.state.name == tracker_policy.in_progress_state()
			|| continuation_startable_snapshot)
		&& !issue.has_label(tracker_policy.opt_out_label())
		&& !issue.has_label(tracker_policy.needs_attention_label())
		&& !issue_is_terminal_retry_guarded(issue, project, state_store)?)
}

fn issue_has_service_ownership<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		&tracker::automation_active_label(service_id),
	)
}

fn issue_is_terminal_retry_guarded(
	issue: &TrackerIssue,
	project: &ServiceConfig,
	state_store: &StateStore,
) -> Result<bool> {
	Ok(state_store
		.latest_run_attempt_for_issue(&issue.id)?
		.is_some_and(|attempt| attempt.status() == TERMINAL_GUARDED_RUN_STATUS)
		|| terminal_guard_marker_path(project, &issue.identifier).exists())
}

fn terminal_guard_marker_path(project: &ServiceConfig, issue_identifier: &str) -> PathBuf {
	project.worktree_root().join(issue_identifier).join(TERMINAL_GUARD_MARKER_FILE)
}

fn write_terminal_guard_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
) -> Result<()> {
	let marker_path = worktree_path.join(TERMINAL_GUARD_MARKER_FILE);
	let marker_body = format!("run_id={run_id}\nattempt_number={attempt_number}\n");

	fs::write(marker_path, marker_body)?;

	Ok(())
}

fn write_retry_budget_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	retry_budget_attempt_count: i64,
) -> Result<()> {
	state::write_run_retry_budget_attempt_count(
		worktree_path,
		run_id,
		attempt_number,
		retry_budget_attempt_count,
	)
}

fn retry_budget_base_for_issue_worktree(
	state_store: &StateStore,
	issue_id: &str,
	worktree_path: &Path,
) -> Result<i64> {
	Ok(state_store
		.retry_budget_attempt_count(issue_id)?
			.max(state::read_run_retry_budget_attempt_count(worktree_path)?.unwrap_or(0)))
}

fn retry_budget_base_for_dispatch_mode(
	state_store: &StateStore,
	issue_id: &str,
	worktree_path: &Path,
	dispatch_mode: IssueDispatchMode,
	preferred_retry_budget_base: Option<i64>,
) -> Result<i64> {
	let preferred_retry_budget_base = preferred_retry_budget_base.unwrap_or(0);

	if matches!(dispatch_mode, IssueDispatchMode::Normal | IssueDispatchMode::Program) {
		return Ok(preferred_retry_budget_base);
	}

	Ok(preferred_retry_budget_base.max(retry_budget_base_for_issue_worktree(
		state_store,
		issue_id,
		worktree_path,
	)?))
}

fn issue_retry_budget_exhausted(
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_id: &str,
) -> Result<bool> {
	if let Some(mapping) = state_store.worktree_for_issue(issue_id)? {
		return issue_retry_budget_exhausted_for_worktree(
			workflow,
			state_store,
			issue_id,
			mapping.worktree_path(),
		);
	}

	let retry_budget_attempts = state_store.retry_budget_attempt_count(issue_id)?;

	Ok(retry_budget_attempts >= i64::from(workflow.frontmatter().execution().max_attempts()))
}

fn issue_retry_budget_exhausted_for_worktree(
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_id: &str,
	worktree_path: &Path,
) -> Result<bool> {
	let retry_budget_attempts =
		retry_budget_base_for_issue_worktree(state_store, issue_id, worktree_path)?;

	Ok(retry_budget_attempts >= i64::from(workflow.frontmatter().execution().max_attempts()))
}

fn clear_terminal_guard_marker(worktree_path: &Path) -> Result<()> {
	let marker_path = worktree_path.join(TERMINAL_GUARD_MARKER_FILE);

	match fs::remove_file(&marker_path) {
		Ok(()) => Ok(()),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
		Err(error) => Err(error.into()),
	}
}

fn clear_recovered_issue_lease(
	project_id: &str,
	issue_id: &str,
	expected_run_id: Option<&str>,
	state_store: &StateStore,
) -> Result<()> {
	let Some(lease) = state_store.lease_for_issue(issue_id)? else {
		return Ok(());
	};

	if lease.project_id() != project_id {
		return Ok(());
	}
	if expected_run_id.is_some_and(|run_id| lease.run_id() != run_id) {
		return Ok(());
	}

	state_store.clear_lease(issue_id)
}

fn is_issue_eligible<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let queue_label = tracker::automation_queue_label(project_id);

	if !issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, true)? {
		return Ok(false);
	}

	Ok(state_store.lease_for_issue(&issue.id)?.is_none())
}

fn todo_blocker_rule_passes(issue: &TrackerIssue, workflow: &WorkflowDocument) -> bool {
	if issue.state.name != "Todo" {
		return true;
	}

	issue.blockers.iter().all(|blocker| state_name_is_terminal(&blocker.state.name, workflow))
}

fn refresh_issue<T>(tracker: &T, issue_id: &str) -> Result<Option<TrackerIssue>>
where
	T: IssueTracker,
{
	let issue_ids = [issue_id.to_owned()];
	let mut refreshed_issues = tracker.refresh_issues(&issue_ids)?;

	Ok(refreshed_issues.pop())
}

fn is_terminal_issue(issue: &TrackerIssue, workflow: &WorkflowDocument) -> bool {
	state_name_is_terminal(&issue.state.name, workflow)
}

fn state_name_is_terminal(state_name: &str, workflow: &WorkflowDocument) -> bool {
	workflow.frontmatter().tracker().terminal_states().iter().any(|state| state == state_name)
}

fn is_issue_in_progress_for_run(issue: &TrackerIssue, workflow: &WorkflowDocument) -> bool {
	let tracker_policy = workflow.frontmatter().tracker();

	issue.state.name == tracker_policy.in_progress_state()
		&& !issue.has_label(tracker_policy.needs_attention_label())
}

fn is_issue_not_dispatchable_for_run(issue: &TrackerIssue, workflow: &WorkflowDocument) -> bool {
	let tracker_policy = workflow.frontmatter().tracker();

	issue.has_label(tracker_policy.opt_out_label())
		|| issue.has_label(tracker_policy.needs_attention_label())
		|| (issue.state.name != tracker_policy.in_progress_state()
			&& !tracker_policy.startable_states().iter().any(|state| state == &issue.state.name))
}

fn is_issue_not_dispatchable_for_current_dispatch<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	dispatch_mode: IssueDispatchMode,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	match dispatch_mode {
		IssueDispatchMode::ReviewRepair => {
			Ok(!issue_passes_review_repair_dispatch_policy(tracker, issue, project, workflow)?)
		},
		IssueDispatchMode::Normal
		| IssueDispatchMode::Program
		| IssueDispatchMode::Retry
		| IssueDispatchMode::Closeout => Ok(is_issue_not_dispatchable_for_run(issue, workflow)),
	}
}

fn mark_run_attempt_if_active(
	state_store: &StateStore,
	run_id: &str,
	reconciled_status: &str,
) -> Result<()> {
	let Some(run_attempt) = state_store.run_attempt(run_id)? else {
		return Ok(());
	};

	if matches!(run_attempt.status(), "starting" | "running") {
		state_store.update_run_status(run_id, reconciled_status)?;
	}

	Ok(())
}

fn cleanup_worktree_mapping(
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	workflow: &WorkflowDocument,
	issue_identifier: &str,
	mapping: &WorktreeMapping,
) -> Result<()> {
	worktree_manager.remove_worktree_path_with_hooks(
		issue_identifier,
		mapping.branch_name(),
		mapping.worktree_path(),
		workflow.frontmatter().execution().workspace_hooks(),
	)?;
	state_store.clear_worktree(mapping.issue_id())?;

	Ok(())
}

fn cleanup_terminal_worktree(
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	workflow: &WorkflowDocument,
	issue_id: &str,
	issue_identifier: &str,
	branch_name: &str,
	worktree_path: &Path,
) -> Result<()> {
	worktree_manager.remove_worktree_path_with_hooks(
		issue_identifier,
		branch_name,
		worktree_path,
		workflow.frontmatter().execution().workspace_hooks(),
	)?;
	state_store.clear_worktree(issue_id)?;

	Ok(())
}

fn clear_worktree_retry_schedule(
	state_store: &StateStore,
	issue_id: &str,
) -> Result<()> {
	let Some(worktree) = state_store.worktree_for_issue(issue_id)? else {
		return Ok(());
	};

	state::clear_run_retry_schedule(worktree.worktree_path())
}

fn cleanup_completed_post_review_lane(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
) -> Result<()>
{
	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());
	let review_handoff = state_store
		.review_handoff_marker(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.worktree.branch_name,
		)?
		.ok_or_else(|| {
			eyre::eyre!(
				"Retained closeout cleanup for issue `{}` requires an existing runtime review handoff.",
				issue_run.issue.identifier
			)
		})?;
	let default_branch =
		review_handoff.target_base_ref_name().ok_or_else(|| {
			eyre::eyre!(
				"Retained closeout cleanup for issue `{}` requires the review handoff marker to record the PR target base branch.",
				issue_run.issue.identifier
			)
		})?;
	let github_token = project.github().resolve_token()?;
	let landing_state = github::inspect_pull_request_landing_state(
		&issue_run.worktree.path,
		review_handoff.pr_url(),
		&github_token,
		project.github().command_path(),
	)?;

	if landing_state.state != "MERGED" {
		eyre::bail!(
			"Retained closeout cleanup for issue `{}` requires PR `{}` to be merged, but GitHub reports `{}`.",
			issue_run.issue.identifier,
			review_handoff.pr_url(),
			landing_state.state
		);
	}
	if landing_state.base_ref_name != default_branch {
		eyre::bail!(
			"Retained closeout cleanup for issue `{}` expected PR `{}` target branch `{}`, but GitHub reports `{}`. Re-run review handoff/repair before cleanup.",
			issue_run.issue.identifier,
			review_handoff.pr_url(),
			default_branch,
			landing_state.base_ref_name
		);
	}

	let git_credentials = GitCredentialSource::new(
		project.github().token_env_var(),
		&github_token,
		project.worktree_root(),
	);

	default_branch_sync::sync_repo_root_default_branch(
		project.repo_root(),
		default_branch,
		Some(git_credentials),
	)?;
	github::delete_pull_request_head_branch_if_present(
		project.repo_root(),
		review_handoff.pr_url(),
		&issue_run.worktree.branch_name,
		&github_token,
		project.github().command_path(),
	)?;

	detach_worktree_head_from_branch_if_checked_out(
		&issue_run.worktree.path,
		&issue_run.worktree.branch_name,
	)?;
	delete_local_branch_if_present(project.repo_root(), &issue_run.worktree.branch_name)?;

	worktree_manager.remove_worktree_path_with_hooks(
		&issue_run.issue.identifier,
		&issue_run.worktree.branch_name,
		&issue_run.worktree.path,
		workflow.frontmatter().execution().workspace_hooks(),
	)?;
	state_store.clear_worktree(&issue_run.issue.id)?;

	Ok(())
}
