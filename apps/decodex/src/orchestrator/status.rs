use records::LinearExecutionEventRecord;

use crate::pull_request::{self, PullRequestLandingGateView};
use crate::worktree;
use crate::worktree::MergedWorktreeCleanupDebt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RetainedCloseoutPrMergeGate {
	Merged,
	NotMerged,
	PullRequestStateReadFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExternalReviewRequestCiGate {
	Ready,
	WaitForGreenChecks,
	RepairRequired,
	ManualAttention(&'static str),
}

struct PostReviewOrchestrationStatus {
	phase: ReviewOrchestrationPhase,
	request_acknowledged: bool,
	review_result_arrived: bool,
	strict_pass: bool,
	clean_path_landing_gates_satisfied: bool,
	landing_requires_agent_fallback: bool,
}
impl PostReviewOrchestrationStatus {
	fn from_review_state(
		review_state: &PullRequestReviewState,
		orchestration_marker: &ReviewOrchestrationMarker,
	) -> crate::prelude::Result<Self> {
		let phase =
			ReviewOrchestrationPhase::parse(orchestration_marker.phase()).map_err(|error| {
				eyre::eyre!("Failed to parse retained review orchestration phase: {error}")
			})?;

		Ok(Self {
			phase,
			request_acknowledged: request_comment_has_eyes(review_state, orchestration_marker)
				.unwrap_or(false),
			review_result_arrived: external_review_result_arrived(
				review_state,
				orchestration_marker,
			),
			strict_pass: external_review_has_strict_pass_signals(
				review_state,
				orchestration_marker,
			),
			clean_path_landing_gates_satisfied:
				review_state_clean_path_landing_gates_satisfied(review_state),
			landing_requires_agent_fallback: review_state_landing_requires_agent_fallback(
				review_state,
			),
		})
	}
}

struct OperatorRunTiming {
	process_id: Option<u32>,
	process_alive: Option<bool>,
	process_liveness_reason: Option<String>,
	last_run_activity_unix_epoch: Option<i64>,
	last_protocol_activity_unix_epoch: Option<i64>,
	last_progress_unix_epoch: Option<i64>,
	idle_for_seconds: Option<i64>,
	protocol_idle_for_seconds: Option<i64>,
}

#[derive(Clone, Copy)]
struct MarkerProcessLiveness {
	alive: bool,
	reason: &'static str,
}

struct OperatorRunAppServerState {
	thread_id: Option<String>,
	turn_id: Option<String>,
	thread_status: Option<String>,
	thread_active_flags: Vec<String>,
	interactive_requested: bool,
	continuation_pending: bool,
	effective_model: Option<String>,
	effective_model_provider: Option<String>,
	effective_cwd: Option<String>,
	effective_approval_policy: Option<String>,
	effective_approvals_reviewer: Option<String>,
	effective_sandbox_mode: Option<String>,
}

struct OperatorRunProtocolSummary {
	last_event_type: Option<String>,
	last_event_at: Option<String>,
	event_count: i64,
}

struct PostReviewLaneBuildContext<'a, I> {
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	review_state_inspector: &'a I,
	success_state: &'a str,
	completed_state: &'a str,
}

struct OperatorHistoryLedgerRecord {
	record: LinearExecutionEventRecord,
	event_unix_epoch: Option<i64>,
	sort_unix_epoch: Option<i64>,
	comment_index: usize,
}

struct OperatorIssueDisplayMetadata {
	issue_identifier: String,
	title: Option<String>,
	author: Option<String>,
}

struct WorktreeOwnership {
	kind: &'static str,
	reason: String,
}

pub(crate) fn ensure_project_has_no_merged_worktree_cleanup_debt(
	project: &ServiceConfig,
) -> crate::prelude::Result<()> {
	let debts = project_merged_worktree_cleanup_debts(project)?;

	if debts.is_empty() {
		return Ok(());
	}

	eyre::bail!(
		"Post-land worktree cleanup is pending for project `{}`; remove or salvage merged linked worktrees before continuing automation: {}",
		project.service_id(),
		format_merged_worktree_cleanup_debts(&debts)
	);
}

fn build_operator_status_snapshot(
	project: &ServiceConfig,
	state_store: &StateStore,
	limit: usize,
) -> crate::prelude::Result<OperatorStatusSnapshot> {
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let (active_runs, recent_runs) = state_store.list_project_runs(project.service_id(), limit)?;
	let recent_runs = recent_runs
		.into_iter()
		.map(|run| operator_run_status(project, run, now_unix_epoch))
		.collect::<crate::prelude::Result<Vec<_>>>()?;
	let mut active_runs = active_runs
		.into_iter()
		.map(|run| operator_run_status(project, run, now_unix_epoch))
		.collect::<crate::prelude::Result<Vec<_>>>()?
		.into_iter()
		.filter(operator_run_counts_as_active)
		.collect::<Vec<_>>();
	let mut active_run_ids =
		active_runs.iter().map(|run| run.run_id.clone()).collect::<HashSet<_>>();

	for run in &recent_runs {
		if !active_run_ids.contains(&run.run_id) && operator_run_has_live_process(run) {
			active_run_ids.insert(run.run_id.clone());
			active_runs.push(run.clone());
		}
	}

	let history_lanes = operator_history_lanes(&active_runs, &recent_runs);
	let (worktrees, mut warnings) = operator_status_worktrees(project, state_store)?;
	let accounts = codex_account_activity_summaries(project, &mut warnings);
	let mut snapshot = OperatorStatusSnapshot {
		project_id: project.service_id().to_owned(),
		run_limit: limit,
		warnings,
		connector_backoffs: Vec::new(),
		projects: vec![OperatorProjectStatus {
			project_id: project.service_id().to_owned(),
			config_path: String::new(),
			repo_root: project.repo_root().display().to_string(),
			enabled: true,
			active_run_count: active_runs.len(),
			queued_candidate_count: 0,
			post_review_lane_count: 0,
			retained_worktree_count: 0,
			waiting_lane_count: 0,
			attention_count: 0,
			connector_state: String::from("ok"),
			last_activity_at: None,
			warning_count: 0,
		}],
		account_control: global_codex_account_control_status(),
		accounts,
		active_runs,
		recent_runs,
		history_lanes,
		queued_candidates: Vec::new(),
		worktrees,
		post_review_lanes: Vec::new(),
	};

	refresh_worktree_ownership(&mut snapshot, None);
	refresh_operator_project_summary(&mut snapshot);

	Ok(snapshot)
}

fn global_codex_account_control_status() -> OperatorCodexAccountControlStatus {
	let account_selector = runtime::global_fixed_account_selector()
		.ok()
		.flatten();
	let mode = if account_selector.is_some() { "fixed" } else { "balanced" };

	OperatorCodexAccountControlStatus {
		mode: String::from(mode),
		account_selector,
	}
}

fn build_live_operator_status_snapshot<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
) -> crate::prelude::Result<OperatorStatusSnapshot>
where
	T: IssueTracker,
{
	build_live_operator_status_snapshot_with_history_ledger(
		tracker,
		project,
		workflow,
		state_store,
		limit,
		true,
	)
}

fn build_control_plane_operator_status_snapshot<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
) -> crate::prelude::Result<OperatorStatusSnapshot>
where
	T: IssueTracker,
{
	build_live_operator_status_snapshot_with_history_ledger(
		tracker,
		project,
		workflow,
		state_store,
		limit,
		false,
	)
}

fn build_live_operator_status_snapshot_with_history_ledger<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
	hydrate_history_ledger: bool,
) -> crate::prelude::Result<OperatorStatusSnapshot>
where
	T: IssueTracker,
{
	state_store.configure_dispatch_slot_root(
		project.service_id(),
		project.worktree_root(),
		workflow.frontmatter().execution().max_concurrent_agents(),
	)?;

	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
	};
	let mut snapshot = build_operator_status_snapshot(project, state_store, limit)?;

	hydrate_history_lanes_from_local_ledger(project, state_store, &mut snapshot)?;

	if hydrate_operator_run_rows_from_tracker(
		tracker,
		project,
		&mut snapshot,
	) {
		add_operator_snapshot_warning(&mut snapshot, "run_issue_metadata_unavailable");
	}
	if hydrate_history_ledger && hydrate_history_lanes_from_linear_ledger(tracker, project, &mut snapshot) {
		add_operator_snapshot_warning(&mut snapshot, "execution_ledger_status_unavailable");
	}

	match build_queued_candidate_statuses(tracker, project, workflow, state_store) {
		Ok(queued_candidates) => snapshot.queued_candidates = queued_candidates,
		Err(error) => {
			let _ = error;

			tracing::warn!(
				"Skipped queued candidate status while publishing an operator snapshot; sensitive runtime details were withheld."
			);

			add_operator_snapshot_warning(&mut snapshot, "queued_candidate_status_unavailable");
		},
	}
	match build_post_review_lane_statuses_and_hydrate_worktrees(
		tracker,
		project,
		workflow,
		state_store,
		&review_state_inspector,
		&mut snapshot,
	) {
		Ok(post_review_lanes) => snapshot.post_review_lanes = post_review_lanes,
		Err(error) => {
			let _ = error;

			tracing::warn!(
				"Skipped post-review lane status while publishing an operator snapshot; sensitive runtime details were withheld."
			);

			add_operator_snapshot_warning(&mut snapshot, "post_review_lane_status_unavailable");
		},
	}

	refresh_worktree_ownership(
		&mut snapshot,
		Some(workflow.frontmatter().tracker().resolved_completed_state()),
	);
	refresh_operator_project_summary(&mut snapshot);

	Ok(snapshot)
}

fn hydrate_history_lanes_from_local_ledger(
	project: &ServiceConfig,
	state_store: &StateStore,
	snapshot: &mut OperatorStatusSnapshot,
) -> crate::prelude::Result<()> {
	for lane in &mut snapshot.history_lanes {
		let records =
			state_store.list_linear_execution_events(project.service_id(), &lane.issue_id)?;

		if records.is_empty() {
			lane.ledger_outcome = missing_history_ledger_outcome();

			continue;
		}

		let records = local_history_ledger_records(records);

		hydrate_history_lane_from_ledger_records(lane, &records);

		lane.ledger_outcome = operator_history_ledger_outcome(&records);
	}

	Ok(())
}

fn refresh_worktree_ownership(
	snapshot: &mut OperatorStatusSnapshot,
	completed_state: Option<&str>,
) {
	let ownership = snapshot
		.worktrees
		.iter()
		.map(|worktree| worktree_ownership(worktree, snapshot, completed_state))
		.collect::<Vec<_>>();

	for (worktree, ownership) in snapshot.worktrees.iter_mut().zip(ownership) {
		worktree.ownership = ownership.kind.to_owned();
		worktree.ownership_reason = ownership.reason;
	}
}

fn worktree_ownership(
	worktree: &OperatorWorktreeStatus,
	snapshot: &OperatorStatusSnapshot,
	completed_state: Option<&str>,
) -> WorktreeOwnership {
	if let Some(run) = worktree_active_run_owner(worktree, snapshot) {
		return WorktreeOwnership {
			kind: "active_lane",
			reason: format!("Active lane `{}` owns this worktree.", run.run_id),
		};
	}
	if let Some(lane) = worktree_post_review_owner(worktree, snapshot) {
		return WorktreeOwnership {
			kind: "post_review_lane",
			reason: format!(
				"Review & Landing owns this worktree as `{}`.",
				lane.classification
			),
		};
	}

	if worktree_has_queued_attention_owner(worktree, snapshot) {
		return WorktreeOwnership {
			kind: "queued_attention",
			reason: String::from(
				"Intake Queue owns this worktree because the issue needs operator attention.",
			),
		};
	}

	if let Some(hygiene) = &worktree.hygiene {
		return WorktreeOwnership {
			kind: "post_land_cleanup",
			reason: hygiene.reason.clone(),
		};
	}

	WorktreeOwnership {
		kind: "cleanup_only",
		reason: worktree_cleanup_only_reason(worktree, completed_state),
	}
}

fn worktree_active_run_owner<'a>(
	worktree: &OperatorWorktreeStatus,
	snapshot: &'a OperatorStatusSnapshot,
) -> Option<&'a OperatorRunStatus> {
	snapshot.active_runs.iter().find(|run| {
		run.worktree_path.as_deref() == Some(worktree.worktree_path.as_str())
			|| run.branch_name.as_deref() == Some(worktree.branch_name.as_str())
			|| run.issue_id == worktree.issue_id
	})
}

fn worktree_post_review_owner<'a>(
	worktree: &OperatorWorktreeStatus,
	snapshot: &'a OperatorStatusSnapshot,
) -> Option<&'a OperatorPostReviewLaneStatus> {
	snapshot.post_review_lanes.iter().find(|lane| {
		lane.worktree_path == worktree.worktree_path
			|| lane.branch_name == worktree.branch_name
			|| lane.issue_id == worktree.issue_id
			|| lane.issue_identifier == worktree.issue_id
			|| worktree.issue_identifier.as_deref() == Some(lane.issue_identifier.as_str())
	})
}

fn worktree_has_queued_attention_owner(
	worktree: &OperatorWorktreeStatus,
	snapshot: &OperatorStatusSnapshot,
) -> bool {
	snapshot.queued_candidates.iter().any(|candidate| {
		candidate.reason == "issue_needs_attention"
			&& (candidate
				.attention
				.as_ref()
				.and_then(|attention| attention.worktree_path.as_deref())
				== Some(worktree.worktree_path.as_str())
				|| candidate.issue_id == worktree.issue_id
				|| candidate.issue_identifier == worktree.issue_id
				|| worktree.issue_identifier.as_deref() == Some(candidate.issue_identifier.as_str()))
	})
}

fn worktree_cleanup_only_reason(
	worktree: &OperatorWorktreeStatus,
	completed_state: Option<&str>,
) -> String {
	if let (Some(issue_state), Some(completed_state)) = (worktree.issue_state.as_deref(), completed_state)
		&& issue_state == completed_state
	{
		return format!(
			"Issue is {completed_state}; no active or post-review lane owns this worktree, so it is local cleanup only."
		);
	}

	String::from(
		"No active lane, queued recovery, or post-review lane owns this worktree; local cleanup only.",
	)
}

fn refresh_operator_project_summary(snapshot: &mut OperatorStatusSnapshot) {
	let active_run_count =
		snapshot.active_runs.iter().filter(|run| operator_run_counts_as_running(run)).count();
	let queued_candidate_count = snapshot
		.queued_candidates
		.iter()
		.filter(|candidate| queued_candidate_counts_as_waiting_intake(candidate))
		.count();
	let post_review_lane_count = snapshot.post_review_lanes.len();
	let retained_worktree_count = rendered_recovery_worktrees(snapshot).len();
	let waiting_lane_count = project_waiting_lane_count(snapshot);
	let attention_count = project_attention_count(snapshot);
	let connector_state = project_connector_state(snapshot);
	let last_activity_at = project_last_activity_at(snapshot);
	let warning_count = snapshot.warnings.len();

	if let Some(project_status) = snapshot.projects.first_mut() {
		project_status.active_run_count = active_run_count;
		project_status.queued_candidate_count = queued_candidate_count;
		project_status.post_review_lane_count = post_review_lane_count;
		project_status.retained_worktree_count = retained_worktree_count;
		project_status.waiting_lane_count = waiting_lane_count;
		project_status.attention_count = attention_count;
		project_status.connector_state = connector_state;
		project_status.last_activity_at = last_activity_at;
		project_status.warning_count = warning_count;
	}
}

fn project_waiting_lane_count(snapshot: &OperatorStatusSnapshot) -> usize {
	let waiting_run_count = project_summary_runs(snapshot)
		.into_iter()
		.filter(|run| operator_run_counts_as_waiting(run))
		.map(|run| run.run_id.as_str())
		.collect::<HashSet<_>>()
		.len();
	let queued_waiting = snapshot
		.queued_candidates
		.iter()
		.filter(|candidate| candidate.classification == "waiting")
		.count();
	let review_waiting = snapshot
		.post_review_lanes
		.iter()
		.filter(|lane| lane.classification == "wait_for_review")
		.count();

	waiting_run_count + queued_waiting + review_waiting
}

fn project_summary_runs(snapshot: &OperatorStatusSnapshot) -> Vec<&OperatorRunStatus> {
	let mut runs = snapshot.active_runs.iter().collect::<Vec<_>>();

	runs.extend(snapshot.history_lanes.iter().map(|lane| &lane.latest_run));

	runs
}

fn operator_run_counts_as_waiting(run: &OperatorRunStatus) -> bool {
	run.phase == "retry_backoff" || run.phase == "waiting_continuation" || run.wait_reason.is_some()
}

fn queued_candidate_counts_as_waiting_intake(candidate: &OperatorQueuedIssueStatus) -> bool {
	!matches!(candidate.classification.as_str(), "claimed" | "closed")
}

fn project_attention_count(snapshot: &OperatorStatusSnapshot) -> usize {
	let active_attention = snapshot
		.active_runs
		.iter()
		.filter(|run| operator_run_needs_attention(run))
		.count();
	let queued_attention = snapshot
		.queued_candidates
		.iter()
		.filter(|candidate| candidate.classification == "blocked" || candidate.attention.is_some())
		.count();
	let review_attention = snapshot
		.post_review_lanes
		.iter()
		.filter(|lane| {
			matches!(
				lane.classification.as_str(),
				"blocked" | "needs_review_repair" | "closeout_blocked" | "cleanup_blocked"
			)
		})
		.count();
	let hygiene_attention = snapshot
		.worktrees
		.iter()
		.filter(|worktree| worktree.hygiene.is_some())
		.count();

	active_attention + queued_attention + review_attention + hygiene_attention
}

fn operator_run_counts_as_active(run: &OperatorRunStatus) -> bool {
	(run.active_lease || operator_run_has_live_process(run))
		&& !matches!(run.phase.as_str(), "completed" | "failed" | "terminated")
}

fn operator_run_has_live_process(run: &OperatorRunStatus) -> bool {
	matches!(run.status.as_str(), "starting" | "running") && run.process_alive == Some(true)
}

fn operator_run_counts_as_running(run: &OperatorRunStatus) -> bool {
	matches!(run.status.as_str(), "starting" | "running")
		&& run.phase == "executing"
		&& run.process_alive != Some(false)
		&& !operator_run_needs_attention(run)
}

fn operator_run_needs_attention(run: &OperatorRunStatus) -> bool {
	run.suspected_stall
		|| run.phase == "stalled"
		|| run.process_alive == Some(false)
			&& matches!(run.status.as_str(), "starting" | "running")
			&& run.wait_reason.is_none()
		|| operator_run_has_stale_execution_without_known_process(run)
}

fn operator_run_has_stale_execution_without_known_process(run: &OperatorRunStatus) -> bool {
	matches!(run.status.as_str(), "starting" | "running")
		&& run.phase == "executing"
		&& run.wait_reason.is_none()
		&& run.process_alive != Some(true)
		&& [run.idle_for_seconds, run.protocol_idle_for_seconds].iter().any(|idle_for| {
			idle_for.is_some_and(|idle_for| {
				u64::try_from(idle_for).is_ok_and(|idle_for| idle_for >= ACTIVE_RUN_IDLE_TIMEOUT.as_secs())
			})
		})
}

fn project_connector_state(snapshot: &OperatorStatusSnapshot) -> String {
	if !snapshot.connector_backoffs.is_empty()
		|| snapshot.warnings.iter().any(|warning| warning == TRACKER_RATE_LIMIT_WARNING)
	{
		return String::from("backoff");
	}
	if !snapshot.warnings.is_empty() {
		return String::from("degraded");
	}
	if project_summary_runs(snapshot)
		.into_iter()
		.any(|run| run.phase == "retry_backoff" || run.next_retry_at.is_some())
	{
		return String::from("backoff");
	}

	String::from("ok")
}

