use github::PullRequestMergeViewResponse;
use records::LinearExecutionEventRecord;
use state::{
	ProjectLoopEvidenceSnapshot, ProtocolActivityEventSummary, ReviewCheckpointArtifactLookup,
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
