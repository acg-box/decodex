#[cfg(target_os = "macos")] use std::mem;
#[cfg(target_os = "macos")] use std::mem::MaybeUninit;

use github::PullRequestMergeViewResponse;
#[cfg(target_os = "macos")] use libc::PROC_PIDTBSDINFO;
#[cfg(target_os = "macos")] use libc::SZOMB;
#[cfg(target_os = "macos")] use libc::c_void;
#[cfg(target_os = "macos")] use libc::proc_bsdinfo;
use records::LinearExecutionEventRecord;
use state::{
	ProjectLoopEvidenceSnapshot, ProtocolActivityEventSummary, ReviewCheckpointArtifactLookup,
	ReviewLifecycleRecord,
};

use crate::{
	agent::REVIEW_POLICY_CONVERGENCE_BUDGET,
	pull_request::{self, PullRequestLandingGateView},
	tracker::public_text,
};

const QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT: &str = "linear_active_label_present";
const ATTENTION_ERROR_EVIDENCE_MISSING: &str = "evidence_missing";
const EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH: &str = "process_identity_mismatch";
const GHOST_LANE_CONDITION_TRACKER_ISSUE_MISSING: &str = "tracker_issue_missing";
const GHOST_LANE_OWNERSHIP_STATE: &str = "ghost_lane";
const GHOST_LANE_POLICY_STATE: &str = "runtime_recovery_required";
const GHOST_LANE_NEXT_ACTION: &str = "run_ghost_lane_recovery";
const GHOST_LANE_TERMINAL_STATUS: &str = "terminal_guarded";
const AUTONOMY_REPLAY_EVIDENCE_SCHEMA: &str = "decodex.autonomy_replay_evidence/1";

fn build_operator_status_snapshot(
	project: &ServiceConfig,
	state_store: &StateStore,
	limit: usize,
) -> crate::prelude::Result<OperatorStatusSnapshot> {
	build_operator_status_snapshot_with_account_mode(
		project,
		state_store,
		limit,
		AccountActivityMode::Probe,
	)
}

fn build_operator_status_snapshot_with_account_mode(
	project: &ServiceConfig,
	state_store: &StateStore,
	limit: usize,
	account_activity_mode: AccountActivityMode,
) -> crate::prelude::Result<OperatorStatusSnapshot> {
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let (leased_runs, recent_runs) = state_store.list_project_runs(project.service_id(), limit)?;
	let loop_evidence = state_store.project_loop_evidence_snapshot(project.service_id())?;
	let project_display_name = operator_project_display_name(project);
	let recent_runs = recent_runs
		.into_iter()
		.map(|run| {
			operator_run_status(project, &loop_evidence, &project_display_name, run, now_unix_epoch)
		})
		.collect::<crate::prelude::Result<Vec<_>>>()?;
	let current_lanes = operator_current_lane_statuses(
		project,
		state_store,
		&loop_evidence,
		&project_display_name,
		leased_runs,
		&recent_runs,
		now_unix_epoch,
	)?;
	let history_lanes = operator_history_lanes(&current_lanes, &recent_runs);
	let (worktrees, mut warnings, warning_details) =
		operator_status_worktrees(project, state_store)?;
	let accounts = codex_account_activity_summaries(project, &mut warnings, account_activity_mode);
	let mut snapshot = OperatorStatusSnapshot {
		project_id: project.service_id().to_owned(),
		run_limit: limit,
		status_source: None,
		snapshot_age_seconds: None,
		warnings,
		warning_details,
		connector_backoffs: Vec::new(),
		projects: vec![OperatorProjectStatus {
			project_id: project.service_id().to_owned(),
			config_path: String::new(),
			repo_root: project.repo_root().display().to_string(),
			enabled: true,
			github_cli_authority: operator_github_cli_authority(project),
			current_lane_count: current_lanes.len(),
			running_lane_count: current_lanes.len(),
			queued_candidate_count: 0,
			post_review_lane_count: 0,
			retained_worktree_count: 0,
			waiting_lane_count: 0,
			attention_count: 0,
			cleanup_blocked_count: 0,
			cleanup_pending_count: 0,
			connector_state: String::from("ok"),
			last_activity_at: None,
			warning_count: 0,
		}],
		account_control: global_codex_account_control_status(),
		accounts,
		current_lanes,
		recent_runs,
		history_lanes,
		execution_programs: Vec::new(),
		queued_candidates: Vec::new(),
		worktrees,
		post_review_lanes: Vec::new(),
	};

	refresh_worktree_ownership(&mut snapshot, None);
	refresh_operator_project_summary(&mut snapshot, None);

	Ok(snapshot)
}

fn build_lane_inspect_operator_runs(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &str,
	run_id: Option<&str>,
	limit: usize,
) -> crate::prelude::Result<Vec<OperatorRunStatus>> {
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let (current_lanes, recent_runs) =
		state_store.list_project_runs(project.service_id(), limit)?;
	let loop_evidence = state_store.project_loop_evidence_snapshot(project.service_id())?;
	let project_display_name = operator_project_display_name(project);
	let mut seen_run_ids = HashSet::new();
	let mut runs = Vec::new();

	for run in current_lanes.into_iter().chain(recent_runs) {
		if !seen_run_ids.insert(run.run_id().to_owned()) {
			continue;
		}
		if !project_run_status_issue_matches(&run, issue) {
			continue;
		}
		if run_id.is_some_and(|expected| expected != run.run_id()) {
			continue;
		}

		let mut run = operator_run_status(
			project,
			&loop_evidence,
			&project_display_name,
			run,
			now_unix_epoch,
		)?;

		apply_terminal_ledger_projection_to_lane_inspect_run(project, state_store, &mut run)?;

		runs.push(run);
	}

	Ok(runs)
}

fn apply_terminal_ledger_projection_to_lane_inspect_run(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &mut OperatorRunStatus,
) -> crate::prelude::Result<()> {
	let records = state_store.list_linear_execution_events(project.service_id(), &run.issue_id)?;

	if records.is_empty() {
		return Ok(());
	}

	let records = local_history_ledger_records(records);
	let outcome = operator_history_ledger_outcome(&records);

	if history_ledger_outcome_is_terminal(&outcome)
		&& !current_lane_has_authoritative_live_owner(run)
	{
		apply_terminal_history_ledger_outcome_to_run(run, &outcome);
	}

	Ok(())
}

fn project_run_status_issue_matches(run: &ProjectRunStatus, issue: &str) -> bool {
	let issue = issue.trim();
	let worktree_path = run.worktree_path().map(|path| path.display().to_string());
	let issue_identifier = operator_run_issue_identifier_from_fields(
		run.run_id(),
		run.branch_name(),
		worktree_path.as_deref(),
	);

	run.issue_id() == issue
		|| issue_identifier.as_deref() == Some(issue)
		|| issue_identifier
			.as_ref()
			.is_some_and(|identifier| identifier.eq_ignore_ascii_case(issue))
}

fn operator_current_lane_statuses(
	project: &ServiceConfig,
	state_store: &StateStore,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	project_display_name: &str,
	leased_runs: Vec<ProjectRunStatus>,
	recent_runs: &[OperatorRunStatus],
	now_unix_epoch: i64,
) -> crate::prelude::Result<Vec<OperatorRunStatus>> {
	let mut current_lanes = leased_runs
		.into_iter()
		.map(|run| {
			operator_run_status(project, loop_evidence, project_display_name, run, now_unix_epoch)
		})
		.collect::<crate::prelude::Result<Vec<_>>>()?
		.into_iter()
		.filter(operator_run_counts_as_current_lane)
		.collect::<Vec<_>>();
	let latest_attempt_by_issue_key =
		operator_latest_attempt_by_issue_key(current_lanes.iter().chain(recent_runs.iter()));

	current_lanes.retain(|run| {
		!operator_run_is_superseded_by_newer_attempt(run, &latest_attempt_by_issue_key)
	});

	let mut current_lane_run_ids =
		current_lanes.iter().map(|run| run.run_id.clone()).collect::<HashSet<_>>();

	for run in recent_runs {
		if current_lane_run_ids.contains(&run.run_id)
			|| operator_run_is_superseded_by_newer_attempt(run, &latest_attempt_by_issue_key)
			|| !operator_run_has_live_execution(run)
		{
			continue;
		}

		current_lane_run_ids.insert(run.run_id.clone());
		current_lanes.push(run.clone());
	}

	hydrate_current_lane_lifecycle_metrics(
		project,
		state_store,
		loop_evidence,
		project_display_name,
		&mut current_lanes,
		recent_runs,
		now_unix_epoch,
	)?;

	Ok(current_lanes)
}

fn operator_latest_attempt_by_issue_key<'a>(
	runs: impl Iterator<Item = &'a OperatorRunStatus>,
) -> HashMap<String, i64> {
	let mut latest_attempt_by_issue_key = HashMap::new();

	for run in runs {
		let issue_key = operator_run_group_key(run);
		let latest_attempt =
			latest_attempt_by_issue_key.entry(issue_key).or_insert(run.attempt_number);

		*latest_attempt = (*latest_attempt).max(run.attempt_number);
	}

	latest_attempt_by_issue_key
}

fn operator_run_is_superseded_by_newer_attempt(
	run: &OperatorRunStatus,
	latest_attempt_by_issue_key: &HashMap<String, i64>,
) -> bool {
	latest_attempt_by_issue_key
		.get(&operator_run_group_key(run))
		.is_some_and(|latest_attempt| run.attempt_number < *latest_attempt)
}

fn global_codex_account_control_status() -> OperatorCodexAccountControlStatus {
	let account_selector = runtime::global_fixed_account_selector().ok().flatten();
	let mode = if account_selector.is_some() { "fixed" } else { "balanced" };

	OperatorCodexAccountControlStatus { mode: String::from(mode), account_selector }
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
		LiveOperatorStatusSnapshotOptions {
			hydrate_history_ledger: true,
			run_issue_metadata_hydration: RunIssueMetadataHydration::AllRows,
			account_activity_mode: AccountActivityMode::Probe,
			configure_dispatch_slots: true,
		},
	)
}

fn build_status_command_operator_status_snapshot<T>(
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
		LiveOperatorStatusSnapshotOptions {
			hydrate_history_ledger: true,
			run_issue_metadata_hydration: RunIssueMetadataHydration::AllRows,
			account_activity_mode: AccountActivityMode::Snapshot,
			configure_dispatch_slots: true,
		},
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
		LiveOperatorStatusSnapshotOptions {
			hydrate_history_ledger: false,
			run_issue_metadata_hydration: RunIssueMetadataHydration::CurrentLaneRowsOnly,
			account_activity_mode: AccountActivityMode::Snapshot,
			configure_dispatch_slots: true,
		},
	)
}

fn build_live_operator_status_snapshot_with_history_ledger<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
	options: LiveOperatorStatusSnapshotOptions,
) -> crate::prelude::Result<OperatorStatusSnapshot>
where
	T: IssueTracker,
{
	if options.configure_dispatch_slots {
		state_store.configure_dispatch_slot_root(project.service_id(), project.worktree_root())?;
	}

	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};
	let execution_program_readback =
		operator_execution_program_statuses(tracker, project, workflow, state_store)?;
	let mut snapshot = build_operator_status_snapshot_with_account_mode(
		project,
		state_store,
		limit,
		options.account_activity_mode,
	)?;

	snapshot.execution_programs = execution_program_readback.statuses;

	if execution_program_readback.issue_metadata_unavailable {
		add_operator_snapshot_warning(
			&mut snapshot,
			"execution_program_issue_metadata_unavailable",
		);
	}

	hydrate_history_lanes_from_local_ledger(project, state_store, &mut snapshot)?;
	hydrate_live_operator_external_observers(
		LiveOperatorStatusObserverContext {
			tracker,
			project,
			workflow,
			state_store,
			review_state_inspector: &review_state_inspector,
			hydrate_history_ledger: options.hydrate_history_ledger,
			run_issue_metadata_hydration: options.run_issue_metadata_hydration,
		},
		&mut snapshot,
	)?;
	apply_missing_issue_ghost_lane_projection(project, state_store, &mut snapshot)?;

	let terminal_projection =
		current_lane_terminal_projection_from_local_ledger(project, state_store, &snapshot)?;

	apply_operator_lane_terminal_projection(
		&mut snapshot,
		terminal_projection,
		Some(workflow.frontmatter().tracker().resolved_completed_state()),
	);
	suppress_terminal_attention_queue_echoes(&mut snapshot);
	hydrate_post_review_lane_current_lane_shadowing(&mut snapshot);
	refresh_worktree_ownership(
		&mut snapshot,
		Some(workflow.frontmatter().tracker().resolved_completed_state()),
	);
	refresh_operator_project_summary(
		&mut snapshot,
		Some(workflow.frontmatter().tracker().resolved_completed_state()),
	);

	Ok(snapshot)
}

fn hydrate_live_operator_external_observers<T>(
	context: LiveOperatorStatusObserverContext<'_, T>,
	snapshot: &mut OperatorStatusSnapshot,
) -> crate::prelude::Result<()>
where
	T: IssueTracker,
{
	let stale_terminal_local_issue_ids =
		stale_terminal_local_issue_ids(context.project, context.state_store)?;
	let mut paused = pause_operator_snapshot_for_stored_tracker_backoff(&context, snapshot)?;

	if !paused {
		paused = apply_tracker_observer_outcome(
			hydrate_operator_run_rows_from_tracker(
				context.tracker,
				context.project,
				context.workflow,
				snapshot,
				context.run_issue_metadata_hydration,
				&stale_terminal_local_issue_ids,
			),
			snapshot,
			context.state_store,
			context.project,
			"run_issue_metadata_unavailable",
		);
	}
	if !paused && context.hydrate_history_ledger {
		paused = apply_tracker_observer_outcome(
			hydrate_history_lanes_from_linear_ledger(
				context.tracker,
				context.project,
				snapshot,
				&stale_terminal_local_issue_ids,
			),
			snapshot,
			context.state_store,
			context.project,
			"execution_ledger_status_unavailable",
		);
	}
	if !paused {
		paused = hydrate_queued_candidate_status_observer(&context, snapshot);
	}
	if !paused {
		paused = hydrate_post_review_lane_status_observer(&context, snapshot)?;
	}
	if paused {
		if snapshot.post_review_lanes.is_empty() {
			snapshot.post_review_lanes = build_degraded_post_review_lane_statuses(
				context.project,
				context.state_store,
				context.review_state_inspector,
			)?;
		}

		add_operator_snapshot_warning(snapshot, "external_observer_status_skipped");
	}

	Ok(())
}

fn pause_operator_snapshot_for_stored_tracker_backoff<T>(
	context: &LiveOperatorStatusObserverContext<'_, T>,
	snapshot: &mut OperatorStatusSnapshot,
) -> crate::prelude::Result<bool> {
	let Some(backoff) =
		active_stored_tracker_backoff_status(context.state_store, context.project.service_id())?
	else {
		return Ok(false);
	};

	add_tracker_backoff_to_operator_snapshot(snapshot, &backoff);

	Ok(true)
}

fn apply_tracker_observer_outcome(
	outcome: TrackerObserverOutcome,
	snapshot: &mut OperatorStatusSnapshot,
	state_store: &StateStore,
	project: &ServiceConfig,
	unavailable_warning: &'static str,
) -> bool {
	match outcome {
		TrackerObserverOutcome::Ok => false,
		TrackerObserverOutcome::Unavailable => {
			add_operator_snapshot_warning(snapshot, unavailable_warning);

			false
		},
		TrackerObserverOutcome::Backoff(backoff) => {
			pause_operator_snapshot_for_tracker_backoff(snapshot, state_store, project, &backoff);

			true
		},
	}
}

fn pause_operator_snapshot_for_tracker_backoff(
	snapshot: &mut OperatorStatusSnapshot,
	state_store: &StateStore,
	project: &ServiceConfig,
	backoff: &TrackerConnectorBackoff,
) {
	persist_tracker_backoff_state(state_store, project.service_id(), backoff);

	let backoff = backoff
		.to_operator_status(project.service_id(), OffsetDateTime::now_utc().unix_timestamp());

	add_tracker_backoff_to_operator_snapshot(snapshot, &backoff);
}

fn hydrate_queued_candidate_status_observer<T>(
	context: &LiveOperatorStatusObserverContext<'_, T>,
	snapshot: &mut OperatorStatusSnapshot,
) -> bool
where
	T: IssueTracker,
{
	match build_queued_candidate_statuses(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
	) {
		Ok(queued_candidates) => {
			snapshot.queued_candidates = queued_candidates;

			false
		},
		Err(error) => {
			let Some(backoff) =
				tracker_connector_backoff(&error, Instant::now(), "queued_candidate_status")
			else {
				let _ = error;

				tracing::warn!(
					"Skipped queued candidate status while publishing an operator snapshot; sensitive runtime details were withheld."
				);

				add_operator_snapshot_warning(snapshot, "queued_candidate_status_unavailable");

				return false;
			};

			pause_operator_snapshot_for_tracker_backoff(
				snapshot,
				context.state_store,
				context.project,
				&backoff,
			);

			true
		},
	}
}

fn hydrate_post_review_lane_status_observer<T>(
	context: &LiveOperatorStatusObserverContext<'_, T>,
	snapshot: &mut OperatorStatusSnapshot,
) -> crate::prelude::Result<bool>
where
	T: IssueTracker,
{
	match build_post_review_lane_statuses_and_hydrate_worktrees(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		context.review_state_inspector,
		snapshot,
	) {
		Ok(post_review_lanes) => {
			snapshot.post_review_lanes = post_review_lanes;

			Ok(false)
		},
		Err(error) => {
			let Some(backoff) =
				tracker_connector_backoff(&error, Instant::now(), "post_review_lane_status")
			else {
				let _ = error;

				tracing::warn!(
					"Skipped post-review lane status while publishing an operator snapshot; sensitive runtime details were withheld."
				);

				add_operator_snapshot_warning(snapshot, "post_review_lane_status_unavailable");

				return Ok(false);
			};

			pause_operator_snapshot_for_tracker_backoff(
				snapshot,
				context.state_store,
				context.project,
				&backoff,
			);

			snapshot.post_review_lanes = build_degraded_post_review_lane_statuses(
				context.project,
				context.state_store,
				context.review_state_inspector,
			)?;

			Ok(true)
		},
	}
}

fn add_tracker_backoff_to_operator_snapshot(
	snapshot: &mut OperatorStatusSnapshot,
	backoff: &OperatorConnectorBackoffStatus,
) {
	add_operator_snapshot_warning(snapshot, &backoff.warning);

	if !snapshot.connector_backoffs.iter().any(|existing| {
		existing.project_id == backoff.project_id && existing.connector == backoff.connector
	}) {
		snapshot.connector_backoffs.push(backoff.clone());
	}
}

fn add_operator_snapshot_warning(snapshot: &mut OperatorStatusSnapshot, warning: &str) {
	if !snapshot.warnings.iter().any(|existing| existing == warning) {
		snapshot.warnings.push(warning.to_owned());
	}
}

fn codex_account_activity_summaries(
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

fn hydrate_operator_run_rows_from_tracker<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	snapshot: &mut OperatorStatusSnapshot,
	hydration: RunIssueMetadataHydration,
	stale_terminal_local_issue_ids: &HashSet<String>,
) -> TrackerObserverOutcome
where
	T: IssueTracker,
{
	let issue_ids =
		operator_snapshot_run_issue_ids(snapshot, hydration, stale_terminal_local_issue_ids);

	if issue_ids.is_empty() {
		return TrackerObserverOutcome::Ok;
	}

	match tracker.refresh_issues(&issue_ids) {
		Ok(issues) => {
			let active_label = crate::tracker::automation_active_label(project.service_id());
			let needs_attention_label =
				workflow.frontmatter().tracker().needs_attention_label().to_owned();
			let metadata_by_issue_id = issues
				.into_iter()
				.map(|issue| {
					let active_label_present = issue.has_label(&active_label);
					let needs_attention_label_present = issue.has_label(&needs_attention_label);

					(
						issue.id,
						OperatorIssueDisplayMetadata {
							issue_identifier: issue.identifier,
							title: Some(issue.title),
							author: issue.author,
							issue_state: Some(issue.state.name),
							active_label_present: Some(active_label_present),
							needs_attention_label_present: Some(needs_attention_label_present),
						},
					)
				})
				.collect::<HashMap<_, _>>();

			hydrate_operator_snapshot_run_rows(snapshot, &metadata_by_issue_id);

			if let Err(error) = hydrate_missing_current_lane_tracker_metadata(
				tracker,
				snapshot,
				&active_label,
				&needs_attention_label,
			) {
				if let Some(backoff) = tracker_connector_backoff(
					&error,
					Instant::now(),
					"run_issue_identifier_metadata",
				) {
					return TrackerObserverOutcome::Backoff(backoff);
				}

				let _ = error;

				tracing::warn!(
					project_id = project.service_id(),
					"Skipped missing-issue current lane classification; sensitive tracker details were withheld."
				);

				return TrackerObserverOutcome::Unavailable;
			}

			TrackerObserverOutcome::Ok
		},
		Err(error)
			if issue_ids.iter().any(|issue_id| {
				tracker::issue_lookup_missing_error_for_candidate(&error, issue_id)
			}) =>
		{
			let active_label = crate::tracker::automation_active_label(project.service_id());
			let needs_attention_label =
				workflow.frontmatter().tracker().needs_attention_label().to_owned();

			if let Err(error) = hydrate_missing_current_lane_tracker_metadata(
				tracker,
				snapshot,
				&active_label,
				&needs_attention_label,
			) {
				if let Some(backoff) = tracker_connector_backoff(
					&error,
					Instant::now(),
					"run_issue_identifier_metadata",
				) {
					return TrackerObserverOutcome::Backoff(backoff);
				}

				let _ = error;

				tracing::warn!(
					project_id = project.service_id(),
					"Skipped missing-issue current lane classification; sensitive tracker details were withheld."
				);

				return TrackerObserverOutcome::Unavailable;
			}

			TrackerObserverOutcome::Ok
		},
		Err(error) => {
			if let Some(backoff) =
				tracker_connector_backoff(&error, Instant::now(), "run_issue_metadata")
			{
				return TrackerObserverOutcome::Backoff(backoff);
			}

			let _ = error;

			tracing::warn!(
				project_id = project.service_id(),
				"Skipped tracker issue metadata hydration for operator run rows; sensitive tracker details were withheld."
			);

			TrackerObserverOutcome::Unavailable
		},
	}
}