fn project_last_activity_at(snapshot: &OperatorStatusSnapshot) -> Option<String> {
	snapshot
		.active_runs
		.iter()
		.chain(snapshot.recent_runs.iter())
		.flat_map(|run| {
			[
				run.last_progress_at.as_deref(),
				run.last_run_activity_at.as_deref(),
				run.last_protocol_activity_at.as_deref(),
				run.last_event_at.as_deref(),
				Some(run.updated_at.as_str()),
			]
		})
		.flatten()
		.max()
		.map(str::to_owned)
}

fn operator_status_worktrees(
	project: &ServiceConfig,
	state_store: &StateStore,
) -> crate::prelude::Result<(Vec<OperatorWorktreeStatus>, Vec<String>)> {
	let mut worktrees = state_store
		.list_worktrees(project.service_id())?
		.into_iter()
		.map(|mapping| OperatorWorktreeStatus {
			issue_id: mapping.issue_id().to_owned(),
			issue_identifier: issue_identifier_in_text(mapping.branch_name())
				.or_else(|| issue_identifier_in_text(&mapping.worktree_path().display().to_string())),
			issue_state: None,
			branch_name: mapping.branch_name().to_owned(),
			worktree_path: relative_worktree_path_for_path(project, mapping.worktree_path()),
			ownership: String::from("cleanup_only"),
			ownership_reason: String::from(
				"No active lane, queued recovery, or post-review lane currently owns this worktree.",
			),
			hygiene: None,
		})
		.collect::<Vec<_>>();
	let mut seen_paths =
		worktrees.iter().map(|worktree| worktree.worktree_path.clone()).collect::<HashSet<_>>();
	let mut warnings = Vec::new();

	for issue_identifier in recoverable_worktree_identifiers(project.worktree_root())? {
		let worktree_path = project.worktree_root().join(&issue_identifier);
		let relative_path = relative_worktree_path_for_path(project, &worktree_path);

		if !seen_paths.insert(relative_path.clone()) {
			continue;
		}

		let branch_name = worktree_checkout_branch_name(&worktree_path)
			.ok()
			.flatten()
			.unwrap_or_else(|| issue_identifier.clone());

		worktrees.push(OperatorWorktreeStatus {
			issue_identifier: Some(issue_identifier.clone()),
			issue_id: issue_identifier,
			issue_state: None,
			branch_name,
			worktree_path: relative_path,
			ownership: String::from("cleanup_only"),
			ownership_reason: String::from(
				"No active lane, queued recovery, or post-review lane currently owns this worktree.",
			),
			hygiene: None,
		});
	}

	append_merged_worktree_cleanup_debts(project, &mut worktrees, &mut seen_paths, &mut warnings);

	worktrees.sort_by(|left, right| {
		left.issue_id
			.cmp(&right.issue_id)
			.then_with(|| left.branch_name.cmp(&right.branch_name))
			.then_with(|| left.worktree_path.cmp(&right.worktree_path))
	});

	Ok((worktrees, warnings))
}

fn append_merged_worktree_cleanup_debts(
	project: &ServiceConfig,
	worktrees: &mut Vec<OperatorWorktreeStatus>,
	seen_paths: &mut HashSet<String>,
	warnings: &mut Vec<String>,
) {
	let debts = match project_merged_worktree_cleanup_debts(project) {
		Ok(debts) => debts,
		Err(error) => {
			tracing::warn!(
				project_id = project.service_id(),
				error = %error,
				"Skipped merged worktree cleanup debt scan while publishing an operator snapshot."
			);

			warnings.push(String::from("worktree_hygiene_unavailable"));

			return;
		},
	};

	if debts.is_empty() {
		return;
	}

	let mut surfaced_cleanup_debt = false;
	let mut surfaced_dirty_cleanup_debt = false;

	for debt in debts {
		let relative_path = relative_worktree_path_for_path(project, &debt.path);
		let is_dirty = debt.cleanliness.is_dirty();
		let debt_status = operator_worktree_status_from_cleanup_debt(debt, relative_path.clone());

		if !seen_paths.insert(relative_path.clone()) {
			if let Some(existing) =
				worktrees.iter_mut().find(|worktree| worktree.worktree_path == relative_path)
			{
				existing.hygiene = debt_status.hygiene;
			}

			continue;
		}

		surfaced_cleanup_debt = true;
		surfaced_dirty_cleanup_debt |= is_dirty;

		worktrees.push(debt_status);
	}

	if surfaced_cleanup_debt {
		warnings.push(String::from("merged_worktree_cleanup_pending"));
	}
	if surfaced_dirty_cleanup_debt {
		warnings.push(String::from("merged_dirty_worktree"));
	}
}

fn operator_worktree_status_from_cleanup_debt(
	debt: MergedWorktreeCleanupDebt,
	relative_path: String,
) -> OperatorWorktreeStatus {
	let dirty = debt.cleanliness.is_dirty();
	let classification = if dirty {
		"merged_dirty_worktree"
	} else {
		"merged_worktree_cleanup_pending"
	};
	let default_branch = debt.default_branch.clone();
	let reason = format!(
		"Branch `{}` is already merged into `{}` but linked worktree `{}` still exists{}; remove or salvage it before continuing automation.",
		debt.branch_name,
		default_branch,
		relative_path,
		if dirty { " with local changes" } else { "" },
	);
	let branch_name = debt.branch_name;

	OperatorWorktreeStatus {
		issue_id: branch_name.clone(),
		issue_identifier: issue_identifier_in_text(&branch_name)
			.or_else(|| issue_identifier_in_text(&relative_path)),
		issue_state: None,
		branch_name,
		worktree_path: relative_path,
		ownership: String::from("post_land_cleanup"),
		ownership_reason: reason.clone(),
		hygiene: Some(OperatorWorktreeHygieneStatus {
			classification: String::from(classification),
			default_branch,
			dirty,
			reason,
		}),
	}
}

fn add_operator_snapshot_warning(snapshot: &mut OperatorStatusSnapshot, warning: &str) {
	if !snapshot.warnings.iter().any(|existing| existing == warning) {
		snapshot.warnings.push(warning.to_owned());
	}
}

fn project_merged_worktree_cleanup_debts(
	project: &ServiceConfig,
) -> crate::prelude::Result<Vec<MergedWorktreeCleanupDebt>> {
	let Some(default_branch) = worktree::infer_default_branch_name(project.repo_root())? else {
		return Ok(Vec::new());
	};

	worktree::merged_worktree_cleanup_debts(
		project.repo_root(),
		project.worktree_root(),
		&default_branch,
	)
}

fn format_merged_worktree_cleanup_debts(
	debts: &[MergedWorktreeCleanupDebt],
) -> String {
	debts
		.iter()
		.map(|debt| {
			format!(
				"{} on {} ({})",
				debt.path.display(),
				debt.branch_name,
				if debt.cleanliness.is_dirty() { "dirty" } else { "clean" }
			)
		})
		.collect::<Vec<_>>()
		.join(", ")
}

fn codex_account_activity_summaries(
	project: &ServiceConfig,
	warnings: &mut Vec<String>,
) -> Vec<CodexAccountActivitySummary> {
	let Some(accounts_config) = project.codex().accounts() else {
		return Vec::new();
	};

	match CodexAccountPool::from_config(accounts_config)
		.and_then(|pool| pool.account_activity_summaries_cached(false))
	{
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

fn hydrate_operator_run_rows_from_tracker<T>(
	tracker: &T,
	project: &ServiceConfig,
	snapshot: &mut OperatorStatusSnapshot,
) -> bool
where
	T: IssueTracker,
{
	let issue_ids = operator_snapshot_run_issue_ids(snapshot);

	if issue_ids.is_empty() {
		return false;
	}

	match tracker.refresh_issues(&issue_ids) {
		Ok(issues) => {
			let metadata_by_issue_id = issues
				.into_iter()
				.map(|issue| {
					(
						issue.id,
						OperatorIssueDisplayMetadata {
							issue_identifier: issue.identifier,
							title: Some(issue.title),
							author: issue.author,
						},
					)
				})
				.collect::<HashMap<_, _>>();

			hydrate_operator_snapshot_run_rows(snapshot, &metadata_by_issue_id);

			false
		},
		Err(error) => {
			let _ = error;

			tracing::warn!(
				project_id = project.service_id(),
				"Skipped tracker issue metadata hydration for operator run rows; sensitive tracker details were withheld."
			);

			true
		},
	}
}

fn operator_snapshot_run_issue_ids(snapshot: &OperatorStatusSnapshot) -> Vec<String> {
	let mut issue_ids = BTreeSet::new();

	for run in snapshot.active_runs.iter().chain(snapshot.recent_runs.iter()) {
		append_operator_run_issue_id(&mut issue_ids, run);
	}
	for lane in &snapshot.history_lanes {
		append_operator_run_issue_id(&mut issue_ids, &lane.latest_run);

		for attempt in &lane.attempts {
			append_operator_run_issue_id(&mut issue_ids, attempt);
		}
	}

	issue_ids.into_iter().collect()
}

fn append_operator_run_issue_id(issue_ids: &mut BTreeSet<String>, run: &OperatorRunStatus) {
	let issue_id = run.issue_id.trim();

	if !issue_id.is_empty() && !issue_id.eq_ignore_ascii_case("unknown") {
		issue_ids.insert(issue_id.to_owned());
	}
}

fn hydrate_operator_snapshot_run_rows(
	snapshot: &mut OperatorStatusSnapshot,
	metadata_by_issue_id: &HashMap<String, OperatorIssueDisplayMetadata>,
) {
	for run in snapshot.active_runs.iter_mut().chain(snapshot.recent_runs.iter_mut()) {
		hydrate_operator_run_row_from_issue_metadata(run, metadata_by_issue_id);
	}
	for lane in &mut snapshot.history_lanes {
		hydrate_history_lane_from_issue_metadata(lane, metadata_by_issue_id);
	}
}

fn hydrate_history_lane_from_issue_metadata(
	lane: &mut OperatorHistoryLaneStatus,
	metadata_by_issue_id: &HashMap<String, OperatorIssueDisplayMetadata>,
) {
	if let Some(metadata) = metadata_by_issue_id.get(&lane.issue_id) {
		apply_history_lane_issue_metadata(lane, metadata);
	}

	hydrate_operator_run_row_from_issue_metadata(&mut lane.latest_run, metadata_by_issue_id);

	for attempt in &mut lane.attempts {
		hydrate_operator_run_row_from_issue_metadata(attempt, metadata_by_issue_id);
	}
}

fn hydrate_operator_run_row_from_issue_metadata(
	run: &mut OperatorRunStatus,
	metadata_by_issue_id: &HashMap<String, OperatorIssueDisplayMetadata>,
) {
	if let Some(metadata) = metadata_by_issue_id.get(&run.issue_id) {
		apply_run_issue_metadata(run, metadata);
	}
}

fn apply_history_lane_issue_metadata(
	lane: &mut OperatorHistoryLaneStatus,
	metadata: &OperatorIssueDisplayMetadata,
) {
	if !metadata.issue_identifier.trim().is_empty() {
		lane.issue_identifier = Some(metadata.issue_identifier.clone());
		lane.issue_key = metadata.issue_identifier.clone();
	}

	if let Some(title) = metadata.title.as_ref().filter(|title| !title.trim().is_empty()) {
		lane.title = Some(title.clone());
	}
	if let Some(author) = metadata.author.as_ref().filter(|author| !author.trim().is_empty()) {
		lane.author = Some(author.clone());
	}
}

fn apply_run_issue_metadata(
	run: &mut OperatorRunStatus,
	metadata: &OperatorIssueDisplayMetadata,
) {
	if !metadata.issue_identifier.trim().is_empty() {
		run.issue_identifier = Some(metadata.issue_identifier.clone());
	}

	if let Some(title) = metadata.title.as_ref().filter(|title| !title.trim().is_empty()) {
		run.title = Some(title.clone());
	}
	if let Some(author) = metadata.author.as_ref().filter(|author| !author.trim().is_empty()) {
		run.author = Some(author.clone());
	}
}

fn fill_missing_history_lane_issue_metadata(
	lane: &mut OperatorHistoryLaneStatus,
	metadata: &OperatorIssueDisplayMetadata,
) {
	if lane
		.issue_identifier
		.as_ref()
		.is_none_or(|identifier| identifier.trim().is_empty())
		&& !metadata.issue_identifier.trim().is_empty()
	{
		lane.issue_identifier = Some(metadata.issue_identifier.clone());
		lane.issue_key = metadata.issue_identifier.clone();
	}
	if lane.title.as_ref().is_none_or(|title| title.trim().is_empty())
		&& let Some(title) = metadata.title.as_ref().filter(|title| !title.trim().is_empty())
	{
		lane.title = Some(title.clone());
	}
	if lane.author.as_ref().is_none_or(|author| author.trim().is_empty())
		&& let Some(author) = metadata.author.as_ref().filter(|author| !author.trim().is_empty())
	{
		lane.author = Some(author.clone());
	}
}

fn fill_missing_run_issue_metadata(
	run: &mut OperatorRunStatus,
	metadata: &OperatorIssueDisplayMetadata,
) {
	if run
		.issue_identifier
		.as_ref()
		.is_none_or(|identifier| identifier.trim().is_empty())
		&& !metadata.issue_identifier.trim().is_empty()
	{
		run.issue_identifier = Some(metadata.issue_identifier.clone());
	}
	if run.title.as_ref().is_none_or(|title| title.trim().is_empty())
		&& let Some(title) = metadata.title.as_ref().filter(|title| !title.trim().is_empty())
	{
		run.title = Some(title.clone());
	}
	if run.author.as_ref().is_none_or(|author| author.trim().is_empty())
		&& let Some(author) = metadata.author.as_ref().filter(|author| !author.trim().is_empty())
	{
		run.author = Some(author.clone());
	}
}

fn hydrate_history_lanes_from_linear_ledger<T>(
	tracker: &T,
	project: &ServiceConfig,
	snapshot: &mut OperatorStatusSnapshot,
) -> bool
where
	T: IssueTracker,
{
	let mut unavailable = false;

	for lane in &mut snapshot.history_lanes {
		match tracker.list_comments(&lane.issue_id) {
			Ok(comments) => {
				let records =
					collect_history_ledger_records(project.service_id(), &lane.issue_id, &comments);

				hydrate_history_lane_from_ledger_records(lane, &records);

				lane.ledger_outcome = operator_history_ledger_outcome(&records);
			},
			Err(error) => {
				let _ = error;

				tracing::warn!(
					issue_id = %lane.issue_id,
					"Skipped Linear execution ledger lookup for a history lane; sensitive tracker details were withheld."
				);

				unavailable = true;
				lane.ledger_outcome = unavailable_history_ledger_outcome();
			},
		}
	}

	unavailable
}

fn hydrate_history_lane_from_ledger_records(
	lane: &mut OperatorHistoryLaneStatus,
	records: &[OperatorHistoryLedgerRecord],
) {
	let Some(record) =
		records.iter().rev().find(|entry| !entry.record.issue_identifier.trim().is_empty())
	else {
		return;
	};
	let metadata = OperatorIssueDisplayMetadata {
		issue_identifier: record.record.issue_identifier.clone(),
		title: None,
		author: None,
	};

	fill_missing_history_lane_issue_metadata(lane, &metadata);
	fill_missing_run_issue_metadata(&mut lane.latest_run, &metadata);

	for attempt in &mut lane.attempts {
		fill_missing_run_issue_metadata(attempt, &metadata);
	}
}

fn local_history_ledger_records(
	records: Vec<LinearExecutionEventRecord>,
) -> Vec<OperatorHistoryLedgerRecord> {
	let mut records = records
		.into_iter()
		.enumerate()
		.map(|(comment_index, record)| {
			let event_unix_epoch = parse_rfc3339_unix_epoch(&record.event_timestamp);

			OperatorHistoryLedgerRecord {
				record,
				event_unix_epoch,
				sort_unix_epoch: event_unix_epoch,
				comment_index,
			}
		})
		.collect::<Vec<_>>();

	records.sort_by(compare_history_ledger_record_position);

	records
}

fn operator_history_ledger_outcome(
	records: &[OperatorHistoryLedgerRecord],
) -> OperatorHistoryLedgerOutcome {
	let Some(final_record) = final_history_ledger_record(records) else {
		return missing_history_ledger_outcome();
	};
	let ledger_status = if history_ledger_event_outcome_rank(&final_record.record.event_type) > 1 {
		String::from("present")
	} else {
		String::from("partial")
	};
	let (started_at, finished_at, elapsed_seconds) = history_ledger_timing(records);

	OperatorHistoryLedgerOutcome {
		ledger_status,
		final_outcome: final_record.record.event_type.clone(),
		final_event_type: Some(final_record.record.event_type.clone()),
		final_event_at: Some(final_record.record.event_timestamp.clone()),
		summary: history_ledger_summary(final_record, records),
		pr_url: latest_history_ledger_text(records, |record| record.pr_url.as_deref()),
		commit_sha: latest_history_ledger_text(records, |record| record.commit_sha.as_deref()),
		branch: latest_history_ledger_text(records, |record| record.branch.as_deref()),
		closeout_status: history_closeout_status(final_record, records),
		needs_attention_reason: history_attention_reason(final_record),
		lifecycle_started_at: started_at,
		lifecycle_finished_at: finished_at,
		lifecycle_elapsed_seconds: elapsed_seconds,
		record_count: records.len(),
	}
}

fn collect_history_ledger_records(
	service_id: &str,
	issue_id: &str,
	comments: &[TrackerComment],
) -> Vec<OperatorHistoryLedgerRecord> {
	let mut seen_keys = HashSet::new();
	let mut records = comments
		.iter()
		.enumerate()
		.filter_map(|(comment_index, comment)| {
			let record = records::parse_linear_execution_event_record(&comment.body)?;

			if record.service_id != service_id || record.issue_id != issue_id {
				return None;
			}
			if !seen_keys.insert(record.idempotency_key.clone()) {
				return None;
			}

			let event_unix_epoch = parse_rfc3339_unix_epoch(&record.event_timestamp);
			let comment_unix_epoch = parse_rfc3339_unix_epoch(&comment.created_at);

			Some(OperatorHistoryLedgerRecord {
				record,
				event_unix_epoch,
				sort_unix_epoch: event_unix_epoch.or(comment_unix_epoch),
				comment_index,
			})
		})
		.collect::<Vec<_>>();

	records.sort_by(compare_history_ledger_record_position);

	records
}

fn final_history_ledger_record(
	records: &[OperatorHistoryLedgerRecord],
) -> Option<&OperatorHistoryLedgerRecord> {
	records
		.iter()
		.filter(|entry| history_ledger_event_outcome_rank(&entry.record.event_type) > 1)
		.max_by(|left, right| compare_history_ledger_record_position(left, right))
		.or_else(|| records.iter().max_by(|left, right| {
			compare_history_ledger_record_position(left, right)
		}))
}

fn compare_history_ledger_record_position(
	left: &OperatorHistoryLedgerRecord,
	right: &OperatorHistoryLedgerRecord,
) -> Ordering {
	left.sort_unix_epoch
		.cmp(&right.sort_unix_epoch)
		.then_with(|| left.comment_index.cmp(&right.comment_index))
}

fn history_ledger_event_outcome_rank(event_type: &str) -> u8 {
	match event_type {
		"cleanup_complete" => 7,
		"closeout" => 6,
		"needs_attention" | "terminal_failure" => 5,
		"landed" => 4,
		"review_handoff" | "repair_handoff" => 3,
		"pr_opened" | "pr_updated" => 2,
		_ => 1,
	}
}

fn history_ledger_timing(
	records: &[OperatorHistoryLedgerRecord],
) -> (Option<String>, Option<String>, Option<i64>) {
	let started = records.iter().filter_map(|entry| entry.event_unix_epoch).min();
	let finished = records.iter().filter_map(|entry| entry.event_unix_epoch).max();
	let elapsed = started
		.zip(finished)
		.and_then(|(started, finished)| finished.checked_sub(started))
		.filter(|elapsed| *elapsed >= 0);

	(
		started.and_then(|timestamp| format_optional_unix_timestamp(Some(timestamp))),
		finished.and_then(|timestamp| format_optional_unix_timestamp(Some(timestamp))),
		elapsed,
	)
}

fn history_ledger_summary(
	final_record: &OperatorHistoryLedgerRecord,
	records: &[OperatorHistoryLedgerRecord],
) -> Option<String> {
	if history_ledger_event_outcome_rank(&final_record.record.event_type) > 1 {
		return final_record.record.summary.clone();
	}

	Some(format!(
		"Ledger has {} records but no final lane outcome yet; latest event is `{}`.",
		records.len(),
		final_record.record.event_type
	))
}

fn latest_history_ledger_text<F>(
	records: &[OperatorHistoryLedgerRecord],
	field: F,
) -> Option<String>
where
	F: Fn(&LinearExecutionEventRecord) -> Option<&str>,
{
	records.iter().rev().find_map(|entry| field(&entry.record).map(str::to_owned))
}

fn history_closeout_status(
	final_record: &OperatorHistoryLedgerRecord,
	records: &[OperatorHistoryLedgerRecord],
) -> Option<String> {
	match final_record.record.event_type.as_str() {
		"closeout" => closeout_status_from_record(&final_record.record),
		"cleanup_complete" => final_record.record.cleanup_status.clone().or_else(|| {
			records.iter().rev().find_map(|entry| {
				(entry.record.event_type == "closeout")
					.then(|| closeout_status_from_record(&entry.record))
					.flatten()
			})
		}),
		_ => None,
	}
}

fn closeout_status_from_record(record: &LinearExecutionEventRecord) -> Option<String> {
	record
		.target_state
		.clone()
		.or_else(|| record.validation_result.clone())
		.or_else(|| Some(String::from("recorded")))
}

fn history_attention_reason(final_record: &OperatorHistoryLedgerRecord) -> Option<String> {
	match final_record.record.event_type.as_str() {
		"needs_attention" | "terminal_failure" => final_record
			.record
			.summary
			.clone()
			.or_else(|| final_record.record.error_class.clone())
			.or_else(|| final_record.record.next_action.clone()),
		_ => None,
	}
}

fn parse_rfc3339_unix_epoch(value: &str) -> Option<i64> {
	OffsetDateTime::parse(value, &Rfc3339).ok().map(|timestamp| timestamp.unix_timestamp())
}

fn not_loaded_history_ledger_outcome() -> OperatorHistoryLedgerOutcome {
	OperatorHistoryLedgerOutcome {
		ledger_status: String::from("not_loaded"),
		final_outcome: String::from("local_attempt_history"),
		final_event_type: None,
		final_event_at: None,
		summary: Some(String::from(
			"Linear execution ledger was not loaded for this local-only snapshot.",
		)),
		pr_url: None,
		commit_sha: None,
		branch: None,
		closeout_status: None,
		needs_attention_reason: None,
		lifecycle_started_at: None,
		lifecycle_finished_at: None,
		lifecycle_elapsed_seconds: None,
		record_count: 0,
	}
}

fn missing_history_ledger_outcome() -> OperatorHistoryLedgerOutcome {
	OperatorHistoryLedgerOutcome {
		ledger_status: String::from("missing"),
		final_outcome: String::from("execution_ledger_missing"),
		final_event_type: None,
		final_event_at: None,
		summary: Some(String::from(
			"No decodex.linear_execution_event records are available for this history lane.",
		)),
		pr_url: None,
		commit_sha: None,
		branch: None,
		closeout_status: None,
		needs_attention_reason: None,
		lifecycle_started_at: None,
		lifecycle_finished_at: None,
		lifecycle_elapsed_seconds: None,
		record_count: 0,
	}
}

fn unavailable_history_ledger_outcome() -> OperatorHistoryLedgerOutcome {
	OperatorHistoryLedgerOutcome {
		ledger_status: String::from("unavailable"),
		final_outcome: String::from("ledger_unavailable"),
		final_event_type: None,
		final_event_at: None,
		summary: Some(String::from(
			"Linear execution ledger records could not be loaded for this issue.",
		)),
		pr_url: None,
		commit_sha: None,
		branch: None,
		closeout_status: None,
		needs_attention_reason: None,
		lifecycle_started_at: None,
		lifecycle_finished_at: None,
		lifecycle_elapsed_seconds: None,
		record_count: 0,
	}
}

fn build_queued_candidate_statuses<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> crate::prelude::Result<Vec<OperatorQueuedIssueStatus>>
where
	T: IssueTracker,
{
	let queue_label = tracker::automation_queue_label(project.service_id());
	let concurrency = ConcurrencySnapshot::new(project.service_id(), state_store)?;
	let mut issues = tracker.list_issues_with_label(&queue_label)?;

	issues.sort_by(compare_issue_candidates);

	issues
		.into_iter()
		.filter(|issue| !is_terminal_issue(issue, workflow))
		.map(|issue| {
			operator_queued_issue_status(
				tracker,
				project,
				workflow,
				state_store,
				&concurrency,
				issue,
			)
		})
		.collect()
}

fn operator_queued_issue_status<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	concurrency: &ConcurrencySnapshot,
	issue: TrackerIssue,
) -> crate::prelude::Result<OperatorQueuedIssueStatus>
where
	T: IssueTracker,
{
	let (classification, reason) =
		classify_queued_issue(tracker, project, workflow, state_store, concurrency, &issue)?;
	let attention = operator_queued_issue_attention_status(
		tracker,
		project,
		workflow,
		state_store,
		&issue,
		reason,
	)?;

	Ok(OperatorQueuedIssueStatus {
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
		blocker_identifiers: issue.blockers.into_iter().map(|blocker| blocker.identifier).collect(),
	})
}

fn classify_queued_issue<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	concurrency: &ConcurrencySnapshot,
	issue: &TrackerIssue,
) -> crate::prelude::Result<(&'static str, &'static str)>
where
	T: IssueTracker,
{
	let tracker_policy = workflow.frontmatter().tracker();

	if state_store.issue_has_active_shared_claim(project.service_id(), &issue.id)? {
		return Ok(("claimed", "shared_claim_present"));
	}
	if tracker_policy.terminal_states().iter().any(|state| state == &issue.state.name) {
		return Ok(("closed", "terminal_state"));
	}
	if !tracker_policy.startable_states().iter().any(|state| state == &issue.state.name) {
		return Ok(("blocked", "non_startable_state"));
	}
	if issue.has_label(tracker_policy.opt_out_label()) {
		return Ok(("blocked", "issue_opted_out"));
	}
	if issue.has_label(tracker_policy.needs_attention_label()) {
		return Ok(("blocked", "issue_needs_attention"));
	}
	if !todo_blocker_rule_passes(issue, workflow) {
		return Ok(("blocked", "open_tracker_blockers"));
	}
	if !issue_has_generic_dispatch_briefing(issue) {
		return Ok(("blocked", "missing_dispatch_briefing"));
	}
	if !concurrency.has_global_capacity(workflow.frontmatter().execution()) {
		return Ok(("waiting", "global_concurrency_exhausted"));
	}

	let queue_label = tracker::automation_queue_label(project.service_id());

	if !issue_passes_dispatch_policy(tracker, issue, workflow, &queue_label, true)? {
		return Ok(("blocked", "dispatch_policy_rejected"));
	}

	Ok(("ready", "eligible_for_dispatch"))
}

