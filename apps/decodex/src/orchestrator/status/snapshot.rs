use super::{
	AccountActivityMode, GhPullRequestReviewStateInspector, HashMap, HashSet, Instant,
	IssueTracker, LiveOperatorStatusObserverContext, LiveOperatorStatusSnapshotOptions,
	OffsetDateTime, OperatorCodexAccountControlStatus, OperatorConnectorBackoffStatus,
	OperatorProjectStatus, OperatorRunStatus, OperatorStatusSnapshot, Path,
	ProjectLoopEvidenceSnapshot, ProjectRunStatus, RunIssueMetadataHydration, ServiceConfig,
	StateStore, TrackerConnectorBackoff, TrackerObserverOutcome, WorkflowDocument,
	active_stored_tracker_backoff_status, apply_missing_issue_ghost_lane_projection,
	apply_operator_lane_terminal_projection, apply_terminal_history_ledger_outcome_to_run,
	build_degraded_post_review_lane_statuses,
	build_post_review_lane_statuses_and_hydrate_worktrees, build_queued_candidate_statuses,
	codex_account_activity_summaries, current_lane_has_authoritative_live_owner,
	current_lane_terminal_projection_from_local_ledger, history_ledger_outcome_is_terminal,
	hydrate_current_lane_lifecycle_metrics, hydrate_history_lanes_from_linear_ledger,
	hydrate_history_lanes_from_local_ledger, hydrate_operator_run_rows_from_tracker,
	hydrate_post_review_lane_current_lane_shadowing, local_history_ledger_records,
	operator_execution_program_statuses, operator_github_cli_authority, operator_history_lanes,
	operator_history_ledger_outcome, operator_project_display_name,
	operator_run_counts_as_current_lane, operator_run_group_key, operator_run_has_live_execution,
	operator_run_issue_identifier_from_fields, operator_run_status, operator_status_worktrees,
	persist_tracker_backoff_state, refresh_operator_project_summary, refresh_worktree_ownership,
	runtime, stale_terminal_local_issue_ids, suppress_terminal_attention_queue_echoes,
	tracker_connector_backoff,
};

pub(in crate::orchestrator) fn build_operator_status_snapshot(
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

pub(in crate::orchestrator) fn build_operator_status_snapshot_with_account_mode(
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

pub(in crate::orchestrator) fn build_lane_inspect_operator_runs(
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

pub(in crate::orchestrator) fn apply_terminal_ledger_projection_to_lane_inspect_run(
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

pub(in crate::orchestrator) fn project_run_status_issue_matches(
	run: &ProjectRunStatus,
	issue: &str,
) -> bool {
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

pub(in crate::orchestrator) fn operator_current_lane_statuses(
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

pub(in crate::orchestrator) fn operator_latest_attempt_by_issue_key<'a>(
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

pub(in crate::orchestrator) fn operator_run_is_superseded_by_newer_attempt(
	run: &OperatorRunStatus,
	latest_attempt_by_issue_key: &HashMap<String, i64>,
) -> bool {
	latest_attempt_by_issue_key
		.get(&operator_run_group_key(run))
		.is_some_and(|latest_attempt| run.attempt_number < *latest_attempt)
}

pub(in crate::orchestrator) fn global_codex_account_control_status()
-> OperatorCodexAccountControlStatus {
	let account_selector = runtime::global_fixed_account_selector().ok().flatten();
	let mode = if account_selector.is_some() { "fixed" } else { "balanced" };

	OperatorCodexAccountControlStatus { mode: String::from(mode), account_selector }
}

pub(in crate::orchestrator) fn build_live_operator_status_snapshot<T>(
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

pub(in crate::orchestrator) fn build_status_command_operator_status_snapshot<T>(
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

pub(in crate::orchestrator) fn build_control_plane_operator_status_snapshot<T>(
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

pub(in crate::orchestrator) fn build_live_operator_status_snapshot_with_history_ledger<T>(
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

pub(in crate::orchestrator) fn hydrate_live_operator_external_observers<T>(
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

pub(in crate::orchestrator) fn pause_operator_snapshot_for_stored_tracker_backoff<T>(
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

pub(in crate::orchestrator) fn apply_tracker_observer_outcome(
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

pub(in crate::orchestrator) fn pause_operator_snapshot_for_tracker_backoff(
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

pub(in crate::orchestrator) fn hydrate_queued_candidate_status_observer<T>(
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

pub(in crate::orchestrator) fn hydrate_post_review_lane_status_observer<T>(
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

pub(in crate::orchestrator) fn add_tracker_backoff_to_operator_snapshot(
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

pub(in crate::orchestrator) fn add_operator_snapshot_warning(
	snapshot: &mut OperatorStatusSnapshot,
	warning: &str,
) {
	if !snapshot.warnings.iter().any(|existing| existing == warning) {
		snapshot.warnings.push(warning.to_owned());
	}
}