fn operator_snapshot_run_issue_ids(
	snapshot: &OperatorStatusSnapshot,
	hydration: RunIssueMetadataHydration,
	stale_terminal_local_issue_ids: &HashSet<String>,
) -> Vec<String> {
	let mut issue_ids = BTreeSet::new();

	for run in &snapshot.current_lanes {
		append_operator_run_issue_id(&mut issue_ids, run, stale_terminal_local_issue_ids);
	}

	if matches!(hydration, RunIssueMetadataHydration::AllRows) {
		for run in &snapshot.recent_runs {
			append_operator_run_issue_id(&mut issue_ids, run, stale_terminal_local_issue_ids);
		}
		for lane in &snapshot.history_lanes {
			append_operator_run_issue_id(
				&mut issue_ids,
				&lane.latest_run,
				stale_terminal_local_issue_ids,
			);

			for attempt in &lane.attempts {
				append_operator_run_issue_id(
					&mut issue_ids,
					attempt,
					stale_terminal_local_issue_ids,
				);
			}
		}
	}

	issue_ids.into_iter().collect()
}

fn append_operator_run_issue_id(
	issue_ids: &mut BTreeSet<String>,
	run: &OperatorRunStatus,
	stale_terminal_local_issue_ids: &HashSet<String>,
) {
	if operator_run_is_stale_terminal_local_residue(run, stale_terminal_local_issue_ids) {
		return;
	}

	let issue_id = run.issue_id.trim();

	if !issue_id.is_empty() && !issue_id.eq_ignore_ascii_case("unknown") {
		issue_ids.insert(issue_id.to_owned());
	}
}

fn operator_run_is_terminal_unleased_identifier(run: &OperatorRunStatus) -> bool {
	!run.run_lease
		&& looks_like_tracker_issue_identifier_key(&run.issue_id)
		&& local_run_attempt_status_is_terminal(&run.attempt_status)
}

fn operator_run_is_stale_terminal_local_residue(
	run: &OperatorRunStatus,
	stale_terminal_local_issue_ids: &HashSet<String>,
) -> bool {
	operator_run_is_terminal_unleased_identifier(run)
		&& stale_terminal_local_issue_ids.contains(run.issue_id.trim())
}

fn hydrate_operator_snapshot_run_rows(
	snapshot: &mut OperatorStatusSnapshot,
	metadata_by_issue_id: &HashMap<String, OperatorIssueDisplayMetadata>,
) {
	for run in snapshot.current_lanes.iter_mut().chain(snapshot.recent_runs.iter_mut()) {
		hydrate_operator_run_row_from_issue_metadata(run, metadata_by_issue_id);
	}
	for lane in &mut snapshot.history_lanes {
		hydrate_history_lane_from_issue_metadata(lane, metadata_by_issue_id);
	}
}

fn hydrate_missing_current_lane_tracker_metadata<T>(
	tracker: &T,
	snapshot: &mut OperatorStatusSnapshot,
	active_label: &str,
	needs_attention_label: &str,
) -> crate::prelude::Result<()>
where
	T: IssueTracker,
{
	let mut metadata_by_issue_id = HashMap::new();
	let mut missing_rows = Vec::new();

	for run in &snapshot.current_lanes {
		if run.issue_state.is_some() {
			continue;
		}

		let Some(selector) = operator_run_tracker_issue_identifier_selector(run) else {
			continue;
		};

		match tracker.get_issue_by_identifier(&selector) {
			Ok(Some(issue)) => {
				metadata_by_issue_id.insert(
					run.issue_id.clone(),
					operator_issue_display_metadata(&issue, active_label, needs_attention_label),
				);
			},
			Ok(None) => missing_rows.push((run.run_id.clone(), run.issue_id.clone(), selector)),
			Err(error) if tracker::issue_lookup_missing_error_for_candidate(&error, &selector) => {
				missing_rows.push((run.run_id.clone(), run.issue_id.clone(), selector));
			},
			Err(error) => return Err(error),
		}
	}

	if !metadata_by_issue_id.is_empty() {
		hydrate_operator_snapshot_run_rows(snapshot, &metadata_by_issue_id);
	}

	for (run_id, issue_id, selector) in missing_rows {
		mark_operator_run_tracker_issue_missing(snapshot, &run_id, &issue_id, &selector);
	}

	Ok(())
}

fn operator_issue_display_metadata(
	issue: &TrackerIssue,
	active_label: &str,
	needs_attention_label: &str,
) -> OperatorIssueDisplayMetadata {
	OperatorIssueDisplayMetadata {
		issue_identifier: issue.identifier.clone(),
		title: Some(issue.title.clone()),
		author: issue.author.clone(),
		issue_state: Some(issue.state.name.clone()),
		active_label_present: Some(issue.has_label(active_label)),
		needs_attention_label_present: Some(issue.has_label(needs_attention_label)),
	}
}

fn operator_run_tracker_issue_identifier_selector(run: &OperatorRunStatus) -> Option<String> {
	run.issue_identifier
		.as_ref()
		.filter(|identifier| commit_message::looks_like_issue_identifier(identifier))
		.map(|identifier| identifier.to_ascii_uppercase())
		.or_else(|| {
			operator_run_issue_identifier_from_fields(
				&run.run_id,
				run.branch_name.as_deref(),
				run.worktree_path.as_deref(),
			)
		})
		.or_else(|| {
			commit_message::looks_like_issue_identifier(&run.issue_id)
				.then(|| run.issue_id.to_ascii_uppercase())
		})
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
	if let Some(issue_state) =
		metadata.issue_state.as_ref().filter(|issue_state| !issue_state.trim().is_empty())
	{
		lane.issue_state = Some(issue_state.clone());
	}
	if let Some(active_label_present) = metadata.active_label_present {
		lane.active_label_present = Some(active_label_present);
	}
	if let Some(needs_attention_label_present) = metadata.needs_attention_label_present {
		lane.needs_attention_label_present = Some(needs_attention_label_present);
	}
}

fn apply_run_issue_metadata(run: &mut OperatorRunStatus, metadata: &OperatorIssueDisplayMetadata) {
	if !metadata.issue_identifier.trim().is_empty() {
		run.issue_identifier = Some(metadata.issue_identifier.clone());
	}

	if let Some(title) = metadata.title.as_ref().filter(|title| !title.trim().is_empty()) {
		run.title = Some(title.clone());
	}
	if let Some(author) = metadata.author.as_ref().filter(|author| !author.trim().is_empty()) {
		run.author = Some(author.clone());
	}
	if let Some(issue_state) =
		metadata.issue_state.as_ref().filter(|issue_state| !issue_state.trim().is_empty())
	{
		run.issue_state = Some(issue_state.clone());
	}
	if let Some(active_label_present) = metadata.active_label_present {
		run.active_label_present = Some(active_label_present);
	}
	if let Some(needs_attention_label_present) = metadata.needs_attention_label_present {
		run.needs_attention_label_present = Some(needs_attention_label_present);
	}
}