fn operator_queued_issue_attention_status<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: &TrackerIssue,
	reason: &str,
) -> crate::prelude::Result<Option<OperatorQueuedIssueAttentionStatus>>
where
	T: IssueTracker,
{
	if !matches!(reason, "issue_needs_attention" | "retry_budget_exhausted") {
		return Ok(None);
	}

	let worktree_path = project.worktree_root().join(&issue.identifier);
	let marker = state::read_run_activity_marker_snapshot(&worktree_path)?;
	let state_retry_attempts = state_store.retry_budget_attempt_count(&issue.id)?;
	let marker_retry_attempts =
		marker.as_ref().and_then(RunActivityMarker::retry_budget_attempt_count).unwrap_or(0);
	let retry_budget_attempts = state_retry_attempts.max(marker_retry_attempts);
	let retry_budget_attempt_count = (retry_budget_attempts > 0).then_some(retry_budget_attempts);
	let retry_budget_max_attempts = i64::from(workflow.frontmatter().execution().max_attempts());
	let auto_retry_blocked_reason =
		(reason == "issue_needs_attention").then(|| String::from("needs_attention_label"));
	let attention_record =
		operator_queued_issue_latest_attention_record(tracker, project, state_store, issue);
	let attempt_status = marker
		.as_ref()
		.and_then(|marker| state_store.run_attempt(marker.run_id()).transpose())
		.transpose()?
		.map(|run_attempt| run_attempt.status().to_owned());
	let worktree_has_tracked_changes = worktree_has_tracked_changes(&worktree_path);
	let summary = operator_queued_issue_attention_summary(
		reason,
		marker.as_ref(),
		attempt_status.as_deref(),
		retry_budget_attempts,
		worktree_has_tracked_changes,
	);
	let process_liveness = marker.as_ref().and_then(marker_process_liveness_for_marker);

	Ok(Some(OperatorQueuedIssueAttentionStatus {
		summary,
		run_id: marker.as_ref().map(|marker| marker.run_id().to_owned()),
		attempt_number: marker.as_ref().map(RunActivityMarker::attempt_number),
		current_operation: marker
			.as_ref()
			.and_then(RunActivityMarker::current_operation)
			.map(str::to_owned),
		thread_status: marker
			.as_ref()
			.and_then(RunActivityMarker::thread_status)
			.map(str::to_owned),
		attempt_status,
		auto_retry_blocked_reason,
		attention_error_class: attention_record
			.as_ref()
			.and_then(|record| record.error_class.clone()),
		attention_next_action: attention_record
			.as_ref()
			.and_then(|record| record.next_action.clone()),
		retry_budget_attempt_count,
		retry_budget_max_attempts,
		last_activity_at: marker
			.as_ref()
			.and_then(RunActivityMarker::last_activity_unix_epoch)
			.and_then(|unix_epoch| format_optional_unix_timestamp(Some(unix_epoch))),
		last_progress_at: marker
			.as_ref()
			.and_then(RunActivityMarker::last_progress_unix_epoch)
			.and_then(|unix_epoch| format_optional_unix_timestamp(Some(unix_epoch))),
		last_event_type: marker
			.as_ref()
			.and_then(RunActivityMarker::last_event_type)
			.map(str::to_owned),
		event_count: marker.as_ref().map_or(0, RunActivityMarker::event_count),
		process_alive: process_liveness.map(|liveness| liveness.alive),
		process_liveness_reason: process_liveness.map(|liveness| liveness.reason.to_owned()),
		worktree_path: worktree_path
			.exists()
			.then(|| relative_worktree_path_for_path(project, &worktree_path)),
		worktree_has_tracked_changes,
	}))
}

fn operator_queued_issue_latest_attention_record<T>(
	tracker: &T,
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Option<LinearExecutionEventRecord>
where
	T: IssueTracker,
{
	let local_records = state_store
		.list_linear_execution_events(project.service_id(), &issue.id)
		.inspect_err(|error| {
			tracing::debug!(
				?error,
				issue_id = issue.id,
				issue = issue.identifier,
				"Failed to load local attention records for queued issue."
			);
		})
		.ok();

	if let Some(record) =
		local_records.as_deref().and_then(latest_attention_record_from_linear_records)
	{
		return Some(record.clone());
	}

	let comments = tracker
		.list_comments(&issue.id)
		.inspect_err(|error| {
			tracing::debug!(
				?error,
				issue_id = issue.id,
				issue = issue.identifier,
				"Failed to load tracker comments for queued attention issue."
			);
		})
		.ok()?;
	let records = collect_history_ledger_records(project.service_id(), &issue.id, &comments);

	latest_attention_record_from_history_ledger_records(&records)
		.map(|record| record.record.clone())
}

fn latest_attention_record_from_linear_records(
	records: &[LinearExecutionEventRecord],
) -> Option<&LinearExecutionEventRecord> {
	records
		.iter()
		.filter(|record| {
			matches!(record.event_type.as_str(), "needs_attention" | "terminal_failure")
		})
		.max_by(|left, right| {
			parse_rfc3339_unix_epoch(&left.event_timestamp)
				.cmp(&parse_rfc3339_unix_epoch(&right.event_timestamp))
				.then_with(|| left.idempotency_key.cmp(&right.idempotency_key))
		})
}

fn latest_attention_record_from_history_ledger_records(
	records: &[OperatorHistoryLedgerRecord],
) -> Option<&OperatorHistoryLedgerRecord> {
	records
		.iter()
		.filter(|entry| {
			matches!(entry.record.event_type.as_str(), "needs_attention" | "terminal_failure")
		})
		.max_by(|left, right| compare_history_ledger_record_position(left, right))
}

fn operator_queued_issue_attention_summary(
	reason: &str,
	marker: Option<&RunActivityMarker>,
	attempt_status: Option<&str>,
	retry_budget_attempts: i64,
	worktree_has_tracked_changes: bool,
) -> String {
	if retry_budget_attempts > 0 && worktree_has_tracked_changes {
		return format!(
			"Partial worktree changes are retained after {retry_budget_attempts} failed attempts; inspect the patch, finish validation, then land or reset manually."
		);
	}
	if marker
		.and_then(RunActivityMarker::thread_status)
		.is_some_and(|status| status == "systemError")
	{
		return if retry_budget_attempts > 0 {
			format!(
				"App-server thread ended with systemError after {retry_budget_attempts} retry-budget attempts."
			)
		} else {
			String::from("App-server thread ended with systemError.")
		};
	}
	if reason == "retry_budget_exhausted" {
		return if retry_budget_attempts > 0 {
			format!(
				"Retry budget has {retry_budget_attempts} recorded failed attempts; operator recovery required."
			)
		} else {
			String::from("Retry budget exhausted; operator recovery required.")
		};
	}

	if let Some(status) = attempt_status {
		let operation = operator_recovery_operation_label(marker);

		match status {
			"interrupted" =>
				return format!(
					"Previous attempt was interrupted during {operation}; operator recovery required."
				),
			"stalled" =>
				return format!(
					"Previous attempt stalled during {operation}; operator recovery required."
				),
			"failed" =>
				return format!(
					"Previous attempt failed during {operation}; operator recovery required."
				),
			"terminal_guarded" =>
				return format!(
					"Previous attempt hit a terminal guard during {operation}; operator recovery required."
				),
			_ => {},
		}
	}

	if marker
		.and_then(RunActivityMarker::last_event_type)
		.is_some_and(|event_type| event_type == "item/tool/call")
	{
		return String::from("Stopped during a tool call; operator recovery required.");
	}

	match marker.and_then(RunActivityMarker::current_operation) {
		Some(RUN_OPERATION_GIT_CREDENTIALS) => {
			String::from("Git credential preflight failed; operator recovery required.")
		},
		Some(RUN_OPERATION_APP_SERVER_PREFLIGHT) => {
			String::from("Codex app-server preflight failed; operator recovery required.")
		},
		Some(RUN_OPERATION_RECONCILIATION) => {
			String::from("Stopped during reconciliation or tracker handoff; operator recovery required.")
		},
		Some(RUN_OPERATION_AGENT_RUN) => {
			String::from("Stopped during agent execution; operator recovery required.")
		},
		Some(operation) =>
			format!("Stopped during `{operation}`; operator recovery required."),
		None => String::from("Needs operator recovery; no local run marker was found."),
	}
}

fn operator_recovery_operation_label(marker: Option<&RunActivityMarker>) -> String {
	match marker.and_then(RunActivityMarker::current_operation) {
		Some(RUN_OPERATION_GIT_CREDENTIALS) => String::from("git credential preflight"),
		Some(RUN_OPERATION_APP_SERVER_PREFLIGHT) => {
			String::from("Codex app-server preflight")
		},
		Some(RUN_OPERATION_RECONCILIATION) => {
			String::from("reconciliation or tracker handoff")
		},
		Some(RUN_OPERATION_AGENT_RUN) => String::from("agent execution"),
		Some(operation) => format!("`{operation}`"),
		None => String::from("the lane"),
	}
}

fn build_post_review_lane_statuses<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> crate::prelude::Result<Vec<OperatorPostReviewLaneStatus>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let worktree_issues = load_post_review_worktree_issues(tracker, project, state_store)?;

	build_post_review_lane_statuses_from_worktree_issues(
		project,
		workflow,
		state_store,
		review_state_inspector,
		worktree_issues,
	)
}

fn build_post_review_lane_statuses_and_hydrate_worktrees<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
	snapshot: &mut OperatorStatusSnapshot,
) -> crate::prelude::Result<Vec<OperatorPostReviewLaneStatus>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let worktree_issues = load_post_review_worktree_issues(tracker, project, state_store)?;

	hydrate_worktree_issue_metadata(snapshot, &worktree_issues);

	build_post_review_lane_statuses_from_worktree_issues(
		project,
		workflow,
		state_store,
		review_state_inspector,
		worktree_issues,
	)
}

fn load_post_review_worktree_issues<T>(
	tracker: &T,
	project: &ServiceConfig,
	state_store: &StateStore,
) -> crate::prelude::Result<Vec<(WorktreeMapping, TrackerIssue)>>
where
	T: IssueTracker,
{
	let worktrees = state_store.list_worktrees(project.service_id())?;

	if worktrees.is_empty() {
		return Ok(Vec::new());
	}

	let issue_ids =
		worktrees.iter().map(|mapping| mapping.issue_id().to_owned()).collect::<Vec<_>>();
	let issues = tracker.refresh_issues(&issue_ids)?;
	let issues_by_id =
		issues.into_iter().map(|issue| (issue.id.clone(), issue)).collect::<HashMap<_, _>>();

	Ok(worktrees
		.into_iter()
		.filter_map(|worktree| {
			issues_by_id
				.get(worktree.issue_id())
				.cloned()
				.map(|issue| (worktree, issue))
		})
		.collect())
}

fn build_post_review_lane_statuses_from_worktree_issues<I>(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
	worktree_issues: Vec<(WorktreeMapping, TrackerIssue)>,
) -> crate::prelude::Result<Vec<OperatorPostReviewLaneStatus>>
where
	I: PullRequestReviewStateInspector,
{
	let tracker_policy = workflow.frontmatter().tracker();
	let success_state = tracker_policy.success_state();
	let completed_state = tracker_policy.resolved_completed_state();
	let lane_context = PostReviewLaneBuildContext {
		project,
		workflow,
		state_store,
		review_state_inspector,
		success_state,
		completed_state,
	};
	let mut lanes = Vec::new();

	for (worktree, issue) in worktree_issues {
		let Some(lane) = build_post_review_lane_status(&lane_context, issue, worktree)? else {
			continue;
		};

		lanes.push(lane);
	}

	lanes.sort_by(|left, right| left.issue_identifier.cmp(&right.issue_identifier));

	Ok(lanes)
}

fn hydrate_worktree_issue_metadata(
	snapshot: &mut OperatorStatusSnapshot,
	worktree_issues: &[(WorktreeMapping, TrackerIssue)],
) {
	let issues_by_id =
		worktree_issues.iter().map(|(_, issue)| (issue.id.as_str(), issue)).collect::<HashMap<_, _>>();

	for worktree in &mut snapshot.worktrees {
		let Some(issue) = issues_by_id.get(worktree.issue_id.as_str()) else {
			continue;
		};

		worktree.issue_identifier = Some(issue.identifier.clone());
		worktree.issue_state = Some(issue.state.name.clone());
	}
}

fn build_post_review_lane_status<I>(
	context: &PostReviewLaneBuildContext<'_, I>,
	issue: TrackerIssue,
	worktree: WorktreeMapping,
) -> crate::prelude::Result<Option<OperatorPostReviewLaneStatus>>
where
	I: PullRequestReviewStateInspector,
{
	if issue.state.name != context.success_state && issue.state.name != context.completed_state {
		return Ok(None);
	}

	if let Some(reason) = post_review_lane_static_block_reason(&issue, context.workflow)? {
		return Ok(Some(blocked_post_review_lane_status(
			context.project,
			&issue,
			&worktree,
			reason,
		)));
	}

	let retry_budget_exhausted = issue_retry_budget_exhausted_for_worktree(
		context.workflow,
		context.state_store,
		&issue.id,
		worktree.worktree_path(),
	)?;
	let review_handoff = context.state_store.review_handoff_marker(
		context.project.service_id(),
		&issue.id,
		worktree.branch_name(),
	)?;

	if issue.state.name == context.completed_state && review_handoff.is_none() {
		return Ok(None);
	}

	let local_branch_name = match worktree_checkout_branch_name(worktree.worktree_path()) {
		Ok(local_branch_name) => local_branch_name,
		Err(_error) =>
			return Ok(Some(blocked_post_review_lane_status(
				context.project,
				&issue,
				&worktree,
				"worktree_checkout_branch_read_failed",
			))),
	};
	let local_head_oid = match worktree_head_oid(worktree.worktree_path()) {
		Ok(local_head_oid) => local_head_oid,
		Err(_error) =>
			return Ok(Some(blocked_post_review_lane_status(
				context.project,
				&issue,
				&worktree,
				"worktree_head_read_failed",
			))),
	};
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff,
		local_branch_name,
		local_head_oid,
	};
		let mut classification = classify_post_review_lane_with_project(
			&snapshot,
			context.project,
			context.workflow,
			context.state_store,
			context.review_state_inspector,
		)?;

	if retry_budget_exhausted {
		classification = retry_budget_exhausted_post_review_lane_classification(
			&snapshot,
			context.project,
			context.workflow,
			context.review_state_inspector,
			classification,
		);
	}

	Ok(Some(post_review_lane_status_from_classification(
		context.project,
		&snapshot,
		classification,
	)))
}

fn post_review_lane_status_from_classification(
	project: &ServiceConfig,
	snapshot: &PostReviewLaneSnapshot,
	classification: PostReviewLaneClassification,
) -> OperatorPostReviewLaneStatus {
	OperatorPostReviewLaneStatus {
		issue_id: snapshot.issue.id.clone(),
		issue_identifier: snapshot.issue.identifier.clone(),
		issue_state: snapshot.issue.state.name.clone(),
		branch_name: snapshot.worktree.branch_name().to_owned(),
		worktree_path: relative_worktree_path_for_path(
			project,
			snapshot.worktree.worktree_path(),
		),
		classification: classification.decision.as_str().to_owned(),
		reason: classification.reason,
		pr_url: classification.pr_url,
		pr_state: classification.pr_state,
		review_decision: classification.review_decision,
		mergeable: classification.mergeable,
		check_state: classification.check_state,
		unresolved_review_threads: classification.unresolved_review_threads,
	}
}

fn post_review_lane_static_block_reason(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> crate::prelude::Result<Option<&'static str>> {
	let tracker_policy = workflow.frontmatter().tracker();

	if issue.has_label(tracker_policy.opt_out_label()) {
		return Ok(Some("issue_opted_out"));
	}
	if issue.has_label(tracker_policy.needs_attention_label()) {
		return Ok(Some("issue_needs_attention"));
	}

	Ok(None)
}

#[cfg_attr(not(test), allow(dead_code))]
#[cfg(test)]
fn classify_post_review_lane<I>(
	snapshot: &PostReviewLaneSnapshot,
	state_store: &StateStore,
	workflow: &WorkflowDocument,
	review_state_inspector: &I,
) -> crate::prelude::Result<PostReviewLaneClassification>
where
	I: PullRequestReviewStateInspector,
{
	classify_post_review_lane_with_external_review(
		snapshot,
		workflow,
		review_state_inspector,
		true,
		Some((state_store, "pubfi")),
	)
}

fn classify_post_review_lane_with_project<I>(
	snapshot: &PostReviewLaneSnapshot,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> crate::prelude::Result<PostReviewLaneClassification>
where
	I: PullRequestReviewStateInspector,
{
	classify_post_review_lane_with_external_review(
		snapshot,
		workflow,
		review_state_inspector,
		project.codex().external_review_enabled(),
		Some((state_store, project.service_id())),
	)
}

fn classify_post_review_lane_with_external_review<I>(
	snapshot: &PostReviewLaneSnapshot,
	workflow: &WorkflowDocument,
	review_state_inspector: &I,
	external_review_enabled: bool,
	runtime_state: Option<(&StateStore, &str)>,
) -> crate::prelude::Result<PostReviewLaneClassification>
where
	I: PullRequestReviewStateInspector,
{
	let review_state = match load_post_review_lane_review_state(snapshot, review_state_inspector)? {
		PostReviewLaneStateLoad::Classification(classification) => return Ok(classification),
		PostReviewLaneStateLoad::ReviewState(review_state) => review_state,
	};
	let mut classification = initial_post_review_lane_classification(&review_state);

	if apply_pre_orchestration_post_review_classification(
		snapshot,
		workflow,
		&review_state,
		&mut classification,
	) {
		return Ok(classification);
	}
		if !external_review_enabled {
			let orchestration_marker = load_post_review_orchestration_marker(
				snapshot,
				&review_state,
				&mut classification,
				runtime_state,
			)?;

		if classification.decision == PostReviewLaneDecision::Block {
			return Ok(classification);
		}

		apply_internal_review_only_post_review_classification(
			&mut classification,
			&review_state,
			orchestration_marker.as_ref(),
			OffsetDateTime::now_utc().unix_timestamp(),
		)?;

		return Ok(classification);
	}

	let Some(orchestration_marker) =
		load_post_review_orchestration_marker(
			snapshot,
			&review_state,
			&mut classification,
			runtime_state,
		)?
	else {
		return Ok(classification);
	};
	let orchestration_status =
		PostReviewOrchestrationStatus::from_review_state(&review_state, &orchestration_marker)?;

	apply_review_orchestration_phase_classification(
		&mut classification,
		&review_state,
		&orchestration_marker,
		&orchestration_status,
		OffsetDateTime::now_utc().unix_timestamp(),
	);

	Ok(classification)
}

fn retry_budget_exhausted_post_review_lane_classification<I>(
	snapshot: &PostReviewLaneSnapshot,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	review_state_inspector: &I,
	mut classification: PostReviewLaneClassification,
) -> PostReviewLaneClassification
where
	I: PullRequestReviewStateInspector,
{
	if classification.pr_url.is_none() {
		classification.pr_url =
			snapshot.review_handoff.as_ref().map(|marker| marker.pr_url().to_owned());
	}
	if classification.pr_state.is_none()
		&& let Some(review_state) =
			retry_budget_exhausted_merged_review_state(snapshot, review_state_inspector)
	{
		classification = initial_post_review_lane_classification(&review_state);

		apply_pre_orchestration_post_review_classification(
			snapshot,
			workflow,
			&review_state,
			&mut classification,
		);
	}
	if merged_closeout_pending_classification(&classification)
		&& worktree_has_no_tracked_changes(snapshot.worktree.worktree_path())
	{
		if snapshot.issue.state.name == workflow.frontmatter().tracker().resolved_completed_state()
			&& !worktree_has_no_tracked_changes(project.repo_root())
		{
			classification.decision = PostReviewLaneDecision::CleanupBlocked;
			classification.reason = String::from("default_branch_worktree_dirty");

			return classification;
		}

		return classification;
	}
	if classification.pr_state.as_deref() == Some("MERGED")
		&& worktree_has_no_tracked_changes(snapshot.worktree.worktree_path())
	{
		classification.decision =
			if snapshot.issue.state.name == workflow.frontmatter().tracker().resolved_completed_state()
			{
				PostReviewLaneDecision::CleanupBlocked
			} else {
				PostReviewLaneDecision::CloseoutBlocked
			};
		classification.reason = String::from("retry_budget_exhausted");

		return classification;
	}

	classification.decision = PostReviewLaneDecision::Block;
	classification.reason = String::from("retry_budget_exhausted");

	classification
}

fn merged_closeout_pending_classification(
	classification: &PostReviewLaneClassification,
) -> bool {
	classification.decision == PostReviewLaneDecision::Continue
		&& classification.reason == "pull_request_merged_closeout_pending"
		&& classification.pr_state.as_deref() == Some("MERGED")
}

fn retry_budget_exhausted_merged_review_state<I>(
	snapshot: &PostReviewLaneSnapshot,
	review_state_inspector: &I,
) -> Option<PullRequestReviewState>
where
	I: PullRequestReviewStateInspector,
{
	let review_handoff = snapshot.review_handoff.as_ref()?;

	if !worktree_has_no_tracked_changes(snapshot.worktree.worktree_path()) {
		return None;
	}

	let review_state = review_state_inspector
		.inspect_review_state(snapshot.worktree.worktree_path(), review_handoff.pr_url())
		.ok()?;

	(review_state.state == "MERGED").then_some(review_state)
}

fn worktree_has_no_tracked_changes(worktree_path: &Path) -> bool {
	let Ok(output) = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["status", "--porcelain", "--untracked-files=no"])
		.output()
	else {
		return false;
	};

	output.status.success() && String::from_utf8_lossy(&output.stdout).trim().is_empty()
}

fn worktree_has_tracked_changes(worktree_path: &Path) -> bool {
	let Ok(output) = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["status", "--porcelain", "--untracked-files=no"])
		.output()
	else {
		return false;
	};

	output.status.success() && !String::from_utf8_lossy(&output.stdout).trim().is_empty()
}

fn apply_pre_orchestration_post_review_classification(
	snapshot: &PostReviewLaneSnapshot,
	workflow: &WorkflowDocument,
	review_state: &PullRequestReviewState,
	classification: &mut PostReviewLaneClassification,
) -> bool {
	if review_state.state == "MERGED" {
		classification.decision = PostReviewLaneDecision::Continue;
		classification.reason = String::from("pull_request_merged_closeout_pending");

		return true;
	}
	if snapshot.issue.state.name == workflow.frontmatter().tracker().resolved_completed_state() {
		*classification = blocked_post_review_lane_from_state(
			review_state,
			"issue_completed_before_pull_request_merged",
		);

		return true;
	}
	if review_state.state != "OPEN" {
		*classification =
			blocked_post_review_lane_from_state(review_state, "pull_request_not_open");

		return true;
	}
	if review_state.is_draft {
		*classification =
			blocked_post_review_lane_from_state(review_state, "pull_request_is_draft");

		return true;
	}
	if review_state.unresolved_review_threads > 0 {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from("unresolved_review_threads");

		return true;
	}
	if matches!(review_state.review_decision.as_deref(), Some("CHANGES_REQUESTED")) {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from("review_changes_requested");

		return true;
	}
	if failed_checks_require_repair(
		review_state.status_check_rollup_state.as_deref(),
		&review_state.merge_state_status,
	) {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from("required_checks_failed");

		return true;
	}

	if let Some(reason) = merge_state_requires_review_repair(
		&review_state.mergeable,
		&review_state.merge_state_status,
	) {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from(reason);

		return true;
	}

	false
}

fn apply_internal_review_only_post_review_classification(
	classification: &mut PostReviewLaneClassification,
	review_state: &PullRequestReviewState,
	orchestration_marker: Option<&ReviewOrchestrationMarker>,
	now_unix_epoch: i64,
) -> crate::prelude::Result<()> {
	if let Some(orchestration_marker) = orchestration_marker {
		let phase =
			ReviewOrchestrationPhase::parse(orchestration_marker.phase()).map_err(|error| {
				eyre::eyre!("Failed to parse retained review orchestration phase: {error}")
			})?;

		if phase == ReviewOrchestrationPhase::WaitingForMerge {
			if let Some(auto_merge_enabled_at) =
				orchestration_marker.auto_merge_enabled_at_unix_epoch()
				&& now_unix_epoch - auto_merge_enabled_at
					> EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS
			{
				*classification = blocked_post_review_lane_from_state(
					review_state,
					"internal_review_only_merge_visibility_timeout",
				);
			} else {
				classification.reason = String::from("internal_review_only_waiting_for_merge");
			}

			return Ok(());
		}
		if phase == ReviewOrchestrationPhase::RepairRequired {
			classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
			classification.reason = if review_state_landing_requires_agent_fallback(review_state) {
				String::from("retained_landing_agent_fallback_required")
			} else {
				String::from("internal_review_only_repair_required")
			};

			return Ok(());
		}
	}

	if review_state_clean_path_landing_gates_satisfied(review_state) {
		classification.decision = PostReviewLaneDecision::ReadyToLand;
		classification.reason = String::from("internal_review_only_ready_to_land");
	} else if review_state_landing_requires_agent_fallback(review_state) {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from("retained_landing_agent_fallback_required");
	} else {
		classification.reason = String::from("internal_review_only_waiting_gates");
	}

	Ok(())
}

fn load_post_review_orchestration_marker(
	snapshot: &PostReviewLaneSnapshot,
	review_state: &PullRequestReviewState,
	classification: &mut PostReviewLaneClassification,
	runtime_state: Option<(&StateStore, &str)>,
) -> crate::prelude::Result<Option<ReviewOrchestrationMarker>> {
	let review_handoff = snapshot
		.review_handoff
		.as_ref()
		.expect("review handoff should exist before orchestration classification");
	let orchestration_marker = if let Some((state_store, project_id)) = runtime_state {
		state_store.review_orchestration_marker(project_id, &snapshot.issue.id, review_handoff)?
	} else {
		None
	};
	let Some(orchestration_marker) = orchestration_marker else {
		classification.reason = String::from("external_review_request_pending");

		return Ok(None);
	};

	if let Some(reason) =
		validate_review_orchestration_marker(snapshot, review_state, &orchestration_marker)
	{
		*classification = blocked_post_review_lane_from_state(review_state, reason);

		return Ok(None);
	}

	Ok(Some(orchestration_marker))
}

fn apply_review_orchestration_phase_classification(
	classification: &mut PostReviewLaneClassification,
	review_state: &PullRequestReviewState,
	orchestration_marker: &ReviewOrchestrationMarker,
	orchestration_status: &PostReviewOrchestrationStatus,
	now_unix_epoch: i64,
) {
	match orchestration_status.phase {
		ReviewOrchestrationPhase::RequestPending => {
			match external_review_request_ci_gate(review_state) {
				ExternalReviewRequestCiGate::Ready => {
					classification.reason = String::from("external_review_request_pending");
				},
				ExternalReviewRequestCiGate::WaitForGreenChecks => {
					classification.reason =
						String::from("external_review_request_waiting_for_green_checks");
				},
				ExternalReviewRequestCiGate::RepairRequired => {
					classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
					classification.reason =
						String::from("external_review_request_ci_red_repair_required");
				},
				ExternalReviewRequestCiGate::ManualAttention(reason) => {
					*classification = blocked_post_review_lane_from_state(review_state, reason);
				},
			}
		},
		ReviewOrchestrationPhase::WaitingForAck => {
			if orchestration_status.request_acknowledged {
				classification.reason = String::from("external_review_result_pending");
			} else if request_ack_timed_out(orchestration_marker, now_unix_epoch) {
				*classification = blocked_post_review_lane_from_state(
					review_state,
					"external_review_ack_timeout",
				);
			} else {
				classification.reason = String::from("external_review_ack_pending");
			}
		},
		ReviewOrchestrationPhase::WaitingForResult => {
			if !orchestration_status.request_acknowledged {
				classification.reason = String::from("external_review_ack_pending");
			} else if external_review_has_actionable_feedback(review_state, orchestration_marker) {
				classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
				classification.reason = String::from("external_review_feedback_pending_repair");
			} else if orchestration_status.strict_pass
				&& orchestration_status.clean_path_landing_gates_satisfied
			{
				classification.decision = PostReviewLaneDecision::ReadyToLand;
				classification.reason = String::from("external_review_passed_strict");
			} else if orchestration_status.strict_pass
				&& orchestration_status.landing_requires_agent_fallback
			{
				classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
				classification.reason = String::from("retained_landing_agent_fallback_required");
			} else if orchestration_status.strict_pass {
				classification.reason = String::from("external_review_passed_waiting_gates");
			} else if orchestration_status.review_result_arrived {
				*classification = blocked_post_review_lane_from_state(
					review_state,
					"external_review_pass_signal_missing",
				);
			} else {
				classification.reason = String::from("external_review_result_pending");
			}
		},
		ReviewOrchestrationPhase::RepairRequired => {
			classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
			classification.reason = if orchestration_status.landing_requires_agent_fallback {
				String::from("retained_landing_agent_fallback_required")
			} else {
				String::from("external_review_feedback_pending_repair")
			};
		},
		ReviewOrchestrationPhase::PassWaitingForGates => {
			if external_review_has_actionable_feedback(review_state, orchestration_marker) {
				classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
				classification.reason = String::from("external_review_feedback_pending_repair");
			} else if orchestration_status.strict_pass
				&& orchestration_status.clean_path_landing_gates_satisfied
			{
				classification.decision = PostReviewLaneDecision::ReadyToLand;
				classification.reason = String::from("external_review_passed_strict");
			} else if orchestration_status.strict_pass
				&& orchestration_status.landing_requires_agent_fallback
			{
				classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
				classification.reason = String::from("retained_landing_agent_fallback_required");
			} else if orchestration_status.strict_pass {
				classification.reason = String::from("external_review_passed_waiting_gates");
			} else {
				*classification = blocked_post_review_lane_from_state(
					review_state,
					"external_review_pass_signal_missing",
				);
			}
		},
		ReviewOrchestrationPhase::WaitingForMerge => {
			if let Some(auto_merge_enabled_at) =
				orchestration_marker.auto_merge_enabled_at_unix_epoch()
				&& now_unix_epoch - auto_merge_enabled_at
					> EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS
			{
				*classification = blocked_post_review_lane_from_state(
					review_state,
					"external_review_merge_visibility_timeout",
				);
			} else {
				classification.reason = String::from("external_review_waiting_for_merge");
			}
		},
	}
}

fn load_post_review_lane_review_state<I>(
	snapshot: &PostReviewLaneSnapshot,
	review_state_inspector: &I,
) -> crate::prelude::Result<PostReviewLaneStateLoad>
where
	I: PullRequestReviewStateInspector,
{
	if let Some(review_handoff) = snapshot.review_handoff.as_ref() {
		let local_head_oid = match validate_post_review_lane_worktree(snapshot, review_handoff) {
			Ok(local_head_oid) => local_head_oid,
			Err(reason) => {
				return Ok(PostReviewLaneStateLoad::Classification(blocked_post_review_lane(
					reason,
				)));
			},
		};
		let review_state = match review_state_inspector
			.inspect_review_state(snapshot.worktree.worktree_path(), review_handoff.pr_url())
		{
			Ok(review_state) => review_state,
			Err(_error) => {
				return Ok(PostReviewLaneStateLoad::Classification(blocked_post_review_lane(
					"pull_request_state_read_failed",
				)));
			},
		};

		return Ok(validate_post_review_lane_review_state(
			review_state,
			snapshot.worktree.branch_name(),
			local_head_oid,
			snapshot.worktree.worktree_path(),
		));
	}

	Ok(PostReviewLaneStateLoad::Classification(blocked_post_review_lane(
		"missing_review_handoff_record",
	)))
}