fn fill_missing_history_lane_issue_metadata(
	lane: &mut OperatorHistoryLaneStatus,
	metadata: &OperatorIssueDisplayMetadata,
) {
	if lane.issue_identifier.as_ref().is_none_or(|identifier| identifier.trim().is_empty())
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
	if run.issue_identifier.as_ref().is_none_or(|identifier| identifier.trim().is_empty())
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
	if run.issue_state.as_ref().is_none_or(|issue_state| issue_state.trim().is_empty())
		&& let Some(issue_state) =
			metadata.issue_state.as_ref().filter(|issue_state| !issue_state.trim().is_empty())
	{
		run.issue_state = Some(issue_state.clone());
	}
	if run.active_label_present.is_none() {
		run.active_label_present = metadata.active_label_present;
	}
	if run.needs_attention_label_present.is_none() {
		run.needs_attention_label_present = metadata.needs_attention_label_present;
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

fn queued_issue_is_retained_post_review_lane(
	issue: &TrackerIssue,
	success_state: &str,
	retained_post_review_issue_ids: &HashSet<String>,
) -> bool {
	issue.state.name == success_state && retained_post_review_issue_ids.contains(&issue.id)
}

fn operator_queued_issue_status<T>(
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

fn queued_issue_blocker_identifiers(
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

fn observe_dependency_program_stale_guardrail(
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

fn dependency_blocker_fingerprint(issue: &TrackerIssue, workflow: &WorkflowDocument) -> String {
	let mut blockers = issue
		.blockers
		.iter()
		.filter(|blocker| !state_name_is_terminal(&blocker.state.name, workflow))
		.map(|blocker| format!("{}:{}", blocker.identifier, blocker.state.name))
		.collect::<Vec<_>>();

	blockers.sort();

	loop_guardrail_text_hash(&blockers.join("|"))
}

fn classify_queued_issue<T>(
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
	let active_issue_ids = active_shared_issue_ids(project, state_store)?;
	let worktrees = state_store
		.list_worktrees(project.service_id())?
		.into_iter()
		.filter_map(|mapping| {
			match worktree_mapping_is_stale_terminal_local_residue(
				project,
				state_store,
				&mapping,
				&active_issue_ids,
			) {
				Ok(true) => None,
				Ok(false) => Some(Ok(mapping)),
				Err(error) => Some(Err(error)),
			}
		})
		.collect::<crate::prelude::Result<Vec<_>>>()?;

	if worktrees.is_empty() {
		return Ok(Vec::new());
	}

	let issue_ids =
		worktrees.iter().map(|mapping| mapping.issue_id().to_owned()).collect::<Vec<_>>();
	let issues = refresh_recoverable_runtime_issues(tracker, &issue_ids)?;
	let issues_by_id =
		issues.into_iter().map(|issue| (issue.id.clone(), issue)).collect::<HashMap<_, _>>();

	Ok(worktrees
		.into_iter()
		.filter_map(|worktree| {
			issues_by_id.get(worktree.issue_id()).cloned().map(|issue| (worktree, issue))
		})
		.collect())
}

fn build_degraded_post_review_lane_statuses<I>(
	project: &ServiceConfig,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> crate::prelude::Result<Vec<OperatorPostReviewLaneStatus>>
where
	I: PullRequestReviewStateInspector,
{
	let mut lanes = Vec::new();

	for worktree in state_store.list_worktrees(project.service_id())? {
		let Some(review_handoff) = state_store.review_handoff_marker(
			project.service_id(),
			worktree.issue_id(),
			worktree.branch_name(),
		)?
		else {
			continue;
		};
		let issue_identifier = retained_issue_identifier_from_worktree(&worktree);
		let review_state = review_state_inspector
			.inspect_review_state(worktree.worktree_path(), review_handoff.pr_url())
			.ok();
		let classification =
			PostReviewReadbackDegradation::tracker_issue_from_handoff(&review_handoff)
				.wait_for_review_classification(review_state);

		lanes.push(degraded_post_review_lane_status_from_classification(
			project,
			state_store,
			&worktree,
			&review_handoff,
			issue_identifier,
			classification,
		)?);
	}

	lanes.sort_by(|left, right| left.issue_identifier.cmp(&right.issue_identifier));

	Ok(lanes)
}

fn degraded_post_review_lane_status_from_classification(
	project: &ServiceConfig,
	state_store: &StateStore,
	worktree: &WorktreeMapping,
	review_handoff: &ReviewHandoffMarker,
	issue_identifier: String,
	classification: PostReviewLaneClassification,
) -> crate::prelude::Result<OperatorPostReviewLaneStatus> {
	let loop_status = operator_loop_status_for_run(
		project,
		state_store,
		worktree.issue_id(),
		review_handoff.run_id(),
		review_handoff.attempt_number(),
		Some("repair"),
		None,
	)?;

	Ok(OperatorPostReviewLaneStatus {
		project_id: project.service_id().to_owned(),
		issue_id: worktree.issue_id().to_owned(),
		issue_identifier,
		issue_state: String::from("tracker_readback_degraded"),
		branch_name: worktree.branch_name().to_owned(),
		worktree_path: relative_worktree_path_for_path(project, worktree.worktree_path()),
		classification: classification.decision.as_str().to_owned(),
		reason: classification.reason,
		pr_url: classification.pr_url,
		pr_head_sha: classification.pr_head_sha,
		pr_state: classification.pr_state,
		review_decision: classification.review_decision,
		mergeable: classification.mergeable,
		check_state: classification.check_state,
		unresolved_review_threads: classification.unresolved_review_threads,
		shadowed_by_current_lane: false,
		readback_warning: classification.readback_warning,
		readback_root_cause: classification.readback_root_cause,
		loop_status: Some(loop_status),
	})
}

fn retained_issue_identifier_from_worktree(worktree: &WorktreeMapping) -> String {
	worktree
		.worktree_path()
		.file_name()
		.and_then(|name| name.to_str())
		.map(str::trim)
		.filter(|name| !name.is_empty())
		.unwrap_or_else(|| worktree.issue_id())
		.to_ascii_uppercase()
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
	let issues_by_id = worktree_issues
		.iter()
		.map(|(_, issue)| (issue.id.as_str(), issue))
		.collect::<HashMap<_, _>>();

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
		Err(_error) => {
			return Ok(Some(blocked_post_review_lane_status(
				context.project,
				&issue,
				&worktree,
				"worktree_checkout_branch_read_failed",
			)));
		},
	};
	let local_head_oid = match worktree_head_oid(worktree.worktree_path()) {
		Ok(local_head_oid) => local_head_oid,
		Err(_error) => {
			return Ok(Some(blocked_post_review_lane_status(
				context.project,
				&issue,
				&worktree,
				"worktree_head_read_failed",
			)));
		},
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

	apply_active_ownership_warning_to_post_review_lane(
		context.project,
		context.success_state,
		&snapshot,
		&mut classification,
	);

	Ok(Some(post_review_lane_status_from_classification(
		context.project,
		context.state_store,
		&snapshot,
		classification,
	)?))
}

fn apply_active_ownership_warning_to_post_review_lane(
	project: &ServiceConfig,
	success_state: &str,
	snapshot: &PostReviewLaneSnapshot,
	classification: &mut PostReviewLaneClassification,
) {
	if snapshot.review_handoff.is_none()
		|| snapshot.issue.state.name != success_state
		|| !snapshot.issue.labels_complete
		|| snapshot.issue.has_label(&tracker::automation_active_label(project.service_id()))
	{
		return;
	}
	if classification.readback_warning.is_none() {
		classification.readback_warning = Some(String::from("active_ownership_label_missing"));
	}
}

fn post_review_lane_status_from_classification(
	project: &ServiceConfig,
	state_store: &StateStore,
	snapshot: &PostReviewLaneSnapshot,
	classification: PostReviewLaneClassification,
) -> crate::prelude::Result<OperatorPostReviewLaneStatus> {
	let loop_status =
		operator_post_review_loop_status(project, state_store, snapshot, classification.decision)?;

	Ok(OperatorPostReviewLaneStatus {
		project_id: project.service_id().to_owned(),
		issue_id: snapshot.issue.id.clone(),
		issue_identifier: snapshot.issue.identifier.clone(),
		issue_state: snapshot.issue.state.name.clone(),
		branch_name: snapshot.worktree.branch_name().to_owned(),
		worktree_path: relative_worktree_path_for_path(project, snapshot.worktree.worktree_path()),
		classification: classification.decision.as_str().to_owned(),
		reason: classification.reason,
		pr_url: classification.pr_url,
		pr_head_sha: classification.pr_head_sha,
		pr_state: classification.pr_state,
		review_decision: classification.review_decision,
		mergeable: classification.mergeable,
		check_state: classification.check_state,
		unresolved_review_threads: classification.unresolved_review_threads,
		shadowed_by_current_lane: false,
		readback_warning: classification.readback_warning,
		readback_root_cause: classification.readback_root_cause,
		loop_status,
	})
}

fn operator_post_review_loop_status(
	project: &ServiceConfig,
	state_store: &StateStore,
	snapshot: &PostReviewLaneSnapshot,
	decision: PostReviewLaneDecision,
) -> crate::prelude::Result<Option<OperatorLoopStatus>> {
	let Some(review_handoff) = snapshot.review_handoff.as_ref() else {
		return Ok(None);
	};
	let default_review_phase = match decision {
		PostReviewLaneDecision::ReadyToLand | PostReviewLaneDecision::WaitForReview => None,
		_ => Some("repair"),
	};

	operator_loop_status_for_run(
		project,
		state_store,
		&snapshot.issue.id,
		review_handoff.run_id(),
		review_handoff.attempt_number(),
		default_review_phase,
		None,
	)
	.map(Some)
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
		Some(PostReviewRuntimeState {
			state_store,
			project_id: "pubfi",
			review_level: ReviewLevel::Standard,
		}),
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
	let mut classification = classify_post_review_lane_with_external_review(
		snapshot,
		workflow,
		review_state_inspector,
		project.codex().review_level().uses_github_review(),
		Some(PostReviewRuntimeState {
			state_store,
			project_id: project.service_id(),
			review_level: project.codex().review_level(),
		}),
	)?;

	confirm_status_visible_merged_closeout(snapshot, project, &mut classification);

	Ok(classification)
}

fn classify_post_review_lane_with_external_review<I>(
	snapshot: &PostReviewLaneSnapshot,
	workflow: &WorkflowDocument,
	review_state_inspector: &I,
	github_review_enabled: bool,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
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
	if !github_review_enabled {
		let orchestration_marker = load_post_review_orchestration_marker(
			snapshot,
			&review_state,
			&mut classification,
			runtime_state,
		)?;

		if classification.decision == PostReviewLaneDecision::Block {
			return Ok(classification);
		}

		apply_non_github_review_post_review_classification(
			&mut classification,
			&review_state,
			orchestration_marker.as_ref(),
			OffsetDateTime::now_utc().unix_timestamp(),
		)?;
		apply_authority_boundary_landing_policy(snapshot, &mut classification, runtime_state)?;

		return Ok(classification);
	}

	let Some(orchestration_marker) = load_post_review_orchestration_marker(
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
	apply_authority_boundary_landing_policy(snapshot, &mut classification, runtime_state)?;

	Ok(classification)
}

fn apply_authority_boundary_landing_policy(
	snapshot: &PostReviewLaneSnapshot,
	classification: &mut PostReviewLaneClassification,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> crate::prelude::Result<()> {
	if classification.decision != PostReviewLaneDecision::ReadyToLand {
		return Ok(());
	}

	let Some(reason) = authority_boundary_landing_requirement(snapshot, runtime_state)? else {
		return Ok(());
	};

	classification.decision = if reason == "authority_boundary_requires_human_decision" {
		PostReviewLaneDecision::Block
	} else {
		PostReviewLaneDecision::NeedsReviewRepair
	};
	classification.reason = reason.to_owned();

	Ok(())
}

fn authority_boundary_landing_requirement(
	snapshot: &PostReviewLaneSnapshot,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> crate::prelude::Result<Option<&'static str>> {
	let Some(runtime_state) = runtime_state else {
		return Ok(None);
	};
	let events = runtime_state
		.state_store
		.list_private_execution_events_for_issue(runtime_state.project_id, &snapshot.issue.id)?;

	if events.iter().any(|event| event.event_type() == AUTHORITY_DECISION_REQUEST_EVENT_TYPE) {
		return Ok(Some("authority_boundary_requires_human_decision"));
	}
	if events.iter().rev().any(|event| {
		event.event_type() == AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE
			&& authority_boundary_event_requires_human_decision(event.payload())
	}) {
		return Ok(Some("authority_boundary_requires_human_decision"));
	}

	let latest_clean_review_record_id = events
		.iter()
		.rev()
		.find(|event| authority_boundary_clearance_review_checkpoint(event, snapshot))
		.map_or(0, PrivateExecutionEvent::record_id);

	for event in events.iter().rev() {
		if event.record_id() <= latest_clean_review_record_id
			|| event.event_type() != AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE
		{
			continue;
		}

		if let Some(reason) = authority_boundary_event_landing_requirement(event.payload()) {
			return Ok(Some(reason));
		}
	}

	Ok(None)
}

fn authority_boundary_clearance_review_checkpoint(
	event: &PrivateExecutionEvent,
	snapshot: &PostReviewLaneSnapshot,
) -> bool {
	if event.event_type() != "review_checkpoint"
		|| event.payload().get("status").and_then(Value::as_str) != Some("clean")
	{
		return false;
	}

	let Some(checkpoint_head) = event.payload().get("head_sha").and_then(Value::as_str) else {
		return false;
	};
	let expected_head = snapshot
		.local_head_oid
		.as_deref()
		.or_else(|| snapshot.review_handoff.as_ref().map(ReviewHandoffMarker::pr_head_oid));

	expected_head == Some(checkpoint_head)
}

fn authority_boundary_event_blocks_landing(payload: &Value) -> bool {
	payload
		.get("policy")
		.and_then(|policy| policy.get("blocks_landing"))
		.and_then(Value::as_bool)
		.or_else(|| payload.get("blocks_landing").and_then(Value::as_bool))
		.unwrap_or_else(|| {
			authority_boundary_event_policy_decision(payload)
				.is_some_and(operator_boundary_policy_blocks_landing)
		})
}

fn authority_boundary_event_requires_enhanced_evidence(payload: &Value) -> bool {
	payload
		.get("policy")
		.and_then(|policy| policy.get("requires_enhanced_evidence"))
		.and_then(Value::as_bool)
		.or_else(|| payload.get("requires_enhanced_evidence").and_then(Value::as_bool))
		.unwrap_or_else(|| {
			authority_boundary_event_policy_decision(payload)
				.is_some_and(operator_boundary_policy_requires_enhanced_evidence)
		})
}

fn authority_boundary_event_landing_requirement(payload: &Value) -> Option<&'static str> {
	if authority_boundary_event_blocks_landing(payload) {
		return Some("authority_boundary_blocks_landing");
	}
	if authority_boundary_event_requires_enhanced_evidence(payload) {
		return Some("authority_boundary_requires_enhanced_evidence");
	}

	None
}

fn authority_boundary_event_requires_human_decision(payload: &Value) -> bool {
	authority_boundary_event_policy_decision(payload)
		.is_some_and(|policy_decision| policy_decision == "requires_human_decision")
		|| payload
			.get("policy")
			.and_then(|policy| policy.get("requires_human_decision"))
			.and_then(Value::as_bool)
			.unwrap_or(false)
		|| matches!(
			payload.get("disposition").and_then(Value::as_str).or_else(|| {
				payload
					.get("final_disposition")
					.and_then(|final_disposition| final_disposition.get("disposition"))
					.and_then(Value::as_str)
			}),
			Some("requires_human" | "insufficient_evidence")
		)
}

fn authority_boundary_event_policy_decision(payload: &Value) -> Option<&str> {
	payload.get("policy_decision").and_then(Value::as_str).or_else(|| {
		payload.get("policy").and_then(|policy| policy.get("decision")).and_then(Value::as_str)
	})
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
		classification.decision = if snapshot.issue.state.name
			== workflow.frontmatter().tracker().resolved_completed_state()
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

fn merged_closeout_pending_classification(classification: &PostReviewLaneClassification) -> bool {
	classification.decision == PostReviewLaneDecision::Continue
		&& classification.reason == "pull_request_merged_closeout_pending"
		&& classification.pr_state.as_deref() == Some("MERGED")
}

fn confirm_status_visible_merged_closeout(
	snapshot: &PostReviewLaneSnapshot,
	project: &ServiceConfig,
	classification: &mut PostReviewLaneClassification,
) {
	if !merged_closeout_pending_classification(classification) {
		return;
	}

	let Some(pr_url) = classification.pr_url.as_deref() else {
		mark_merged_closeout_confirmation_conflict(classification, None, None);

		return;
	};
	let expected_head_sha = snapshot
		.review_handoff
		.as_ref()
		.map(ReviewHandoffMarker::pr_head_oid)
		.or(classification.pr_head_sha.as_deref());
	let Some(expected_head_sha) = expected_head_sha else {
		mark_merged_closeout_confirmation_conflict(classification, None, None);

		return;
	};
	let github_token = match resolve_configured_env_var(
		"github.token_env_var",
		Some(project.github().token_env_var()),
	) {
		Ok(github_token) => github_token,
		Err(error) => {
			let root_cause = classify_pull_request_readback_report(&error);

			mark_merged_closeout_confirmation_conflict(classification, None, Some(root_cause));

			return;
		},
	};
	let merge_readback = match github::inspect_pull_request_merge_readback(
		snapshot.worktree.worktree_path(),
		pr_url,
		&github_token,
		project.github().command_path(),
	) {
		Ok(merge_readback) => merge_readback,
		Err(error) => {
			let root_cause = classify_pull_request_readback_report(&error);

			mark_merged_closeout_confirmation_conflict(classification, None, Some(root_cause));

			return;
		},
	};

	if merge_readback.state == "MERGED"
		&& merge_readback.head_ref_oid.as_deref() == Some(expected_head_sha)
	{
		return;
	}

	mark_merged_closeout_confirmation_conflict(
		classification,
		Some(merge_readback),
		Some(PullRequestReadbackRootCause::LineageValidationFailed),
	);
}

fn mark_merged_closeout_confirmation_conflict(
	classification: &mut PostReviewLaneClassification,
	merge_readback: Option<PullRequestMergeViewResponse>,
	root_cause: Option<PullRequestReadbackRootCause>,
) {
	classification.decision = PostReviewLaneDecision::Block;
	classification.reason = String::from("pull_request_merge_state_conflict");
	classification.readback_warning = Some(String::from("pull_request_merge_state_conflict"));
	classification.readback_root_cause =
		root_cause.map(|root_cause| root_cause.as_str().to_owned());

	if let Some(merge_readback) = merge_readback {
		classification.pr_state = Some(merge_readback.state);
		classification.pr_head_sha =
			merge_readback.head_ref_oid.or_else(|| classification.pr_head_sha.clone());
	}
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorktreeTrackedChangeState {
	Clean,
	TrackedChanges,
	Unknown,
}

impl WorktreeTrackedChangeState {
	fn has_tracked_changes(self) -> bool {
		self == Self::TrackedChanges
	}

	fn is_unknown(self) -> bool {
		self == Self::Unknown
	}
}

fn worktree_tracked_change_state(worktree_path: &Path) -> WorktreeTrackedChangeState {
	match worktree_path.try_exists() {
		Ok(false) => WorktreeTrackedChangeState::Clean,
		Ok(true) => match worktree_path.join(".git").try_exists() {
			Ok(false) => {
				match state::retained_path_contains_only_decodex_runtime_artifacts(worktree_path) {
					Ok(true) => WorktreeTrackedChangeState::Clean,
					Ok(false) => WorktreeTrackedChangeState::TrackedChanges,
					Err(_) => WorktreeTrackedChangeState::Unknown,
				}
			},
			Ok(true) => {
				let Ok(output) = Command::new("git")
					.arg("-C")
					.arg(worktree_path)
					.args(["status", "--porcelain"])
					.output()
				else {
					return WorktreeTrackedChangeState::Unknown;
				};

				if !output.status.success() {
					return WorktreeTrackedChangeState::Unknown;
				}

				let has_blocking_status = String::from_utf8_lossy(&output.stdout)
					.lines()
					.filter(|line| !line.trim_end().is_empty())
					.any(|line| !state::is_untracked_decodex_runtime_artifact_status_line(line));

				if has_blocking_status {
					WorktreeTrackedChangeState::TrackedChanges
				} else {
					WorktreeTrackedChangeState::Clean
				}
			},
			Err(_) => WorktreeTrackedChangeState::Unknown,
		},
		Err(_) => WorktreeTrackedChangeState::Unknown,
	}
}

fn worktree_has_tracked_changes(worktree_path: &Path) -> bool {
	worktree_tracked_change_state(worktree_path).has_tracked_changes()
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

fn apply_non_github_review_post_review_classification(
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
					"non_github_review_merge_visibility_timeout",
				);
			} else {
				classification.reason = String::from("non_github_review_waiting_for_merge");
			}

			return Ok(());
		}
		if phase == ReviewOrchestrationPhase::RepairRequired {
			classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
			classification.reason = if review_state_landing_requires_agent_fallback(review_state) {
				String::from("retained_landing_agent_fallback_required")
			} else {
				String::from("non_github_review_repair_required")
			};

			return Ok(());
		}
	}

	if review_state_clean_path_landing_gates_satisfied(review_state) {
		classification.decision = PostReviewLaneDecision::ReadyToLand;
		classification.reason = String::from("non_github_review_ready_to_land");
	} else if review_state_landing_requires_agent_fallback(review_state) {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from("retained_landing_agent_fallback_required");
	} else {
		classification.reason = String::from("non_github_review_waiting_gates");
	}

	Ok(())
}

fn load_post_review_orchestration_marker(
	snapshot: &PostReviewLaneSnapshot,
	review_state: &PullRequestReviewState,
	classification: &mut PostReviewLaneClassification,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> crate::prelude::Result<Option<ReviewOrchestrationMarker>> {
	let review_handoff = snapshot
		.review_handoff
		.as_ref()
		.expect("review handoff should exist before orchestration classification");
	let orchestration_marker = if let Some(runtime_state) = runtime_state {
		runtime_state.state_store.review_orchestration_marker(
			runtime_state.project_id,
			&snapshot.issue.id,
			review_handoff,
		)?
	} else {
		None
	};
	let Some(orchestration_marker) = orchestration_marker else {
		if clean_current_head_review_repair_writeback_pending(
			snapshot,
			review_state,
			runtime_state,
		)? {
			classification.reason =
				String::from("review_repair_writeback_missing_lifecycle_marker");
			classification.readback_warning =
				Some(String::from("review_repair_writeback_missing_lifecycle_marker"));

			return Ok(None);
		}

		classification.reason = String::from("external_review_request_pending");

		return Ok(None);
	};

	if let Some(reason) =
		validate_review_orchestration_marker(snapshot, review_state, &orchestration_marker)
	{
		if reason == "review_orchestration_head_mismatch"
			&& clean_current_head_review_repair_writeback_pending(
				snapshot,
				review_state,
				runtime_state,
			)? {
			classification.reason = String::from("review_repair_writeback_stale_lifecycle_marker");
			classification.readback_warning =
				Some(String::from("review_repair_writeback_stale_lifecycle_marker"));

			return Ok(None);
		}

		*classification = blocked_post_review_lane_from_state(review_state, reason);

		return Ok(None);
	}

	Ok(Some(orchestration_marker))
}

fn clean_current_head_review_repair_writeback_pending(
	snapshot: &PostReviewLaneSnapshot,
	review_state: &PullRequestReviewState,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> crate::prelude::Result<bool> {
	let Some(runtime_state) = runtime_state else {
		return Ok(false);
	};
	let Some(local_head_oid) = snapshot.local_head_oid.as_deref() else {
		return Ok(false);
	};

	if review_state.head_ref_oid != local_head_oid
		|| review_state.head_ref_name != snapshot.worktree.branch_name()
	{
		return Ok(false);
	}

	let events = runtime_state
		.state_store
		.list_private_execution_events_for_issue(runtime_state.project_id, &snapshot.issue.id)?;

	for terminal_event in events.iter().rev() {
		if !review_repair_terminal_finalize_event_matches_snapshot(terminal_event, snapshot) {
			continue;
		}

		let Some(intent_event) = events.iter().rev().find(|event| {
			event.run_id() == terminal_event.run_id()
				&& event.attempt_number() == terminal_event.attempt_number()
				&& review_repair_completion_intent_matches_current_head(
					event,
					snapshot,
					review_state,
					local_head_oid,
				)
		}) else {
			continue;
		};
		let Some(checkpoint) = runtime_state.state_store.review_checkpoint_artifact(
			ReviewCheckpointArtifactLookup {
				project_id: runtime_state.project_id,
				issue_id: &snapshot.issue.id,
				phase: "repair",
				review_level: runtime_state.review_level.as_str(),
				head_sha: local_head_oid,
			},
		)?
		else {
			continue;
		};

		if checkpoint.status() == "clean"
			&& checkpoint.head_sha() == local_head_oid
			&& checkpoint.run_id() == intent_event.run_id()
			&& checkpoint.attempt_number() == intent_event.attempt_number()
		{
			return Ok(true);
		}
	}

	Ok(false)
}

fn review_repair_terminal_finalize_event_matches_snapshot(
	event: &PrivateExecutionEvent,
	snapshot: &PostReviewLaneSnapshot,
) -> bool {
	let payload = event.payload();

	event.event_type() == "terminal_finalize"
		&& payload.get("path").and_then(Value::as_str) == Some("review_repair")
		&& payload.get("mode").and_then(Value::as_str) == Some("repair")
		&& payload.get("branch").and_then(Value::as_str) == Some(snapshot.worktree.branch_name())
		&& payload.get("worktree_path").and_then(Value::as_str)
			== Some(snapshot.worktree.worktree_path().display().to_string().as_str())
}

fn review_repair_completion_intent_matches_current_head(
	event: &PrivateExecutionEvent,
	snapshot: &PostReviewLaneSnapshot,
	review_state: &PullRequestReviewState,
	local_head_oid: &str,
) -> bool {
	let payload = event.payload();

	event.event_type() == "review_completion_intent"
		&& payload.get("path").and_then(Value::as_str) == Some("review_repair")
		&& payload.get("mode").and_then(Value::as_str) == Some("repair")
		&& payload.get("branch").and_then(Value::as_str) == Some(snapshot.worktree.branch_name())
		&& payload.get("worktree_path").and_then(Value::as_str)
			== Some(snapshot.worktree.worktree_path().display().to_string().as_str())
		&& payload.get("pr_url").and_then(Value::as_str) == Some(review_state.url.as_str())
		&& payload.get("pr_head_ref").and_then(Value::as_str)
			== Some(review_state.head_ref_name.as_str())
		&& payload.get("pr_head_oid").and_then(Value::as_str) == Some(local_head_oid)
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
				return Ok(PostReviewLaneStateLoad::Classification(
					blocked_post_review_lane_from_handoff(review_handoff, reason),
				));
			},
		};
		let review_state = match review_state_inspector.inspect_review_state_readback(
			snapshot.worktree.worktree_path(),
			review_handoff.pr_url(),
		) {
			Ok(review_state) => review_state,
			Err(error) => {
				return Ok(PostReviewLaneStateLoad::Classification(
					readback_degraded_post_review_lane_from_handoff(
						review_handoff,
						error.root_cause(),
					),
				));
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

	worktree_head_descends_from_review_handoff(worktree_path, merge_commit_oid, local_head_oid)
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
		pr_head_sha: Some(review_state.head_ref_oid.clone()),
		pr_state: Some(review_state.state.clone()),
		review_decision: review_state.review_decision.clone(),
		mergeable: Some(review_state.mergeable.clone()),
		check_state: review_state.status_check_rollup_state.clone(),
		unresolved_review_threads: Some(review_state.unresolved_review_threads),
		readback_warning: None,
		readback_root_cause: None,
	}
}

fn blocked_post_review_lane_from_state(
	review_state: &PullRequestReviewState,
	reason: &str,
) -> PostReviewLaneClassification {
	let mut classification = initial_post_review_lane_classification(review_state);

	classification.decision = PostReviewLaneDecision::Block;
	classification.reason = reason.to_owned();
	classification.readback_root_cause = post_review_readback_root_cause_for_reason(reason)
		.map(|root_cause| root_cause.as_str().to_owned());

	classification
}

fn blocked_post_review_lane(reason: &str) -> PostReviewLaneClassification {
	PostReviewLaneClassification {
		decision: PostReviewLaneDecision::Block,
		reason: reason.to_owned(),
		pr_url: None,
		pr_head_sha: None,
		pr_state: None,
		review_decision: None,
		mergeable: None,
		check_state: None,
		unresolved_review_threads: None,
		readback_warning: None,
		readback_root_cause: post_review_readback_root_cause_for_reason(reason)
			.map(|root_cause| root_cause.as_str().to_owned()),
	}
}

fn blocked_post_review_lane_from_handoff(
	review_handoff: &ReviewHandoffMarker,
	reason: &str,
) -> PostReviewLaneClassification {
	let mut classification = blocked_post_review_lane(reason);

	classification.pr_url = Some(review_handoff.pr_url().to_owned());
	classification.pr_head_sha = Some(review_handoff.pr_head_oid().to_owned());

	classification
}

fn readback_degraded_post_review_lane_from_handoff(
	review_handoff: &ReviewHandoffMarker,
	root_cause: PullRequestReadbackRootCause,
) -> PostReviewLaneClassification {
	PostReviewReadbackDegradation::pull_request_state_from_handoff(review_handoff, root_cause)
		.wait_for_review_classification(None)
}

fn blocked_post_review_lane_status(
	project: &ServiceConfig,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	reason: &str,
) -> OperatorPostReviewLaneStatus {
	OperatorPostReviewLaneStatus {
		project_id: project.service_id().to_owned(),
		issue_id: issue.id.clone(),
		issue_identifier: issue.identifier.clone(),
		issue_state: issue.state.name.clone(),
		branch_name: worktree.branch_name().to_owned(),
		worktree_path: relative_worktree_path_for_path(project, worktree.worktree_path()),
		classification: String::from("blocked"),
		reason: String::from(reason),
		pr_url: None,
		pr_head_sha: None,
		pr_state: None,
		review_decision: None,
		mergeable: None,
		check_state: None,
		unresolved_review_threads: None,
		shadowed_by_current_lane: false,
		readback_warning: None,
		readback_root_cause: post_review_readback_root_cause_for_reason(reason)
			.map(|root_cause| root_cause.as_str().to_owned()),
		loop_status: None,
	}
}

fn post_review_readback_root_cause_for_reason(
	reason: &str,
) -> Option<PullRequestReadbackRootCause> {
	match reason {
		"pull_request_repository_parse_failed" => {
			Some(PullRequestReadbackRootCause::PullRequestShapeReadFailed)
		},
		"pull_request_branch_mismatch"
		| "pull_request_head_mismatch"
		| "pull_request_head_repository_name_mismatch"
		| "pull_request_head_repository_owner_mismatch"
		| "pull_request_merge_commit_lineage_check_failed"
		| "review_handoff_lineage_check_failed"
		| "review_handoff_lineage_mismatch"
		| "review_orchestration_branch_mismatch"
		| "review_orchestration_head_mismatch"
		| "review_orchestration_pr_mismatch" => {
			Some(PullRequestReadbackRootCause::LineageValidationFailed)
		},
		_ => None,
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
		Some("ERROR" | "FAILURE") => ExternalReviewRequestCiGate::RepairRequired,
		Some(_) => ExternalReviewRequestCiGate::WaitForGreenChecks,
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

fn review_state_clean_path_landing_gates_satisfied(review_state: &PullRequestReviewState) -> bool {
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
	recover_runtime_state_from_tracker_and_worktrees_with_skip_cache(
		tracker,
		project,
		workflow,
		state_store,
		None,
	)
}

fn recover_runtime_state_from_tracker_and_worktrees_with_skip_cache<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	mut recoverable_worktree_skip_cache: Option<&mut RecoverableWorktreeSkipCache>,
) -> crate::prelude::Result<RecoveredRuntimeState>
where
	T: IssueTracker,
{
	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());
	let active_issue_ids = active_shared_issue_ids(project, state_store)?;
	let mut issue_ids = Vec::new();

	for mapping in state_store.list_worktrees(project.service_id())? {
		if worktree_mapping_is_stale_terminal_local_residue(
			project,
			state_store,
			&mapping,
			&active_issue_ids,
		)? {
			continue;
		}

		issue_ids.push(mapping.issue_id().to_owned());
	}
	for lease in state_store.list_active_shared_leases(project.service_id())? {
		if !issue_ids.iter().any(|issue_id| issue_id == lease.issue_id()) {
			issue_ids.push(lease.issue_id().to_owned());
		}
	}

	let mut issues = if issue_ids.is_empty() && recoverable_worktree_skip_cache.is_some() {
		Vec::new()
	} else {
		refresh_recoverable_runtime_issues(tracker, &issue_ids)?
	};
	let mut known_identifiers =
		issues.iter().map(|issue| issue.identifier.to_ascii_uppercase()).collect::<BTreeSet<_>>();

	for issue_identifier in recoverable_worktree_identifiers(project.worktree_root())? {
		append_recoverable_tracker_issue(
			tracker,
			project,
			&issue_identifier,
			&mut known_identifiers,
			&mut issues,
			recoverable_worktree_skip_cache.as_deref_mut(),
		)?;
	}

	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let mut recoverable_issues = Vec::new();

	for issue in issues {
		if let Some(recoverable_issue) = recover_issue_runtime_state(
			tracker,
			project,
			workflow,
			state_store,
			&worktree_manager,
			issue,
			now_unix_epoch,
		)? {
			recoverable_issues.push(recoverable_issue);
		}
	}

	recoverable_issues.sort_by(compare_issue_candidates);

	Ok(RecoveredRuntimeState { recoverable_issues })
}

fn refresh_recoverable_runtime_issues<T>(
	tracker: &T,
	issue_ids: &[String],
) -> crate::prelude::Result<Vec<TrackerIssue>>
where
	T: IssueTracker,
{
	match tracker.refresh_issues(issue_ids) {
		Ok(issues) => Ok(issues),
		Err(error)
			if issue_ids.iter().any(|issue_id| {
				tracker::issue_lookup_missing_error_for_candidate(&error, issue_id)
			}) =>
		{
			let mut issues = Vec::new();

			for issue_id in issue_ids {
				match tracker.refresh_issues(slice::from_ref(issue_id)) {
					Ok(mut refreshed) => issues.append(&mut refreshed),
					Err(error)
						if tracker::issue_lookup_missing_error_for_candidate(&error, issue_id) => {},
					Err(error) => return Err(error),
				}
			}

			Ok(issues)
		},
		Err(error) => Err(error),
	}
}

fn recover_issue_runtime_state<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
	issue: TrackerIssue,
	now_unix_epoch: i64,
) -> crate::prelude::Result<Option<TrackerIssue>>
where
	T: IssueTracker,
{
	let planned_worktree = worktree_manager.plan_for_issue(&issue.identifier);
	let existing_worktree_mapping = state_store.worktree_for_issue(&issue.id)?;
	let existing_worktree = existing_recoverable_worktree_spec(
		project.service_id(),
		&issue,
		existing_worktree_mapping.as_ref(),
	)?;
	let worktree = existing_worktree.unwrap_or(planned_worktree);

	if !worktree.path.exists() {
		return Ok(None);
	}

	state_store.canonicalize_issue_identity(&issue.identifier, &issue.id)?;

	let activity_marker = state::read_run_activity_marker_snapshot(&worktree.path)?;
	let recovered_service_ownership =
		issue_has_recovered_service_ownership(tracker, &issue, project.service_id())?;

	if existing_worktree_mapping.is_none() && recovered_service_ownership {
		upsert_recovered_worktree_mapping(
			project,
			state_store,
			&issue,
			&worktree,
			activity_marker.as_ref(),
		)?;
	}
	if issue.state.name == workflow.frontmatter().tracker().success_state()
		&& recovered_service_ownership
		&& let Some(marker) = activity_marker.as_ref()
		&& worktree_activity_marker_is_fresh(marker, now_unix_epoch)
	{
		upsert_recovered_worktree_mapping(
			project,
			state_store,
			&issue,
			&worktree,
			activity_marker.as_ref(),
		)?;
		record_recovered_activity_lease(project, state_store, &issue, marker)?;

		return Ok(None);
	}
	if issue_passes_closeout_dispatch_policy(tracker, &issue, project, workflow, state_store)? {
		upsert_recovered_worktree_mapping(
			project,
			state_store,
			&issue,
			&worktree,
			activity_marker.as_ref(),
		)?;

		match activity_marker.as_ref() {
			Some(marker) if worktree_activity_marker_is_fresh(marker, now_unix_epoch) => {
				record_recovered_activity_lease(project, state_store, &issue, marker)?;

				return Ok(None);
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
		upsert_recovered_worktree_mapping(
			project,
			state_store,
			&issue,
			&worktree,
			activity_marker.as_ref(),
		)?;

		match activity_marker.as_ref() {
			Some(marker) if worktree_activity_marker_is_fresh(marker, now_unix_epoch) => {
				record_recovered_activity_lease(project, state_store, &issue, marker)?;

				return Ok(None);
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
				clear_recovered_issue_lease(project.service_id(), &issue.id, None, state_store)?;
			},
		}

		return Ok(Some(issue));
	}

	Ok(None)
}

fn existing_recoverable_worktree_spec(
	project_id: &str,
	issue: &TrackerIssue,
	mapping: Option<&WorktreeMapping>,
) -> crate::prelude::Result<Option<WorktreeSpec>> {
	let Some(mapping) = mapping else {
		return Ok(None);
	};
	if mapping.project_id() != project_id || !mapping.worktree_path().try_exists()? {
		return Ok(None);
	}

	Ok(Some(WorktreeSpec {
		branch_name: mapping.branch_name().to_owned(),
		issue_identifier: issue.identifier.clone(),
		path: mapping.worktree_path().to_path_buf(),
		reused_existing: true,
	}))
}

fn upsert_recovered_worktree_mapping(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
	worktree: &WorktreeSpec,
	activity_marker: Option<&RunActivityMarker>,
) -> crate::prelude::Result<()> {
	state_store.upsert_recovered_worktree(
		project.service_id(),
		&issue.id,
		&worktree.branch_name,
		&worktree.path.display().to_string(),
		recovered_worktree_observed_at_unix(activity_marker),
	)
}

fn recovered_worktree_observed_at_unix(activity_marker: Option<&RunActivityMarker>) -> Option<i64> {
	activity_marker.and_then(|marker| {
		[
			marker.last_activity_unix_epoch(),
			marker.last_protocol_activity_unix_epoch(),
			marker.last_progress_unix_epoch(),
		]
		.into_iter()
		.flatten()
		.max()
	})
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
	mut recoverable_worktree_skip_cache: Option<&mut RecoverableWorktreeSkipCache>,
) -> crate::prelude::Result<()>
where
	T: IssueTracker,
{
	let canonical_identifier = issue_identifier.to_ascii_uppercase();

	if known_identifiers.contains(&canonical_identifier) {
		return Ok(());
	}

	let now = Instant::now();

	if let Some(cache) = recoverable_worktree_skip_cache.as_deref_mut()
		&& cache.is_suppressed(&canonical_identifier, now)
	{
		tracing::debug!(
			issue = canonical_identifier,
			"Skipped retained worktree tracker lookup because a recent recovery probe already found no service ownership."
		);

		return Ok(());
	}

	let issue = match tracker.get_issue_by_identifier(issue_identifier) {
		Ok(issue) => issue,
		Err(error)
			if tracker::issue_lookup_missing_error_for_candidate(&error, issue_identifier) =>
		{
			None
		},
		Err(error) => return Err(error),
	};
	let Some(issue) = issue else {
		if let Some(cache) = recoverable_worktree_skip_cache {
			cache.remember(&canonical_identifier, now);
		}

		return Ok(());
	};

	if !issue_has_recovered_service_ownership(tracker, &issue, project.service_id())? {
		tracing::warn!(
			issue = issue.identifier,
			active_label = tracker::automation_active_label(project.service_id()),
			labels_complete = issue.labels_complete,
			"Skipping retained worktree recovery because the tracker issue is not explicitly owned by this service."
		);

		if let Some(cache) = recoverable_worktree_skip_cache {
			cache.remember(&canonical_identifier, now);
		}

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
			.is_some_and(|idle_for| idle_for < run_activity_idle_timeout(Some(marker)))
}

fn run_activity_idle_timeout(marker: Option<&RunActivityMarker>) -> Duration {
	agent::protocol_activity_idle_timeout(
		marker.and_then(RunActivityMarker::protocol_activity),
		RUN_LEASE_IDLE_TIMEOUT,
	)
}

fn marker_process_is_alive(marker: &RunActivityMarker) -> bool {
	marker_process_liveness(marker).alive
}

fn marker_process_liveness_for_marker(marker: &RunActivityMarker) -> Option<MarkerProcessLiveness> {
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
		return MarkerProcessLiveness { alive: false, reason: "process_start_identity_mismatch" };
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
		0 => !process_is_zombie_or_uninspectable_after_signalable_probe(process_id),
		-1 => {
			matches!(std::io::Error::last_os_error().raw_os_error(), Some(libc::EPERM))
				&& !process_is_zombie(process_id)
		},
		_ => false,
	}
}

fn process_is_zombie_or_uninspectable_after_signalable_probe(process_id: pid_t) -> bool {
	process_is_zombie_or_uninspectable(process_id)
}

#[cfg(not(target_os = "macos"))]
fn process_is_zombie_or_uninspectable(process_id: pid_t) -> bool {
	process_is_zombie(process_id)
}

#[cfg(target_os = "linux")]
fn process_is_zombie(process_id: pid_t) -> bool {
	let Ok(stat) = fs::read_to_string(format!("/proc/{process_id}/stat")) else {
		return false;
	};
	let Some(comm_end) = stat.rfind(')') else {
		return false;
	};
	let Some(after_comm) = stat.get(comm_end + 2..) else {
		return false;
	};

	after_comm.split_whitespace().next() == Some("Z")
}

#[cfg(target_os = "macos")]
fn process_is_zombie_or_uninspectable(process_id: pid_t) -> bool {
	match macos_process_bsd_status(process_id) {
		Some(status) => status == SZOMB,
		None => true,
	}
}

#[cfg(target_os = "macos")]
fn process_is_zombie(process_id: pid_t) -> bool {
	macos_process_bsd_status(process_id) == Some(SZOMB)
}

#[cfg(target_os = "macos")]
fn macos_process_bsd_status(process_id: pid_t) -> Option<u32> {
	if process_id <= 0 {
		return None;
	}

	let mut info = MaybeUninit::<proc_bsdinfo>::zeroed();
	let Ok(info_size) = i32::try_from(mem::size_of::<proc_bsdinfo>()) else {
		return None;
	};
	let read_size = unsafe {
		libc::proc_pidinfo(
			process_id,
			PROC_PIDTBSDINFO,
			0,
			info.as_mut_ptr().cast::<c_void>(),
			info_size,
		)
	};

	if read_size != info_size {
		return None;
	}

	let info = unsafe { info.assume_init() };

	Some(info.pbi_status)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_is_zombie(_process_id: pid_t) -> bool {
	false
}

fn hydrate_status_snapshot_state(
	_project: &ServiceConfig,
	_state_store: &StateStore,
	_recovered_state: RecoveredRuntimeState,
) -> crate::prelude::Result<()> {
	Ok(())
}

fn append_primary_account_if_missing(
	accounts: &mut Vec<CodexAccountActivitySummary>,
	account: Option<&CodexAccountActivitySummary>,
) {
	if accounts.is_empty()
		&& let Some(account) = account
	{
		accounts.push(account.clone());
	}
}

fn operator_run_status(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	project_display_name: &str,
	run: ProjectRunStatus,
	now_unix_epoch: i64,
) -> crate::prelude::Result<OperatorRunStatus> {
	let marker = load_operator_run_marker(&run)?;
	let timing = operator_run_timing(&run, marker.as_ref(), now_unix_epoch);
	let app_server_state = operator_run_app_server_state(&run, marker.as_ref());
	let protocol_summary = operator_run_protocol_summary(&run, marker.as_ref());
	let terminal_finalize_projection =
		operator_run_terminal_finalize_projection(loop_evidence, &run);
	let lifecycle = operator_run_lifecycle_projection(
		&run,
		marker.as_ref(),
		terminal_finalize_projection,
		&timing,
		&app_server_state,
		&protocol_summary,
		now_unix_epoch,
	);
	let child_agent_activity = operator_run_child_agent_activity(
		marker.as_ref(),
		run.child_agent_activity(),
		now_unix_epoch,
	);
	let protocol_activity = operator_run_protocol_activity(
		marker.as_ref(),
		run.protocol_activity(),
		&app_server_state,
		child_agent_activity.as_ref(),
		timing.protocol_idle_for_seconds,
		matches!(lifecycle.status.as_str(), "starting" | "running"),
	);
	let wait_reason = operator_run_wait_reason(
		&lifecycle.phase,
		lifecycle.wait_reason.clone(),
		protocol_activity.as_ref(),
	);
	let private_events =
		loop_evidence.private_events(run.issue_id(), run.run_id(), run.attempt_number());
	let progress_diagnostic = operator_run_progress_diagnostic(
		&lifecycle.phase,
		&timing,
		protocol_activity.as_ref(),
		private_events,
		now_unix_epoch,
		run_activity_idle_timeout(marker.as_ref()),
	);
	let (account, accounts) = operator_run_accounts(marker.as_ref());
	let branch_name = run.branch_name().map(str::to_owned);
	let worktree_path = operator_run_relative_worktree_path(project, &run);
	let issue_identifier = operator_run_issue_identifier_from_fields(
		run.run_id(),
		branch_name.as_deref(),
		worktree_path.as_deref(),
	);
	let private_evidence =
		operator_run_private_evidence(project, &run, issue_identifier.as_deref());
	let continuation_recovery = operator_run_continuation_recovery_status(loop_evidence, &run);
	let active_goal_phase = operator_run_active_goal_phase(private_events);
	let public_progress_phase = operator_run_public_progress_phase(private_events);
	let phase_acceptance = operator_run_phase_acceptance_status(private_events);
	let loop_status = operator_run_loop_status(
		project,
		loop_evidence,
		&run,
		&lifecycle.status,
		&lifecycle.phase,
		&lifecycle.current_operation,
	)?;

	Ok(hydrate_operator_run_derived_status(operator_run_status_from_parts(
		project,
		project_display_name,
		&run,
		lifecycle,
		wait_reason,
		app_server_state,
		timing,
		protocol_summary,
		child_agent_activity,
		protocol_activity,
		progress_diagnostic,
		account,
		accounts,
		branch_name,
		worktree_path,
		issue_identifier,
		private_evidence,
		continuation_recovery,
		phase_acceptance,
		active_goal_phase,
		public_progress_phase,
		loop_status,
	)))
}

#[allow(clippy::too_many_arguments)]
fn operator_run_status_from_parts(
	project: &ServiceConfig,
	project_display_name: &str,
	run: &ProjectRunStatus,
	lifecycle: OperatorRunLifecycleProjection,
	wait_reason: Option<String>,
	app_server_state: OperatorRunAppServerState,
	timing: OperatorRunTiming,
	protocol_summary: OperatorRunProtocolSummary,
	child_agent_activity: Option<ChildAgentActivitySummary>,
	protocol_activity: Option<ProtocolActivitySummary>,
	progress_diagnostic: Option<String>,
	account: Option<CodexAccountActivitySummary>,
	accounts: Vec<CodexAccountActivitySummary>,
	branch_name: Option<String>,
	worktree_path: Option<String>,
	issue_identifier: Option<String>,
	private_evidence: AgentPrivateEvidenceRef,
	continuation_recovery: Option<OperatorContinuationRecoveryStatus>,
	phase_acceptance: Option<OperatorPhaseAcceptanceStatus>,
	active_goal_phase: Option<String>,
	public_progress_phase: Option<String>,
	loop_status: OperatorLoopStatus,
) -> OperatorRunStatus {
	let run_phase = lifecycle.phase.clone();

	OperatorRunStatus {
		project_id: project.service_id().to_owned(),
		project_display_name: project_display_name.to_owned(),
		run_id: run.run_id().to_owned(),
		issue_id: run.issue_id().to_owned(),
		issue_identifier,
		title: None,
		author: None,
		issue_state: None,
		active_label_present: None,
		needs_attention_label_present: None,
		attempt_number: run.attempt_number(),
		status: lifecycle.status,
		attempt_status: run.status().to_owned(),
		status_projection_reason: lifecycle.status_projection_reason,
		ownership_state: String::new(),
		liveness_state: String::new(),
		policy_state: String::new(),
		terminalization_state: String::new(),
		lane_control_next_action: String::new(),
		lane_control_conditions: Vec::new(),
		phase: lifecycle.phase,
		run_phase,
		wait_reason,
		current_operation: lifecycle.current_operation,
		active_goal_phase,
		public_progress_phase,
		control_capability: operator_run_control_capability(run, &app_server_state),
		thread_id: app_server_state.thread_id,
		turn_id: app_server_state.turn_id,
		thread_status: app_server_state.thread_status,
		thread_active_flags: app_server_state.thread_active_flags,
		interactive_requested: app_server_state.interactive_requested,
		continuation_pending: app_server_state.continuation_pending,
		continuation_recovery,
		phase_acceptance,
		run_lease: lifecycle.run_lease,
		queue_lease_state: operator_run_queue_lease_state(lifecycle.run_lease),
		execution_liveness: lifecycle.execution_liveness,
		has_fresh_execution: false,
		counts_as_running: false,
		needs_attention: false,
		updated_at: run.updated_at().to_owned(),
		last_run_activity_at: format_optional_unix_timestamp(timing.last_run_activity_unix_epoch),
		last_protocol_activity_at: format_optional_unix_timestamp(
			timing.last_protocol_activity_unix_epoch,
		),
		last_progress_at: format_optional_unix_timestamp(timing.last_progress_unix_epoch),
		idle_for_seconds: timing.idle_for_seconds,
		protocol_idle_for_seconds: timing.protocol_idle_for_seconds,
		suspected_stall: lifecycle.suspected_stall,
		progress_diagnostic,
		last_event_type: protocol_summary.last_event_type,
		last_event_at: protocol_summary.last_event_at,
		event_count: protocol_summary.event_count,
		private_evidence,
		loop_status: Some(loop_status),
		process_id: timing.process_id,
		process_alive: timing.process_alive,
		process_liveness_reason: timing.process_liveness_reason,
		retry_kind: lifecycle.retry_kind,
		next_retry_at: format_optional_unix_timestamp(lifecycle.retry_ready_at_unix_epoch),
		effective_model: app_server_state.effective_model,
		effective_model_provider: app_server_state.effective_model_provider,
		effective_cwd: app_server_state.effective_cwd,
		effective_approval_policy: app_server_state.effective_approval_policy,
		effective_approvals_reviewer: app_server_state.effective_approvals_reviewer,
		effective_sandbox_mode: app_server_state.effective_sandbox_mode,
		child_agent_activity,
		protocol_activity,
		lifecycle_source: run.recovery_source().to_owned(),
		lifecycle_evidence: run.recovery_evidence().to_vec(),
		lifecycle_gaps: run.recovery_gaps().to_vec(),
		lifecycle_metrics: OperatorLaneLifecycleMetrics::default(),
		account,
		accounts,
		branch_name,
		worktree_path,
	}
}

fn operator_run_active_goal_phase(events: &[PrivateExecutionEvent]) -> Option<String> {
	for event in events.iter().rev() {
		if matches!(event.event_type(), "phase_goal_completed" | "phase_goal_transition") {
			return None;
		}
		if !matches!(event.event_type(), "phase_goal_set" | "phase_goal_status") {
			continue;
		}

		let payload = event.payload();
		let nested = payload.get("payload").unwrap_or(payload);
		let status = nested.get("status").or_else(|| payload.get("status")).and_then(Value::as_str);

		if status.is_some_and(|value| matches!(value, "complete" | "completed" | "blocked")) {
			return None;
		}

		return nested
			.get("phase")
			.or_else(|| payload.get("phase"))
			.and_then(Value::as_str)
			.map(str::to_owned);
	}

	None
}

fn operator_run_public_progress_phase(events: &[PrivateExecutionEvent]) -> Option<String> {
	events.iter().rev().find_map(|event| {
		(event.event_type() == "progress_checkpoint")
			.then_some(event.payload())
			.and_then(|payload| payload.get("phase"))
			.and_then(Value::as_str)
			.map(str::to_owned)
	})
}

fn operator_run_phase_acceptance_status(
	events: &[PrivateExecutionEvent],
) -> Option<OperatorPhaseAcceptanceStatus> {
	let event = events
		.iter()
		.rev()
		.find(|event| event.event_type() == PHASE_ACCEPTANCE_CHECK_EVENT_TYPE)?;
	let payload = event.payload();
	let phase = payload.get("phase")?.as_str()?.to_owned();
	let decision = payload.get("decision")?.as_str()?.to_owned();
	let reason_code = payload.get("reason_code")?.as_str()?.to_owned();
	let objective_covered = payload
		.get("objective_coverage")
		.and_then(|objective| objective.get("covered"))
		.and_then(Value::as_bool)
		.unwrap_or(false);
	let effective_delta_present = payload
		.get("effective_delta")
		.and_then(|delta| delta.get("present"))
		.and_then(Value::as_bool)
		.unwrap_or(false);
	let changed_surfaces = payload
		.get("effective_delta")
		.and_then(|delta| delta.get("changed_surfaces"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(str::to_owned)
		.collect();
	let non_goal_passed = payload
		.get("non_goal_check")
		.and_then(|check| check.get("passed"))
		.and_then(Value::as_bool)
		.unwrap_or(false);
	let validation_passed = payload
		.get("validation_evidence")
		.and_then(|evidence| evidence.get("repo_gate_passed"))
		.and_then(Value::as_bool)
		.unwrap_or(false);

	Some(OperatorPhaseAcceptanceStatus {
		phase,
		decision,
		reason_code,
		objective_covered,
		effective_delta_present,
		changed_surfaces,
		non_goal_passed,
		validation_passed,
		recorded_at: event.recorded_at().to_owned(),
		run_id: event.run_id().to_owned(),
		attempt_number: event.attempt_number(),
		next_action: payload
			.get("next_action")
			.and_then(Value::as_str)
			.unwrap_or("inspect_phase_acceptance_check")
			.to_owned(),
	})
}

fn hydrate_operator_run_derived_status(mut status: OperatorRunStatus) -> OperatorRunStatus {
	status.has_fresh_execution = operator_run_has_fresh_execution(&status);
	status.needs_attention = operator_run_needs_attention(&status);

	let lane_control_state = operator_lane_control_state(&status);

	status.ownership_state = lane_control_state.ownership_state;
	status.liveness_state = lane_control_state.liveness_state;
	status.policy_state = lane_control_state.policy_state;
	status.terminalization_state = lane_control_state.terminalization_state;
	status.lane_control_next_action = lane_control_state.next_action;
	status.lane_control_conditions = lane_control_state.conditions;
	status.needs_attention = operator_run_counts_as_attention(&status);
	status.counts_as_running = operator_run_counts_as_running(&status);

	status
}

fn operator_lane_control_state(run: &OperatorRunStatus) -> OperatorLaneControlProjection {
	let liveness_state = operator_run_liveness_state(run);
	let policy_state = operator_run_policy_state(run);
	let terminalization_state = operator_run_terminalization_state(run, &liveness_state);
	let ownership_state =
		operator_run_ownership_state(run, &liveness_state, &policy_state, &terminalization_state);
	let next_action = operator_run_lane_control_next_action(
		run,
		&ownership_state,
		&liveness_state,
		&policy_state,
		&terminalization_state,
	);
	let mut conditions = operator_run_lane_control_conditions(run, &liveness_state, &policy_state);

	if ownership_state == "leased_run" && !run.run_lease {
		conditions.push(String::from("invalid_leased_run_without_lease"));
	}

	OperatorLaneControlProjection {
		ownership_state,
		liveness_state,
		policy_state,
		terminalization_state,
		next_action,
		conditions,
	}
}

fn operator_run_ownership_state(
	run: &OperatorRunStatus,
	liveness_state: &str,
	policy_state: &str,
	terminalization_state: &str,
) -> String {
	if run.run_lease
		&& matches!(run.attempt_status.as_str(), "starting" | "running" | "continuation_pending")
		&& !matches!(
			policy_state,
			"review_churn_exceeded"
				| "continuation_recovery_churn_exceeded"
				| "authority_boundary_required"
				| "human_attention_required"
		) {
		return String::from("leased_run");
	}
	if matches!(
		policy_state,
		"review_churn_exceeded"
			| "continuation_recovery_churn_exceeded"
			| "authority_boundary_required"
			| "human_attention_required"
	) || run.needs_attention
		|| (!run.run_lease && liveness_state == "host_boot_mismatch")
	{
		return String::from("retained_attention");
	}
	if operator_run_is_continuation_wait(run) {
		return String::from("continuation_pending");
	}
	if !run.run_lease
		&& matches!(liveness_state, "process_alive" | "thread_active" | "protocol_recent")
	{
		return String::from("orphaned_live_thread");
	}
	if terminalization_state != "none" && terminalization_state != "cleanup_complete" {
		return String::from("terminalizing");
	}
	if matches!(run.attempt_status.as_str(), "starting" | "running" | "continuation_pending") {
		return String::from("pending");
	}

	String::from("closed")
}

fn operator_run_is_continuation_wait(run: &OperatorRunStatus) -> bool {
	run.attempt_status == CONTINUATION_PENDING_RUN_STATUS
		|| run.phase == "waiting_continuation"
		|| run.retry_kind.as_deref() == Some("continuation")
		|| run.wait_reason.as_deref() == Some("continuation_retry")
}

fn operator_run_liveness_state(run: &OperatorRunStatus) -> String {
	if matches!(run.process_liveness_reason.as_deref(), Some("host_boot_id_mismatch")) {
		return String::from("host_boot_mismatch");
	}
	if run.process_alive == Some(true) {
		return String::from("process_alive");
	}
	if matches!(run.thread_status.as_deref(), Some("active")) || !run.thread_active_flags.is_empty()
	{
		return String::from("thread_active");
	}
	if operator_run_has_recent_app_server_execution(run) {
		return String::from("protocol_recent");
	}
	if run.process_alive == Some(false)
		|| matches!(run.execution_liveness.as_str(), "not_running" | "process_identity_mismatch")
	{
		return String::from("not_running");
	}

	String::from("unknown")
}

fn operator_run_policy_state(run: &OperatorRunStatus) -> String {
	if run.continuation_recovery.as_ref().is_some_and(|recovery| recovery.budget_exceeded) {
		return String::from("continuation_recovery_churn_exceeded");
	}

	let Some(loop_status) = run.loop_status.as_ref() else {
		return String::from("allowed");
	};

	if loop_status.decision_request.is_some() {
		return String::from("authority_boundary_required");
	}
	if loop_status.autonomy == "human_required" {
		return String::from("human_attention_required");
	}

	if let Some(recovery) = loop_status.architecture_recovery.as_ref() {
		return if recovery.status == "active" {
			String::from("architecture_recovery_pending")
		} else {
			String::from("human_attention_required")
		};
	}
	if let Some(review) = loop_status.review.as_ref() {
		return match review.status.as_str() {
			"pending" => String::from("review_pending"),
			"findings" => {
				if review.checkpoint.as_ref().is_some_and(|checkpoint| {
					checkpoint.nonclean_rounds >= REVIEW_POLICY_CONVERGENCE_BUDGET
				}) {
					String::from("review_churn_exceeded")
				} else {
					String::from("review_findings")
				}
			},
			"blocked" | "needs_architecture_review" => String::from("human_attention_required"),
			_ => String::from("allowed"),
		};
	}

	String::from("allowed")
}

fn operator_run_terminalization_state(run: &OperatorRunStatus, liveness_state: &str) -> String {
	if matches!(run.status.as_str(), "cleanup_complete" | "merged_closeout_reconciled")
		|| matches!(run.current_operation.as_str(), "ledger_outcome")
			&& matches!(run.phase.as_str(), "completed")
	{
		return String::from("cleanup_complete");
	}
	if matches!(run.phase.as_str(), "completed" | "failed" | "terminated")
		&& !run.run_lease
		&& matches!(liveness_state, "not_running" | "unknown")
	{
		return String::from("cleanup_complete");
	}
	if matches!(run.phase.as_str(), "completed" | "failed" | "terminated") {
		return String::from("barrier_started");
	}

	String::from("none")
}

fn operator_run_lane_control_conditions(
	run: &OperatorRunStatus,
	liveness_state: &str,
	policy_state: &str,
) -> Vec<String> {
	let mut conditions = Vec::new();

	if !run.run_lease
		&& matches!(run.attempt_status.as_str(), "starting" | "running" | "continuation_pending")
	{
		conditions.push(String::from("run_lease_missing"));
	}
	if matches!(run.attempt_status.as_str(), "failed" | "interrupted" | "stalled" | "succeeded")
		&& matches!(liveness_state, "process_alive" | "thread_active" | "protocol_recent")
	{
		conditions.push(String::from("terminal_attempt_has_live_evidence"));
	}
	if liveness_state == "host_boot_mismatch" {
		conditions.push(String::from("host_boot_id_mismatch"));
	}
	if policy_state == "review_churn_exceeded" {
		conditions.push(String::from("review_churn_threshold_exceeded"));
	}
	if policy_state == "continuation_recovery_churn_exceeded" {
		conditions.push(String::from("continuation_recovery_budget_exceeded"));
	}
	if matches!(policy_state, "authority_boundary_required" | "human_attention_required") {
		conditions.push(String::from("policy_requires_human_attention"));
	}

	conditions
}

fn operator_run_lane_control_next_action(
	run: &OperatorRunStatus,
	ownership_state: &str,
	liveness_state: &str,
	policy_state: &str,
	terminalization_state: &str,
) -> String {
	if policy_state == "review_churn_exceeded" {
		return String::from("start_architecture_recovery_or_stop_for_human_attention");
	}
	if policy_state == "continuation_recovery_churn_exceeded" {
		return String::from("stop_auto_continuation_and_request_architecture_recovery");
	}
	if matches!(policy_state, "authority_boundary_required" | "human_attention_required") {
		return String::from("resolve_policy_stop_before_mutating_lane");
	}
	if ownership_state == "orphaned_live_thread" {
		return String::from("inspect_or_interrupt_orphaned_live_thread");
	}
	if liveness_state == "host_boot_mismatch" {
		return String::from("inspect_recovery_evidence");
	}
	if terminalization_state != "none" && terminalization_state != "cleanup_complete" {
		return String::from("finish_terminalization");
	}
	if ownership_state == "leased_run" {
		if let Some(next_action) =
			run.loop_status.as_ref().and_then(|loop_status| loop_status.next_action.clone())
		{
			return next_action;
		}

		return String::from("continue_owned_attempt");
	}
	if ownership_state == "continuation_pending" {
		return String::from("wait_for_continuation_reentry");
	}
	if ownership_state == "closed" {
		return String::from("no_action");
	}

	if let Some(next_action) =
		run.loop_status.as_ref().and_then(|loop_status| loop_status.next_action.clone())
	{
		return next_action;
	}

	String::from("inspect_lane_state")
}

fn operator_run_lifecycle_projection(
	run: &ProjectRunStatus,
	marker: Option<&RunActivityMarker>,
	terminal_finalize_projection: Option<OperatorTerminalFinalizeProjection>,
	timing: &OperatorRunTiming,
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
	now_unix_epoch: i64,
) -> OperatorRunLifecycleProjection {
	let marker_current_operation = marker.and_then(RunActivityMarker::current_operation);
	let status = terminal_finalize_projection
		.map(|projection| projection.status.to_owned())
		.unwrap_or_else(|| {
			operator_run_visible_status(
				run.status(),
				app_server_state,
				protocol_summary,
				timing,
				marker_current_operation,
			)
		});
	let status_projection_reason = if terminal_finalize_projection.is_some() {
		None
	} else {
		operator_run_status_projection_reason(
			run.status(),
			&status,
			app_server_state,
			protocol_summary,
			timing,
			marker_current_operation,
		)
	};
	let (retry_kind, retry_ready_at_unix_epoch) = visible_operator_run_retry_schedule(
		&status,
		marker.and_then(RunActivityMarker::retry_kind),
		marker.and_then(RunActivityMarker::retry_ready_at_unix_epoch),
		now_unix_epoch,
	);
	let (phase, wait_reason) = if let Some(projection) = terminal_finalize_projection {
		(String::from(projection.phase), Some(String::from(projection.wait_reason)))
	} else {
		classify_operator_run_phase(
			&status,
			retry_kind.as_deref(),
			retry_ready_at_unix_epoch,
			now_unix_epoch,
		)
	};
	let current_operation = terminal_finalize_projection
		.map(|projection| projection.current_operation.to_owned())
		.unwrap_or_else(|| classify_operator_run_operation(&phase, marker_current_operation));
	let suspected_stall = terminal_finalize_projection.is_none()
		&& operator_run_is_suspected_stall(
			&phase,
			timing.last_progress_unix_epoch,
			now_unix_epoch,
			run_activity_idle_timeout(marker),
		);
	let execution_liveness = if terminal_finalize_projection.is_some() {
		String::from("not_running")
	} else {
		operator_run_execution_liveness(&status, timing, app_server_state, protocol_summary)
	};
	let run_lease = terminal_finalize_projection.is_none() && run.run_lease();

	OperatorRunLifecycleProjection {
		status,
		status_projection_reason,
		phase,
		wait_reason,
		current_operation,
		suspected_stall,
		execution_liveness,
		run_lease,
		retry_kind,
		retry_ready_at_unix_epoch,
	}
}

fn operator_run_wait_reason(
	phase: &str,
	wait_reason: Option<String>,
	protocol_activity: Option<&ProtocolActivitySummary>,
) -> Option<String> {
	if wait_reason.is_some() || phase != "executing" {
		return wait_reason;
	}

	protocol_activity
		.and_then(|summary| summary.waiting_reason.clone())
		.filter(|reason| reason != "turn_completed")
}

fn operator_run_accounts(
	marker: Option<&RunActivityMarker>,
) -> (Option<CodexAccountActivitySummary>, Vec<CodexAccountActivitySummary>) {
	let account = marker.and_then(RunActivityMarker::account).cloned();
	let mut accounts = marker.map(|marker| marker.accounts().to_vec()).unwrap_or_default();

	append_primary_account_if_missing(&mut accounts, account.as_ref());

	(account, accounts)
}

fn operator_run_relative_worktree_path(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
) -> Option<String> {
	run.worktree_path().map(|path| relative_worktree_path_for_path(project, path))
}

fn operator_run_private_evidence(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
) -> AgentPrivateEvidenceRef {
	private_evidence_ref_for_run_fields(
		project.service_id(),
		project.config_path(),
		run.issue_id(),
		issue_identifier,
		run.run_id(),
		run.attempt_number(),
	)
}

fn operator_run_loop_status(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
	status: &str,
	phase: &str,
	current_operation: &str,
) -> crate::prelude::Result<OperatorLoopStatus> {
	operator_loop_status_for_run_with_evidence(
		project,
		loop_evidence,
		run.issue_id(),
		run.run_id(),
		run.attempt_number(),
		operator_run_default_review_phase(status, phase, current_operation),
		operator_run_lifecycle_loop_summary(status, phase, current_operation),
	)
}

fn operator_run_default_review_phase(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> Option<&'static str> {
	if operator_run_has_terminal_lifecycle(status, phase, current_operation) {
		return None;
	}
	if current_operation == RUN_OPERATION_REVIEW_WRITEBACK {
		return Some("handoff");
	}

	None
}

fn operator_run_lifecycle_loop_summary(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> Option<String> {
	operator_run_has_terminal_lifecycle(status, phase, current_operation)
		.then(|| format!("terminal lifecycle: {status}"))
}

fn operator_run_has_terminal_lifecycle(status: &str, phase: &str, current_operation: &str) -> bool {
	phase == "completed"
		|| phase == "terminal_pending"
		|| current_operation == "ledger_outcome"
		|| matches!(
			status,
			"succeeded"
				| "failed" | "interrupted"
				| "review_handoff_pending"
				| "review_repair_pending"
				| "closeout_pending"
				| "manual_attention_pending"
				| "cleanup_complete"
				| "closeout" | "landed"
				| "manual_attention"
				| TERMINAL_GUARDED_RUN_STATUS
		)
}

fn operator_loop_status_for_run(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_review_phase: Option<&str>,
	lifecycle_summary: Option<String>,
) -> crate::prelude::Result<OperatorLoopStatus> {
	let loop_evidence = state_store.project_loop_evidence_snapshot(project.service_id())?;

	operator_loop_status_for_run_with_evidence(
		project,
		&loop_evidence,
		issue_id,
		run_id,
		attempt_number,
		default_review_phase,
		lifecycle_summary,
	)
}

fn operator_loop_status_for_run_with_evidence(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_review_phase: Option<&str>,
	lifecycle_summary: Option<String>,
) -> crate::prelude::Result<OperatorLoopStatus> {
	let review_level = project.codex().review_level();
	let review = operator_review_loop_status(
		review_level,
		loop_evidence,
		issue_id,
		run_id,
		attempt_number,
		default_review_phase,
	)?;
	let events = loop_evidence.private_events(issue_id, run_id, attempt_number);
	let architecture_recovery =
		events.iter().rev().find_map(operator_architecture_recovery_status_from_event);
	let boundary = events.iter().rev().find_map(operator_boundary_status_from_event);
	let decision_request = events
		.iter()
		.rev()
		.find(|event| event.event_type() == AUTHORITY_DECISION_REQUEST_EVENT_TYPE)
		.and_then(operator_authority_decision_request_status_from_event);
	let autonomy_objective = operator_autonomy_objective_status(project, loop_evidence);
	let autonomy_signals = operator_autonomy_signal_statuses(loop_evidence);
	let autonomy_proposals = operator_autonomy_proposal_statuses(loop_evidence);
	let autonomy_lineage = operator_autonomy_lineage_statuses(loop_evidence);
	let autonomy_report = operator_autonomy_report_status(
		autonomy_objective.as_ref(),
		&autonomy_signals,
		&autonomy_proposals,
		&autonomy_lineage,
	);
	let autonomy = operator_loop_autonomy(
		boundary.as_ref(),
		architecture_recovery.as_ref(),
		decision_request.as_ref(),
	);
	let summary = operator_loop_status_summary(
		review.as_ref(),
		architecture_recovery.as_ref(),
		boundary.as_ref(),
		decision_request.as_ref(),
		autonomy,
		lifecycle_summary.as_deref(),
	);
	let next_action = operator_loop_status_next_action(
		review.as_ref(),
		architecture_recovery.as_ref(),
		boundary.as_ref(),
		decision_request.as_ref(),
	);

	Ok(OperatorLoopStatus {
		review_level: review_level.as_str().to_owned(),
		autonomy: autonomy.to_owned(),
		summary,
		next_action,
		autonomy_objective,
		autonomy_signals,
		autonomy_proposals,
		autonomy_lineage,
		autonomy_report,
		review,
		architecture_recovery,
		boundary,
		decision_request,
	})
}

fn operator_autonomy_objective_status(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Option<OperatorAutonomyObjectiveStatus> {
	if let Some(policy) = project.autonomy().runtime_policy() {
		let version = policy.accepted_objective_version().parse::<u64>().unwrap_or_default();
		let source_ref = operator_autonomy_objective_ref(policy.accepted_objective_id(), version);

		if let Some(record) =
			loop_evidence.autonomy_objective(policy.accepted_objective_id(), version)
		{
			let objective = record.objective();
			let mut known_gaps = Vec::new();

			if record.state().as_str() != "accepted" {
				known_gaps.push(format!("objective_state_{}", record.state().as_str()));
			}

			return Some(OperatorAutonomyObjectiveStatus {
				objective_id: objective.id().to_owned(),
				objective_version: objective.version(),
				state: objective.state().as_str().to_owned(),
				summary: public_or_redacted_status_value(objective.summary()),
				source_ref,
				updated_at: record.updated_at().to_owned(),
				completeness: operator_autonomy_completeness(&known_gaps),
				known_gaps,
			});
		}

		let mut known_gaps = vec![String::from("objective_runtime_record_missing")];

		if version == 0 {
			known_gaps.push(String::from("objective_version_unparseable"));
		}

		return Some(OperatorAutonomyObjectiveStatus {
			objective_id: policy.accepted_objective_id().to_owned(),
			objective_version: version,
			state: String::from("missing_runtime_record"),
			summary: String::from(
				"Accepted runtime policy references an Objective Contract that is not in local readback.",
			),
			source_ref,
			updated_at: String::from("none"),
			completeness: String::from("partial"),
			known_gaps,
		});
	}

	loop_evidence.accepted_autonomy_objectives().into_iter().next().map(|record| {
		let objective = record.objective();

		OperatorAutonomyObjectiveStatus {
			objective_id: objective.id().to_owned(),
			objective_version: objective.version(),
			state: objective.state().as_str().to_owned(),
			summary: public_or_redacted_status_value(objective.summary()),
			source_ref: operator_autonomy_objective_ref(objective.id(), objective.version()),
			updated_at: record.updated_at().to_owned(),
			completeness: String::from("complete"),
			known_gaps: Vec::new(),
		}
	})
}

fn operator_autonomy_signal_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Vec<OperatorAutonomySignalStatus> {
	loop_evidence
		.recent_autonomy_signals(5)
		.into_iter()
		.map(|record| {
			let signal = record.signal();
			let (source_refs, source_refs_redacted) = public_autonomy_refs(signal.source_refs());
			let (primary_source_refs, primary_source_refs_redacted) =
				public_autonomy_refs(signal.primary_source_refs());
			let (gaps, gaps_redacted) = public_status_values(signal.gaps());
			let (contradictions, contradictions_redacted) =
				public_status_values(signal.contradictions());
			let mut known_gaps = gaps.clone();

			if source_refs.is_empty() {
				known_gaps.push(String::from("source_refs_missing_or_redacted"));
			}
			if source_refs_redacted || primary_source_refs_redacted {
				known_gaps.push(String::from("source_refs_redacted"));
			}
			if gaps_redacted || contradictions_redacted {
				known_gaps.push(String::from("gap_or_contradiction_redacted"));
			}
			if signal.freshness().as_str() != "fresh" {
				known_gaps.push(format!("freshness_{}", signal.freshness().as_str()));
			}

			known_gaps.sort();
			known_gaps.dedup();
			OperatorAutonomySignalStatus {
				signal_id: signal.id().to_owned(),
				objective_id: signal.objective_id().to_owned(),
				objective_version: signal.objective_version(),
				kind: signal.kind().as_str().to_owned(),
				source_type: signal.source_type().as_str().to_owned(),
				source_refs,
				primary_source_refs,
				freshness: signal.freshness().as_str().to_owned(),
				evidence_class: signal.evidence_class().as_str().to_owned(),
				confidence: signal.confidence().as_str().to_owned(),
				privacy: signal.privacy().as_str().to_owned(),
				redaction_level: signal.privacy().as_str().to_owned(),
				completeness: operator_autonomy_completeness(&known_gaps),
				gaps,
				known_gaps,
				contradictions,
				updated_at: record.updated_at().to_owned(),
			}
		})
		.collect()
}

fn operator_autonomy_proposal_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Vec<OperatorAutonomyProposalStatus> {
	loop_evidence
		.recent_autonomy_proposals(5)
		.into_iter()
		.map(|record| {
			let proposal = record.proposal();
			let (source_family, source_family_redacted) =
				public_status_value(proposal.source_family());
			let (intended_surface, intended_surface_redacted) =
				public_status_value(proposal.intended_surface());
			let (affected_identifiers, affected_identifiers_redacted) =
				public_status_values(proposal.affected_identifiers());
			let (gaps, gaps_redacted) = public_status_values(proposal.gaps());
			let (contradictions, contradictions_redacted) =
				public_status_values(proposal.contradictions());
			let refusals = proposal
				.refusal_reasons()
				.iter()
				.map(|refusal| {
					let (evidence_refs, _) = public_autonomy_refs(refusal.evidence_refs());

					OperatorAutonomyProposalRefusalStatus {
						reason: refusal.reason().as_str().to_owned(),
						detail: public_or_redacted_status_value(refusal.detail()),
						evidence_refs,
					}
				})
				.collect::<Vec<_>>();
			let mut known_gaps = gaps.clone();

			if proposal.source_signal_ids().is_empty() {
				known_gaps.push(String::from("source_signal_ids_missing"));
			}
			if !proposal.refusal_reasons().is_empty() {
				known_gaps.push(String::from("proposal_refused"));
			}
			if source_family_redacted
				|| intended_surface_redacted
				|| affected_identifiers_redacted
				|| gaps_redacted
				|| contradictions_redacted
			{
				known_gaps.push(String::from("proposal_public_fields_redacted"));
			}

			known_gaps.sort();
			known_gaps.dedup();
			OperatorAutonomyProposalStatus {
				proposal_id: proposal.id().to_owned(),
				objective_id: proposal.objective_id().to_owned(),
				objective_version: proposal.objective_version(),
				state: proposal.state().as_str().to_owned(),
				summary: public_or_redacted_status_value(proposal.summary()),
				source_family,
				intended_surface,
				affected_identifiers,
				source_signal_ids: proposal.source_signal_ids().to_vec(),
				refusal_reasons: proposal
					.refusal_reasons()
					.iter()
					.map(|refusal| refusal.reason().as_str().to_owned())
					.collect(),
				refusals,
				completeness: operator_autonomy_completeness(&known_gaps),
				known_gaps,
				gaps,
				contradictions,
				challenge_evidence_count: proposal.challenge_evidence().len(),
				updated_at: record.updated_at().to_owned(),
			}
		})
		.collect()
}

fn operator_autonomy_lineage_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Vec<OperatorAutonomyLineageStatus> {
	loop_evidence
		.recent_autonomy_proposals(5)
		.into_iter()
		.map(|record| {
			let proposal = record.proposal();
			let contract_records =
				loop_evidence.decision_contracts_for_autonomy_proposal(proposal.id());
			let decision_contracts = contract_records
				.iter()
				.map(|record| OperatorAutonomyDecisionContractStatus {
					contract_id: record.contract_id().to_owned(),
					status: record.status().as_str().to_owned(),
					updated_at: record.updated_at().to_owned(),
					generated_issue_identifiers: record
						.contract()
						.links()
						.generated_issue_identifiers()
						.to_vec(),
				})
				.collect::<Vec<_>>();
			let execution_evidence = operator_autonomy_execution_evidence_statuses(
				loop_evidence,
				proposal.id(),
				&contract_records,
			);
			let program_intake = decision_contracts
				.iter()
				.flat_map(|contract| {
					loop_evidence
						.program_intake_plans_for_contract(&contract.contract_id)
						.into_iter()
						.map(|plan| OperatorAutonomyProgramIntakeStatus {
							program_id: plan.program_id().to_owned(),
							plan_id: plan.plan_id().to_owned(),
							intake_kind: plan.intake_kind().to_owned(),
							source_contract_id: plan
								.source_contract_id()
								.unwrap_or("none")
								.to_owned(),
							public_summary: public_or_redacted_status_value(plan.public_summary()),
							updated_at: plan.updated_at().to_owned(),
						})
						.collect::<Vec<_>>()
				})
				.collect::<Vec<_>>();
			let mut known_gaps = Vec::new();

			if proposal.source_signal_ids().is_empty() {
				known_gaps.push(String::from("signal_lineage_missing"));
			}
			if decision_contracts.is_empty() {
				known_gaps.push(String::from("decision_contract_not_materialized"));
			}
			if program_intake.is_empty() {
				known_gaps.push(String::from("program_intake_not_materialized"));
			}
			if !program_intake.is_empty() {
				let evidence_kinds = execution_evidence
					.iter()
					.map(|evidence| evidence.kind.as_str())
					.collect::<BTreeSet<_>>();

				for (kind, gap) in [
					("pr", "pr_evidence_missing"),
					("validation", "validation_evidence_missing"),
					("post_land", "post_land_evidence_missing"),
				] {
					if !evidence_kinds.contains(kind) {
						known_gaps.push(String::from(gap));
					}
				}

				known_gaps.extend(
					execution_evidence
						.iter()
						.flat_map(|evidence| evidence.known_gaps.iter().cloned()),
				);
			}

			let (proposal_gaps, proposal_gaps_redacted) = public_status_values(proposal.gaps());

			known_gaps.extend(proposal_gaps);

			if proposal_gaps_redacted {
				known_gaps.push(String::from("proposal_gaps_redacted"));
			}

			known_gaps.sort();
			known_gaps.dedup();
			OperatorAutonomyLineageStatus {
				objective_ref: operator_autonomy_objective_ref(
					proposal.objective_id(),
					proposal.objective_version(),
				),
				signal_ids: proposal.source_signal_ids().to_vec(),
				proposal_id: Some(proposal.id().to_owned()),
				proposal_state: Some(proposal.state().as_str().to_owned()),
				decision_contracts,
				program_intake,
				execution_evidence,
				completeness: operator_autonomy_completeness(&known_gaps),
				known_gaps,
			}
		})
		.collect()
}

fn operator_autonomy_execution_evidence_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	proposal_id: &str,
	contracts: &[&DecisionContractRecord],
) -> Vec<OperatorAutonomyExecutionEvidenceStatus> {
	let contract_ids = contracts.iter().map(|record| record.contract_id()).collect::<BTreeSet<_>>();
	let mut evidence = Vec::new();

	for (issue_id, issue_identifier) in operator_autonomy_generated_issue_pairs(contracts) {
		let review_lifecycle_records = loop_evidence.review_lifecycle_records_for_issue(&issue_id);

		for event in loop_evidence.private_events_for_issue(&issue_id) {
			if let Some(status) = operator_autonomy_replay_evidence_status_from_event(
				event,
				proposal_id,
				&contract_ids,
				issue_identifier.as_deref(),
				&review_lifecycle_records,
			) {
				evidence.push(status);
			}
		}
	}

	evidence.sort_by(|left, right| {
		left.kind
			.cmp(&right.kind)
			.then_with(|| left.issue_identifier.cmp(&right.issue_identifier))
			.then_with(|| left.source_refs.cmp(&right.source_refs))
			.then_with(|| {
				operator_autonomy_evidence_completeness_rank(&right.completeness)
					.cmp(&operator_autonomy_evidence_completeness_rank(&left.completeness))
			})
			.then_with(|| right.updated_at.cmp(&left.updated_at))
			.then_with(|| left.summary.cmp(&right.summary))
	});
	evidence.dedup_by(|left, right| {
		left.kind == right.kind
			&& left.issue_identifier == right.issue_identifier
			&& left.source_refs == right.source_refs
	});

	evidence
}

fn operator_autonomy_generated_issue_pairs(
	contracts: &[&DecisionContractRecord],
) -> Vec<(String, Option<String>)> {
	let mut pairs = contracts
		.iter()
		.flat_map(|record| {
			let links = record.contract().links();

			links
				.generated_issue_ids()
				.iter()
				.enumerate()
				.map(|(index, issue_id)| {
					(issue_id.clone(), links.generated_issue_identifiers().get(index).cloned())
				})
				.collect::<Vec<_>>()
		})
		.collect::<Vec<_>>();

	pairs.sort();
	pairs.dedup();

	pairs
}

fn operator_autonomy_pr_evidence_status_from_event(
	event: &PrivateExecutionEvent,
	review: &ReviewLifecycleRecord,
	issue_identifier: Option<&str>,
	summary: String,
	summary_redacted: bool,
) -> OperatorAutonomyExecutionEvidenceStatus {
	let (source_refs, refs_redacted) = public_autonomy_refs(&[review.pr_url().to_owned()]);
	let mut known_gaps = Vec::new();

	if source_refs.is_empty() {
		known_gaps.push(String::from("source_refs_missing_or_redacted"));
	}
	if refs_redacted {
		known_gaps.push(String::from("source_refs_redacted"));
	}
	if summary_redacted {
		known_gaps.push(String::from("summary_redacted"));
	}

	OperatorAutonomyExecutionEvidenceStatus {
		kind: String::from("pr"),
		issue_identifier: issue_identifier.map(str::to_owned),
		source_refs,
		summary,
		updated_at: [review.updated_at(), event.recorded_at()]
			.into_iter()
			.max()
			.unwrap_or_else(|| event.recorded_at())
			.to_owned(),
		completeness: operator_autonomy_completeness(&known_gaps),
		known_gaps,
	}
}

fn operator_autonomy_replay_evidence_status_from_event(
	event: &PrivateExecutionEvent,
	proposal_id: &str,
	contract_ids: &BTreeSet<&str>,
	issue_identifier: Option<&str>,
	review_lifecycle_records: &[&ReviewLifecycleRecord],
) -> Option<OperatorAutonomyExecutionEvidenceStatus> {
	let payload = event.payload();

	if payload.get("schema").and_then(Value::as_str) != Some(AUTONOMY_REPLAY_EVIDENCE_SCHEMA) {
		return None;
	}
	if !operator_autonomy_replay_evidence_matches(payload, proposal_id, contract_ids) {
		return None;
	}

	let kind = match payload.get("kind").and_then(Value::as_str) {
		Some(kind @ ("pr" | "validation" | "post_land")) => kind.to_owned(),
		_ => return None,
	};
	let raw_source_refs = payload
		.get("source_refs")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(str::to_owned)
		.collect::<Vec<_>>();
	let (source_refs, refs_redacted) = public_autonomy_refs(&raw_source_refs);
	let (summary, summary_redacted) = public_status_value(
		payload
			.get("summary")
			.and_then(Value::as_str)
			.unwrap_or("Dogfood replay evidence recorded."),
	);
	let mut known_gaps = Vec::new();

	if kind == "pr" {
		let pr_head_ref = payload
			.get("pr_head_ref")
			.and_then(Value::as_str)
			.map(str::trim)
			.filter(|value| !value.is_empty());
		let pr_head_oid = payload
			.get("pr_head_oid")
			.and_then(Value::as_str)
			.map(str::trim)
			.filter(|value| !value.is_empty());

		return Some(
			match operator_autonomy_matching_pr_review(
				event,
				&raw_source_refs,
				pr_head_ref,
				pr_head_oid,
				review_lifecycle_records,
			) {
				Some(review) => operator_autonomy_pr_evidence_status_from_event(
					event,
					review,
					issue_identifier,
					summary,
					summary_redacted,
				),
				None => {
					if source_refs.is_empty() {
						known_gaps.push(String::from("source_refs_missing_or_redacted"));
					}
					if refs_redacted {
						known_gaps.push(String::from("source_refs_redacted"));
					}
					if summary_redacted {
						known_gaps.push(String::from("summary_redacted"));
					}
					if pr_head_ref.is_none() || pr_head_oid.is_none() {
						known_gaps.push(String::from("pr_head_identity_missing"));
					} else if operator_autonomy_pr_review_candidate_exists(
						event,
						&raw_source_refs,
						review_lifecycle_records,
					) {
						known_gaps.push(String::from("review_lifecycle_stale_or_mismatched"));
					} else {
						known_gaps.push(String::from("review_lifecycle_missing"));
					}

					OperatorAutonomyExecutionEvidenceStatus {
						kind,
						issue_identifier: issue_identifier.map(str::to_owned),
						source_refs,
						summary,
						updated_at: event.recorded_at().to_owned(),
						completeness: operator_autonomy_completeness(&known_gaps),
						known_gaps,
					}
				},
			},
		);
	}
	if source_refs.is_empty() {
		known_gaps.push(String::from("source_refs_missing_or_redacted"));
	}
	if refs_redacted {
		known_gaps.push(String::from("source_refs_redacted"));
	}
	if summary_redacted {
		known_gaps.push(String::from("summary_redacted"));
	}

	Some(OperatorAutonomyExecutionEvidenceStatus {
		kind,
		issue_identifier: issue_identifier.map(str::to_owned),
		source_refs,
		summary,
		updated_at: event.recorded_at().to_owned(),
		completeness: operator_autonomy_completeness(&known_gaps),
		known_gaps,
	})
}

fn operator_autonomy_matching_pr_review<'a>(
	event: &PrivateExecutionEvent,
	raw_source_refs: &[String],
	pr_head_ref: Option<&str>,
	pr_head_oid: Option<&str>,
	review_lifecycle_records: &'a [&'a ReviewLifecycleRecord],
) -> Option<&'a ReviewLifecycleRecord> {
	let pr_head_ref = pr_head_ref?;
	let pr_head_oid = pr_head_oid?;
	let raw_source_refs =
		raw_source_refs.iter().map(|source_ref| source_ref.trim()).collect::<BTreeSet<_>>();

	review_lifecycle_records
		.iter()
		.copied()
		.filter(|review| {
			review.run_id() == event.run_id()
				&& review.attempt_number() == event.attempt_number()
				&& raw_source_refs.contains(review.pr_url())
				&& review.branch_name() == pr_head_ref
				&& review.pr_head_ref_name() == pr_head_ref
				&& review.pr_head_oid() == pr_head_oid
				&& review.head_sha() == pr_head_oid
		})
		.max_by(|left, right| {
			left.updated_at_unix()
				.cmp(&right.updated_at_unix())
				.then_with(|| left.branch_name().cmp(right.branch_name()))
		})
}

fn operator_autonomy_pr_review_candidate_exists(
	event: &PrivateExecutionEvent,
	raw_source_refs: &[String],
	review_lifecycle_records: &[&ReviewLifecycleRecord],
) -> bool {
	let raw_source_refs =
		raw_source_refs.iter().map(|source_ref| source_ref.trim()).collect::<BTreeSet<_>>();

	review_lifecycle_records.iter().any(|review| {
		review.run_id() == event.run_id()
			&& review.attempt_number() == event.attempt_number()
			&& raw_source_refs.contains(review.pr_url())
	})
}

fn operator_autonomy_replay_evidence_matches(
	payload: &Value,
	proposal_id: &str,
	contract_ids: &BTreeSet<&str>,
) -> bool {
	payload.get("proposal_id").and_then(Value::as_str) == Some(proposal_id)
		|| payload
			.get("contract_id")
			.and_then(Value::as_str)
			.is_some_and(|contract_id| contract_ids.contains(contract_id))
}

fn operator_autonomy_report_status(
	objective: Option<&OperatorAutonomyObjectiveStatus>,
	signals: &[OperatorAutonomySignalStatus],
	proposals: &[OperatorAutonomyProposalStatus],
	lineage: &[OperatorAutonomyLineageStatus],
) -> Option<OperatorAutonomyReportReadbackStatus> {
	if objective.is_none() && signals.is_empty() && proposals.is_empty() && lineage.is_empty() {
		return None;
	}

	let mut source_refs = BTreeSet::new();
	let mut known_gaps = BTreeSet::new();
	let mut redaction_level = "public";

	if let Some(objective) = objective {
		source_refs.insert(objective.source_ref.clone());

		for gap in &objective.known_gaps {
			known_gaps.insert(gap.clone());
		}
	}

	for signal in signals {
		for source_ref in &signal.source_refs {
			source_refs.insert(source_ref.clone());
		}
		for primary_source_ref in &signal.primary_source_refs {
			source_refs.insert(primary_source_ref.clone());
		}
		for gap in &signal.known_gaps {
			known_gaps.insert(gap.clone());
		}

		redaction_level = operator_autonomy_max_redaction_level(redaction_level, &signal.privacy);
	}
	for proposal in proposals {
		for gap in &proposal.known_gaps {
			known_gaps.insert(gap.clone());
		}
	}
	for item in lineage {
		for evidence in &item.execution_evidence {
			for source_ref in &evidence.source_refs {
				source_refs.insert(source_ref.clone());
			}
			for gap in &evidence.known_gaps {
				known_gaps.insert(gap.clone());
			}
		}
		for gap in &item.known_gaps {
			known_gaps.insert(gap.clone());
		}
	}

	if source_refs.is_empty() {
		known_gaps.insert(String::from("source_refs_missing_or_redacted"));
	}

	let known_gaps = known_gaps.into_iter().collect::<Vec<_>>();

	Some(OperatorAutonomyReportReadbackStatus {
		surface: String::from("operator_status_autonomy"),
		authority: String::from("derived_query_view"),
		audit_authority: false,
		source_refs: source_refs.into_iter().collect(),
		redaction_level: redaction_level.to_owned(),
		completeness: operator_autonomy_completeness(&known_gaps),
		known_gaps,
	})
}

fn operator_autonomy_objective_ref(objective_id: &str, objective_version: u64) -> String {
	format!("{objective_id}@v{objective_version}")
}

fn operator_autonomy_completeness(known_gaps: &[String]) -> String {
	if known_gaps.is_empty() { String::from("complete") } else { String::from("partial") }
}

fn operator_autonomy_evidence_completeness_rank(value: &str) -> u8 {
	match value {
		"complete" => 1,
		_ => 0,
	}
}

fn operator_autonomy_max_redaction_level(left: &str, right: &str) -> &'static str {
	match (operator_autonomy_redaction_rank(left), operator_autonomy_redaction_rank(right)) {
		(left_rank, right_rank) if left_rank >= right_rank => {
			operator_autonomy_redaction_label(left)
		},
		_ => operator_autonomy_redaction_label(right),
	}
}

fn operator_autonomy_redaction_rank(value: &str) -> u8 {
	match value {
		"local_private" => 2,
		"team" => 1,
		_ => 0,
	}
}

fn operator_autonomy_redaction_label(value: &str) -> &'static str {
	match value {
		"local_private" => "local_private",
		"team" => "team",
		_ => "public",
	}
}

fn public_autonomy_refs(refs: &[String]) -> (Vec<String>, bool) {
	let mut redacted = false;
	let refs = refs
		.iter()
		.filter_map(|value| {
			let Some(value) = public_autonomy_ref(value) else {
				redacted = true;

				return None;
			};

			Some(value)
		})
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect::<Vec<_>>();

	(refs, redacted)
}

fn public_autonomy_ref(value: &str) -> Option<String> {
	let value = value.trim();

	if value.is_empty()
		|| public_text::validate_public_text_field("autonomy source_ref", value).is_err()
	{
		return None;
	}

	Some(value.to_owned())
}

fn public_status_values(values: &[String]) -> (Vec<String>, bool) {
	let mut redacted = false;
	let values = values
		.iter()
		.map(|value| {
			let (value, value_redacted) = public_status_value(value);

			redacted |= value_redacted;

			value
		})
		.collect();

	(values, redacted)
}

fn public_or_redacted_status_value(value: &str) -> String {
	public_status_value(value).0
}

fn public_status_value(value: &str) -> (String, bool) {
	let value = value.trim();

	if value.is_empty() {
		return (String::from("none"), false);
	}
	if public_text::validate_public_text_field("autonomy status value", value).is_err() {
		return (String::from("redacted_sensitive_detail"), true);
	}

	(value.to_owned(), false)
}

fn operator_review_loop_status(
	review_level: ReviewLevel,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_review_phase: Option<&str>,
) -> crate::prelude::Result<Option<OperatorReviewLoopStatus>> {
	if let Some(checkpoint) = operator_latest_review_checkpoint_event_status(
		loop_evidence,
		issue_id,
		run_id,
		attempt_number,
	) {
		return Ok(Some(checkpoint));
	}

	let latest_checkpoint = ["handoff", "repair"]
		.into_iter()
		.filter_map(|phase| {
			loop_evidence.review_policy_checkpoint(issue_id, run_id, attempt_number, phase)
		})
		.max_by(|left, right| {
			left.updated_at_unix()
				.cmp(&right.updated_at_unix())
				.then_with(|| left.phase().cmp(right.phase()))
		});

	if let Some(checkpoint) = latest_checkpoint {
		let nonclean_rounds = checkpoint.nonclean_rounds();
		let summary = operator_review_checkpoint_summary_fields(checkpoint.details_json());

		return Ok(Some(OperatorReviewLoopStatus {
			phase: checkpoint.phase().to_owned(),
			status: checkpoint.status().to_owned(),
			checkpoint: Some(OperatorReviewCheckpointStatus {
				head_sha: checkpoint.head_sha().to_owned(),
				round: nonclean_rounds,
				nonclean_rounds,
				review_class: summary.review_class,
				risk_class: summary.risk_class,
				compact_eligible: summary.compact_eligible,
				fallback_reason: summary.fallback_reason,
				active_fingerprints: summary.active_fingerprints,
				stop_fingerprint: summary.stop_fingerprint,
				route_counts: summary.route_counts,
				route_next_action: summary.route_next_action,
				updated_at: checkpoint.updated_at().to_owned(),
			}),
		}));
	}

	if review_level.requires_review_checkpoint()
		&& let Some(default_review_phase) = default_review_phase
	{
		return Ok(Some(OperatorReviewLoopStatus {
			phase: default_review_phase.to_owned(),
			status: String::from("pending"),
			checkpoint: None,
		}));
	}

	Ok(None)
}

fn operator_latest_review_checkpoint_event_status(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
) -> Option<OperatorReviewLoopStatus> {
	loop_evidence.private_events(issue_id, run_id, attempt_number).iter().rev().find_map(|event| {
		let payload = event.payload();

		if event.event_type() != "review_checkpoint" {
			return None;
		}

		let phase = payload.get("phase").and_then(Value::as_str)?;
		let status = payload.get("status").and_then(Value::as_str)?;
		let head_sha = payload.get("head_sha").and_then(Value::as_str)?;
		let nonclean_rounds = payload.get("nonclean_rounds").and_then(Value::as_i64).unwrap_or(0);
		let checkpoint =
			loop_evidence.review_policy_checkpoint(issue_id, run_id, attempt_number, phase)?;

		if checkpoint.status() != status
			|| checkpoint.head_sha() != head_sha
			|| checkpoint.nonclean_rounds() != nonclean_rounds
		{
			return None;
		}

		let details_json = payload.get("review").unwrap_or(payload).to_string();
		let summary = operator_review_checkpoint_summary_fields(&details_json);

		Some(OperatorReviewLoopStatus {
			phase: phase.to_owned(),
			status: status.to_owned(),
			checkpoint: Some(OperatorReviewCheckpointStatus {
				head_sha: head_sha.to_owned(),
				round: nonclean_rounds,
				nonclean_rounds,
				review_class: summary.review_class,
				risk_class: summary.risk_class,
				compact_eligible: summary.compact_eligible,
				fallback_reason: summary.fallback_reason,
				active_fingerprints: summary.active_fingerprints,
				stop_fingerprint: summary.stop_fingerprint,
				route_counts: summary.route_counts,
				route_next_action: summary.route_next_action,
				updated_at: checkpoint.updated_at().to_owned(),
			}),
		})
	})
}

fn operator_review_checkpoint_summary_fields(
	details_json: &str,
) -> OperatorReviewCheckpointSummaryFields {
	let Ok(details) = serde_json::from_str::<Value>(details_json) else {
		return OperatorReviewCheckpointSummaryFields {
			review_class: None,
			risk_class: None,
			compact_eligible: None,
			fallback_reason: None,
			active_fingerprints: Vec::new(),
			stop_fingerprint: None,
			route_counts: Vec::new(),
			route_next_action: None,
		};
	};
	let policy = details.get("finding_policy");
	let cost_control = details.get("review_cost_control");
	let review_class = cost_control
		.and_then(|cost_control| cost_control.get("review_class"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let risk_class = cost_control
		.and_then(|cost_control| cost_control.get("risk_class"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let compact_eligible = cost_control
		.and_then(|cost_control| cost_control.get("compact_eligible"))
		.and_then(Value::as_bool);
	let fallback_reason = cost_control
		.and_then(|cost_control| cost_control.get("fallback_reason"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let active_fingerprints = policy
		.and_then(|policy| policy.get("active_fingerprints"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(str::to_owned)
		.collect();
	let stop_fingerprint = policy
		.and_then(|policy| policy.get("stop_fingerprint"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let route_summary = details.get("finding_route_summary");
	let route_counts = route_summary
		.and_then(|summary| summary.get("route_counts"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|count| {
			Some(OperatorReviewRouteCount {
				route: count.get("route")?.as_str()?.to_owned(),
				count: usize::try_from(count.get("count")?.as_u64()?).ok()?,
			})
		})
		.collect();
	let route_next_action = route_summary
		.and_then(|summary| summary.get("next_action"))
		.and_then(Value::as_str)
		.map(str::to_owned);

	OperatorReviewCheckpointSummaryFields {
		review_class,
		risk_class,
		compact_eligible,
		fallback_reason,
		active_fingerprints,
		stop_fingerprint,
		route_counts,
		route_next_action,
	}
}

fn operator_architecture_recovery_status_from_event(
	event: &PrivateExecutionEvent,
) -> Option<OperatorArchitectureRecoveryStatus> {
	if !matches!(
		event.event_type(),
		ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE
			| ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE
			| ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE
	) {
		return None;
	}

	let payload = event.payload();
	let reason_code = payload.get("reason_code")?.as_str()?.to_owned();
	let guardrail_reason = payload
		.get("guardrail_reason")
		.and_then(Value::as_str)
		.or_else(|| {
			payload
				.get("loop_guardrail")
				.and_then(|guardrail| guardrail.get("reason"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned);
	let boundary_disposition = payload
		.get("boundary_disposition")
		.and_then(Value::as_str)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("disposition"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned);
	let boundary_policy_decision = payload
		.get("boundary_policy_decision")
		.and_then(Value::as_str)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("policy_decision"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned)
		.or_else(|| {
			boundary_disposition
				.as_deref()
				.map(operator_boundary_policy_decision_from_disposition)
				.map(str::to_owned)
		});
	let requires_enhanced_evidence = payload
		.get("requires_enhanced_evidence")
		.and_then(Value::as_bool)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("requires_enhanced_evidence"))
				.and_then(Value::as_bool)
		})
		.unwrap_or_else(|| {
			boundary_policy_decision
				.as_deref()
				.is_some_and(operator_boundary_policy_requires_enhanced_evidence)
		});
	let blocks_landing = payload
		.get("blocks_landing")
		.and_then(Value::as_bool)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("blocks_landing"))
				.and_then(Value::as_bool)
		})
		.unwrap_or_else(|| {
			boundary_policy_decision.as_deref().is_some_and(operator_boundary_policy_blocks_landing)
		});
	let recovery_budget_attempt = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("attempt"))
		.and_then(Value::as_u64);
	let recovery_budget_max_attempts = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("max_attempts"))
		.and_then(Value::as_u64);
	let budget = recovery_budget_attempt
		.zip(recovery_budget_max_attempts)
		.map(|(attempt, max_attempts)| OperatorRecoveryBudgetStatus { attempt, max_attempts });
	let next_action = operator_architecture_recovery_next_action(
		&reason_code,
		boundary_policy_decision.as_deref(),
		requires_enhanced_evidence,
		blocks_landing,
	);

	Some(OperatorArchitectureRecoveryStatus {
		status: operator_architecture_recovery_status_for_reason(&reason_code).to_owned(),
		reason_code,
		guardrail_reason,
		boundary_disposition,
		boundary_policy_decision,
		requires_enhanced_evidence,
		blocks_landing,
		round: recovery_budget_attempt,
		budget,
		next_action,
	})
}

fn operator_architecture_recovery_status_for_reason(reason_code: &str) -> &'static str {
	match reason_code {
		"architecture_recovery_started" => "active",
		"architecture_recovery_exhausted" => "exhausted",
		"contract_boundary_required" | "external_dependency_required" => "human_required",
		_ => "terminal",
	}
}

fn operator_architecture_recovery_next_action(
	reason_code: &str,
	policy_decision: Option<&str>,
	requires_enhanced_evidence: bool,
	blocks_landing: bool,
) -> String {
	match reason_code {
		"architecture_recovery_started" => {
			match (policy_decision, blocks_landing, requires_enhanced_evidence) {
				(Some(policy), true, _) => format!(
					"Retry with a materially different implementation strategy under authority policy `{policy}`; keep landing blocked until validation or review-policy evidence is restored."
				),
				(Some(policy), false, true) => format!(
					"Retry with a materially different implementation strategy under authority policy `{policy}`; preserve enhanced evidence before review handoff or landing."
				),
				(Some(policy), false, false) => format!(
					"Retry with a materially different implementation strategy under authority policy `{policy}`."
				),
				(None, true, _) => String::from(
					"Retry with a materially different implementation strategy; keep landing blocked until validation or review-policy evidence is restored.",
				),
				(None, false, true) => String::from(
					"Retry with a materially different implementation strategy; preserve enhanced evidence before review handoff or landing.",
				),
				(None, false, false) => String::from(
					"Retry with a materially different implementation strategy inside authority.",
				),
			}
		},
		"architecture_recovery_exhausted" => String::from(
			"Require a new accepted recovery strategy or architecture decision before retrying.",
		),
		"external_dependency_required" => String::from(
			"Resolve the dependency or Execution Program readiness blocker before retrying.",
		),
		"contract_boundary_required" => String::from(
			"Resolve the Decision Contract or Authority Envelope boundary before retrying.",
		),
		_ => String::from("Inspect the Architecture Recovery Packet before retrying."),
	}
}

fn operator_boundary_policy_decision_from_disposition(disposition: &str) -> &'static str {
	match disposition {
		"requires_human" | "insufficient_evidence" => "requires_human_decision",
		_ => "auto_continue",
	}
}

fn operator_boundary_policy_requires_enhanced_evidence(policy_decision: &str) -> bool {
	matches!(policy_decision, "requires_enhanced_evidence" | "block_landing")
}

fn operator_boundary_policy_blocks_landing(policy_decision: &str) -> bool {
	policy_decision == "block_landing"
}

fn operator_boundary_status_from_event(
	event: &PrivateExecutionEvent,
) -> Option<OperatorBoundaryStatus> {
	if event.event_type() != AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE {
		return None;
	}

	let payload = event.payload();
	let disposition = payload
		.get("final_disposition")
		.and_then(|final_disposition| final_disposition.get("disposition"))
		.and_then(Value::as_str)
		.or_else(|| payload.get("disposition").and_then(Value::as_str))?
		.to_owned();
	let reason = payload
		.get("final_disposition")
		.and_then(|final_disposition| final_disposition.get("reason"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let policy_decision = payload
		.get("policy_decision")
		.and_then(Value::as_str)
		.or_else(|| {
			payload.get("policy").and_then(|policy| policy.get("decision")).and_then(Value::as_str)
		})
		.map(str::to_owned)
		.unwrap_or_else(|| {
			operator_boundary_policy_decision_from_disposition(&disposition).to_owned()
		});
	let attempted_recovery_reason =
		payload.get("attempted_recovery_reason").and_then(Value::as_str).map(str::to_owned);
	let changed_surface_count =
		payload.get("changed_surfaces").and_then(Value::as_array).map_or(0, Vec::len);
	let improvement_signal_count =
		payload.get("improvement_signals").and_then(Value::as_array).map_or(0, Vec::len);
	let requires_enhanced_evidence = payload
		.get("policy")
		.and_then(|policy| policy.get("requires_enhanced_evidence"))
		.and_then(Value::as_bool)
		.unwrap_or_else(|| operator_boundary_policy_requires_enhanced_evidence(&policy_decision));
	let blocks_landing = payload
		.get("policy")
		.and_then(|policy| policy.get("blocks_landing"))
		.and_then(Value::as_bool)
		.unwrap_or_else(|| operator_boundary_policy_blocks_landing(&policy_decision));

	Some(OperatorBoundaryStatus {
		disposition,
		policy_decision,
		reason,
		attempted_recovery_reason,
		changed_surface_count,
		improvement_signal_count,
		requires_enhanced_evidence,
		blocks_landing,
	})
}

fn operator_loop_autonomy(
	boundary: Option<&OperatorBoundaryStatus>,
	architecture_recovery: Option<&OperatorArchitectureRecoveryStatus>,
	decision_request: Option<&OperatorAuthorityDecisionRequestStatus>,
) -> &'static str {
	if decision_request.is_some() {
		return "human_required";
	}
	if boundary.is_some_and(|boundary| boundary.policy_decision == "requires_human_decision") {
		return "human_required";
	}
	if architecture_recovery.is_some_and(|recovery| recovery.status != "active") {
		return "human_required";
	}

	"autonomous"
}

fn operator_loop_status_summary(
	review: Option<&OperatorReviewLoopStatus>,
	architecture_recovery: Option<&OperatorArchitectureRecoveryStatus>,
	boundary: Option<&OperatorBoundaryStatus>,
	decision_request: Option<&OperatorAuthorityDecisionRequestStatus>,
	autonomy: &str,
	lifecycle_summary: Option<&str>,
) -> String {
	if let Some(request) = decision_request {
		return format!("human-required boundary stop: {} on {}", request.reason, request.boundary);
	}
	if let Some(recovery) = architecture_recovery {
		return format!("architecture recovery {}: {}", recovery.status, recovery.reason_code);
	}
	if let Some(review) = review {
		if let Some(fingerprint) =
			review.checkpoint.as_ref().and_then(|checkpoint| checkpoint.stop_fingerprint.as_ref())
		{
			return format!(
				"review {}: {} stopped on fingerprint {}",
				review.phase, review.status, fingerprint
			);
		}

		return format!("review {}: {}", review.phase, review.status);
	}
	if let Some(boundary) = boundary {
		return format!("boundary check: {}", boundary.disposition);
	}
	if let Some(lifecycle_summary) = lifecycle_summary {
		return lifecycle_summary.to_owned();
	}

	format!("loop autonomy: {autonomy}")
}

fn operator_loop_status_next_action(
	review: Option<&OperatorReviewLoopStatus>,
	architecture_recovery: Option<&OperatorArchitectureRecoveryStatus>,
	boundary: Option<&OperatorBoundaryStatus>,
	decision_request: Option<&OperatorAuthorityDecisionRequestStatus>,
) -> Option<String> {
	if let Some(request) = decision_request {
		return Some(request.next_action.clone());
	}
	if let Some(recovery) = architecture_recovery {
		return Some(recovery.next_action.clone());
	}
	if let Some(boundary) = boundary {
		return match boundary.policy_decision.as_str() {
			"requires_human_decision" => {
				Some(String::from("Resolve the Authority Boundary Check before retrying the lane."))
			},
			"block_landing" => Some(String::from(
				"Continue recovery, but block landing until review or validation policy evidence is restored.",
			)),
			"requires_enhanced_evidence" => Some(String::from(
				"Continue recovery and preserve enhanced evidence before review handoff or landing.",
			)),
			_ => None,
		};
	}

	review.and_then(|review| {
		if review.status != "clean"
			&& let Some(route_next_action) = review
				.checkpoint
				.as_ref()
				.and_then(|checkpoint| checkpoint.route_next_action.clone())
		{
			return Some(route_next_action);
		}

		match review.status.as_str() {
			"clean" if review.phase == "handoff" => Some(String::from(
				"Push or update the PR and record review handoff for the clean current lane head.",
			)),
			"clean" if review.phase == "repair" => Some(String::from(
				"Record a fresh current-head handoff review checkpoint for the repaired lane head.",
			)),
			"pending" => Some(String::from(
				"Record the independent Decodex Review checkpoint for the current lane head.",
			)),
			"findings" => Some(String::from(
				"Repair validated review findings and record a fresh checkpoint.",
			)),
			"blocked" => {
				Some(String::from("Resolve the blocked Decodex Review before continuing."))
			},
			"needs_architecture_review" => {
				Some(String::from("Get architecture direction before continuing review repair."))
			},
			_ => None,
		}
	})
}

fn operator_run_control_capability(
	run: &ProjectRunStatus,
	app_server_state: &OperatorRunAppServerState,
) -> Option<OperatorRunControlCapability> {
	let channel = run.control_channel()?;

	Some(OperatorRunControlCapability {
		project_id: channel.project_id().to_owned(),
		issue_id: channel.issue_id().to_owned(),
		run_id: channel.run_id().to_owned(),
		attempt_number: channel.attempt_number(),
		thread_id: app_server_state.thread_id.clone(),
		turn_id: app_server_state.turn_id.clone(),
		transport: channel.transport().to_owned(),
		channel_path: channel.channel_path().display().to_string(),
		status: channel.status().to_owned(),
		published_at: channel.published_at().to_owned(),
		updated_at: channel.updated_at().to_owned(),
	})
}

fn load_operator_run_marker(
	run: &ProjectRunStatus,
) -> crate::prelude::Result<Option<RunActivityMarker>> {
	let marker = run.worktree_path().and_then(|worktree_path| {
		state::read_run_activity_marker_snapshot(worktree_path).unwrap_or_default()
	});

	Ok(marker.filter(|marker| {
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
	let run_event_progress_unix_epoch = run
		.last_event_type()
		.filter(|event_type| state::protocol_event_counts_as_work_progress(event_type))
		.and_then(|_| run.last_event_at_unix());
	let last_progress_unix_epoch = max_optional_i64(
		marker.and_then(RunActivityMarker::last_progress_unix_epoch),
		run_event_progress_unix_epoch,
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
		run.event_count() == 0 && run.last_event_type().is_none() && run.last_event_at().is_none()
			|| marker_protocol_summary_supersedes_run(run, marker);

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

fn marker_protocol_summary_supersedes_run(
	run: &ProjectRunStatus,
	marker: Option<&RunActivityMarker>,
) -> bool {
	let Some(marker) = marker else {
		return false;
	};

	if marker.last_event_type().is_none() {
		return false;
	}

	let Some(marker_event_at) = marker.last_protocol_activity_unix_epoch() else {
		return false;
	};

	run.last_event_at_unix().is_none_or(|run_event_at| {
		marker_event_at > run_event_at
			|| marker_event_at == run_event_at && marker.event_count() > run.event_count()
	})
}

fn operator_run_terminal_finalize_projection(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
) -> Option<OperatorTerminalFinalizeProjection> {
	let events = loop_evidence.private_events(run.issue_id(), run.run_id(), run.attempt_number());
	let path = events
		.iter()
		.rev()
		.find(|event| event.event_type() == "terminal_finalize")
		.and_then(|event| event.payload().get("path"))
		.and_then(Value::as_str)?;

	match path {
		"review_handoff" => Some(OperatorTerminalFinalizeProjection {
			status: "review_handoff_pending",
			phase: "terminal_pending",
			wait_reason: review_handoff_terminal_finalize_wait_reason(loop_evidence, run, events),
			current_operation: RUN_OPERATION_REVIEW_WRITEBACK,
		}),
		"review_repair" => Some(OperatorTerminalFinalizeProjection {
			status: "review_repair_pending",
			phase: "terminal_pending",
			wait_reason: review_repair_terminal_finalize_wait_reason(loop_evidence, run, events),
			current_operation: RUN_OPERATION_REVIEW_WRITEBACK,
		}),
		"closeout" => Some(OperatorTerminalFinalizeProjection {
			status: "closeout_pending",
			phase: "terminal_pending",
			wait_reason: "closeout_writeback",
			current_operation: RUN_OPERATION_REVIEW_WRITEBACK,
		}),
		"manual_attention" => Some(OperatorTerminalFinalizeProjection {
			status: "manual_attention_pending",
			phase: "terminal_pending",
			wait_reason: "manual_attention_writeback",
			current_operation: RUN_OPERATION_REVIEW_WRITEBACK,
		}),
		_ => None,
	}
}

fn review_handoff_terminal_finalize_wait_reason(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
	events: &[PrivateExecutionEvent],
) -> &'static str {
	let Some(intent) = events.iter().rev().find(|event| {
		let payload = event.payload();

		event.event_type() == "review_completion_intent"
			&& payload.get("path").and_then(Value::as_str) == Some("review_handoff")
			&& payload.get("mode").and_then(Value::as_str) == Some("handoff")
			&& payload.get("pr_url").and_then(Value::as_str).is_some()
			&& payload.get("pr_head_oid").and_then(Value::as_str).is_some()
			&& payload.get("worktree_path").and_then(Value::as_str).is_some()
	}) else {
		return "review_handoff_writeback";
	};
	let Some(branch) = intent.payload().get("branch").and_then(Value::as_str) else {
		return "review_handoff_writeback";
	};

	if loop_evidence.review_lifecycle_record(run.issue_id(), branch).is_none() {
		return "review_handoff_writeback_missing_lifecycle_marker";
	}

	"review_handoff_writeback"
}

fn review_repair_terminal_finalize_wait_reason(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
	events: &[PrivateExecutionEvent],
) -> &'static str {
	let Some(intent) = events.iter().rev().find(|event| {
		let payload = event.payload();

		event.event_type() == "review_completion_intent"
			&& payload.get("path").and_then(Value::as_str) == Some("review_repair")
			&& payload.get("mode").and_then(Value::as_str) == Some("repair")
			&& payload.get("pr_url").and_then(Value::as_str).is_some()
			&& payload.get("pr_head_ref").and_then(Value::as_str).is_some()
			&& payload.get("pr_head_oid").and_then(Value::as_str).is_some()
			&& payload.get("worktree_path").and_then(Value::as_str).is_some()
	}) else {
		return "review_repair_writeback";
	};
	let payload = intent.payload();
	let Some(branch) = payload.get("branch").and_then(Value::as_str) else {
		return "review_repair_writeback";
	};
	let Some(pr_url) = payload.get("pr_url").and_then(Value::as_str) else {
		return "review_repair_writeback";
	};
	let Some(pr_head_ref) = payload.get("pr_head_ref").and_then(Value::as_str) else {
		return "review_repair_writeback";
	};
	let Some(pr_head_oid) = payload.get("pr_head_oid").and_then(Value::as_str) else {
		return "review_repair_writeback";
	};
	let Some(lifecycle_record) = loop_evidence.review_lifecycle_record(run.issue_id(), branch)
	else {
		return "review_repair_writeback_missing_lifecycle_marker";
	};

	if lifecycle_record.pr_url() != pr_url
		|| lifecycle_record.pr_head_ref_name() != pr_head_ref
		|| lifecycle_record.pr_head_oid() != pr_head_oid
		|| lifecycle_record.head_sha() != pr_head_oid
	{
		return "review_repair_writeback_stale_lifecycle_marker";
	}

	"review_repair_writeback"
}

fn operator_run_continuation_recovery_status(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
) -> Option<OperatorContinuationRecoveryStatus> {
	let recovery_events = loop_evidence
		.private_events_for_issue(run.issue_id())
		.into_iter()
		.filter(|event| event.attempt_number() <= run.attempt_number())
		.filter_map(operator_continuation_recovery_event_status)
		.collect::<Vec<_>>();
	let latest = recovery_events.last()?.clone();
	let recovery_count = recovery_events
		.iter()
		.filter(|event| {
			event.source_phase == latest.source_phase
				&& event.source_error_class == latest.source_error_class
				&& event.state == "continuation_scheduled"
		})
		.count() as i64;
	let budget_exceeded = latest.state == "continuation_blocked"
		|| recovery_count > PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT;

	Some(OperatorContinuationRecoveryStatus {
		state: latest.state,
		source_phase: latest.source_phase,
		next_phase: latest.next_phase,
		source_error_class: latest.source_error_class,
		source_error_message: latest.source_error_message,
		recorded_at: latest.recorded_at,
		run_id: latest.run_id,
		attempt_number: latest.attempt_number,
		recovery_count,
		automatic_continuation_limit: PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT,
		budget_exceeded,
		next_action: if budget_exceeded {
			String::from("stop_auto_continuation_and_request_architecture_recovery")
		} else {
			String::from("monitor_continuation_recovery")
		},
	})
}

fn operator_continuation_recovery_event_status(
	event: &PrivateExecutionEvent,
) -> Option<OperatorContinuationRecoveryStatus> {
	let state = match event.event_type() {
		PHASE_GOAL_RECOVERY_EVENT_TYPE => "continuation_scheduled",
		PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE => "continuation_blocked",
		_ => return None,
	};
	let payload = event.payload();
	let event_payload = payload.get("payload").unwrap_or(payload);
	let source_phase = payload
		.get("phase")
		.and_then(Value::as_str)
		.or_else(|| event_payload.get("sourcePhase").and_then(Value::as_str))?
		.to_owned();
	let next_phase = event_payload.get("nextPhase")?.as_str()?.to_owned();
	let source_error_class = event_payload.get("sourceErrorClass")?.as_str()?.to_owned();
	let source_error_message =
		event_payload.get("sourceErrorMessage").and_then(Value::as_str).map(str::to_owned);

	Some(OperatorContinuationRecoveryStatus {
		state: String::from(state),
		source_phase,
		next_phase,
		source_error_class,
		source_error_message,
		recorded_at: event.recorded_at().to_owned(),
		run_id: event.run_id().to_owned(),
		attempt_number: event.attempt_number(),
		recovery_count: 0,
		automatic_continuation_limit: PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT,
		budget_exceeded: false,
		next_action: String::new(),
	})
}

fn operator_run_visible_status(
	attempt_status: &str,
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
	_marker_current_operation: Option<&str>,
) -> String {
	if attempt_status == "starting"
		&& operator_run_has_app_server_execution_evidence(
			app_server_state,
			protocol_summary,
			timing,
		) {
		return String::from("running");
	}

	attempt_status.to_owned()
}

fn operator_run_status_projection_reason(
	attempt_status: &str,
	visible_status: &str,
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
	_marker_current_operation: Option<&str>,
) -> Option<String> {
	if attempt_status == visible_status || visible_status != "running" {
		return None;
	}

	let projection_kind = if attempt_status == "starting" {
		"starting_attempt"
	} else {
		return None;
	};

	operator_run_live_evidence_source(app_server_state, protocol_summary, timing)
		.map(|source| format!("{projection_kind}_promoted_by_{source}"))
}

fn operator_run_live_evidence_source(
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
) -> Option<&'static str> {
	if timing.process_alive == Some(true) {
		return Some("process_alive");
	}
	if matches!(app_server_state.thread_status.as_deref(), Some("active")) {
		return Some("thread_active");
	}
	if !app_server_state.thread_active_flags.is_empty() {
		return Some("thread_active_flags");
	}
	if operator_run_has_recent_protocol_execution_evidence(protocol_summary, timing) {
		return Some("recent_protocol_activity");
	}
	if app_server_state.effective_model.is_some()
		|| app_server_state.effective_model_provider.is_some()
		|| protocol_summary.event_count > 0
		|| protocol_summary.last_event_type.is_some()
	{
		return Some("app_server_metadata");
	}
	if timing.protocol_idle_for_seconds.is_some() {
		return Some("protocol_timing");
	}

	None
}

fn operator_run_has_recent_protocol_execution_evidence(
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
) -> bool {
	operator_protocol_event_counts_as_live_execution(protocol_summary.last_event_type.as_deref())
		&& timing.protocol_idle_for_seconds.is_some_and(|idle_for| {
			u64::try_from(idle_for)
				.is_ok_and(|idle_for| idle_for < RUN_LEASE_IDLE_TIMEOUT.as_secs())
		})
}

fn operator_protocol_event_counts_as_live_execution(event_type: Option<&str>) -> bool {
	let Some(event_type) = event_type else {
		return false;
	};

	state::protocol_event_counts_as_work_progress(event_type)
		&& !matches!(event_type.to_ascii_lowercase().as_str(), "thread/archive" | "turn/completed")
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
				.is_ok_and(|idle_for| idle_for < RUN_LEASE_IDLE_TIMEOUT.as_secs())
		})
}

fn operator_run_queue_lease_state(run_lease: bool) -> String {
	if run_lease { String::from("held") } else { String::from("not_held") }
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
		if process_liveness_reason_is_identity_mismatch(timing.process_liveness_reason.as_deref()) {
			return String::from(EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH);
		}

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

fn process_liveness_reason_is_identity_mismatch(reason: Option<&str>) -> bool {
	matches!(reason, Some("host_boot_id_mismatch" | "process_start_identity_mismatch"))
}

fn operator_run_child_agent_activity(
	marker: Option<&RunActivityMarker>,
	stored_summary: Option<&ChildAgentActivitySummary>,
	now_unix_epoch: i64,
) -> Option<ChildAgentActivitySummary> {
	if let Some(marker) = marker
		&& let Some(summary) = marker.child_agent_activity()
	{
		return Some(summary.clone().live_projection(now_unix_epoch));
	}

	stored_summary.cloned().map(ChildAgentActivitySummary::sealed_durable)
}

fn operator_run_protocol_activity(
	marker: Option<&RunActivityMarker>,
	stored_summary: Option<&ProtocolActivitySummary>,
	app_server_state: &OperatorRunAppServerState,
	child_agent_activity: Option<&ChildAgentActivitySummary>,
	protocol_idle_for_seconds: Option<i64>,
	is_running: bool,
) -> Option<ProtocolActivitySummary> {
	let mut summary = marker
		.and_then(RunActivityMarker::protocol_activity)
		.or(stored_summary)
		.cloned()
		.unwrap_or_default();

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
			u64::try_from(idle_for)
				.is_ok_and(|idle_for| idle_for < RUN_LEASE_IDLE_TIMEOUT.as_secs())
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

	sanitize_operator_protocol_activity_summary(&mut summary);

	Some(summary)
}

fn sanitize_operator_protocol_activity_summary(summary: &mut ProtocolActivitySummary) {
	for event in &mut summary.recent_events {
		if let Some(detail) = event.detail.as_deref()
			&& !operator_protocol_activity_detail_is_public(detail)
		{
			event.detail = Some(String::from("redacted_sensitive_detail"));
		}
	}
}

fn operator_protocol_activity_detail_is_public(detail: &str) -> bool {
	public_text::validate_public_text_field("protocol_activity.detail", detail).is_ok()
		&& !contains_protocol_activity_host_path_shape(detail)
		&& !contains_protocol_activity_secret_shape(detail)
}

fn contains_protocol_activity_host_path_shape(detail: &str) -> bool {
	let mut previous = None;
	let mut chars = detail.char_indices().peekable();

	while let Some((index, character)) = chars.next() {
		if character != '/' {
			previous = Some(character);

			continue;
		}
		if previous == Some(':') || previous == Some('/') {
			previous = Some(character);

			continue;
		}

		let path_boundary = index == 0
			|| previous.is_some_and(|previous| {
				previous.is_whitespace()
					|| matches!(previous, '"' | '\'' | '`' | '(' | '[' | '{' | '=')
			});
		let path_component = chars
			.peek()
			.map(|(_, next)| next.is_ascii_alphanumeric() || matches!(next, '.' | '_' | '-'))
			.unwrap_or(false);

		if path_boundary && path_component {
			return true;
		}

		previous = Some(character);
	}

	false
}

fn contains_protocol_activity_secret_shape(detail: &str) -> bool {
	detail.split(protocol_activity_token_separator).any(|token| {
		let normalized = token.to_ascii_lowercase();

		normalized.starts_with("ghp_")
			|| normalized.starts_with("github_pat_")
			|| is_high_entropy_protocol_activity_token(token)
	})
}

fn protocol_activity_token_separator(character: char) -> bool {
	!(character.is_ascii_alphanumeric() || character == '_' || character == '-')
}

fn is_high_entropy_protocol_activity_token(token: &str) -> bool {
	if token.len() < 24 {
		return false;
	}

	let mut has_uppercase = false;
	let mut has_lowercase = false;
	let mut has_digit = false;
	let mut alphanumeric_count = 0;

	for character in token.chars() {
		if !character.is_ascii_alphanumeric() {
			continue;
		}

		alphanumeric_count += 1;
		has_uppercase |= character.is_ascii_uppercase();
		has_lowercase |= character.is_ascii_lowercase();
		has_digit |= character.is_ascii_digit();
	}

	alphanumeric_count >= 24 && has_uppercase && has_lowercase && has_digit
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

fn format_optional_i64(value: Option<i64>) -> String {
	value.map_or_else(|| String::from("none"), |value| value.to_string())
}

fn classify_operator_run_operation(phase: &str, marker_current_operation: Option<&str>) -> String {
	match phase {
		"retry_backoff" | "waiting_continuation" => String::from(RUN_OPERATION_WAITING_EXTERNAL),
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
	idle_timeout: Duration,
) -> bool {
	if phase != "executing" {
		return false;
	}

	last_progress_unix_epoch
		.and_then(|last_progress| observed_idle_duration(last_progress, now_unix_epoch))
		.is_some_and(|idle_for| {
			idle_for >= suspected_operator_run_stall_threshold(idle_timeout)
				&& idle_for < idle_timeout
		})
}

fn suspected_operator_run_stall_threshold(idle_timeout: Duration) -> Duration {
	Duration::from_secs((idle_timeout.as_secs() / 2).max(1))
}

fn operator_run_progress_diagnostic(
	phase: &str,
	timing: &OperatorRunTiming,
	protocol_activity: Option<&ProtocolActivitySummary>,
	private_events: &[PrivateExecutionEvent],
	now_unix_epoch: i64,
	idle_timeout: Duration,
) -> Option<String> {
	if let Some(repo_gate_diagnostic) =
		operator_latest_repo_gate_failure_progress_diagnostic(private_events)
	{
		return Some(repo_gate_diagnostic);
	}

	if phase != "executing" {
		return None;
	}

	let protocol_activity = protocol_activity?;

	if protocol_activity.waiting_reason.as_deref() != Some("model_execution")
		|| !protocol_activity_is_non_work_only(protocol_activity)
	{
		return None;
	}

	let protocol_idle = timing
		.last_protocol_activity_unix_epoch
		.and_then(|last_protocol| observed_idle_duration(last_protocol, now_unix_epoch))?;

	if protocol_idle >= idle_timeout {
		return None;
	}

	let progress_is_stale = timing
		.last_progress_unix_epoch
		.and_then(|last_progress| observed_idle_duration(last_progress, now_unix_epoch))
		.is_none_or(|idle_for| idle_for >= suspected_operator_run_stall_threshold(idle_timeout));

	progress_is_stale.then(|| String::from("protocol_only_activity"))
}

fn operator_latest_repo_gate_failure_progress_diagnostic(
	private_events: &[PrivateExecutionEvent],
) -> Option<String> {
	private_events
		.iter()
		.rev()
		.find(|event| event.event_type() == "phase_goal_transition")
		.and_then(operator_repo_gate_failure_progress_diagnostic)
}

fn operator_repo_gate_failure_progress_diagnostic(event: &PrivateExecutionEvent) -> Option<String> {
	if event.event_type() != "phase_goal_transition" {
		return None;
	}

	let transition_payload = event.payload().get("payload")?;
	let error_class = transition_payload.get("errorClass")?.as_str()?;

	if !error_class.starts_with("repo_gate_") {
		return None;
	}

	let failed_command = transition_payload
		.get("repoGateFailure")
		.and_then(|diagnostic| diagnostic.get("failed_command"))
		.and_then(Value::as_str)
		.unwrap_or("inspect_private_evidence");

	Some(format!("repo_gate_failure:{error_class}; failed_command:{failed_command}"))
}

fn protocol_activity_is_non_work_only(protocol_activity: &ProtocolActivitySummary) -> bool {
	!protocol_activity.recent_events.is_empty()
		&& protocol_activity
			.recent_events
			.iter()
			.all(|event| !state::protocol_event_counts_as_work_progress(&event.event_type))
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

fn operator_history_lanes(
	current_lanes: &[OperatorRunStatus],
	recent_runs: &[OperatorRunStatus],
) -> Vec<OperatorHistoryLaneStatus> {
	let current_lane_run_ids =
		current_lanes.iter().map(|run| run.run_id.as_str()).collect::<HashSet<_>>();
	let current_lane_issue_ids =
		current_lanes.iter().map(|run| run.issue_id.as_str()).collect::<HashSet<_>>();
	let mut lane_indexes = HashMap::new();
	let mut lanes = Vec::new();

	for run in recent_runs {
		if current_lane_run_ids.contains(run.run_id.as_str())
			|| current_lane_issue_ids.contains(run.issue_id.as_str())
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

			lane.lifecycle_metrics = operator_lane_lifecycle_metrics(&lane.attempts);

			continue;
		}

		lane_indexes.insert(group_key, lanes.len());

		let attempts = vec![run.clone()];
		let lifecycle_metrics = operator_lane_lifecycle_metrics(&attempts);

		lanes.push(OperatorHistoryLaneStatus {
			project_id: run.project_id.clone(),
			issue_id: run.issue_id.clone(),
			issue_identifier: run.issue_identifier.clone(),
			title: run.title.clone(),
			author: run.author.clone(),
			issue_state: None,
			active_label_present: None,
			needs_attention_label_present: None,
			issue_key: operator_run_issue_key(run),
			attempt_count: 1,
			ledger_outcome: not_loaded_history_ledger_outcome(),
			lifecycle_metrics,
			latest_run: run.clone(),
			attempts,
		});
	}

	lanes
}

fn hydrate_history_lane_from_run(lane: &mut OperatorHistoryLaneStatus, run: &OperatorRunStatus) {
	if lane.issue_identifier.is_none()
		&& let Some(issue_identifier) =
			run.issue_identifier.as_ref().filter(|value| !value.trim().is_empty())
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

fn hydrate_current_lane_lifecycle_metrics(
	project: &ServiceConfig,
	state_store: &StateStore,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	project_display_name: &str,
	current_lanes: &mut [OperatorRunStatus],
	recent_runs: &[OperatorRunStatus],
	now_unix_epoch: i64,
) -> crate::prelude::Result<()> {
	for current_lane in current_lanes {
		let attempts = current_lane_lifecycle_attempts(
			project,
			state_store,
			loop_evidence,
			project_display_name,
			current_lane,
			recent_runs,
			now_unix_epoch,
		)?;

		current_lane.lifecycle_metrics = operator_lane_lifecycle_metrics(&attempts);
	}

	Ok(())
}

fn current_lane_lifecycle_attempts(
	project: &ServiceConfig,
	state_store: &StateStore,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	project_display_name: &str,
	current_lane: &OperatorRunStatus,
	recent_runs: &[OperatorRunStatus],
	now_unix_epoch: i64,
) -> crate::prelude::Result<Vec<OperatorRunStatus>> {
	let issue_runs =
		state_store.list_project_issue_runs(project.service_id(), &current_lane.issue_id)?;
	let mut attempts = issue_runs
		.into_iter()
		.map(|run| {
			operator_run_status(project, loop_evidence, project_display_name, run, now_unix_epoch)
		})
		.collect::<crate::prelude::Result<Vec<_>>>()?;

	if attempts.is_empty() {
		let group_key = operator_run_group_key(current_lane);

		attempts.extend(
			recent_runs.iter().filter(|run| operator_run_group_key(run) == group_key).cloned(),
		);
	}

	let current_lane_snapshot = operator_run_current_lane_snapshot_attempt(current_lane);

	if let Some(attempt) = attempts.iter_mut().find(|run| run.run_id == current_lane.run_id) {
		*attempt = current_lane_snapshot;
	} else {
		attempts.push(current_lane_snapshot);
	}

	Ok(attempts)
}

fn operator_run_current_lane_snapshot_attempt(run: &OperatorRunStatus) -> OperatorRunStatus {
	let mut snapshot = run.clone();
	let mut evidence = std::collections::BTreeSet::<String>::new();

	evidence.insert(String::from("current_lane_snapshot"));
	evidence.extend(snapshot.lifecycle_evidence.iter().cloned());

	snapshot.lifecycle_source = String::from("current_snapshot");
	snapshot.lifecycle_evidence = evidence.into_iter().collect();

	snapshot
}

fn operator_lane_lifecycle_metrics(attempts: &[OperatorRunStatus]) -> OperatorLaneLifecycleMetrics {
	let mut metrics = operator_lane_lifecycle_totals(attempts.iter());

	metrics.phases = operator_lane_lifecycle_phase_metrics(attempts);

	metrics
}

fn operator_lane_lifecycle_totals<'a>(
	runs: impl IntoIterator<Item = &'a OperatorRunStatus>,
) -> OperatorLaneLifecycleMetrics {
	let mut bucket_totals = HashMap::<String, ChildAgentActivityBucket>::new();
	let mut warning_set = HashSet::<String>::new();
	let mut run_ids = HashSet::<String>::new();
	let mut metrics = OperatorLaneLifecycleMetrics::default();

	for run in runs {
		metrics.attempt_count += 1;

		run_ids.insert(run.run_id.clone());

		match run.lifecycle_source.as_str() {
			"recorded" => metrics.recorded_attempt_count += 1,
			"recovered" => metrics.recovered_attempt_count += 1,
			"current_snapshot" => metrics.current_snapshot_attempt_count += 1,
			_ => {},
		}

		metrics.recovery_gaps.extend(run.lifecycle_gaps.iter().cloned());
		metrics.attempt_evidence.push(operator_lane_lifecycle_attempt_evidence(run));

		metrics.protocol_event_count =
			metrics.protocol_event_count.saturating_add(run.event_count.max(0));

		let Some(summary) = run.child_agent_activity.as_ref() else {
			continue;
		};

		metrics.captured_attempt_count += 1;
		metrics.child_event_count =
			metrics.child_event_count.saturating_add(summary.event_count.max(0));
		metrics.wall_seconds = metrics.wall_seconds.saturating_add(summary.wall_seconds.max(0));
		metrics.tool_call_count =
			metrics.tool_call_count.saturating_add(summary.tool_call_count.max(0));
		metrics.input_tokens_current =
			max_optional_i64(metrics.input_tokens_current, summary.input_tokens_current);
		metrics.input_tokens_peak =
			max_optional_i64(metrics.input_tokens_peak, summary.input_tokens_max);
		metrics.input_tokens_cumulative =
			metrics.input_tokens_cumulative.saturating_add(summary.input_tokens_cumulative.max(0));
		metrics.output_tokens_cumulative = metrics
			.output_tokens_cumulative
			.saturating_add(summary.output_tokens_cumulative.max(0));

		if summary.largest_tool_output_bytes.is_some_and(|bytes| {
			metrics.largest_tool_output_bytes.is_none_or(|current| bytes > current)
		}) {
			metrics.largest_tool_output_bytes = summary.largest_tool_output_bytes;
			metrics.largest_tool_output_tool = summary.largest_tool_output_tool.clone();
		}

		for warning in &summary.large_output_warnings {
			if !warning.trim().is_empty() {
				warning_set.insert(warning.clone());
			}
		}
		for bucket in &summary.buckets {
			let total = bucket_totals.entry(bucket.name.clone()).or_insert_with(|| {
				ChildAgentActivityBucket {
					name: bucket.name.clone(),
					..ChildAgentActivityBucket::default()
				}
			});

			total.wall_seconds = total.wall_seconds.saturating_add(bucket.wall_seconds.max(0));
			total.event_count = total.event_count.saturating_add(bucket.event_count.max(0));
			total.tool_call_count =
				total.tool_call_count.saturating_add(bucket.tool_call_count.max(0));
			total.input_tokens = total.input_tokens.saturating_add(bucket.input_tokens.max(0));
			total.output_tokens = total.output_tokens.saturating_add(bucket.output_tokens.max(0));
			total.output_bytes = total.output_bytes.saturating_add(bucket.output_bytes.max(0));
		}
	}

	metrics.missing_attempt_count =
		metrics.attempt_count.saturating_sub(metrics.captured_attempt_count);
	metrics.run_count = run_ids.len();
	metrics.large_output_warnings = warning_set.into_iter().collect();

	metrics.recovery_gaps.sort();
	metrics.recovery_gaps.dedup();
	metrics.attempt_evidence.sort_by(|left, right| {
		left.attempt_number.cmp(&right.attempt_number).then_with(|| left.run_id.cmp(&right.run_id))
	});
	metrics.large_output_warnings.sort();

	metrics.buckets = bucket_totals.into_values().collect();

	metrics.buckets.sort_by(|left, right| {
		right
			.wall_seconds
			.cmp(&left.wall_seconds)
			.then_with(|| right.event_count.cmp(&left.event_count))
			.then_with(|| left.name.cmp(&right.name))
	});

	metrics
}

fn operator_lane_lifecycle_phase_metrics(
	attempts: &[OperatorRunStatus],
) -> Vec<OperatorLaneLifecyclePhaseMetrics> {
	let mut groups = HashMap::<String, (String, u8, Vec<&OperatorRunStatus>)>::new();

	for run in attempts {
		let phase = operator_run_lifecycle_metric_phase(run);
		let entry = groups
			.entry(phase.key.to_owned())
			.or_insert_with(|| (phase.label.to_owned(), phase.rank, Vec::new()));

		entry.2.push(run);
	}

	let mut phases = groups
		.into_iter()
		.map(|(phase, (label, rank, runs))| {
			let totals = operator_lane_lifecycle_totals(runs);

			(
				rank,
				OperatorLaneLifecyclePhaseMetrics {
					phase,
					label,
					attempt_count: totals.attempt_count,
					run_count: totals.run_count,
					recorded_attempt_count: totals.recorded_attempt_count,
					recovered_attempt_count: totals.recovered_attempt_count,
					current_snapshot_attempt_count: totals.current_snapshot_attempt_count,
					captured_attempt_count: totals.captured_attempt_count,
					missing_attempt_count: totals.missing_attempt_count,
					protocol_event_count: totals.protocol_event_count,
					child_event_count: totals.child_event_count,
					wall_seconds: totals.wall_seconds,
					tool_call_count: totals.tool_call_count,
					input_tokens_current: totals.input_tokens_current,
					input_tokens_peak: totals.input_tokens_peak,
					input_tokens_cumulative: totals.input_tokens_cumulative,
					output_tokens_cumulative: totals.output_tokens_cumulative,
					largest_tool_output_bytes: totals.largest_tool_output_bytes,
					largest_tool_output_tool: totals.largest_tool_output_tool,
					large_output_warnings: totals.large_output_warnings,
					buckets: totals.buckets,
					attempt_evidence: totals.attempt_evidence,
					recovery_gaps: totals.recovery_gaps,
				},
			)
		})
		.collect::<Vec<_>>();

	phases.sort_by(|(left_rank, left), (right_rank, right)| {
		left_rank.cmp(right_rank).then_with(|| left.phase.cmp(&right.phase))
	});

	phases.into_iter().map(|(_rank, phase)| phase).collect()
}

fn operator_run_lifecycle_metric_phase(run: &OperatorRunStatus) -> OperatorLifecycleMetricPhase {
	if matches!(
		run.status.as_str(),
		"cleanup_complete" | "closeout" | "closeout_pending" | "landed"
	) {
		return operator_lifecycle_metric_phase("closeout", "Closeout", 30);
	}
	if matches!(
		run.status.as_str(),
		"manual_attention" | "manual_attention_pending" | "needs_attention" | "terminal_failure"
	) || run.phase == "needs_attention"
	{
		return operator_lifecycle_metric_phase("manual_attention", "Manual attention", 40);
	}

	if let Some(review) = run
		.loop_status
		.as_ref()
		.and_then(|status| status.review.as_ref())
		.filter(|review| review.checkpoint.is_some() || review.status != "pending")
	{
		return match review.phase.as_str() {
			"repair" => operator_lifecycle_metric_phase("review_repair", "Review repair", 20),
			_ => operator_lifecycle_metric_phase("review", "Review", 10),
		};
	}

	if run.status == "review_repair_pending" {
		return operator_lifecycle_metric_phase("review_repair", "Review repair", 20);
	}
	if run.status == "review_handoff_pending"
		|| run.current_operation == RUN_OPERATION_REVIEW_WRITEBACK
	{
		return operator_lifecycle_metric_phase("review", "Review", 10);
	}

	operator_lifecycle_metric_phase("development", "Development", 0)
}

fn operator_lane_lifecycle_attempt_evidence(
	run: &OperatorRunStatus,
) -> OperatorLaneLifecycleAttemptEvidence {
	let phase = operator_run_lifecycle_metric_phase(run);
	let child_event_count =
		run.child_agent_activity.as_ref().map(|summary| summary.event_count.max(0)).unwrap_or(0);

	OperatorLaneLifecycleAttemptEvidence {
		run_id: run.run_id.clone(),
		issue_id: run.issue_id.clone(),
		attempt_number: run.attempt_number,
		status: run.status.clone(),
		phase: phase.key.to_owned(),
		source: run.lifecycle_source.clone(),
		evidence: run.lifecycle_evidence.clone(),
		gaps: run.lifecycle_gaps.clone(),
		protocol_event_count: run.event_count.max(0),
		child_event_count,
		updated_at: run.updated_at.clone(),
	}
}

fn operator_lifecycle_metric_phase(
	key: &'static str,
	label: &'static str,
	rank: u8,
) -> OperatorLifecycleMetricPhase {
	OperatorLifecycleMetricPhase { key, label, rank }
}

fn operator_run_group_key(run: &OperatorRunStatus) -> String {
	let issue_id = run.issue_id.trim();

	if !issue_id.is_empty() && !issue_id.eq_ignore_ascii_case("unknown") {
		return issue_id.to_ascii_uppercase();
	}

	operator_run_issue_key(run)
}

fn operator_run_issue_key(run: &OperatorRunStatus) -> String {
	if let Some(issue_identifier) = run
		.issue_identifier
		.as_ref()
		.filter(|value| !value.trim().is_empty() && !value.eq_ignore_ascii_case("unknown"))
	{
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

	if issue_id.is_empty() { String::from("unknown") } else { issue_id.to_owned() }
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