fn validate_post_review_lane_review_state(
	review_state: PullRequestReviewState,
	expected_branch_name: &str,
	local_head_oid: &str,
	worktree_path: &Path,
) -> PostReviewLaneStateLoad {
	let Some(pr_owner) =
		github::parse_pull_request_url(&review_state.url).ok().map(|locator| locator.owner)
	else {
		return PostReviewLaneStateLoad::Classification(blocked_post_review_lane_from_state(
			&review_state,
			"pull_request_repository_parse_failed",
		));
	};
	let Some(pr_repo) =
		github::parse_pull_request_url(&review_state.url).ok().map(|locator| locator.repo)
	else {
		return PostReviewLaneStateLoad::Classification(blocked_post_review_lane_from_state(
			&review_state,
			"pull_request_repository_parse_failed",
		));
	};

	if review_state.head_repository_owner.as_deref() != Some(pr_owner.as_str()) {
		return PostReviewLaneStateLoad::Classification(blocked_post_review_lane_from_state(
			&review_state,
			"pull_request_head_repository_owner_mismatch",
		));
	}
	if review_state.head_repository_name.as_deref() != Some(pr_repo.as_str()) {
		return PostReviewLaneStateLoad::Classification(blocked_post_review_lane_from_state(
			&review_state,
			"pull_request_head_repository_name_mismatch",
		));
	}
	if review_state.head_ref_name != expected_branch_name {
		return PostReviewLaneStateLoad::Classification(blocked_post_review_lane_from_state(
			&review_state,
			"pull_request_branch_mismatch",
		));
	}
	if review_state.head_ref_oid != local_head_oid {
		match merged_pr_local_head_matches_landed_lineage(
			worktree_path,
			&review_state,
			local_head_oid,
		) {
			Ok(true) => return PostReviewLaneStateLoad::ReviewState(review_state),
			Ok(false) => {},
			Err(reason) => {
				return PostReviewLaneStateLoad::Classification(
					blocked_post_review_lane_from_state(&review_state, reason),
				);
			},
		}

		return PostReviewLaneStateLoad::Classification(blocked_post_review_lane_from_state(
			&review_state,
			"pull_request_head_mismatch",
		));
	}

	PostReviewLaneStateLoad::ReviewState(review_state)
}

fn merged_pr_local_head_matches_landed_lineage(
	worktree_path: &Path,
	review_state: &PullRequestReviewState,
	local_head_oid: &str,
) -> std::result::Result<bool, &'static str> {
	if review_state.state != "MERGED" {
		return Ok(false);
	}

	let Some(merge_commit_oid) = review_state.merge_commit_oid.as_deref() else {
		return Ok(false);
	};

	if merge_commit_oid == local_head_oid {
		return Ok(true);
	}

	worktree_head_descends_from_review_handoff(
		worktree_path,
		merge_commit_oid,
		local_head_oid,
	)
	.map_err(|()| "pull_request_merge_commit_lineage_check_failed")
}

fn validate_review_orchestration_marker(
	snapshot: &PostReviewLaneSnapshot,
	review_state: &PullRequestReviewState,
	marker: &ReviewOrchestrationMarker,
) -> Option<&'static str> {
	let Some(local_head_oid) = snapshot.local_head_oid.as_deref() else {
		return Some("worktree_head_missing");
	};

	if marker.branch_name() != snapshot.worktree.branch_name() {
		return Some("review_orchestration_branch_mismatch");
	}
	if marker.pr_url() != review_state.url {
		return Some("review_orchestration_pr_mismatch");
	}
	if marker.head_sha() != local_head_oid {
		return Some("review_orchestration_head_mismatch");
	}

	None
}

fn request_comment_has_eyes(
	review_state: &PullRequestReviewState,
	marker: &ReviewOrchestrationMarker,
) -> Option<bool> {
	let request_comment_id = marker.request_comment_database_id()?;

	Some(
		review_state
			.issue_comments
			.iter()
			.find(|comment| comment.database_id == request_comment_id)
			.is_some_and(|comment| comment.external_review_eyes_reaction_count > 0),
	)
}

fn request_ack_timed_out(marker: &ReviewOrchestrationMarker, now_unix_epoch: i64) -> bool {
	let Some(request_created_at_unix_epoch) = marker.request_created_at_unix_epoch() else {
		return false;
	};

	now_unix_epoch - request_created_at_unix_epoch > EXTERNAL_REVIEW_ACK_TIMEOUT_SECS
		&& marker.request_retry_count() >= 1
}

fn external_review_result_arrived(
	review_state: &PullRequestReviewState,
	marker: &ReviewOrchestrationMarker,
) -> bool {
	let Some(request_created_at_unix_epoch) = marker.request_created_at_unix_epoch() else {
		return false;
	};

	review_state.reviews.iter().any(|review| {
		review.submitted_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(review.author_login.as_deref())
	}) || review_state.issue_comments.iter().any(|comment| {
		Some(comment.database_id) != marker.request_comment_database_id()
			&& comment.created_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(comment.author_login.as_deref())
	})
}

fn external_review_has_strict_pass_signals(
	review_state: &PullRequestReviewState,
	marker: &ReviewOrchestrationMarker,
) -> bool {
	let Some(request_created_at_unix_epoch) = marker.request_created_at_unix_epoch() else {
		return false;
	};
	let pass_phrase_seen_after_request = review_state.reviews.iter().any(|review| {
		review.submitted_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(review.author_login.as_deref())
			&& external_review_body_is_strict_pass_signal(&review.body)
	}) || review_state.issue_comments.iter().any(|comment| {
		Some(comment.database_id) != marker.request_comment_database_id()
			&& comment.created_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(comment.author_login.as_deref())
			&& external_review_body_is_strict_pass_signal(&comment.body)
	});

	pass_phrase_seen_after_request
		&& review_state.issue_description_external_review_thumbs_up_count > 0
}

fn external_review_has_actionable_feedback(
	review_state: &PullRequestReviewState,
	marker: &ReviewOrchestrationMarker,
) -> bool {
	let Some(request_created_at_unix_epoch) = marker.request_created_at_unix_epoch() else {
		return false;
	};

	review_state.reviews.iter().any(|review| {
		review.submitted_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(review.author_login.as_deref())
			&& matches!(review.state.as_str(), "COMMENTED" | "CHANGES_REQUESTED")
			&& external_review_body_has_actionable_feedback(&review.body)
	}) || review_state.issue_comments.iter().any(|comment| {
		Some(comment.database_id) != marker.request_comment_database_id()
			&& comment.created_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(comment.author_login.as_deref())
			&& external_review_body_has_actionable_feedback(&comment.body)
	})
}

fn is_external_review_actor_login(login: Option<&str>) -> bool {
	login.is_some_and(|login| login.eq_ignore_ascii_case(EXTERNAL_REVIEW_ACTOR_LOGIN))
}

fn external_review_body_is_strict_pass_signal(body: &str) -> bool {
	body.trim() == EXTERNAL_REVIEW_PASS_PHRASE
}

fn external_review_body_has_actionable_feedback(body: &str) -> bool {
	let trimmed = body.trim();

	!trimmed.is_empty() && !external_review_body_is_strict_pass_signal(trimmed)
}

fn retained_closeout_pr_merge_gate_with_inspector<I>(
	worktree_path: &Path,
	expected_branch_name: &str,
	pr_url: &str,
	review_state_inspector: &I,
) -> crate::prelude::Result<RetainedCloseoutPrMergeGate>
where
	I: PullRequestReviewStateInspector + ?Sized,
{
	let Some(local_branch_name) = worktree_checkout_branch_name(worktree_path)? else {
		return Ok(RetainedCloseoutPrMergeGate::NotMerged);
	};
	let Some(local_head_oid) = worktree_head_oid(worktree_path)? else {
		return Ok(RetainedCloseoutPrMergeGate::NotMerged);
	};

	if local_branch_name != expected_branch_name {
		return Ok(RetainedCloseoutPrMergeGate::NotMerged);
	}

	let review_state = match review_state_inspector.inspect_review_state(worktree_path, pr_url) {
		Ok(review_state) => review_state,
		Err(_error) => return Ok(RetainedCloseoutPrMergeGate::PullRequestStateReadFailed),
	};

	Ok(
		if matches!(
			validate_post_review_lane_review_state(
				review_state,
				expected_branch_name,
				&local_head_oid,
				worktree_path,
			),
			PostReviewLaneStateLoad::ReviewState(PullRequestReviewState {
				state,
				is_draft: false,
				..
			}) if state == "MERGED"
		) {
			RetainedCloseoutPrMergeGate::Merged
		} else {
			RetainedCloseoutPrMergeGate::NotMerged
		},
	)
}

fn validate_post_review_lane_worktree<'a>(
	snapshot: &'a PostReviewLaneSnapshot,
	review_handoff: &ReviewHandoffMarker,
) -> std::result::Result<&'a str, &'static str> {
	if review_handoff.branch_name() != snapshot.worktree.branch_name() {
		return Err("worktree_branch_mismatch");
	}

	let Some(local_branch_name) = snapshot.local_branch_name.as_deref() else {
		return Err("worktree_checkout_branch_missing");
	};

	if local_branch_name != review_handoff.branch_name()
		|| local_branch_name != snapshot.worktree.branch_name()
	{
		return Err("worktree_checkout_branch_mismatch");
	}

	let Some(local_head_oid) = snapshot.local_head_oid.as_deref() else {
		return Err("worktree_head_missing");
	};

	if local_head_oid != review_handoff.pr_head_oid() {
		match worktree_head_descends_from_review_handoff(
			snapshot.worktree.worktree_path(),
			review_handoff.pr_head_oid(),
			local_head_oid,
		) {
			Ok(true) => {},
			Ok(false) => return Err("review_handoff_lineage_mismatch"),
			Err(()) => return Err("review_handoff_lineage_check_failed"),
		}
	}

	Ok(local_head_oid)
}

fn worktree_head_descends_from_review_handoff(
	worktree_path: &Path,
	recorded_head_oid: &str,
	local_head_oid: &str,
) -> std::result::Result<bool, ()> {
	if recorded_head_oid == local_head_oid {
		return Ok(true);
	}

	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["merge-base", "--is-ancestor", recorded_head_oid, local_head_oid])
		.output()
		.map_err(|_| ())?;

	match output.status.code() {
		Some(0) => Ok(true),
		Some(1) => Ok(false),
		_ => Err(()),
	}
}

fn initial_post_review_lane_classification(
	review_state: &PullRequestReviewState,
) -> PostReviewLaneClassification {
	PostReviewLaneClassification {
		decision: PostReviewLaneDecision::WaitForReview,
		reason: String::from("waiting_for_review_or_checks"),
		pr_url: Some(review_state.url.clone()),
		pr_state: Some(review_state.state.clone()),
		review_decision: review_state.review_decision.clone(),
		mergeable: Some(review_state.mergeable.clone()),
		check_state: review_state.status_check_rollup_state.clone(),
		unresolved_review_threads: Some(review_state.unresolved_review_threads),
	}
}

fn blocked_post_review_lane_from_state(
	review_state: &PullRequestReviewState,
	reason: &str,
) -> PostReviewLaneClassification {
	let mut classification = initial_post_review_lane_classification(review_state);

	classification.decision = PostReviewLaneDecision::Block;
	classification.reason = reason.to_owned();

	classification
}

fn blocked_post_review_lane(reason: &str) -> PostReviewLaneClassification {
	PostReviewLaneClassification {
		decision: PostReviewLaneDecision::Block,
		reason: reason.to_owned(),
		pr_url: None,
		pr_state: None,
		review_decision: None,
		mergeable: None,
		check_state: None,
		unresolved_review_threads: None,
	}
}

fn blocked_post_review_lane_status(
	project: &ServiceConfig,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	reason: &str,
) -> OperatorPostReviewLaneStatus {
	OperatorPostReviewLaneStatus {
		issue_id: issue.id.clone(),
		issue_identifier: issue.identifier.clone(),
		issue_state: issue.state.name.clone(),
		branch_name: worktree.branch_name().to_owned(),
		worktree_path: relative_worktree_path_for_path(project, worktree.worktree_path()),
		classification: String::from("blocked"),
		reason: String::from(reason),
		pr_url: None,
		pr_state: None,
		review_decision: None,
		mergeable: None,
		check_state: None,
		unresolved_review_threads: None,
	}
}

fn worktree_head_oid(worktree_path: &Path) -> crate::prelude::Result<Option<String>> {
	let output =
		Command::new("git").arg("-C").arg(worktree_path).args(["rev-parse", "HEAD"]).output()?;

	if !output.status.success() {
		if !worktree_path.exists() {
			return Ok(None);
		}

		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect worktree HEAD in `{}`: {}",
			worktree_path.display(),
			stderr.trim()
		);
	}

	Ok(Some(String::from_utf8_lossy(&output.stdout).trim().to_owned()))
}

fn worktree_checkout_branch_name(worktree_path: &Path) -> crate::prelude::Result<Option<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["branch", "--show-current"])
		.output()?;

	if !output.status.success() {
		if !worktree_path.exists() {
			return Ok(None);
		}

		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect worktree checkout branch in `{}`: {}",
			worktree_path.display(),
			stderr.trim()
		);
	}

	let branch_name = String::from_utf8_lossy(&output.stdout).trim().to_owned();

	if branch_name.is_empty() {
		return Ok(None);
	}

	Ok(Some(branch_name))
}

fn resolve_configured_env_var(
	field_name: &str,
	env_var: Option<&str>,
) -> crate::prelude::Result<String> {
	let env_var = env_var.ok_or_else(|| {
		eyre::eyre!("`{field_name}` must be configured for this GitHub-backed operation.")
	})?;
	let value = env::var(env_var).map_err(|error| {
		eyre::eyre!(
			"Failed to read environment variable `{env_var}` referenced by `{field_name}`: {error}"
		)
	})?;

	if value.trim().is_empty() {
		eyre::bail!(
			"Environment variable `{env_var}` referenced by `{field_name}` must not be blank."
		);
	}

	Ok(value)
}

fn external_review_request_ci_gate(
	review_state: &PullRequestReviewState,
) -> ExternalReviewRequestCiGate {
	match review_state.status_check_rollup_state.as_deref() {
		None | Some("SUCCESS") => ExternalReviewRequestCiGate::Ready,
		Some("EXPECTED" | "PENDING") => ExternalReviewRequestCiGate::WaitForGreenChecks,
		Some("ERROR" | "FAILURE")
			if failed_checks_require_repair(
				review_state.status_check_rollup_state.as_deref(),
				&review_state.merge_state_status,
			) =>
		{
			ExternalReviewRequestCiGate::RepairRequired
		},
		Some("ERROR" | "FAILURE") | Some(_) => ExternalReviewRequestCiGate::ManualAttention(
			"external_review_request_ci_red_manual_attention",
		),
	}
}

fn failed_checks_require_repair(check_state: Option<&str>, merge_state_status: &str) -> bool {
	pull_request::failed_checks_require_repair(check_state, merge_state_status)
}

fn merge_state_requires_review_repair(
	mergeable: &str,
	merge_state_status: &str,
) -> Option<&'static str> {
	pull_request::merge_state_requires_review_repair(mergeable, merge_state_status)
}

fn review_state_landing_gates_satisfied(review_state: &PullRequestReviewState) -> bool {
	pull_request::retained_landing_gates_satisfied(review_state_landing_gate_view(review_state))
}

fn review_state_clean_path_landing_gates_satisfied(
	review_state: &PullRequestReviewState,
) -> bool {
	pull_request::retained_clean_path_landing_gates_satisfied(review_state_landing_gate_view(
		review_state,
	))
}

fn review_state_landing_requires_agent_fallback(review_state: &PullRequestReviewState) -> bool {
	pull_request::retained_landing_requires_agent_fallback(review_state_landing_gate_view(
		review_state,
	))
}

fn review_state_landing_gate_view(
	review_state: &PullRequestReviewState,
) -> PullRequestLandingGateView<'_> {
	PullRequestLandingGateView {
		state: review_state.state.as_str(),
		is_draft: review_state.is_draft,
		review_decision: review_state.review_decision.as_deref(),
		pending_review_requests: review_state.pending_review_requests,
		mergeable: review_state.mergeable.as_str(),
		merge_state_status: review_state.merge_state_status.as_str(),
		status_check_rollup_state: review_state.status_check_rollup_state.as_deref(),
		unresolved_review_threads: review_state.unresolved_review_threads,
	}
}

fn recover_runtime_state_from_tracker_and_worktrees<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> crate::prelude::Result<RecoveredRuntimeState>
where
	T: IssueTracker,
{
	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());
	let mut issue_ids = state_store
		.list_worktrees(project.service_id())?
		.into_iter()
		.map(|mapping| mapping.issue_id().to_owned())
		.collect::<Vec<_>>();

	for lease in state_store.list_active_shared_leases(project.service_id())? {
		if !issue_ids.iter().any(|issue_id| issue_id == lease.issue_id()) {
			issue_ids.push(lease.issue_id().to_owned());
		}
	}

	let mut issues = tracker.refresh_issues(&issue_ids)?;
	let mut known_identifiers =
		issues.iter().map(|issue| issue.identifier.to_ascii_uppercase()).collect::<BTreeSet<_>>();

	for issue_identifier in recoverable_worktree_identifiers(project.worktree_root())? {
		append_recoverable_tracker_issue(
			tracker,
			project,
			&issue_identifier,
			&mut known_identifiers,
			&mut issues,
		)?;
	}

	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let mut active_issues = Vec::new();

	for issue in issues {
		let worktree = worktree_manager.plan_for_issue(&issue.identifier);

		if !worktree.path.exists() {
			continue;
		}

		state_store.canonicalize_issue_identity(&issue.identifier, &issue.id)?;
		state_store.upsert_worktree(
			project.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)?;

		let activity_marker = state::read_run_activity_marker_snapshot(&worktree.path)?;

		if issue.state.name == workflow.frontmatter().tracker().success_state()
			&& issue_has_service_ownership(tracker, &issue, project.service_id())?
			&& let Some(marker) = activity_marker.as_ref()
			&& worktree_activity_marker_is_fresh(marker, now_unix_epoch)
		{
			record_recovered_activity_lease(project, state_store, &issue, marker)?;

			continue;
		}
		if issue_passes_closeout_dispatch_policy(tracker, &issue, project, workflow, state_store)?
		{
			match activity_marker.as_ref() {
				Some(marker) if worktree_activity_marker_is_fresh(marker, now_unix_epoch) => {
					record_recovered_activity_lease(project, state_store, &issue, marker)?;

					continue;
				},
				_ => {},
			}
		}
		if issue_passes_retry_dispatch_policy(
			tracker,
			&issue,
			project,
			workflow,
			state_store,
			RetryIssueStateHint::default(),
		)? {
			match activity_marker.as_ref() {
				Some(marker) if worktree_activity_marker_is_fresh(marker, now_unix_epoch) => {
					record_recovered_activity_lease(project, state_store, &issue, marker)?;

					continue;
				},
				Some(marker) => {
					clear_recovered_issue_lease(
						project.service_id(),
						&issue.id,
						Some(marker.run_id()),
						state_store,
					)?;
				},
				None => {
					clear_recovered_issue_lease(
						project.service_id(),
						&issue.id,
						None,
						state_store,
					)?;
				},
			}

			active_issues.push(issue);
		}
	}

	active_issues.sort_by(compare_issue_candidates);

	Ok(RecoveredRuntimeState { active_issues })
}

fn record_recovered_activity_lease(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
	marker: &RunActivityMarker,
) -> crate::prelude::Result<()> {
	state_store.record_run_attempt(
		marker.run_id(),
		&issue.id,
		marker.attempt_number(),
		"running",
	)?;
	state_store.upsert_lease(
		project.service_id(),
		&issue.id,
		marker.run_id(),
		&issue.state.name,
	)?;

	Ok(())
}

fn issue_has_recovered_service_ownership<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
) -> crate::prelude::Result<bool>
where
	T: IssueTracker,
{
	tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		&tracker::automation_active_label(service_id),
	)
}

fn append_recoverable_tracker_issue<T>(
	tracker: &T,
	project: &ServiceConfig,
	issue_identifier: &str,
	known_identifiers: &mut BTreeSet<String>,
	issues: &mut Vec<TrackerIssue>,
) -> crate::prelude::Result<()>
where
	T: IssueTracker,
{
	let canonical_identifier = issue_identifier.to_ascii_uppercase();

	if known_identifiers.contains(&canonical_identifier) {
		return Ok(());
	}

	let Some(issue) = tracker.get_issue_by_identifier(issue_identifier)? else {
		return Ok(());
	};

	if !issue_has_recovered_service_ownership(tracker, &issue, project.service_id())? {
		tracing::warn!(
			issue = issue.identifier,
			active_label = tracker::automation_active_label(project.service_id()),
			labels_complete = issue.labels_complete,
			"Skipping retained worktree recovery because the tracker issue is not explicitly owned by this service."
		);

		return Ok(());
	}

	known_identifiers.insert(issue.identifier.to_ascii_uppercase());
	issues.push(issue);

	Ok(())
}

fn recoverable_worktree_identifiers(worktree_root: &Path) -> crate::prelude::Result<Vec<String>> {
	if !worktree_root.exists() {
		return Ok(Vec::new());
	}

	let mut issue_identifiers = fs::read_dir(worktree_root)?
		.filter_map(|entry| entry.ok())
		.filter_map(|entry| {
			entry
				.file_type()
				.ok()
				.filter(|file_type| file_type.is_dir())
				.and_then(|_| entry.file_name().into_string().ok())
		})
		.filter(|name| commit_message::looks_like_issue_identifier(name))
		.collect::<Vec<_>>();

	issue_identifiers.sort();
	issue_identifiers.dedup();

	Ok(issue_identifiers)
}

fn worktree_activity_marker_is_fresh(marker: &RunActivityMarker, now_unix_epoch: i64) -> bool {
	marker_process_is_alive(marker)
		&& marker
			.last_activity_unix_epoch()
			.and_then(|last_activity| observed_idle_duration(last_activity, now_unix_epoch))
			.is_some_and(|idle_for| idle_for < ACTIVE_RUN_IDLE_TIMEOUT)
}

fn marker_process_is_alive(marker: &RunActivityMarker) -> bool {
	marker_process_liveness(marker).alive
}

fn marker_process_liveness_for_marker(
	marker: &RunActivityMarker,
) -> Option<MarkerProcessLiveness> {
	marker.process_id().map(|_| marker_process_liveness(marker))
}

fn marker_process_liveness(marker: &RunActivityMarker) -> MarkerProcessLiveness {
	let Some(process_id) = marker.process_id() else {
		return MarkerProcessLiveness { alive: false, reason: "process_id_missing" };
	};

	if !process_is_alive(process_id) {
		return MarkerProcessLiveness { alive: false, reason: "process_stopped" };
	}

	let Some(marker_host_boot_id) = marker.host_boot_id() else {
		return MarkerProcessLiveness { alive: false, reason: "host_boot_id_missing" };
	};
	let Some(current_host_boot_id) = state::current_host_boot_id() else {
		return MarkerProcessLiveness { alive: false, reason: "host_boot_id_unavailable" };
	};

	if marker_host_boot_id != current_host_boot_id.as_str() {
		return MarkerProcessLiveness { alive: false, reason: "host_boot_id_mismatch" };
	}

	let Some(marker_process_start_identity) = marker.process_start_identity() else {
		return MarkerProcessLiveness { alive: false, reason: "process_start_identity_missing" };
	};
	let Some(current_process_start_identity) = state::process_start_identity(process_id) else {
		return MarkerProcessLiveness {
			alive: false,
			reason: "process_start_identity_unavailable",
		};
	};

	if marker_process_start_identity != current_process_start_identity.as_str() {
		return MarkerProcessLiveness {
			alive: false,
			reason: "process_start_identity_mismatch",
		};
	}

	MarkerProcessLiveness { alive: true, reason: "process_alive" }
}

fn process_is_alive(process_id: u32) -> bool {
	let Ok(process_id) = pid_t::try_from(process_id) else {
		return false;
	};

	if process_id <= 0 {
		return false;
	}

	// Use the kernel liveness probe directly so recovery does not depend on a shell
	// builtin or `kill` binary being present on PATH.
	match unsafe { libc::kill(process_id, 0) } {
		0 => true,
		-1 => matches!(std::io::Error::last_os_error().raw_os_error(), Some(libc::EPERM)),
		_ => false,
	}
}

fn hydrate_status_snapshot_state(
	_project: &ServiceConfig,
	_state_store: &StateStore,
	_recovered_state: RecoveredRuntimeState,
) -> crate::prelude::Result<()> {
	Ok(())
}

fn operator_run_status(
	project: &ServiceConfig,
	run: ProjectRunStatus,
	now_unix_epoch: i64,
) -> crate::prelude::Result<OperatorRunStatus> {
	let marker = load_operator_run_marker(&run)?;
	let timing = operator_run_timing(&run, marker.as_ref(), now_unix_epoch);
	let app_server_state = operator_run_app_server_state(&run, marker.as_ref());
	let protocol_summary = operator_run_protocol_summary(&run, marker.as_ref());
	let status =
		operator_run_visible_status(run.status(), &app_server_state, &protocol_summary, &timing);
	let (retry_kind, retry_ready_at_unix_epoch) = visible_operator_run_retry_schedule(
		&status,
		marker.as_ref().and_then(RunActivityMarker::retry_kind),
		marker.as_ref().and_then(RunActivityMarker::retry_ready_at_unix_epoch),
		now_unix_epoch,
	);
	let (phase, mut wait_reason) = classify_operator_run_phase(
		&status,
		retry_kind.as_deref(),
		retry_ready_at_unix_epoch,
		now_unix_epoch,
	);
	let current_operation = classify_operator_run_operation(
		&phase,
		marker.as_ref().and_then(RunActivityMarker::current_operation),
	);
	let suspected_stall =
		operator_run_is_suspected_stall(&phase, timing.last_progress_unix_epoch, now_unix_epoch);
	let child_agent_activity = operator_run_child_agent_activity(marker.as_ref(), now_unix_epoch);
	let protocol_activity = operator_run_protocol_activity(
		marker.as_ref(),
		&app_server_state,
		child_agent_activity.as_ref(),
		timing.protocol_idle_for_seconds,
		matches!(status.as_str(), "starting" | "running"),
	);

	if wait_reason.is_none() && phase == "executing" {
		wait_reason = protocol_activity
			.as_ref()
			.and_then(|summary| summary.waiting_reason.clone())
			.filter(|reason| reason != "turn_completed");
	}

	let account = marker.as_ref().and_then(RunActivityMarker::account).cloned();
	let mut accounts = marker
		.as_ref()
		.map(|marker| marker.accounts().to_vec())
		.unwrap_or_default();

	if accounts.is_empty()
		&& let Some(account) = &account
	{
		accounts.push(account.clone());
	}

	let branch_name = run.branch_name().map(str::to_owned);
	let worktree_path = run
		.worktree_path()
		.map(|path| relative_worktree_path_for_path(project, path));
	let issue_identifier = operator_run_issue_identifier_from_fields(
		run.run_id(),
		branch_name.as_deref(),
		worktree_path.as_deref(),
	);
	let execution_liveness =
		operator_run_execution_liveness(&status, &timing, &app_server_state, &protocol_summary);

	Ok(OperatorRunStatus {
		project_id: project.service_id().to_owned(),
		run_id: run.run_id().to_owned(),
		issue_id: run.issue_id().to_owned(),
		issue_identifier,
		title: None,
		author: None,
		attempt_number: run.attempt_number(),
		status,
		attempt_status: run.status().to_owned(),
		phase,
		wait_reason,
		current_operation,
		thread_id: app_server_state.thread_id,
		turn_id: app_server_state.turn_id,
		thread_status: app_server_state.thread_status,
		thread_active_flags: app_server_state.thread_active_flags,
		interactive_requested: app_server_state.interactive_requested,
		continuation_pending: app_server_state.continuation_pending,
		active_lease: run.active_lease(),
		queue_lease_state: operator_run_queue_lease_state(run.active_lease()),
		execution_liveness,
		updated_at: run.updated_at().to_owned(),
		last_run_activity_at: format_optional_unix_timestamp(timing.last_run_activity_unix_epoch),
		last_protocol_activity_at: format_optional_unix_timestamp(
			timing.last_protocol_activity_unix_epoch,
		),
		last_progress_at: format_optional_unix_timestamp(timing.last_progress_unix_epoch),
		idle_for_seconds: timing.idle_for_seconds,
		protocol_idle_for_seconds: timing.protocol_idle_for_seconds,
		suspected_stall,
		last_event_type: protocol_summary.last_event_type,
		last_event_at: protocol_summary.last_event_at,
			event_count: protocol_summary.event_count,
			process_id: timing.process_id,
			process_alive: timing.process_alive,
			process_liveness_reason: timing.process_liveness_reason,
			retry_kind,
			next_retry_at: format_optional_unix_timestamp(retry_ready_at_unix_epoch),
		effective_model: app_server_state.effective_model,
		effective_model_provider: app_server_state.effective_model_provider,
		effective_cwd: app_server_state.effective_cwd,
		effective_approval_policy: app_server_state.effective_approval_policy,
		effective_approvals_reviewer: app_server_state.effective_approvals_reviewer,
		effective_sandbox_mode: app_server_state.effective_sandbox_mode,
		child_agent_activity,
		protocol_activity,
		account,
		accounts,
		branch_name,
		worktree_path,
	})
}

fn load_operator_run_marker(
	run: &ProjectRunStatus,
) -> crate::prelude::Result<Option<RunActivityMarker>> {
	Ok(run
		.worktree_path()
		.map(state::read_run_activity_marker_snapshot)
		.transpose()?
		.flatten()
		.filter(|marker| {
			marker.run_id() == run.run_id() && marker.attempt_number() == run.attempt_number()
		}))
}

fn operator_run_timing(
	run: &ProjectRunStatus,
	marker: Option<&RunActivityMarker>,
	now_unix_epoch: i64,
) -> OperatorRunTiming {
	let process_id = marker.and_then(RunActivityMarker::process_id);
	let last_run_activity_unix_epoch = max_optional_i64(
		Some(run.last_run_activity_unix_epoch()),
		marker.and_then(RunActivityMarker::last_activity_unix_epoch),
	);
	let last_protocol_activity_unix_epoch = max_optional_i64(
		run.last_event_at_unix(),
		marker.and_then(RunActivityMarker::last_protocol_activity_unix_epoch),
	);
	let last_progress_unix_epoch = max_optional_i64(
		marker.and_then(RunActivityMarker::last_progress_unix_epoch),
		last_protocol_activity_unix_epoch,
	);
	let process_liveness = marker.and_then(marker_process_liveness_for_marker);

	OperatorRunTiming {
		process_alive: process_liveness.map(|liveness| liveness.alive),
		process_liveness_reason: process_liveness.map(|liveness| liveness.reason.to_owned()),
		process_id,
		last_run_activity_unix_epoch,
		last_protocol_activity_unix_epoch,
		last_progress_unix_epoch,
		idle_for_seconds: idle_duration_seconds(last_run_activity_unix_epoch, now_unix_epoch),
		protocol_idle_for_seconds: idle_duration_seconds(
			last_protocol_activity_unix_epoch,
			now_unix_epoch,
		),
	}
}

fn operator_run_app_server_state(
	run: &ProjectRunStatus,
	marker: Option<&RunActivityMarker>,
) -> OperatorRunAppServerState {
	let thread_active_flags =
		marker.map(|marker| marker.thread_active_flags().to_vec()).unwrap_or_default();

	OperatorRunAppServerState {
		thread_id: run
			.thread_id()
			.or_else(|| marker.and_then(RunActivityMarker::thread_id))
			.map(str::to_owned),
		turn_id: run
			.turn_id()
			.or_else(|| marker.and_then(RunActivityMarker::turn_id))
			.map(str::to_owned),
		thread_status: marker.and_then(RunActivityMarker::thread_status).map(str::to_owned),
		interactive_requested: thread_active_flags
			.iter()
			.any(|flag| matches!(flag.as_str(), "waitingOnApproval" | "waitingOnUserInput")),
		continuation_pending: run.status() == CONTINUATION_PENDING_RUN_STATUS,
		effective_model: marker.and_then(RunActivityMarker::effective_model).map(str::to_owned),
		effective_model_provider: marker
			.and_then(RunActivityMarker::effective_model_provider)
			.map(str::to_owned),
		effective_cwd: marker.and_then(RunActivityMarker::effective_cwd).map(str::to_owned),
		effective_approval_policy: marker
			.and_then(RunActivityMarker::effective_approval_policy)
			.map(str::to_owned),
		effective_approvals_reviewer: marker
			.and_then(RunActivityMarker::effective_approvals_reviewer)
			.map(str::to_owned),
		effective_sandbox_mode: marker
			.and_then(RunActivityMarker::effective_sandbox_mode)
			.map(str::to_owned),
		thread_active_flags,
	}
}

fn operator_run_protocol_summary(
	run: &ProjectRunStatus,
	marker: Option<&RunActivityMarker>,
) -> OperatorRunProtocolSummary {
	let use_marker_protocol_summary =
		run.event_count() == 0 && run.last_event_type().is_none() && run.last_event_at().is_none();

	if use_marker_protocol_summary {
		return OperatorRunProtocolSummary {
			last_event_type: marker.and_then(RunActivityMarker::last_event_type).map(str::to_owned),
			last_event_at: marker
				.and_then(RunActivityMarker::last_protocol_activity_unix_epoch)
				.and_then(|unix_epoch| format_optional_unix_timestamp(Some(unix_epoch))),
			event_count: marker.map_or(0, RunActivityMarker::event_count),
		};
	}

	OperatorRunProtocolSummary {
		last_event_type: run.last_event_type().map(str::to_owned),
		last_event_at: run.last_event_at().map(str::to_owned),
		event_count: run.event_count(),
	}
}

fn operator_run_visible_status(
	attempt_status: &str,
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
) -> String {
	if attempt_status == "starting"
		&& operator_run_has_app_server_execution_evidence(
			app_server_state,
			protocol_summary,
			timing,
		)
	{
		return String::from("running");
	}

	attempt_status.to_owned()
}

fn operator_run_has_app_server_execution_evidence(
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
) -> bool {
	matches!(app_server_state.thread_status.as_deref(), Some("active"))
		|| !app_server_state.thread_active_flags.is_empty()
		|| app_server_state.effective_model.is_some()
		|| app_server_state.effective_model_provider.is_some()
		|| protocol_summary.event_count > 0
		|| protocol_summary.last_event_type.is_some()
		|| timing.protocol_idle_for_seconds.is_some_and(|idle_for| {
			u64::try_from(idle_for)
				.is_ok_and(|idle_for| idle_for < ACTIVE_RUN_IDLE_TIMEOUT.as_secs())
		})
}

fn operator_run_queue_lease_state(active_lease: bool) -> String {
	if active_lease {
		String::from("held")
	} else {
		String::from("not_held")
	}
}

fn operator_run_execution_liveness(
	status: &str,
	timing: &OperatorRunTiming,
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
) -> String {
	if !matches!(status, "starting" | "running") {
		return String::from("not_running");
	}
	if timing.process_alive == Some(true) {
		return String::from("process_alive");
	}
	if timing.process_alive == Some(false) {
		return String::from("process_stopped");
	}
	if matches!(app_server_state.thread_status.as_deref(), Some("active"))
		|| !app_server_state.thread_active_flags.is_empty()
	{
		return String::from("thread_active");
	}
	if operator_run_has_app_server_execution_evidence(app_server_state, protocol_summary, timing) {
		return String::from("protocol_observed");
	}

	String::from("not_captured")
}

fn operator_run_child_agent_activity(
	marker: Option<&RunActivityMarker>,
	now_unix_epoch: i64,
) -> Option<ChildAgentActivitySummary> {
	let mut summary = marker.and_then(RunActivityMarker::child_agent_activity).cloned()?;

	summary.current_elapsed_seconds =
		summary.current_started_unix_epoch.and_then(|started_at| {
			now_unix_epoch.checked_sub(started_at).filter(|elapsed| *elapsed >= 0)
		});

	if let (Some(current_bucket), Some(current_elapsed_seconds)) =
		(summary.current_bucket.as_deref(), summary.current_elapsed_seconds)
		&& current_elapsed_seconds > 0
	{
		if let Some(bucket) = summary.buckets.iter_mut().find(|bucket| bucket.name == current_bucket) {
			bucket.wall_seconds = bucket.wall_seconds.saturating_add(current_elapsed_seconds);
		} else {
			summary.buckets.push(ChildAgentActivityBucket {
				name: current_bucket.to_owned(),
				wall_seconds: current_elapsed_seconds,
				..ChildAgentActivityBucket::default()
			});
		}
	}

	Some(summary)
}

fn operator_run_protocol_activity(
	marker: Option<&RunActivityMarker>,
	app_server_state: &OperatorRunAppServerState,
	child_agent_activity: Option<&ChildAgentActivitySummary>,
	protocol_idle_for_seconds: Option<i64>,
	is_running: bool,
) -> Option<ProtocolActivitySummary> {
	let mut summary = marker.and_then(RunActivityMarker::protocol_activity).cloned().unwrap_or_default();

	if is_running && summary.waiting_reason.is_none() && app_server_state.interactive_requested {
		summary.waiting_reason = Some(String::from("approval_or_user_input"));
	}
	if is_running
		&& summary.waiting_reason.is_none()
		&& let Some(child_agent_activity) = child_agent_activity
		&& let Some(current_bucket) = child_agent_activity.current_bucket.as_deref()
	{
		summary.waiting_reason = Some(protocol_wait_reason_from_child_bucket(current_bucket));
	}
	if is_running
		&& summary.waiting_reason.is_none()
		&& protocol_idle_for_seconds.is_some_and(|idle_for| {
			u64::try_from(idle_for).is_ok_and(|idle_for| idle_for < ACTIVE_RUN_IDLE_TIMEOUT.as_secs())
		}) {
		summary.waiting_reason = Some(String::from("protocol_idleness"));
	}
	if summary.turn_status.is_none()
		&& summary.waiting_reason.is_none()
		&& summary.rate_limit_status.is_none()
		&& summary.recent_events.is_empty()
	{
		return None;
	}

	Some(summary)
}

fn protocol_wait_reason_from_child_bucket(current_bucket: &str) -> String {
	match current_bucket {
		"Model" => String::from("model_execution"),
		"Protocol" => String::from("protocol_activity"),
		_ => String::from("tool_execution"),
	}
}

fn idle_duration_seconds(
	last_activity_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
) -> Option<i64> {
	last_activity_unix_epoch
		.and_then(|last_activity| now_unix_epoch.checked_sub(last_activity))
		.filter(|idle_for| *idle_for >= 0)
}

fn max_optional_i64(left: Option<i64>, right: Option<i64>) -> Option<i64> {
	match (left, right) {
		(Some(left), Some(right)) => Some(left.max(right)),
		(Some(value), None) | (None, Some(value)) => Some(value),
		(None, None) => None,
	}
}

fn format_optional_unix_timestamp(unix_epoch: Option<i64>) -> Option<String> {
	unix_epoch.and_then(|unix_epoch| {
		OffsetDateTime::from_unix_timestamp(unix_epoch)
			.ok()
			.and_then(|timestamp| timestamp.format(&Rfc3339).ok())
	})
}

fn classify_operator_run_operation(phase: &str, marker_current_operation: Option<&str>) -> String {
	match phase {
		"retry_backoff" | "waiting_continuation" => {
			String::from(RUN_OPERATION_WAITING_EXTERNAL)
		},
		"completed" | "failed" => String::from(RUN_OPERATION_IDLE),
		"stalled" => marker_current_operation
			.map(str::to_owned)
			.unwrap_or_else(|| String::from(RUN_OPERATION_IDLE)),
		"executing" => marker_current_operation
			.map(str::to_owned)
			.unwrap_or_else(|| String::from(RUN_OPERATION_AGENT_RUN)),
		_ => marker_current_operation
			.map(str::to_owned)
			.unwrap_or_else(|| String::from(RUN_OPERATION_IDLE)),
	}
}

fn operator_run_is_suspected_stall(
	phase: &str,
	last_progress_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
) -> bool {
	if phase != "executing" {
		return false;
	}

	last_progress_unix_epoch
		.and_then(|last_progress| observed_idle_duration(last_progress, now_unix_epoch))
		.is_some_and(|idle_for| {
			idle_for >= suspected_operator_run_stall_threshold()
				&& idle_for < ACTIVE_RUN_IDLE_TIMEOUT
		})
}

fn suspected_operator_run_stall_threshold() -> Duration {
	Duration::from_secs((ACTIVE_RUN_IDLE_TIMEOUT.as_secs() / 2).max(1))
}

fn visible_operator_run_retry_schedule(
	status: &str,
	retry_kind: Option<&str>,
	retry_ready_at_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
) -> (Option<String>, Option<i64>) {
	let Some(retry_ready_at_unix_epoch) = retry_ready_at_unix_epoch else {
		return (None, None);
	};

	if matches!(status, "starting" | "running") || retry_ready_at_unix_epoch <= now_unix_epoch {
		return (None, None);
	}

	(retry_kind.map(str::to_owned), Some(retry_ready_at_unix_epoch))
}

fn classify_operator_run_phase(
	status: &str,
	retry_kind: Option<&str>,
	retry_ready_at_unix_epoch: Option<i64>,
	now_unix_epoch: i64,
) -> (String, Option<String>) {
	if status == "stalled" {
		return (String::from("stalled"), Some(String::from("app_server_idle_timeout")));
	}

	if let Some(retry_ready_at_unix_epoch) = retry_ready_at_unix_epoch
		&& retry_ready_at_unix_epoch > now_unix_epoch
	{
		return (
			String::from("retry_backoff"),
			Some(match retry_kind {
				Some("continuation") => String::from("continuation_retry"),
				Some("failure") => String::from("failure_retry"),
				Some(other) => other.to_owned(),
				None => String::from("scheduled_retry"),
			}),
		);
	}

	match status {
		"starting" | "running" => (String::from("executing"), None),
		CONTINUATION_PENDING_RUN_STATUS => {
			(String::from("waiting_continuation"), Some(String::from("turn_boundary")))
		},
		"succeeded" => (String::from("completed"), None),
		"failed" | "interrupted" | TERMINAL_GUARDED_RUN_STATUS => (String::from("failed"), None),
		other => (other.to_owned(), None),
	}
}

fn render_operator_status(snapshot: &OperatorStatusSnapshot) -> String {
	let session_history_attempt_count = snapshot
		.history_lanes
		.iter()
		.map(|lane| lane.attempt_count)
		.sum::<usize>();
	let hides_running_lanes = session_history_attempt_count < snapshot.recent_runs.len();
	let (running_inline_claims, non_running_queued_candidates): (Vec<_>, Vec<_>) = snapshot
		.queued_candidates
		.iter()
		.partition(|queued_issue| queue_claim_belongs_to_active_run(queued_issue, snapshot));
	let (stale_closed_queue_labels, backlog_candidates) =
		rendered_backlog_queue_groups(non_running_queued_candidates);
	let recovery_worktrees = rendered_recovery_worktrees(snapshot);
	let hides_owned_worktrees = recovery_worktrees.len() < snapshot.worktrees.len();
	let mut output = String::new();

	output.push_str(&format!("Project: {}\n", snapshot.project_id));
	output.push_str(&format!("Warnings: {}\n", snapshot.warnings.len()));

	if !snapshot.warnings.is_empty() {
		output.push_str(&format!("Warning details: {}\n", snapshot.warnings.join(", ")));
	}

	output.push_str(&format!("Running lanes: {}\n", snapshot.active_runs.len()));
	output.push_str(&format!(
		"Run ledger shown: {} issue lanes from {} history attempts{}\n",
		snapshot.history_lanes.len(),
		session_history_attempt_count,
		if hides_running_lanes { " (running lanes inline)" } else { "" },
	));
	output.push_str(&format!("Backlog: {}\n", backlog_candidates.len()));
	output.push_str(&format!(
		"Active queue echoes: {}\n",
		running_inline_claims.len()
	));
	output.push_str(&format!(
		"Stale closed queue labels: {}\n",
		stale_closed_queue_labels.len()
	));
	output.push_str(&format!("Recovery worktrees: {}\n", recovery_worktrees.len()));
	output.push_str(&format!("Post-review lanes: {}\n", snapshot.post_review_lanes.len()));
	output.push_str("\nRunning Lanes\n");

	if snapshot.active_runs.is_empty() {
		output.push_str("- none\n");
	} else {
		for run in &snapshot.active_runs {
			append_rendered_run(&mut output, run);
		}
	}

	output.push_str("\nRun Ledger\n");

	if snapshot.history_lanes.is_empty() {
		if hides_running_lanes {
			output.push_str("- none (running lanes are shown above)\n");
		} else {
			output.push_str("- none\n");
		}
	} else {
		for lane in &snapshot.history_lanes {
			append_rendered_history_lane(&mut output, lane);
		}
	}

	append_rendered_queued_issue_section(&mut output, "Backlog", &backlog_candidates, snapshot, false);
	append_rendered_queued_issue_section(
		&mut output,
		"Active Queue Echoes",
		&running_inline_claims,
		snapshot,
		true,
	);
	append_rendered_queued_issue_section(
		&mut output,
		"Stale Closed Queue Labels",
		&stale_closed_queue_labels,
		snapshot,
		false,
	);

	output.push_str("\nRecovery Worktrees\n");

	append_rendered_recovery_worktrees(&mut output, &recovery_worktrees, hides_owned_worktrees);

	output.push_str("\nPost-Review Lanes\n");

	if snapshot.post_review_lanes.is_empty() {
		output.push_str("- none\n");
	} else {
		for lane in &snapshot.post_review_lanes {
			output.push_str(&format!(
				"- issue_id: {}\n  issue: {}\n  state: {}\n  classification: {}\n  reason: {}\n  branch: {}\n  worktree_path: {}\n  pr_url: {}\n  pr_state: {}\n  review_decision: {}\n  mergeable: {}\n  check_state: {}\n  unresolved_review_threads: {}\n",
				lane.issue_id,
				lane.issue_identifier,
				lane.issue_state,
				lane.classification,
				lane.reason,
				lane.branch_name,
				lane.worktree_path,
				lane.pr_url.as_deref().unwrap_or("none"),
				lane.pr_state.as_deref().unwrap_or("none"),
				lane.review_decision.as_deref().unwrap_or("none"),
				lane.mergeable.as_deref().unwrap_or("none"),
				lane.check_state.as_deref().unwrap_or("none"),
				lane
					.unresolved_review_threads
					.map_or_else(|| String::from("none"), |value| value.to_string())
			));
		}
	}

	output
}

fn render_queue_explain(
	config: &ServiceConfig,
	queued_candidates: &[OperatorQueuedIssueStatus],
) -> String {
	let mut output = String::new();

	output.push_str(&format!("Project: {}\n", config.service_id()));
	output.push_str("Mode: dry-run queue explain\n");
	output.push_str(&format!("Queued candidates: {}\n", queued_candidates.len()));
	output.push_str(&format!(
		"Ready: {}\n",
		queued_candidates
			.iter()
			.filter(|candidate| candidate.classification == "ready")
			.count()
	));
	output.push_str(&format!(
		"Waiting: {}\n",
		queued_candidates
			.iter()
			.filter(|candidate| candidate.classification == "waiting")
			.count()
	));
	output.push_str(&format!(
		"Blocked: {}\n",
		queued_candidates
			.iter()
			.filter(|candidate| candidate.classification == "blocked")
			.count()
	));
	output.push_str(&format!(
		"Claimed: {}\n",
		queued_candidates
			.iter()
			.filter(|candidate| candidate.classification == "claimed")
			.count()
	));
	output.push_str(&format!(
		"Closed: {}\n",
		queued_candidates
			.iter()
			.filter(|candidate| candidate.classification == "closed")
			.count()
	));
	output.push_str("\nQueued Candidate Reasons\n");

	if queued_candidates.is_empty() {
		output.push_str("- none\n");
		output.push_str(&format!("  {}\n", format_status_no_eligible_issue_hint(config.service_id())));

		return output;
	}

	for queued_issue in queued_candidates {
		append_rendered_queued_issue(&mut output, queued_issue, None);
	}

	output
}

fn rendered_backlog_queue_groups(
	queued_candidates: Vec<&OperatorQueuedIssueStatus>,
) -> (Vec<&OperatorQueuedIssueStatus>, Vec<&OperatorQueuedIssueStatus>) {
	let (stale_closed_queue_labels, non_closed_queue_candidates): (Vec<_>, Vec<_>) =
		queued_candidates.into_iter().partition(|queued_issue| queued_issue.classification == "closed");
	let backlog_candidates = non_closed_queue_candidates
		.into_iter()
		.filter(|queued_issue| queued_candidate_counts_as_waiting_intake(queued_issue))
		.collect::<Vec<_>>();

	(stale_closed_queue_labels, backlog_candidates)
}

fn rendered_recovery_worktrees(
	snapshot: &OperatorStatusSnapshot,
) -> Vec<(&str, &OperatorWorktreeStatus)> {
	let mut rendered_worktrees = snapshot
		.worktrees
		.iter()
		.map(|worktree| (rendered_worktree_role(worktree, snapshot), worktree))
		.filter(|(role, _)| rendered_worktree_role_rank(role) > 0)
		.collect::<Vec<_>>();

	rendered_worktrees.sort_by(|(left_role, left), (right_role, right)| {
		rendered_worktree_role_rank(left_role)
			.cmp(&rendered_worktree_role_rank(right_role))
			.then_with(|| left.issue_id.cmp(&right.issue_id))
			.then_with(|| left.branch_name.cmp(&right.branch_name))
			.then_with(|| left.worktree_path.cmp(&right.worktree_path))
	});

	rendered_worktrees
}

fn append_rendered_recovery_worktrees(
	output: &mut String,
	rendered_worktrees: &[(&str, &OperatorWorktreeStatus)],
	hides_owned_worktrees: bool,
) {
	if rendered_worktrees.is_empty() {
		if hides_owned_worktrees {
			output.push_str("- none (owned worktrees are shown in their lane sections above)\n");
		} else {
			output.push_str("- none\n");
		}

		return;
	}

	for (role, worktree) in rendered_worktrees {
		output.push_str(&format!(
			"- issue_id: {}\n  issue: {}\n  state: {}\n  role: {}\n  reason: {}\n  branch: {}\n  worktree_path: {}\n",
			worktree.issue_id,
			worktree.issue_identifier.as_deref().unwrap_or("none"),
			worktree.issue_state.as_deref().unwrap_or("unknown"),
			role,
			worktree.ownership_reason,
			worktree.branch_name,
			worktree.worktree_path
		));
	}
}

fn append_rendered_queued_issue_section(
	output: &mut String,
	title: &str,
	queued_issues: &[&OperatorQueuedIssueStatus],
	snapshot: &OperatorStatusSnapshot,
	show_running_owner: bool,
) {
	output.push_str(&format!("\n{title}\n"));

	if queued_issues.is_empty() {
		output.push_str("- none\n");

		if title == "Backlog" {
			output.push_str(&format!(
				"  {}\n",
				format_status_no_eligible_issue_hint(&snapshot.project_id)
			));
		}

		return;
	}

	for queued_issue in queued_issues {
		let running_owner = show_running_owner
			.then(|| active_run_id_for_queue_candidate(queued_issue, snapshot))
			.flatten();

		append_rendered_queued_issue(output, queued_issue, running_owner);
	}
}

fn operator_history_lanes(
	active_runs: &[OperatorRunStatus],
	recent_runs: &[OperatorRunStatus],
) -> Vec<OperatorHistoryLaneStatus> {
	let active_run_ids = active_runs
		.iter()
		.map(|run| run.run_id.as_str())
		.collect::<HashSet<_>>();
	let active_issue_ids = active_runs
		.iter()
		.map(|run| run.issue_id.as_str())
		.collect::<HashSet<_>>();
	let mut lane_indexes = HashMap::new();
	let mut lanes = Vec::new();

	for run in recent_runs {
		if active_run_ids.contains(run.run_id.as_str())
			|| active_issue_ids.contains(run.issue_id.as_str())
		{
			continue;
		}

		let group_key = operator_run_group_key(run);

		if let Some(index) = lane_indexes.get(&group_key) {
			let lane: &mut OperatorHistoryLaneStatus = &mut lanes[*index];

			lane.attempt_count += 1;

			if run.attempt_number > lane.latest_run.attempt_number {
				lane.latest_run = run.clone();
			}

			hydrate_history_lane_from_run(lane, run);

			lane.attempts.push(run.clone());

			continue;
		}

		lane_indexes.insert(group_key, lanes.len());
		lanes.push(OperatorHistoryLaneStatus {
			project_id: run.project_id.clone(),
			issue_id: run.issue_id.clone(),
			issue_identifier: run.issue_identifier.clone(),
			title: run.title.clone(),
			author: run.author.clone(),
			issue_key: operator_run_issue_key(run),
			attempt_count: 1,
			ledger_outcome: not_loaded_history_ledger_outcome(),
			latest_run: run.clone(),
			attempts: vec![run.clone()],
		});
	}

	lanes
}

fn hydrate_history_lane_from_run(lane: &mut OperatorHistoryLaneStatus, run: &OperatorRunStatus) {
	if lane.issue_identifier.is_none()
		&& let Some(issue_identifier) = run
			.issue_identifier
			.as_ref()
			.filter(|value| !value.trim().is_empty())
	{
		lane.issue_identifier = Some(issue_identifier.clone());
		lane.issue_key = issue_identifier.clone();
	}
	if lane.title.is_none() {
		lane.title = run.title.clone();
	}
	if lane.author.is_none() {
		lane.author = run.author.clone();
	}
}

fn operator_run_group_key(run: &OperatorRunStatus) -> String {
	let issue_id = run.issue_id.trim();

	if !issue_id.is_empty() && !issue_id.eq_ignore_ascii_case("unknown") {
		return issue_id.to_ascii_uppercase();
	}

	operator_run_issue_key(run)
}

fn operator_run_issue_key(run: &OperatorRunStatus) -> String {
	if let Some(issue_identifier) = run.issue_identifier.as_ref().filter(|value| {
		!value.trim().is_empty() && !value.eq_ignore_ascii_case("unknown")
	}) {
		return issue_identifier.clone();
	}
	if let Some(issue_identifier) = operator_run_issue_identifier_from_fields(
		&run.run_id,
		run.branch_name.as_deref(),
		run.worktree_path.as_deref(),
	) {
		return issue_identifier;
	}

	let issue_id = run.issue_id.trim();

	if issue_id.is_empty() {
		String::from("unknown")
	} else {
		issue_id.to_owned()
	}
}

fn operator_run_issue_identifier_from_fields(
	run_id: &str,
	branch_name: Option<&str>,
	worktree_path: Option<&str>,
) -> Option<String> {
	if let Some(issue_identifier) = issue_identifier_from_run_id(run_id) {
		return Some(issue_identifier);
	}

	for value in [branch_name, worktree_path] {
		if let Some(issue_identifier) = value.and_then(issue_identifier_in_text) {
			return Some(issue_identifier);
		}
	}

	None
}

fn issue_identifier_from_run_id(run_id: &str) -> Option<String> {
	if let Some((candidate, _attempt_suffix)) = run_id.split_once("-attempt-") {
		return issue_identifier_in_text(candidate);
	}
	if let Some(candidate) = run_id.strip_prefix("recovered-") {
		return issue_identifier_in_text(candidate);
	}

	None
}

fn issue_identifier_in_text(value: &str) -> Option<String> {
	let bytes = value.as_bytes();

	for index in 0..bytes.len() {
		if !bytes[index].is_ascii_alphabetic() {
			continue;
		}

		let mut prefix_end = index + 1;

		while prefix_end < bytes.len() && bytes[prefix_end].is_ascii_alphanumeric() {
			prefix_end += 1;
		}

		if prefix_end >= bytes.len() || bytes[prefix_end] != b'-' {
			continue;
		}

		let mut digit_end = prefix_end + 1;

		while digit_end < bytes.len() && bytes[digit_end].is_ascii_digit() {
			digit_end += 1;
		}

		if digit_end > prefix_end + 1 {
			return Some(value[index..digit_end].to_ascii_uppercase());
		}
	}

	None
}

fn queue_claim_belongs_to_active_run(
	queued_issue: &OperatorQueuedIssueStatus,
	snapshot: &OperatorStatusSnapshot,
) -> bool {
	queued_issue.classification == "claimed"
		&& active_run_id_for_queue_candidate(queued_issue, snapshot).is_some()
}

fn active_run_id_for_queue_candidate<'a>(
	queued_issue: &OperatorQueuedIssueStatus,
	snapshot: &'a OperatorStatusSnapshot,
) -> Option<&'a str> {
	snapshot
		.active_runs
		.iter()
		.find(|run| run.issue_id == queued_issue.issue_id)
		.map(|run| run.run_id.as_str())
}

fn append_rendered_queued_issue(
	output: &mut String,
	queued_issue: &OperatorQueuedIssueStatus,
	active_run_id: Option<&str>,
) {
	let priority = queued_issue
		.priority
		.map_or_else(|| String::from("none"), |value| value.to_string());
	let blockers = if queued_issue.blocker_identifiers.is_empty() {
		String::from("none")
	} else {
		queued_issue.blocker_identifiers.join(", ")
	};
	let running_owner = active_run_id.unwrap_or("none");

	output.push_str(&format!(
		"- issue_id: {}\n  issue: {}\n  title: {}\n  state: {}\n  priority: {}\n  created_at: {}\n  classification: {}\n  reason: {}\n  running_owner_run: {}\n  blockers: {}\n",
		queued_issue.issue_id,
		queued_issue.issue_identifier,
		queued_issue.title,
		queued_issue.state,
		priority,
		queued_issue.created_at,
		queued_issue.classification,
		queued_issue.reason,
		running_owner,
		blockers,
	));

	if let Some(attention) = &queued_issue.attention {
		output.push_str(&format!(
			"  attention: {}\n  attention_run: {}\n  attention_attempt: {}\n  attention_operation: {}\n  attention_thread: {}\n  attention_cause: {}\n  attention_next_action: {}\n  attention_auto_retry: {}\n  attention_retry_budget_attempts: {}\n  attention_worktree: {}\n  attention_last_activity: {}\n",
			attention.summary,
			attention.run_id.as_deref().unwrap_or("none"),
			attention
				.attempt_number
				.map_or_else(|| String::from("none"), |value| value.to_string()),
			attention.current_operation.as_deref().unwrap_or("none"),
			attention.thread_status.as_deref().unwrap_or("none"),
			attention.attention_error_class.as_deref().unwrap_or("none"),
			attention.attention_next_action.as_deref().unwrap_or("none"),
			attention.auto_retry_blocked_reason.as_deref().unwrap_or("none"),
			attention
				.retry_budget_attempt_count
				.map_or_else(|| String::from("none"), |value| value.to_string()),
			attention.worktree_path.as_deref().unwrap_or("none"),
			attention.last_activity_at.as_deref().unwrap_or("none"),
		));
	}
}

fn rendered_worktree_role<'a>(
	worktree: &'a OperatorWorktreeStatus,
	snapshot: &'a OperatorStatusSnapshot,
) -> &'a str {
	if !worktree.ownership.trim().is_empty() {
		return worktree.ownership.as_str();
	}
	if snapshot.active_runs.iter().any(|run| {
		run.worktree_path.as_deref() == Some(worktree.worktree_path.as_str())
			|| run.branch_name.as_deref() == Some(worktree.branch_name.as_str())
			|| run.issue_id == worktree.issue_id
	}) {
		return "active_lane";
	}
	if snapshot.post_review_lanes.iter().any(|lane| {
		lane.worktree_path == worktree.worktree_path
			|| lane.branch_name == worktree.branch_name
			|| lane.issue_id == worktree.issue_id
			|| lane.issue_identifier == worktree.issue_id
	}) {
		return "post_review_lane";
	}
	if snapshot.queued_candidates.iter().any(|candidate| {
		candidate.reason == "issue_needs_attention"
			&& (candidate
				.attention
				.as_ref()
				.and_then(|attention| attention.worktree_path.as_deref())
				== Some(worktree.worktree_path.as_str())
				|| candidate.issue_id == worktree.issue_id
				|| candidate.issue_identifier == worktree.issue_id)
	}) {
		return "blocked_queue_issue";
	}

	"orphaned_local_worktree"
}

fn rendered_worktree_role_rank(role: &str) -> u8 {
	match role {
		"active_lane" | "running_lane" | "blocked_queue_issue" | "queued_attention" => 0,
		"post_review_lane" => 1,
		_ => 2,
	}
}

fn render_child_agent_activity_summary(
	summary: Option<&ChildAgentActivitySummary>,
) -> String {
	let Some(summary) = summary else {
		return String::from("none");
	};
	let current = match (&summary.current_bucket, summary.current_elapsed_seconds) {
		(Some(bucket), Some(seconds)) => format!("{bucket} {}", format_seconds_compact(seconds)),
		(Some(bucket), None) => bucket.clone(),
		(None, _) => String::from("none"),
	};
	let buckets = render_child_agent_bucket_distribution(&summary.buckets);

	format!(
		"current={current}; wall={}; buckets={}; tool_calls={}",
		format_seconds_compact(summary.wall_seconds),
		buckets,
		summary.tool_call_count
	)
}

fn render_protocol_activity_summary(summary: Option<&ProtocolActivitySummary>) -> String {
	let Some(summary) = summary else {
		return String::from("none");
	};
	let turn = summary.turn_status.as_deref().unwrap_or("none");
	let wait = summary.waiting_reason.as_deref().unwrap_or("none");
	let rate_limit = summary.rate_limit_status.as_deref().unwrap_or("none");
	let recent = if summary.recent_events.is_empty() {
		String::from("none")
	} else {
		summary
			.recent_events
			.iter()
			.rev()
			.take(5)
			.map(|event| {
				event
					.detail
					.as_ref()
					.map_or_else(|| event.event_type.clone(), |detail| {
						format!("{}:{detail}", event.event_type)
					})
			})
			.collect::<Vec<_>>()
			.join(", ")
	};

	format!("turn={turn}; waiting={wait}; rate_limit={rate_limit}; recent={recent}")
}

fn render_account_summary(summary: Option<&CodexAccountActivitySummary>) -> String {
	let Some(summary) = summary else {
		return String::from("none");
	};
	let plan = summary.plan_type.as_deref().unwrap_or("unknown");
	let reached = summary.rate_limit_reached_type.as_deref().unwrap_or("none");
	let credits = render_codex_account_credits(summary);
	let token_status = render_codex_account_token_status(&summary.refresh_status);
	let primary = render_codex_account_window(
		summary.primary_window_seconds,
		summary.primary_remaining_percent,
		summary.primary_resets_at_unix_epoch,
	);
	let secondary = render_codex_account_window(
		summary.secondary_window_seconds,
		summary.secondary_remaining_percent,
		summary.secondary_resets_at_unix_epoch,
	);

	format!(
		"account={}; plan={plan}; status={}; token={token_status}; primary={primary}; secondary={secondary}; credits={credits}; reached={reached}",
		summary.account_fingerprint,
		summary.status,
	)
}

fn render_accounts_summary(accounts: &[CodexAccountActivitySummary]) -> String {
	if accounts.is_empty() {
		return String::from("none");
	}

	accounts
		.iter()
		.map(|summary| render_account_summary(Some(summary)))
		.collect::<Vec<_>>()
		.join(" | ")
}

fn render_codex_account_window(
	window_seconds: Option<i64>,
	remaining_percent: Option<i64>,
	resets_at_unix_epoch: Option<i64>,
) -> String {
	let label = window_seconds.map(codex_window_label).unwrap_or_else(|| String::from("window"));
	let remaining = remaining_percent.map_or_else(|| String::from("unknown"), |value| format!("{value}%"));
	let reset = format_optional_unix_timestamp(resets_at_unix_epoch).unwrap_or_else(|| String::from("unknown"));

	format!("{label} remaining={remaining} reset={reset}")
}

fn render_codex_account_credits(summary: &CodexAccountActivitySummary) -> String {
	if summary.credits_unlimited == Some(true) {
		return String::from("unlimited");
	}

	match (summary.credits_has_credits, summary.credits_balance.as_deref()) {
		(Some(false), Some(balance)) => format!("depleted balance={balance}"),
		(Some(false), None) => String::from("depleted"),
		(_, Some(balance)) => format!("balance={balance}"),
		(Some(true), None) => String::from("available"),
		(None, None) => String::from("unknown"),
	}
}

fn render_codex_account_token_status(refresh_status: &str) -> &'static str {
	match refresh_status {
		"not_needed" | "none" => "ok",
		"succeeded" | "refreshed" => "refreshed",
		"failed" => "refresh_failed",
		_ => "unknown",
	}
}

fn codex_window_label(window_seconds: i64) -> String {
	match window_seconds {
		18_000 => String::from("5h"),
		604_800 => String::from("7d"),
		seconds => format_seconds_compact(seconds),
	}
}

fn render_child_agent_context_pressure(
	summary: Option<&ChildAgentActivitySummary>,
) -> String {
	let Some(summary) = summary else {
		return String::from("none");
	};
	let current_input = summary
		.input_tokens_current
		.map(format_count_compact)
		.unwrap_or_else(|| String::from("none"));
	let max_input =
		summary.input_tokens_max.map(format_count_compact).unwrap_or_else(|| String::from("none"));
	let max_input_relation = match (summary.input_tokens_current, summary.input_tokens_max) {
		(Some(current), Some(max)) if current == max => " (same as current)",
		_ => "",
	};
	let largest_output = summary
		.largest_tool_output_bytes
		.map(format_bytes_compact)
		.unwrap_or_else(|| String::from("none"));
	let largest_tool = summary.largest_tool_output_tool.as_deref().unwrap_or("none");
	let warnings = if summary.large_output_warnings.is_empty() {
		String::from("none")
	} else {
		summary.large_output_warnings.join(" | ")
	};

	format!(
		"input=current_window {current_input}, peak_window {max_input}{max_input_relation}, cumulative_input {}; output_tokens={}; largest_output={largest_output} by {largest_tool}; warnings={warnings}",
		format_count_compact(summary.input_tokens_cumulative),
		format_count_compact(summary.output_tokens_cumulative)
	)
}

fn render_child_agent_bucket_distribution(
	buckets: &[ChildAgentActivityBucket],
) -> String {
	if buckets.is_empty() {
		return String::from("none");
	}

	let mut buckets = buckets.iter().collect::<Vec<_>>();

	buckets.sort_by(|left, right| {
		right
			.wall_seconds
			.cmp(&left.wall_seconds)
			.then_with(|| right.event_count.cmp(&left.event_count))
			.then_with(|| left.name.cmp(&right.name))
	});

	buckets
		.into_iter()
		.take(5)
		.map(|bucket| format!("{} {}", bucket.name, format_seconds_compact(bucket.wall_seconds)))
		.collect::<Vec<_>>()
		.join(", ")
}

fn format_seconds_compact(seconds: i64) -> String {
	if seconds >= 3_600 {
		return format!("{}h{}m", seconds / 3_600, (seconds % 3_600) / 60);
	}
	if seconds >= 60 {
		return format!("{}m{}s", seconds / 60, seconds % 60);
	}

	format!("{seconds}s")
}

fn format_count_compact(count: i64) -> String {
	if count >= 1_000_000 {
		return format!("{:.2}M", count as f64 / 1_000_000.0);
	}
	if count >= 1_000 {
		return format!("{:.1}k", count as f64 / 1_000.0);
	}

	count.to_string()
}

fn format_bytes_compact(bytes: i64) -> String {
	if bytes >= 1_048_576 {
		return format!("{:.1}MiB", bytes as f64 / 1_048_576.0);
	}
	if bytes >= 1_024 {
		return format!("{:.1}KiB", bytes as f64 / 1_024.0);
	}

	format!("{bytes}B")
}

fn append_rendered_history_lane(output: &mut String, lane: &OperatorHistoryLaneStatus) {
	output.push_str(&format!(
		"- issue: {}\n  project_id: {}\n  issue_id: {}\n  issue_identifier: {}\n  title: {}\n  attempts: {}\n  ledger_status: {}\n  outcome: {}\n",
		lane.issue_key,
		lane.project_id,
		lane.issue_id,
		lane.issue_identifier.as_deref().unwrap_or("none"),
		lane.title.as_deref().unwrap_or("none"),
		lane.attempt_count,
		lane.ledger_outcome.ledger_status,
		lane.ledger_outcome.final_outcome
	));

	append_rendered_history_ledger_outcome(output, &lane.ledger_outcome);

	if history_ledger_outcome_has_records(&lane.ledger_outcome) {
		output.push_str(&format!(
			"  local_attempts: {}\n  latest_run_id: {}\n",
			lane.attempt_count, lane.latest_run.run_id
		));
	} else {
		append_rendered_run(output, &lane.latest_run);
	}
	if lane.attempts.len() <= 1 {
		return;
	}

	output.push_str("  attempt_timeline:\n");

	for attempt in &lane.attempts {
		output.push_str(&format!(
			"    - run_id: {} attempt: {} status: {} phase: {} updated_at: {}\n",
			attempt.run_id,
			attempt.attempt_number,
			attempt.status,
			attempt.phase,
			attempt.updated_at
		));
	}
}

fn append_rendered_history_ledger_outcome(
	output: &mut String,
	outcome: &OperatorHistoryLedgerOutcome,
) {
	append_rendered_history_field(output, "event_type", outcome.final_event_type.as_deref());
	append_rendered_history_field(output, "event_at", outcome.final_event_at.as_deref());
	append_rendered_history_field(output, "summary", outcome.summary.as_deref());
	append_rendered_history_field(output, "pr_url", outcome.pr_url.as_deref());
	append_rendered_history_field(output, "commit_sha", outcome.commit_sha.as_deref());
	append_rendered_history_field(output, "branch", outcome.branch.as_deref());
	append_rendered_history_field(output, "closeout_status", outcome.closeout_status.as_deref());
	append_rendered_history_field(
		output,
		"needs_attention_reason",
		outcome.needs_attention_reason.as_deref(),
	);
	append_rendered_history_field(
		output,
		"lifecycle_started_at",
		outcome.lifecycle_started_at.as_deref(),
	);
	append_rendered_history_field(
		output,
		"lifecycle_finished_at",
		outcome.lifecycle_finished_at.as_deref(),
	);

	if let Some(elapsed) = outcome.lifecycle_elapsed_seconds {
		output.push_str(&format!("  lifecycle_elapsed_seconds: {elapsed}\n"));
	}

	output.push_str(&format!("  ledger_records: {}\n", outcome.record_count));
}

fn append_rendered_history_field(output: &mut String, label: &str, value: Option<&str>) {
	if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
		output.push_str(&format!("  {label}: {value}\n"));
	}
}

fn history_ledger_outcome_has_records(outcome: &OperatorHistoryLedgerOutcome) -> bool {
	matches!(outcome.ledger_status.as_str(), "present" | "partial")
}

fn append_rendered_run(output: &mut String, run: &OperatorRunStatus) {
	let (freshness_source, freshness_at) = operator_run_freshness(run);
	let protocol_event = match (&run.last_event_type, &run.last_event_at) {
		(Some(event_type), Some(timestamp)) => format!("{event_type} @ {timestamp}"),
		(Some(event_type), None) => event_type.clone(),
		(None, Some(timestamp)) => timestamp.clone(),
		(None, None) => String::from("none"),
	};
	let thread_id = run.thread_id.as_deref().unwrap_or("none");
	let turn_id = run.turn_id.as_deref().unwrap_or("none");
	let thread_status = run.thread_status.as_deref().unwrap_or("none");
	let thread_active_flags = if run.thread_active_flags.is_empty() {
		String::from("none")
	} else {
		run.thread_active_flags.join(",")
	};
	let idle_for_seconds =
		run.idle_for_seconds.map_or_else(|| String::from("none"), |value| value.to_string());
	let protocol_idle_for_seconds = run
		.protocol_idle_for_seconds
		.map_or_else(|| String::from("none"), |value| value.to_string());
	let branch_name = run.branch_name.as_deref().unwrap_or("none");
	let worktree_path = run.worktree_path.as_deref().unwrap_or("none");
	let queue_lease = operator_run_queue_lease_summary(run);
	let child_agent_activity =
		render_child_agent_activity_summary(run.child_agent_activity.as_ref());
	let context_pressure = render_child_agent_context_pressure(run.child_agent_activity.as_ref());
	let protocol_activity = render_protocol_activity_summary(run.protocol_activity.as_ref());
	let account = render_account_summary(run.account.as_ref());
	let accounts = render_accounts_summary(&run.accounts);

	output.push_str(&format!(
		"- run_id: {}\n  project_id: {}\n  issue_id: {}\n  issue_identifier: {}\n  title: {}\n  attempt: {}\n  status: {}\n  attempt_status: {}\n  phase: {}\n  wait_reason: {}\n  current_operation: {}\n  active_lease: {}\n  queue_lease_state: {}\n  queue_lease: {}\n  execution_liveness: {}\n  freshness_at: {}\n  freshness_source: {}\n  timing: run_idle={} protocol_idle={} last_progress={} protocol_event={} events={}\n  account: {}\n  accounts: {}\n  child_agent_activity: {}\n  protocol_activity: {}\n  context_pressure: {}\n  thread_id: {}\n  turn_id: {}\n  thread_status: {}\n  thread_active_flags: {}\n  interactive_requested: {}\n  continuation_pending: {}\n  branch: {}\n  worktree_path: {}\n  updated_at: {}\n  last_run_activity_at: {}\n  last_protocol_activity_at: {}\n  last_progress_at: {}\n  idle_for_seconds: {}\n  protocol_idle_for_seconds: {}\n  suspected_stall: {}\n  process_id: {}\n  process_alive: {}\n  process_liveness_reason: {}\n  retry_kind: {}\n  next_retry_at: {}\n  effective_model: {}\n  effective_model_provider: {}\n  effective_cwd: {}\n  effective_approval_policy: {}\n  effective_approvals_reviewer: {}\n  effective_sandbox_mode: {}\n  protocol_event: {}\n  event_count: {}\n",
		run.run_id,
		run.project_id,
		run.issue_id,
		run.issue_identifier.as_deref().unwrap_or("none"),
		run.title.as_deref().unwrap_or("none"),
		run.attempt_number,
		run.status,
		run.attempt_status,
		run.phase,
		run.wait_reason.as_deref().unwrap_or("none"),
		run.current_operation,
		if run.active_lease { "yes" } else { "no" },
		run.queue_lease_state,
		queue_lease,
		run.execution_liveness,
		freshness_at,
		freshness_source,
		idle_for_seconds,
		protocol_idle_for_seconds,
		run.last_progress_at.as_deref().unwrap_or("none"),
		protocol_event,
		run.event_count,
		account,
		accounts,
		child_agent_activity,
		protocol_activity,
		context_pressure,
		thread_id,
		turn_id,
		thread_status,
		thread_active_flags,
		if run.interactive_requested { "yes" } else { "no" },
		if run.continuation_pending { "yes" } else { "no" },
		branch_name,
		worktree_path,
		run.updated_at,
		run.last_run_activity_at.as_deref().unwrap_or("none"),
		run.last_protocol_activity_at.as_deref().unwrap_or("none"),
		run.last_progress_at.as_deref().unwrap_or("none"),
		idle_for_seconds,
		protocol_idle_for_seconds,
		if run.suspected_stall { "yes" } else { "no" },
		run.process_id.map_or_else(|| String::from("none"), |value| value.to_string()),
		run.process_alive.map_or_else(
			|| String::from("none"),
			|value| if value { String::from("yes") } else { String::from("no") },
		),
		run.process_liveness_reason.as_deref().unwrap_or("none"),
		run.retry_kind.as_deref().unwrap_or("none"),
		run.next_retry_at.as_deref().unwrap_or("none"),
		run.effective_model.as_deref().unwrap_or("none"),
		run.effective_model_provider.as_deref().unwrap_or("none"),
		run.effective_cwd.as_deref().unwrap_or("none"),
		run.effective_approval_policy.as_deref().unwrap_or("none"),
		run.effective_approvals_reviewer.as_deref().unwrap_or("none"),
		run.effective_sandbox_mode.as_deref().unwrap_or("none"),
		protocol_event,
		run.event_count
	));
}

fn operator_run_queue_lease_summary(run: &OperatorRunStatus) -> String {
	if run.active_lease {
		return String::from("held");
	}

	match run.execution_liveness.as_str() {
		"process_alive" => String::from("not_held (process_alive keeps lane visible)"),
		"thread_active" => String::from("not_held (thread_active keeps lane visible)"),
		"protocol_observed" => String::from("not_held (protocol_observed keeps lane visible)"),
		"process_stopped" => String::from("not_held (process_stopped needs attention)"),
		_ => String::from("not_held"),
	}
}

fn operator_run_freshness(run: &OperatorRunStatus) -> (&'static str, &str) {
	if operator_run_counts_as_active(run) {
		if let Some(timestamp) = run.last_run_activity_at.as_deref() {
			return ("last_run_activity_at", timestamp);
		}
		if let Some(timestamp) = run.last_progress_at.as_deref() {
			return ("last_progress_at", timestamp);
		}
		if let Some(timestamp) = run.last_protocol_activity_at.as_deref() {
			return ("last_protocol_activity_at", timestamp);
		}

		return ("none", "none");
	}

	("updated_at", run.updated_at.as_str())
}
