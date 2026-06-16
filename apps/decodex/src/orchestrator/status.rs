#[cfg(target_os = "macos")]
use std::mem;
#[cfg(target_os = "macos")]
use std::mem::MaybeUninit;

use records::LinearExecutionEventRecord;
#[cfg(target_os = "macos")]
use libc::PROC_PIDTBSDINFO;
#[cfg(target_os = "macos")]
use libc::SZOMB;
#[cfg(target_os = "macos")]
use libc::c_void;
#[cfg(target_os = "macos")]
use libc::proc_bsdinfo;
use github::GhCommandResolution;
use github::PullRequestMergeViewResponse;
use state::WORKTREE_PROVENANCE_FILESYSTEM_SCAN;
use state::WORKTREE_PROVENANCE_GIT_HYGIENE_SCAN;
use state::WORKTREE_PROVENANCE_LEGACY_UNKNOWN;
use state::ProjectLoopEvidenceSnapshot;

use crate::pull_request::{self, PullRequestLandingGateView};
use crate::worktree;
use crate::worktree::MergedWorktreeCleanupDebt;

const QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT: &str = "linear_active_label_present";
const ATTENTION_ERROR_EVIDENCE_MISSING: &str = "evidence_missing";
const EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH: &str = "process_identity_mismatch";

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
}

#[derive(Clone, Copy)]
enum AccountActivityMode {
	Probe,
	Snapshot,
}

#[derive(Clone, Copy)]
enum RunIssueMetadataHydration {
	AllRows,
	ActiveRowsOnly,
}

enum TrackerObserverOutcome {
	Ok,
	Unavailable,
	RateLimited(TrackerConnectorBackoff),
}

#[derive(Clone, Copy)]
struct LiveOperatorStatusSnapshotOptions {
	hydrate_history_ledger: bool,
	run_issue_metadata_hydration: RunIssueMetadataHydration,
	account_activity_mode: AccountActivityMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PostReviewReadbackDegradation<'a> {
	reason: &'a str,
	root_cause: PullRequestReadbackRootCause,
	pr_url: &'a str,
	pr_head_sha: &'a str,
}
impl<'a> PostReviewReadbackDegradation<'a> {
	fn tracker_issue_from_handoff(review_handoff: &'a ReviewHandoffMarker) -> Self {
		Self {
			reason: "tracker_issue_readback_degraded",
			root_cause: PullRequestReadbackRootCause::TrackerIssueReadbackFailed,
			pr_url: review_handoff.pr_url(),
			pr_head_sha: review_handoff.pr_head_oid(),
		}
	}

	fn pull_request_state_from_handoff(
		review_handoff: &'a ReviewHandoffMarker,
		root_cause: PullRequestReadbackRootCause,
	) -> Self {
		Self {
			reason: "pull_request_state_read_failed",
			root_cause,
			pr_url: review_handoff.pr_url(),
			pr_head_sha: review_handoff.pr_head_oid(),
		}
	}

	fn wait_for_review_classification(
		self,
		review_state: Option<PullRequestReviewState>,
	) -> PostReviewLaneClassification {
		let (
			pr_head_sha,
			pr_state,
			review_decision,
			mergeable,
			check_state,
			unresolved_review_threads,
		) = match review_state {
			Some(review_state) => (
				Some(review_state.head_ref_oid),
				Some(review_state.state),
				review_state.review_decision,
				Some(review_state.mergeable),
				review_state.status_check_rollup_state,
				Some(review_state.unresolved_review_threads),
			),
			None => (Some(self.pr_head_sha.to_owned()), None, None, None, None, None),
		};

		PostReviewLaneClassification {
			decision: PostReviewLaneDecision::WaitForReview,
			reason: self.reason.to_owned(),
			pr_url: Some(self.pr_url.to_owned()),
			pr_head_sha,
			pr_state,
			review_decision,
			mergeable,
			check_state,
			unresolved_review_threads,
			readback_warning: Some(self.reason.to_owned()),
			readback_root_cause: Some(self.root_cause.as_str().to_owned()),
		}
	}
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OperatorTerminalFinalizeProjection {
	status: &'static str,
	phase: &'static str,
	wait_reason: &'static str,
	current_operation: &'static str,
}

struct OperatorRunLifecycleProjection {
	status: String,
	status_projection_reason: Option<String>,
	phase: String,
	wait_reason: Option<String>,
	current_operation: String,
	suspected_stall: bool,
	execution_liveness: String,
	active_lease: bool,
	retry_kind: Option<String>,
	retry_ready_at_unix_epoch: Option<i64>,
}

struct LiveOperatorStatusObserverContext<'a, T> {
	tracker: &'a T,
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	review_state_inspector: &'a GhPullRequestReviewStateInspector,
	hydrate_history_ledger: bool,
	run_issue_metadata_hydration: RunIssueMetadataHydration,
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
	issue_state: Option<String>,
	active_label_present: Option<bool>,
	needs_attention_label_present: Option<bool>,
}

struct WorktreeOwnership {
	kind: &'static str,
	reason: String,
	next_action: Option<String>,
	audit_required: bool,
}

struct OperatorLifecycleMetricPhase {
	key: &'static str,
	label: &'static str,
	rank: u8,
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
			operator_run_status(
				project,
				&loop_evidence,
				&project_display_name,
				run,
				now_unix_epoch,
			)
		})
		.collect::<crate::prelude::Result<Vec<_>>>()?;
	let active_runs = operator_active_run_statuses(
		project,
		&loop_evidence,
		&project_display_name,
		leased_runs,
		&recent_runs,
		now_unix_epoch,
	)?;
	let history_lanes = operator_history_lanes(&active_runs, &recent_runs);
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
			active_run_count: active_runs.len(),
			running_lane_count: active_runs.len(),
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
		active_runs,
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
	let (active_runs, recent_runs) = state_store.list_project_runs(project.service_id(), limit)?;
	let loop_evidence = state_store.project_loop_evidence_snapshot(project.service_id())?;
	let project_display_name = operator_project_display_name(project);
	let mut seen_run_ids = HashSet::new();
	let mut runs = Vec::new();

	for run in active_runs.into_iter().chain(recent_runs) {
		if !seen_run_ids.insert(run.run_id().to_owned()) {
			continue;
		}
		if !project_run_status_issue_matches(&run, issue) {
			continue;
		}
		if run_id.is_some_and(|expected| expected != run.run_id()) {
			continue;
		}

		runs.push(operator_run_status(
			project,
			&loop_evidence,
			&project_display_name,
			run,
			now_unix_epoch,
		)?);
	}

	Ok(runs)
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

fn operator_active_run_statuses(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	project_display_name: &str,
	leased_runs: Vec<ProjectRunStatus>,
	recent_runs: &[OperatorRunStatus],
	now_unix_epoch: i64,
) -> crate::prelude::Result<Vec<OperatorRunStatus>> {
	let mut active_runs = leased_runs
		.into_iter()
		.map(|run| {
			operator_run_status(
				project,
				loop_evidence,
				project_display_name,
				run,
				now_unix_epoch,
			)
		})
		.collect::<crate::prelude::Result<Vec<_>>>()?
		.into_iter()
		.filter(operator_run_counts_as_active)
		.collect::<Vec<_>>();
	let mut active_run_ids =
		active_runs.iter().map(|run| run.run_id.clone()).collect::<HashSet<_>>();

	for run in recent_runs {
		if !active_run_ids.contains(&run.run_id) && operator_run_has_live_execution(run) {
			active_run_ids.insert(run.run_id.clone());
			active_runs.push(run.clone());
		}
	}

	hydrate_active_run_lifecycle_metrics(&mut active_runs, recent_runs);

	Ok(active_runs)
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

fn operator_github_cli_authority(project: &ServiceConfig) -> OperatorGitHubCliAuthority {
	operator_github_cli_authority_from_resolution(&github::gh_command_resolution(
		project.github().command_path(),
	))
}

fn operator_github_cli_authority_from_registration(
	project: &ProjectRegistration,
) -> OperatorGitHubCliAuthority {
	let configured_path = ServiceConfig::from_path(project.config_path())
		.ok()
		.and_then(|config| config.github().command_path().map(Path::to_path_buf));

	operator_github_cli_authority_from_resolution(&github::gh_command_resolution(
		configured_path.as_deref(),
	))
}

fn operator_github_cli_authority_from_resolution(
	resolution: &GhCommandResolution,
) -> OperatorGitHubCliAuthority {
	let discovery_tier = resolution.discovery_tier().as_str().to_owned();
	let configured_path = resolution.configured_path().map(display_path);
	let available = resolution.available();

	OperatorGitHubCliAuthority {
		command_path: display_path(resolution.command_path()),
		resolved_path: resolution.resolved_path().map(display_path),
		configured_path,
		discovery_tier: discovery_tier.clone(),
		available,
		next_action: github_cli_authority_next_action(discovery_tier.as_str(), available),
	}
}

fn github_cli_authority_next_action(discovery_tier: &str, available: bool) -> String {
	match (discovery_tier, available) {
		("configured", true) =>
			String::from("No action needed; Decodex will use the configured GitHub CLI path."),
		("configured", false) => String::from(
			"Fix `github.command_path` in project.toml so it points to an installed `gh` binary.",
		),
		("path", true) =>
			String::from("No action needed; Decodex resolved `gh` from the process PATH."),
		("user-bin" | "known-fallback", true) => String::from(
			"Set `github.command_path` in project.toml if this fallback path is unexpected.",
		),
		_ => String::from(
			"Install GitHub CLI or set `github.command_path` in project.toml to the expected `gh` binary.",
		),
	}
}

fn display_path(path: &Path) -> String {
	path.display().to_string()
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
			run_issue_metadata_hydration: RunIssueMetadataHydration::ActiveRowsOnly,
			account_activity_mode: AccountActivityMode::Snapshot,
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
	state_store.configure_dispatch_slot_root(
		project.service_id(),
		project.worktree_root(),
		workflow.frontmatter().execution().max_concurrent_agents(),
	)?;

	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};
	let mut snapshot = build_operator_status_snapshot_with_account_mode(
		project,
		state_store,
		limit,
		options.account_activity_mode,
	)?;

	snapshot.execution_programs =
		operator_execution_program_statuses(project, workflow, state_store)?;

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
	apply_terminal_history_ledger_outcomes(&mut snapshot);
	suppress_terminal_attention_queue_echoes(&mut snapshot);
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

fn operator_execution_program_statuses(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> crate::prelude::Result<Vec<OperatorExecutionProgramStatus>> {
	let policy = ExecutionWorkflowPolicy::from_workflow(project.service_id(), workflow)?;
	let records = state_store.list_execution_programs(project.service_id())?;
	let context = operator_execution_program_readiness_context(
		project.service_id(),
		workflow,
		state_store,
		&records,
	)?;
	let mut statuses = Vec::new();

	for record in records {
		let evaluation = if let Some(source_contract_id) = record.source_contract_id() {
			let Some(contract) = state_store.decision_contract(project.service_id(), source_contract_id)?
			else {
				statuses.push(OperatorExecutionProgramStatus::missing_contract(&record));

				continue;
			};

			record.program().evaluate(contract.contract(), &policy, &context)?
		} else {
			record.program().evaluate_issue_batch(&policy, &context)?
		};

		statuses.push(OperatorExecutionProgramStatus::from_summary(
			&record,
			evaluation.operator_summary(),
			&evaluation,
		));
	}

	statuses.sort_by(|left, right| left.program_id.cmp(&right.program_id));

	Ok(statuses)
}

fn operator_execution_program_readiness_context(
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	records: &[ExecutionProgramRecord],
) -> crate::prelude::Result<ExecutionProgramReadinessContext> {
	let dependency_snapshots = operator_execution_program_dependency_snapshots(records)?;
	let occupied_conflict_domains =
		operator_execution_program_occupied_conflict_domains(service_id, workflow, state_store, records)?;

	Ok(ExecutionProgramReadinessContext::new()
		.with_dependency_snapshots(dependency_snapshots)
		.with_occupied_conflict_domains(occupied_conflict_domains))
}

fn operator_execution_program_dependency_snapshots(
	records: &[ExecutionProgramRecord],
) -> crate::prelude::Result<Vec<ExecutionDependencySnapshot>> {
	let mut snapshots = BTreeMap::new();

	for record in records {
		for node in record.program().nodes() {
			let Some(issue) = node.linear_issue() else {
				continue;
			};

			insert_dependency_snapshot(&mut snapshots, node.node_id(), issue.issue_state())?;
			insert_dependency_snapshot(&mut snapshots, issue.issue_identifier(), issue.issue_state())?;
		}
	}

	Ok(snapshots.into_values().collect())
}

fn operator_execution_program_occupied_conflict_domains(
	service_id: &str,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	records: &[ExecutionProgramRecord],
) -> crate::prelude::Result<Vec<ExecutionConflictDomain>> {
	let retained_issue_ids = state_store
		.list_worktrees(service_id)?
		.into_iter()
		.map(|worktree| worktree.issue_id().to_owned())
		.collect::<std::collections::BTreeSet<_>>();
	let mut occupied = Vec::new();
	let mut seen = std::collections::BTreeSet::new();

	for record in records {
		for node in record.program().nodes() {
			let Some(issue) = node.linear_issue() else {
				continue;
			};
			let retained_nonterminal =
				retained_issue_ids.contains(issue.issue_id())
					&& !state_name_is_terminal(issue.issue_state(), workflow);
			let issue_occupies_domain = issue.has_active_label()
				|| issue.has_needs_attention_label()
				|| retained_nonterminal
				|| state_store.issue_has_active_shared_claim(service_id, issue.issue_id())?;

			if !issue_occupies_domain {
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

fn hydrate_live_operator_external_observers<T>(
	context: LiveOperatorStatusObserverContext<'_, T>,
	snapshot: &mut OperatorStatusSnapshot,
) -> crate::prelude::Result<()>
where
	T: IssueTracker,
{
	let mut paused =
		pause_operator_snapshot_for_stored_tracker_backoff(&context, snapshot)?;

	if !paused {
		paused = apply_tracker_observer_outcome(
			hydrate_operator_run_rows_from_tracker(
				context.tracker,
				context.project,
				context.workflow,
				snapshot,
				context.run_issue_metadata_hydration,
			),
			snapshot,
			context.state_store,
			context.project,
			"run_issue_metadata_unavailable",
		);
	}
	if !paused && context.hydrate_history_ledger {
		paused = apply_tracker_observer_outcome(
			hydrate_history_lanes_from_linear_ledger(context.tracker, context.project, snapshot),
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
		TrackerObserverOutcome::RateLimited(backoff) => {
			pause_operator_snapshot_for_rate_limit(snapshot, state_store, project, &backoff);

			true
		},
	}
}

fn pause_operator_snapshot_for_rate_limit(
	snapshot: &mut OperatorStatusSnapshot,
	state_store: &StateStore,
	project: &ServiceConfig,
	backoff: &TrackerConnectorBackoff,
) {
	persist_tracker_backoff_state(state_store, project.service_id(), backoff);

	let backoff = backoff.to_operator_status(
		project.service_id(),
		OffsetDateTime::now_utc().unix_timestamp(),
	);

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
				tracker_rate_limit_backoff(&error, Instant::now(), "queued_candidate_status")
			else {
				let _ = error;

				tracing::warn!(
					"Skipped queued candidate status while publishing an operator snapshot; sensitive runtime details were withheld."
				);

				add_operator_snapshot_warning(snapshot, "queued_candidate_status_unavailable");

				return false;
			};

			pause_operator_snapshot_for_rate_limit(
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
				tracker_rate_limit_backoff(&error, Instant::now(), "post_review_lane_status")
			else {
				let _ = error;

				tracing::warn!(
					"Skipped post-review lane status while publishing an operator snapshot; sensitive runtime details were withheld."
				);

				add_operator_snapshot_warning(snapshot, "post_review_lane_status_unavailable");

				return Ok(false);
			};

			pause_operator_snapshot_for_rate_limit(
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
	add_operator_snapshot_warning(snapshot, TRACKER_RATE_LIMIT_WARNING);

	if !snapshot.connector_backoffs.iter().any(|existing| {
		existing.project_id == backoff.project_id && existing.connector == backoff.connector
	}) {
		snapshot.connector_backoffs.push(backoff.clone());
	}
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

fn apply_terminal_history_ledger_outcomes(snapshot: &mut OperatorStatusSnapshot) {
	let mut terminal_history_keys = HashSet::new();

	for lane in &mut snapshot.history_lanes {
		if !history_ledger_outcome_supersedes_local_attempts(&lane.ledger_outcome) {
			continue;
		}

		terminal_history_keys.insert(history_lane_group_key(lane));

		apply_terminal_history_ledger_outcome_to_latest_run(lane);
	}

	if terminal_history_keys.is_empty() {
		return;
	}

	let active_run_ids = snapshot
		.active_runs
		.iter()
		.map(|run| run.run_id.clone())
		.collect::<HashSet<_>>();
	let active_issue_keys = snapshot
		.active_runs
		.iter()
		.map(operator_run_group_key)
		.collect::<HashSet<_>>();

	snapshot.recent_runs.retain(|run| {
		let run_group_key = operator_run_group_key(run);

		active_run_ids.contains(&run.run_id)
			|| active_issue_keys.contains(&run_group_key)
			|| !terminal_history_keys.contains(&run_group_key)
	});
}

fn suppress_terminal_attention_queue_echoes(snapshot: &mut OperatorStatusSnapshot) {
	let terminal_attention_keys = snapshot
		.history_lanes
		.iter()
		.filter(|lane| history_ledger_outcome_requires_attention(&lane.ledger_outcome))
		.map(history_lane_group_key)
		.collect::<HashSet<_>>();

	if terminal_attention_keys.is_empty() {
		return;
	}

	snapshot.queued_candidates.retain(|candidate| {
		let candidate_key =
			operator_issue_attention_key(&candidate.issue_id, Some(&candidate.issue_identifier));
		let is_terminal_attention_echo =
			candidate.reason == "issue_needs_attention" && terminal_attention_keys.contains(&candidate_key);

		!is_terminal_attention_echo
	});
}

fn history_ledger_outcome_supersedes_local_attempts(
	outcome: &OperatorHistoryLedgerOutcome,
) -> bool {
	history_ledger_outcome_is_terminal(outcome)
}

fn history_ledger_outcome_is_terminal(outcome: &OperatorHistoryLedgerOutcome) -> bool {
	outcome.ledger_status == "present"
		&& matches!(
			outcome.final_outcome.as_str(),
			"cleanup_complete" | "closeout" | "landed" | "needs_attention" | "terminal_failure"
		)
}

fn history_ledger_outcome_requires_attention(outcome: &OperatorHistoryLedgerOutcome) -> bool {
	outcome.ledger_status == "present"
		&& matches!(outcome.final_outcome.as_str(), "needs_attention" | "terminal_failure")
}

fn apply_terminal_history_ledger_outcome_to_latest_run(lane: &mut OperatorHistoryLaneStatus) {
	let final_outcome = lane.ledger_outcome.final_outcome.clone();
	let final_event_at = lane.ledger_outcome.final_event_at.clone();
	let requires_attention = history_ledger_outcome_requires_attention(&lane.ledger_outcome);

	lane.latest_run.status = final_outcome.clone();
	lane.latest_run.attempt_status = final_outcome;
	lane.latest_run.status_projection_reason = None;
	lane.latest_run.phase = String::from(if requires_attention { "needs_attention" } else { "completed" });
	lane.latest_run.wait_reason = None;
	lane.latest_run.current_operation = String::from("ledger_outcome");
	lane.latest_run.continuation_pending = false;
	lane.latest_run.active_lease = false;
	lane.latest_run.queue_lease_state = String::from("not_held");
	lane.latest_run.execution_liveness = String::from("not_running");
	lane.latest_run.suspected_stall = false;
	lane.latest_run.retry_kind = None;
	lane.latest_run.next_retry_at = None;

	if let Some(loop_status) = lane.latest_run.loop_status.as_mut() {
		loop_status.summary = format!(
			"terminal {}: {}",
			if requires_attention { "attention" } else { "lifecycle" },
			lane.latest_run.status
		);
		loop_status.next_action = requires_attention
			.then(|| lane.ledger_outcome.needs_attention_reason.clone())
			.flatten();

		if loop_status
			.review
			.as_ref()
			.is_some_and(|review| review.status == "pending" && review.checkpoint.is_none())
		{
			loop_status.review = None;
		}
	}
	if let Some(final_event_at) = final_event_at {
		lane.latest_run.updated_at = final_event_at.clone();
		lane.latest_run.last_run_activity_at = Some(final_event_at);
	}
}

fn history_lane_group_key(lane: &OperatorHistoryLaneStatus) -> String {
	let issue_id = lane.issue_id.trim();

	if !issue_id.is_empty() && !issue_id.eq_ignore_ascii_case("unknown") {
		return issue_id.to_ascii_uppercase();
	}

	let issue_key = lane.issue_key.trim();

	if !issue_key.is_empty() && !issue_key.eq_ignore_ascii_case("unknown") {
		return issue_key.to_ascii_uppercase();
	}

	operator_run_group_key(&lane.latest_run)
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
		worktree.recovery_next_action = ownership.next_action;
		worktree.provenance.audit_required = ownership.audit_required;
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
			next_action: None,
			audit_required: false,
		};
	}
	if let Some(lane) = worktree_post_review_owner(worktree, snapshot) {
		return WorktreeOwnership {
			kind: "post_review_lane",
			reason: format!(
				"Review & Landing owns this worktree as `{}`.",
				lane.classification
			),
			next_action: None,
			audit_required: false,
		};
	}
	if let Some(lane) = worktree_history_attention_owner(worktree, snapshot) {
		return WorktreeOwnership {
			kind: "retained_attention",
			reason: format!(
				"Run Ledger owns this worktree through terminal `{}` outcome.",
				lane.ledger_outcome.final_outcome
			),
			next_action: Some(
				lane
					.ledger_outcome
					.needs_attention_reason
					.clone()
					.unwrap_or_else(|| {
						String::from(
							"inspect the retained worktree diff and resolve the terminal attention outcome manually",
						)
					}),
			),
			audit_required: false,
		};
	}

	if worktree_has_queued_attention_owner(worktree, snapshot) {
		return WorktreeOwnership {
			kind: "queued_attention",
			reason: String::from(
				"Intake Queue owns this worktree because the issue needs operator attention.",
			),
			next_action: None,
			audit_required: false,
		};
	}

	if let Some(hygiene) = &worktree.hygiene {
		return WorktreeOwnership {
			kind: "post_land_cleanup",
			reason: hygiene.reason.clone(),
			next_action: Some(String::from(
				"inspect the merged worktree, preserve or discard local changes intentionally, then remove the linked worktree",
			)),
			audit_required: false,
		};
	}

	let audit_required = worktree.provenance.source == WORKTREE_PROVENANCE_LEGACY_UNKNOWN;

	WorktreeOwnership {
		kind: "cleanup_only",
		reason: worktree_cleanup_only_reason(worktree, completed_state),
		next_action: audit_required.then(|| legacy_cleanup_next_action(worktree)),
		audit_required,
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
		matches!(
			candidate.reason.as_str(),
			"issue_needs_attention" | QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT
		)
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

fn worktree_history_attention_owner<'a>(
	worktree: &OperatorWorktreeStatus,
	snapshot: &'a OperatorStatusSnapshot,
) -> Option<&'a OperatorHistoryLaneStatus> {
	let worktree_issue_key =
		operator_issue_attention_key(&worktree.issue_id, worktree.issue_identifier.as_deref());

	snapshot.history_lanes.iter().find(|lane| {
		history_ledger_outcome_requires_attention(&lane.ledger_outcome)
			&& (history_lane_group_key(lane) == worktree_issue_key
				|| lane.latest_run.worktree_path.as_deref() == Some(worktree.worktree_path.as_str())
				|| lane.latest_run.branch_name.as_deref() == Some(worktree.branch_name.as_str()))
	})
}

fn worktree_cleanup_only_reason(
	worktree: &OperatorWorktreeStatus,
	completed_state: Option<&str>,
) -> String {
	if worktree.provenance.source == WORKTREE_PROVENANCE_LEGACY_UNKNOWN {
		return String::from(
			"Legacy worktree mapping has no durable runtime provenance; no active, queued, or post-review lane owns it, so Decodex cannot automatically prove PR or closeout lineage.",
		);
	}

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

fn legacy_cleanup_next_action(worktree: &OperatorWorktreeStatus) -> String {
	let issue = worktree.issue_identifier.as_deref().unwrap_or(&worktree.issue_id);

	format!(
		"verify tracker/PR terminal state and clean git status for `{}`, then run `decodex recover legacy-closeout {issue} --pr <MERGED_PR> --dry-run`; rerun with `--manual-authority` before removing this worktree",
		worktree.worktree_path
	)
}

fn refresh_operator_project_summary(
	snapshot: &mut OperatorStatusSnapshot,
	completed_state: Option<&str>,
) {
	let active_run_count = snapshot.active_runs.len();
	let running_lane_count =
		snapshot.active_runs.iter().filter(|run| operator_run_counts_as_running(run)).count();
	let queued_candidate_count = snapshot
		.queued_candidates
		.iter()
		.filter(|candidate| queued_candidate_counts_as_waiting_intake(candidate))
		.count();
	let post_review_lane_count = snapshot.post_review_lanes.len();
	let retained_worktree_count = rendered_recovery_worktrees(snapshot).len();
	let waiting_lane_count = project_waiting_lane_count(snapshot);
	let attention_count = project_attention_count(snapshot, completed_state);
	let cleanup_blocked_count = project_cleanup_blocked_count(snapshot);
	let cleanup_pending_count = project_cleanup_pending_count(snapshot);
	let connector_state = project_connector_state(snapshot);
	let last_activity_at = project_last_activity_at(snapshot);
	let warning_count = snapshot.warnings.len();

	if let Some(project_status) = snapshot.projects.first_mut() {
		project_status.active_run_count = active_run_count;
		project_status.running_lane_count = running_lane_count;
		project_status.queued_candidate_count = queued_candidate_count;
		project_status.post_review_lane_count = post_review_lane_count;
		project_status.retained_worktree_count = retained_worktree_count;
		project_status.waiting_lane_count = waiting_lane_count;
		project_status.attention_count = attention_count;
		project_status.cleanup_blocked_count = cleanup_blocked_count;
		project_status.cleanup_pending_count = cleanup_pending_count;
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

fn project_attention_count(
	snapshot: &OperatorStatusSnapshot,
	completed_state: Option<&str>,
) -> usize {
	let mut attention_keys = HashSet::new();

	for run in snapshot
		.active_runs
		.iter()
		.filter(|run| operator_run_needs_attention(run))
	{
		attention_keys.insert(operator_run_group_key(run));
	}
	for candidate in snapshot
		.queued_candidates
		.iter()
		.filter(|candidate| queued_candidate_counts_as_attention(candidate))
	{
		attention_keys.insert(operator_issue_attention_key(
			&candidate.issue_id,
			Some(&candidate.issue_identifier),
		));
	}
	for lane in snapshot
		.post_review_lanes
		.iter()
		.filter(|lane| post_review_lane_counts_as_attention(lane))
	{
		attention_keys.insert(operator_issue_attention_key(&lane.issue_id, Some(&lane.issue_identifier)));
	}
	for lane in snapshot
		.history_lanes
		.iter()
		.filter(|lane| history_lane_has_current_attention(snapshot, lane, completed_state))
	{
		attention_keys.insert(history_lane_group_key(lane));
	}

	attention_keys.len()
}

fn project_history_only_attention_count(snapshot: &OperatorStatusSnapshot) -> usize {
	snapshot
		.history_lanes
		.iter()
		.filter(|lane| {
			history_ledger_outcome_requires_attention(&lane.ledger_outcome)
				&& !history_lane_has_current_attention_signal(snapshot, lane)
		})
		.count()
}

fn queued_candidate_counts_as_attention(candidate: &OperatorQueuedIssueStatus) -> bool {
	candidate.classification == "blocked" || candidate.attention.is_some()
}

fn post_review_lane_counts_as_attention(lane: &OperatorPostReviewLaneStatus) -> bool {
	matches!(
		lane.classification.as_str(),
		"blocked" | "needs_review_repair" | "closeout_blocked"
	)
}

fn history_lane_has_current_attention(
	snapshot: &OperatorStatusSnapshot,
	lane: &OperatorHistoryLaneStatus,
	completed_state: Option<&str>,
) -> bool {
	if !history_ledger_outcome_requires_attention(&lane.ledger_outcome) {
		return false;
	}

	history_lane_has_current_attention_signal(snapshot, lane)
		&& !history_lane_attention_is_resolved_tracker_echo(snapshot, lane, completed_state)
}

fn history_lane_has_current_attention_signal(
	snapshot: &OperatorStatusSnapshot,
	lane: &OperatorHistoryLaneStatus,
) -> bool {
	if lane.needs_attention_label_present == Some(true) {
		return true;
	}

	let issue_key = history_lane_group_key(lane);
	let has_non_attention_post_review_owner =
		history_lane_has_current_non_attention_post_review_owner(snapshot, &issue_key);

	if lane.active_label_present == Some(true) && !has_non_attention_post_review_owner {
		return true;
	}

	snapshot.worktrees.iter().any(|worktree| {
		!has_non_attention_post_review_owner
			&& operator_issue_attention_key(
				&worktree.issue_id,
				worktree.issue_identifier.as_deref(),
			) == issue_key
	}) || snapshot.post_review_lanes.iter().any(|post_review_lane| {
		post_review_lane_counts_as_attention(post_review_lane)
			&& operator_issue_attention_key(
				&post_review_lane.issue_id,
				Some(&post_review_lane.issue_identifier),
			) == issue_key
	}) || snapshot.queued_candidates.iter().any(|candidate| {
		queued_candidate_counts_as_attention(candidate)
			&& operator_issue_attention_key(&candidate.issue_id, Some(&candidate.issue_identifier))
				== issue_key
	})
}

fn history_lane_has_current_non_attention_post_review_owner(
	snapshot: &OperatorStatusSnapshot,
	issue_key: &str,
) -> bool {
	snapshot.post_review_lanes.iter().any(|post_review_lane| {
		!post_review_lane_counts_as_attention(post_review_lane)
			&& operator_issue_attention_key(
				&post_review_lane.issue_id,
				Some(&post_review_lane.issue_identifier),
			) == issue_key
	})
}

fn history_lane_attention_is_resolved_tracker_echo(
	snapshot: &OperatorStatusSnapshot,
	lane: &OperatorHistoryLaneStatus,
	completed_state: Option<&str>,
) -> bool {
	let Some(completed_state) = completed_state else {
		return false;
	};

	if lane.issue_state.as_deref() != Some(completed_state) {
		return false;
	}
	if lane.active_label_present != Some(false)
		|| lane.needs_attention_label_present != Some(false)
	{
		return false;
	}

	let issue_key = history_lane_group_key(lane);

	!snapshot.worktrees.iter().any(|worktree| {
		operator_issue_attention_key(&worktree.issue_id, worktree.issue_identifier.as_deref())
			== issue_key
	}) && !snapshot.post_review_lanes.iter().any(|post_review_lane| {
		operator_issue_attention_key(
			&post_review_lane.issue_id,
			Some(&post_review_lane.issue_identifier),
		) == issue_key
	}) && !snapshot.queued_candidates.iter().any(|candidate| {
		if candidate.classification == "closed" && candidate.attention.is_none() {
			return false;
		}

		operator_issue_attention_key(&candidate.issue_id, Some(&candidate.issue_identifier))
			== issue_key
	})
}

fn operator_issue_attention_key(issue_id: &str, issue_identifier: Option<&str>) -> String {
	let issue_id = issue_id.trim();

	if !issue_id.is_empty() && !issue_id.eq_ignore_ascii_case("unknown") {
		return issue_id.to_ascii_uppercase();
	}

	if let Some(issue_identifier) = issue_identifier
		.map(str::trim)
		.filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("unknown"))
	{
		return issue_identifier.to_ascii_uppercase();
	}

	String::from("UNKNOWN")
}

fn project_cleanup_blocked_count(snapshot: &OperatorStatusSnapshot) -> usize {
	let mut cleanup_keys = HashSet::new();

	for lane in snapshot
		.post_review_lanes
		.iter()
		.filter(|lane| lane.classification == "cleanup_blocked")
	{
		cleanup_keys.insert(post_review_lane_cleanup_key(lane));
	}
	for worktree in snapshot.worktrees.iter().filter(|worktree| {
		worktree.hygiene.as_ref().is_some_and(|hygiene| {
			hygiene.dirty || hygiene.classification == "merged_dirty_worktree"
		})
	}) {
		cleanup_keys.insert(worktree_cleanup_key(worktree));
	}

	cleanup_keys.len()
}

fn project_cleanup_pending_count(snapshot: &OperatorStatusSnapshot) -> usize {
	snapshot
		.worktrees
		.iter()
		.filter(|worktree| {
			worktree.hygiene.as_ref().is_some_and(|hygiene| {
				!hygiene.dirty && hygiene.classification == "merged_worktree_cleanup_pending"
			})
		})
		.map(worktree_cleanup_key)
		.collect::<HashSet<_>>()
		.len()
}

fn post_review_lane_cleanup_key(lane: &OperatorPostReviewLaneStatus) -> String {
	if lane.issue_identifier.is_empty() {
		return lane.issue_id.clone();
	}

	lane.issue_identifier.clone()
}

fn worktree_cleanup_key(worktree: &OperatorWorktreeStatus) -> String {
	worktree
		.issue_identifier
		.clone()
		.unwrap_or_else(|| worktree.issue_id.clone())
}

fn operator_run_counts_as_active(run: &OperatorRunStatus) -> bool {
	(run.active_lease || operator_run_has_live_execution(run))
		&& !matches!(run.phase.as_str(), "completed" | "failed" | "terminated")
}

fn operator_run_has_live_execution(run: &OperatorRunStatus) -> bool {
	matches!(run.status.as_str(), "starting" | "running")
		&& (matches!(
			run.execution_liveness.as_str(),
			"process_alive" | "thread_active" | "protocol_observed"
		) || operator_run_has_recent_app_server_execution(run))
}

fn operator_run_counts_as_running(run: &OperatorRunStatus) -> bool {
	matches!(run.status.as_str(), "starting" | "running")
		&& run.phase == "executing"
		&& (run.process_alive != Some(false) || operator_run_has_recent_app_server_execution(run))
		&& !operator_run_needs_attention(run)
}

fn operator_run_needs_attention(run: &OperatorRunStatus) -> bool {
	matches!(run.status.as_str(), "needs_attention" | "terminal_failure")
		|| run.phase == "needs_attention"
		|| run.suspected_stall
		|| run.phase == "stalled"
		|| run.process_alive == Some(false)
			&& matches!(run.status.as_str(), "starting" | "running")
			&& run.wait_reason.is_none()
			&& !operator_run_has_recent_app_server_execution(run)
		|| operator_run_has_stale_execution_without_known_process(run)
}

fn operator_run_has_recent_app_server_execution(run: &OperatorRunStatus) -> bool {
	matches!(run.thread_status.as_deref(), Some("active"))
		|| !run.thread_active_flags.is_empty()
		|| run.protocol_idle_for_seconds.is_some_and(|idle_for| {
			u64::try_from(idle_for)
				.is_ok_and(|idle_for| idle_for < ACTIVE_RUN_IDLE_TIMEOUT.as_secs())
		})
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
		.chain(snapshot.history_lanes.iter().map(|lane| &lane.latest_run))
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
) -> crate::prelude::Result<(
	Vec<OperatorWorktreeStatus>,
	Vec<String>,
	Vec<OperatorSnapshotWarningDetail>,
)> {
	let mut worktrees = state_store
		.list_worktrees(project.service_id())?
		.into_iter()
		.map(|mapping| OperatorWorktreeStatus {
			project_id: project.service_id().to_owned(),
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
			provenance: operator_worktree_provenance_from_mapping(&mapping),
			recovery_next_action: None,
			hygiene: None,
		})
		.collect::<Vec<_>>();
	let mut seen_paths =
		worktrees.iter().map(|worktree| worktree.worktree_path.clone()).collect::<HashSet<_>>();
	let mut warnings = Vec::new();
	let mut warning_details = Vec::new();

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
			project_id: project.service_id().to_owned(),
			issue_identifier: Some(issue_identifier.clone()),
			issue_id: issue_identifier,
			issue_state: None,
			branch_name,
			worktree_path: relative_path,
			ownership: String::from("cleanup_only"),
			ownership_reason: String::from(
				"No active lane, queued recovery, or post-review lane currently owns this worktree.",
			),
			provenance: operator_worktree_provenance(
				WORKTREE_PROVENANCE_FILESYSTEM_SCAN,
				None,
				None,
			),
			recovery_next_action: None,
			hygiene: None,
		});
	}

	append_merged_worktree_cleanup_debts(
		project,
		&mut worktrees,
		&mut seen_paths,
		&mut warnings,
		&mut warning_details,
	);

	worktrees.sort_by(|left, right| {
		left.issue_id
			.cmp(&right.issue_id)
			.then_with(|| left.branch_name.cmp(&right.branch_name))
			.then_with(|| left.worktree_path.cmp(&right.worktree_path))
	});

	Ok((worktrees, warnings, warning_details))
}

fn append_merged_worktree_cleanup_debts(
	project: &ServiceConfig,
	worktrees: &mut Vec<OperatorWorktreeStatus>,
	seen_paths: &mut HashSet<String>,
	warnings: &mut Vec<String>,
	warning_details: &mut Vec<OperatorSnapshotWarningDetail>,
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
			warning_details.push(worktree_hygiene_unavailable_warning_detail(project, &error));

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
		let debt_status = operator_worktree_status_from_cleanup_debt(
			project.service_id(),
			debt,
			relative_path.clone(),
		);

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

fn worktree_hygiene_unavailable_warning_detail(
	project: &ServiceConfig,
	error: &Report,
) -> OperatorSnapshotWarningDetail {
	OperatorSnapshotWarningDetail {
		warning: String::from("worktree_hygiene_unavailable"),
		project_id: Some(project.service_id().to_owned()),
		repo_root: Some(project.repo_root().display().to_string()),
		reason: format!("Worktree hygiene scan failed: {error}"),
		next_action: Some(String::from(
			"Remove the stale project registration or restore the Git checkout before running automation.",
		)),
	}
}

fn operator_worktree_status_from_cleanup_debt(
	project_id: &str,
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
		project_id: project_id.to_owned(),
		issue_id: branch_name.clone(),
		issue_identifier: issue_identifier_in_text(&branch_name)
			.or_else(|| issue_identifier_in_text(&relative_path)),
		issue_state: None,
		branch_name,
		worktree_path: relative_path,
		ownership: String::from("post_land_cleanup"),
		ownership_reason: reason.clone(),
		provenance: operator_worktree_provenance(
			WORKTREE_PROVENANCE_GIT_HYGIENE_SCAN,
			None,
			None,
		),
		recovery_next_action: Some(String::from(
			"inspect the merged worktree, preserve or discard local changes intentionally, then remove the linked worktree",
		)),
		hygiene: Some(OperatorWorktreeHygieneStatus {
			classification: String::from(classification),
			default_branch,
			dirty,
			reason,
		}),
	}
}

fn operator_worktree_provenance_from_mapping(
	mapping: &WorktreeMapping,
) -> OperatorWorktreeProvenanceStatus {
	operator_worktree_provenance(
		mapping.provenance().source(),
		mapping.provenance().created_at_unix(),
		mapping.provenance().updated_at_unix(),
	)
}

fn operator_worktree_provenance(
	source: &str,
	created_at_unix: Option<i64>,
	updated_at_unix: Option<i64>,
) -> OperatorWorktreeProvenanceStatus {
	OperatorWorktreeProvenanceStatus {
		source: source.to_owned(),
		created_at_unix,
		updated_at_unix,
		audit_required: false,
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
) -> TrackerObserverOutcome
where
	T: IssueTracker,
{
	let issue_ids = operator_snapshot_run_issue_ids(snapshot, hydration);

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

			TrackerObserverOutcome::Ok
		},
		Err(error) => {
			if let Some(backoff) =
				tracker_rate_limit_backoff(&error, Instant::now(), "run_issue_metadata")
			{
				return TrackerObserverOutcome::RateLimited(backoff);
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
) -> Vec<String> {
	let mut issue_ids = BTreeSet::new();

	for run in &snapshot.active_runs {
		append_operator_run_issue_id(&mut issue_ids, run);
	}

	if matches!(hydration, RunIssueMetadataHydration::AllRows) {
		for run in &snapshot.recent_runs {
			append_operator_run_issue_id(&mut issue_ids, run);
		}
		for lane in &snapshot.history_lanes {
			append_operator_run_issue_id(&mut issue_ids, &lane.latest_run);

			for attempt in &lane.attempts {
				append_operator_run_issue_id(&mut issue_ids, attempt);
			}
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
	if let Some(issue_state) = metadata
		.issue_state
		.as_ref()
		.filter(|issue_state| !issue_state.trim().is_empty())
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
) -> TrackerObserverOutcome
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
				if let Some(backoff) =
					tracker_rate_limit_backoff(&error, Instant::now(), "execution_ledger_status")
				{
					lane.ledger_outcome = unavailable_history_ledger_outcome();

					return TrackerObserverOutcome::RateLimited(backoff);
				}

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

	if unavailable {
		TrackerObserverOutcome::Unavailable
	} else {
		TrackerObserverOutcome::Ok
	}
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
		issue_state: None,
		active_label_present: None,
		needs_attention_label_present: None,
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
	concurrency: &ConcurrencySnapshot,
	issue: TrackerIssue,
) -> crate::prelude::Result<OperatorQueuedIssueStatus>
where
	T: IssueTracker,
{
	let (classification, reason) =
		classify_queued_issue(tracker, project, workflow, state_store, concurrency, &issue)?;
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
	let checkpoint = state_store.observe_loop_guardrail_checkpoint(
		LoopGuardrailCheckpointInput {
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
		},
	)?;

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
	concurrency: &ConcurrencySnapshot,
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
		let checkpoint = observe_dependency_program_stale_guardrail(
			project,
			workflow,
			state_store,
			issue,
		)?;
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
	if !matches!(
		reason,
		"issue_needs_attention"
			| "retry_budget_exhausted"
			| QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT
	) {
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
	let auto_retry_blocked_reason = match reason {
		"issue_needs_attention" => Some(String::from("needs_attention_label")),
		QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT => {
			Some(String::from(QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT))
		},
		_ => None,
	};
	let attention_record =
		operator_queued_issue_latest_attention_record(tracker, project, state_store, issue);
	let private_evidence_missing = operator_queued_issue_private_evidence_missing(
		project,
		state_store,
		issue,
		marker.as_ref(),
		reason,
	)?;
	let attention_error_class = if private_evidence_missing {
		Some(String::from(ATTENTION_ERROR_EVIDENCE_MISSING))
	} else {
		attention_record.as_ref().and_then(|record| record.error_class.clone())
	};
	let attention_next_action =
		attention_record.as_ref().and_then(|record| record.next_action.clone());
	let decision_request = operator_queued_issue_decision_request_status(
		project,
		state_store,
		issue,
		attention_record.as_ref(),
		marker.as_ref(),
	)?;
	let loop_status = operator_queued_issue_loop_status(
		project,
		state_store,
		issue,
		attention_record.as_ref(),
		marker.as_ref(),
	)?;
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
		attention_error_class.as_deref(),
	);
	let process_liveness = marker.as_ref().and_then(marker_process_liveness_for_marker);

	Ok(Some(OperatorQueuedIssueAttentionStatus {
		summary,
		decision_request,
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
		loop_status,
		auto_retry_blocked_reason,
		attention_error_class,
		attention_next_action,
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

fn operator_queued_issue_loop_status(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
	attention_record: Option<&LinearExecutionEventRecord>,
	marker: Option<&RunActivityMarker>,
) -> crate::prelude::Result<Option<OperatorLoopStatus>> {
	let run_id = attention_record
		.map(|record| record.run_id.as_str())
		.or_else(|| marker.map(RunActivityMarker::run_id));
	let attempt_number = attention_record
		.map(|record| record.attempt_number)
		.or_else(|| marker.map(RunActivityMarker::attempt_number));

	match (run_id, attempt_number) {
		(Some(run_id), Some(attempt_number)) => operator_loop_status_for_run(
			project,
			state_store,
			&issue.id,
			run_id,
			attempt_number,
			Some("handoff"),
			None,
		)
		.map(Some),
		_ => Ok(None),
	}
}

fn operator_queued_issue_decision_request_status(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
	attention_record: Option<&LinearExecutionEventRecord>,
	marker: Option<&RunActivityMarker>,
) -> crate::prelude::Result<Option<OperatorAuthorityDecisionRequestStatus>> {
	let run_id = attention_record
		.map(|record| record.run_id.as_str())
		.or_else(|| marker.map(RunActivityMarker::run_id));
	let attempt_number = attention_record
		.map(|record| record.attempt_number)
		.or_else(|| marker.map(RunActivityMarker::attempt_number));
	let (Some(run_id), Some(attempt_number)) = (run_id, attempt_number) else {
		return Ok(None);
	};
	let events = state_store.list_private_execution_events(
		project.service_id(),
		&issue.id,
		run_id,
		attempt_number,
	)?;

	Ok(events
		.iter()
		.rev()
		.find(|event| event.event_type() == AUTHORITY_DECISION_REQUEST_EVENT_TYPE)
		.and_then(operator_authority_decision_request_status_from_event))
}

fn operator_authority_decision_request_status_from_event(
	event: &PrivateExecutionEvent,
) -> Option<OperatorAuthorityDecisionRequestStatus> {
	let payload = event.payload();
	let decision_request_id = payload.get("decision_request_id")?.as_str()?.to_owned();
	let reason = payload.get("reason")?.as_str()?.to_owned();
	let boundary = payload.get("boundary")?.as_str()?.to_owned();
	let phase = payload
		.get("phase")
		.and_then(Value::as_str)
		.unwrap_or("human_required")
		.to_owned();
	let next_action = payload
		.get("next_action")
		.or_else(|| payload.get("resume_condition"))?
		.as_str()?
		.to_owned();
	let recommendation = payload
		.get("recommendation")
		.and_then(Value::as_str)
		.map(str::to_owned);
	let resume_condition = payload
		.get("resume_condition")
		.and_then(Value::as_str)
		.map(str::to_owned);

	Some(OperatorAuthorityDecisionRequestStatus {
		phase,
		reason,
		boundary,
		decision_request_id,
		next_action,
		recommendation,
		resume_condition,
	})
}

fn operator_queued_issue_private_evidence_missing(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
	marker: Option<&RunActivityMarker>,
	reason: &str,
) -> crate::prelude::Result<bool> {
	if reason != QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT {
		return Ok(false);
	}

	let Some(marker) = marker else {
		return Ok(true);
	};
	let events = state_store.list_private_execution_events(
		project.service_id(),
		&issue.id,
		marker.run_id(),
		marker.attempt_number(),
	)?;

	Ok(events.is_empty())
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
	attention_error_class: Option<&str>,
) -> String {
	if reason == QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT {
		if worktree_has_tracked_changes {
			return String::from(
				"Linear active ownership is still present with retained worktree changes; inspect the patch and reconcile the lane before dispatch.",
			);
		}
		if attention_error_class == Some(ATTENTION_ERROR_EVIDENCE_MISSING) {
			return if marker.is_some() {
				String::from(
					"Linear active ownership is still present but private execution evidence is missing; inspect the retained marker and reconcile before dispatch.",
				)
			} else {
				String::from(
					"Linear active ownership is still present but the retained marker or private execution evidence is missing; reconcile before dispatch.",
				)
			};
		}
		if marker.is_some() {
			return String::from(
				"Linear active ownership is still present alongside queue intake; inspect the retained marker before dispatch.",
			);
		}

		return String::from(
			"Linear active ownership is still present without a matching local active lease; reconcile before dispatch.",
		);
	}
	if worktree_has_tracked_changes {
		if retry_budget_attempts > 0 {
			return format!(
				"Partial worktree changes are retained after {retry_budget_attempts} failed attempts; inspect the patch, finish validation, then land or reset manually."
			);
		}
		if attention_error_class == Some("partial_progress_retained") {
			return String::from(
				"Partial worktree changes are retained after a stalled or failed attempt; inspect the patch, finish validation, then land or reset manually.",
			);
		}
	}
	if attention_error_class == Some("app_server_plugin_list_timeout") {
		return String::from(
			"app_server_preflight_failed: plugin/list timed out during Codex app-server preflight; operator recovery required.",
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
		)? else {
			continue;
		};
		let issue_identifier = retained_issue_identifier_from_worktree(&worktree);
		let review_state = review_state_inspector
			.inspect_review_state(worktree.worktree_path(), review_handoff.pr_url())
			.ok();
		let classification = PostReviewReadbackDegradation::tracker_issue_from_handoff(
			&review_handoff,
		)
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
	let loop_status = operator_post_review_loop_status(project, state_store, snapshot)?;

	Ok(OperatorPostReviewLaneStatus {
		project_id: project.service_id().to_owned(),
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
		pr_head_sha: classification.pr_head_sha,
		pr_state: classification.pr_state,
		review_decision: classification.review_decision,
		mergeable: classification.mergeable,
		check_state: classification.check_state,
		unresolved_review_threads: classification.unresolved_review_threads,
		readback_warning: classification.readback_warning,
		readback_root_cause: classification.readback_root_cause,
		loop_status,
	})
}

fn operator_post_review_loop_status(
	project: &ServiceConfig,
	state_store: &StateStore,
	snapshot: &PostReviewLaneSnapshot,
) -> crate::prelude::Result<Option<OperatorLoopStatus>> {
	let Some(review_handoff) = snapshot.review_handoff.as_ref() else {
		return Ok(None);
	};

	operator_loop_status_for_run(
		project,
		state_store,
		&snapshot.issue.id,
		review_handoff.run_id(),
		review_handoff.attempt_number(),
		Some("repair"),
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
	let mut classification = classify_post_review_lane_with_external_review(
		snapshot,
		workflow,
		review_state_inspector,
		project.codex().review_level().uses_github_review(),
		Some((state_store, project.service_id())),
	)?;

	confirm_status_visible_merged_closeout(snapshot, project, &mut classification);

	Ok(classification)
}

fn classify_post_review_lane_with_external_review<I>(
	snapshot: &PostReviewLaneSnapshot,
	workflow: &WorkflowDocument,
	review_state_inspector: &I,
	github_review_enabled: bool,
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
	classification.readback_root_cause = root_cause.map(|root_cause| root_cause.as_str().to_owned());

	if let Some(merge_readback) = merge_readback {
		classification.pr_state = Some(merge_readback.state);
		classification.pr_head_sha = merge_readback.head_ref_oid.or_else(|| {
			classification.pr_head_sha.clone()
		});
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
		let review_state = match review_state_inspector
			.inspect_review_state_readback(snapshot.worktree.worktree_path(), review_handoff.pr_url())
		{
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
	PostReviewReadbackDegradation::pull_request_state_from_handoff(
		review_handoff,
		root_cause,
	)
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

	let mut issues = if issue_ids.is_empty() && recoverable_worktree_skip_cache.is_some() {
		Vec::new()
	} else {
		tracker.refresh_issues(&issue_ids)?
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
	let mut active_issues = Vec::new();

	for issue in issues {
		if let Some(active_issue) = recover_issue_runtime_state(
			tracker,
			project,
			workflow,
			state_store,
			&worktree_manager,
			issue,
			now_unix_epoch,
		)? {
			active_issues.push(active_issue);
		}
	}

	active_issues.sort_by(compare_issue_candidates);

	Ok(RecoveredRuntimeState { active_issues })
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
	let worktree = worktree_manager.plan_for_issue(&issue.identifier);

	if !worktree.path.exists() {
		return Ok(None);
	}

	state_store.canonicalize_issue_identity(&issue.identifier, &issue.id)?;

	let activity_marker = state::read_run_activity_marker_snapshot(&worktree.path)?;
	let existing_worktree_mapping = state_store.worktree_for_issue(&issue.id)?;
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
				clear_recovered_issue_lease(
					project.service_id(),
					&issue.id,
					None,
					state_store,
				)?;
			},
		}

		return Ok(Some(issue));
	}

	Ok(None)
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

	let Some(issue) = tracker.get_issue_by_identifier(issue_identifier)? else {
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
		ACTIVE_RUN_IDLE_TIMEOUT,
	)
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
	let child_agent_activity =
		operator_run_child_agent_activity(marker.as_ref(), run.child_agent_activity(), now_unix_epoch);
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
	let progress_diagnostic = operator_run_progress_diagnostic(
		&lifecycle.phase,
		&timing,
		protocol_activity.as_ref(),
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
	let private_evidence = operator_run_private_evidence(project, &run, issue_identifier.as_deref());
	let loop_status =
		operator_run_loop_status(
			project,
			loop_evidence,
			&run,
			&lifecycle.status,
			&lifecycle.phase,
			&lifecycle.current_operation,
		)?;
	let control_capability = operator_run_control_capability(&run, &app_server_state);

	Ok(OperatorRunStatus {
		project_id: project.service_id().to_owned(),
		project_display_name: project_display_name.to_owned(),
		run_id: run.run_id().to_owned(),
		issue_id: run.issue_id().to_owned(),
		issue_identifier,
		title: None,
		author: None,
		attempt_number: run.attempt_number(),
		status: lifecycle.status,
		attempt_status: run.status().to_owned(),
		status_projection_reason: lifecycle.status_projection_reason,
		phase: lifecycle.phase,
		wait_reason,
		current_operation: lifecycle.current_operation,
		thread_id: app_server_state.thread_id,
		turn_id: app_server_state.turn_id,
		thread_status: app_server_state.thread_status,
		thread_active_flags: app_server_state.thread_active_flags,
		interactive_requested: app_server_state.interactive_requested,
		continuation_pending: app_server_state.continuation_pending,
		active_lease: lifecycle.active_lease,
		queue_lease_state: operator_run_queue_lease_state(lifecycle.active_lease),
		execution_liveness: lifecycle.execution_liveness,
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
		control_capability,
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
			lifecycle_metrics: OperatorLaneLifecycleMetrics::default(),
			account,
			accounts,
			branch_name,
			worktree_path,
		})
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
	let (phase, wait_reason) =
		if let Some(projection) = terminal_finalize_projection {
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
	let active_lease = terminal_finalize_projection.is_none() && run.active_lease();

	OperatorRunLifecycleProjection {
		status,
		status_projection_reason,
		phase,
		wait_reason,
		current_operation,
		suspected_stall,
		execution_liveness,
		active_lease,
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
	run.worktree_path()
		.map(|path| relative_worktree_path_for_path(project, path))
}

fn operator_run_private_evidence(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
) -> AgentPrivateEvidenceRef {
	private_evidence_ref_for_run_fields(
		project.service_id(),
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

	Some("handoff")
}

fn operator_run_lifecycle_loop_summary(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> Option<String> {
	operator_run_has_terminal_lifecycle(status, phase, current_operation)
		.then(|| format!("terminal lifecycle: {status}"))
}

fn operator_run_has_terminal_lifecycle(
	status: &str,
	phase: &str,
	current_operation: &str,
) -> bool {
	phase == "completed"
		|| phase == "terminal_pending"
		|| current_operation == "ledger_outcome"
		|| matches!(
			status,
			"succeeded"
				| "failed"
				| "interrupted"
				| "review_handoff_pending"
				| "review_repair_pending"
				| "closeout_pending"
				| "manual_attention_pending"
				| "cleanup_complete"
				| "closeout"
				| "landed"
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
	let architecture_recovery = events
		.iter()
		.rev()
		.find_map(operator_architecture_recovery_status_from_event);
	let boundary = events.iter().rev().find_map(operator_boundary_status_from_event);
	let decision_request = events
		.iter()
		.rev()
		.find(|event| event.event_type() == AUTHORITY_DECISION_REQUEST_EVENT_TYPE)
		.and_then(operator_authority_decision_request_status_from_event);
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
		review,
		architecture_recovery,
		boundary,
		decision_request,
	})
}

fn operator_review_loop_status(
	review_level: ReviewLevel,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_review_phase: Option<&str>,
) -> crate::prelude::Result<Option<OperatorReviewLoopStatus>> {
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

		return Ok(Some(OperatorReviewLoopStatus {
			phase: checkpoint.phase().to_owned(),
			status: checkpoint.status().to_owned(),
			checkpoint: Some(OperatorReviewCheckpointStatus {
				head_sha: checkpoint.head_sha().to_owned(),
				round: nonclean_rounds,
				nonclean_rounds,
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
	let next_action = operator_architecture_recovery_next_action(&reason_code);

	Some(OperatorArchitectureRecoveryStatus {
		status: operator_architecture_recovery_status_for_reason(&reason_code).to_owned(),
		reason_code,
		guardrail_reason,
		boundary_disposition,
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

fn operator_architecture_recovery_next_action(reason_code: &str) -> String {
	match reason_code {
		"architecture_recovery_started" => {
			String::from("Retry with a materially different implementation strategy inside authority.")
		},
		"architecture_recovery_exhausted" => {
			String::from("Require a new accepted recovery strategy or architecture decision before retrying.")
		},
		"external_dependency_required" => {
			String::from("Resolve the dependency or Execution Program readiness blocker before retrying.")
		},
		"contract_boundary_required" => {
			String::from("Resolve the Decision Contract or Authority Envelope boundary before retrying.")
		},
		_ => String::from("Inspect the Architecture Recovery Packet before retrying."),
	}
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
	let attempted_recovery_reason = payload
		.get("attempted_recovery_reason")
		.and_then(Value::as_str)
		.map(str::to_owned);
	let changed_surface_count = payload
		.get("changed_surfaces")
		.and_then(Value::as_array)
		.map_or(0, Vec::len);
	let improvement_signal_count = payload
		.get("improvement_signals")
		.and_then(Value::as_array)
		.map_or(0, Vec::len);

	Some(OperatorBoundaryStatus {
		disposition,
		reason,
		attempted_recovery_reason,
		changed_surface_count,
		improvement_signal_count,
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
	if boundary.is_some_and(|boundary| {
		matches!(boundary.disposition.as_str(), "requires_human" | "insufficient_evidence")
	}) {
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
		return format!(
			"human-required boundary stop: {} on {}",
			request.reason, request.boundary
		);
	}
	if let Some(recovery) = architecture_recovery {
		return format!(
			"architecture recovery {}: {}",
			recovery.status, recovery.reason_code
		);
	}
	if let Some(review) = review {
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
	if let Some(boundary) = boundary
		&& matches!(boundary.disposition.as_str(), "requires_human" | "insufficient_evidence")
	{
		return Some(String::from(
			"Resolve the Authority Boundary Check before retrying the lane.",
		));
	}

	review.and_then(|review| match review.status.as_str() {
		"pending" => Some(String::from(
			"Record the independent Decodex Review checkpoint for the current lane head.",
		)),
		"findings" => Some(String::from(
			"Repair validated review findings and record a fresh checkpoint.",
		)),
		"blocked" => Some(String::from(
			"Resolve the blocked Decodex Review before continuing.",
		)),
		"needs_architecture_review" => Some(String::from(
			"Get architecture direction before continuing review repair.",
		)),
		_ => None,
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

fn operator_project_display_name(project: &ServiceConfig) -> String {
	github_repo_slug_from_origin(project.repo_root())
		.or_else(|| repo_root_path_display_name(project.repo_root()))
		.unwrap_or_else(|| project.service_id().to_owned())
}

fn github_repo_slug_from_origin(repo_root: &Path) -> Option<String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["config", "--get", "remote.origin.url"])
		.output()
		.ok()?;

	if !output.status.success() {
		return None;
	}

	let remote_url = String::from_utf8(output.stdout).ok()?;

	parse_github_remote_slug(remote_url.trim())
}

fn parse_github_remote_slug(remote_url: &str) -> Option<String> {
	let path = remote_url
		.strip_prefix("git@github.com:")
		.or_else(|| remote_url.strip_prefix("git@github.com-x:"))
		.or_else(|| remote_url.strip_prefix("git@github.com-y:"))
		.or_else(|| github_remote_path_with_authority(remote_url))?;
	let path = path.trim_start_matches('/').trim_end_matches(".git");
	let mut components = path.split('/').filter(|component| !component.trim().is_empty());
	let owner = components.next()?.trim();
	let repo = components.next()?.trim();

	if components.next().is_some() {
		return None;
	}

	Some(format!("{owner}/{repo}"))
}

fn github_remote_path_with_authority(remote_url: &str) -> Option<&str> {
	let rest = remote_url
		.strip_prefix("https://")
		.or_else(|| remote_url.strip_prefix("http://"))
		.or_else(|| remote_url.strip_prefix("ssh://"))?;
	let (authority, path) = rest.split_once('/')?;
	let host = authority.rsplit('@').next().unwrap_or(authority);
	let host = host.split(':').next().unwrap_or(host);

	if !matches!(host, "github.com" | "github.com-x" | "github.com-y") {
		return None;
	}

	Some(path)
}

fn repo_root_path_display_name(repo_root: &Path) -> Option<String> {
	let repo = repo_root.file_name()?.to_string_lossy();
	let repo = repo.trim();

	if repo.is_empty() {
		return None;
	}

	let Some(parent) = repo_root.parent().and_then(Path::file_name) else {
		return Some(repo.to_owned());
	};
	let parent = parent.to_string_lossy();
	let parent = parent.trim();

	if parent.is_empty() {
		return Some(repo.to_owned());
	}

	Some(format!("{parent}/{repo}"))
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
			wait_reason: "review_handoff_writeback",
			current_operation: RUN_OPERATION_REVIEW_WRITEBACK,
		}),
		"review_repair" => Some(OperatorTerminalFinalizeProjection {
			status: "review_repair_pending",
			phase: "terminal_pending",
			wait_reason: "review_repair_writeback",
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

fn operator_run_visible_status(
	attempt_status: &str,
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
	marker_current_operation: Option<&str>,
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
	if attempt_status == "succeeded"
		&& operator_marker_operation_allows_terminal_status_promotion(marker_current_operation)
		&& operator_run_has_live_process_or_thread_evidence(app_server_state, timing)
	{
		return String::from("running");
	}
	if matches!(attempt_status, "failed" | "interrupted" | "stalled")
		&& operator_marker_operation_allows_terminal_status_promotion(marker_current_operation)
		&& operator_run_has_live_execution_evidence(app_server_state, protocol_summary, timing)
	{
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
	marker_current_operation: Option<&str>,
) -> Option<String> {
	if attempt_status == visible_status || visible_status != "running" {
		return None;
	}

	let projection_kind = if attempt_status == "starting" {
		"starting_attempt"
	} else if matches!(attempt_status, "failed" | "interrupted" | "stalled" | "succeeded")
		&& operator_marker_operation_allows_terminal_status_promotion(marker_current_operation)
	{
		"terminal_attempt"
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

fn operator_run_has_live_process_or_thread_evidence(
	app_server_state: &OperatorRunAppServerState,
	timing: &OperatorRunTiming,
) -> bool {
	timing.process_alive == Some(true)
		|| matches!(app_server_state.thread_status.as_deref(), Some("active"))
		|| !app_server_state.thread_active_flags.is_empty()
}

fn operator_marker_operation_allows_terminal_status_promotion(
	marker_current_operation: Option<&str>,
) -> bool {
	matches!(marker_current_operation, None | Some(RUN_OPERATION_AGENT_RUN))
}

fn operator_run_has_live_execution_evidence(
	app_server_state: &OperatorRunAppServerState,
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
) -> bool {
	operator_run_has_live_process_or_thread_evidence(app_server_state, timing)
		|| operator_run_has_recent_protocol_execution_evidence(protocol_summary, timing)
}

fn operator_run_has_recent_protocol_execution_evidence(
	protocol_summary: &OperatorRunProtocolSummary,
	timing: &OperatorRunTiming,
) -> bool {
	operator_protocol_event_counts_as_live_execution(protocol_summary.last_event_type.as_deref())
		&& timing.protocol_idle_for_seconds.is_some_and(|idle_for| {
			u64::try_from(idle_for)
				.is_ok_and(|idle_for| idle_for < ACTIVE_RUN_IDLE_TIMEOUT.as_secs())
		})
}

fn operator_protocol_event_counts_as_live_execution(event_type: Option<&str>) -> bool {
	let Some(event_type) = event_type else {
		return false;
	};

	state::protocol_event_counts_as_work_progress(event_type)
		&& !matches!(
			event_type.to_ascii_lowercase().as_str(),
			"thread/archive" | "turn/completed"
		)
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
	let mut summary = marker
		.and_then(RunActivityMarker::child_agent_activity)
		.or(stored_summary)
		.cloned()?;

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

fn format_optional_i64(value: Option<i64>) -> String {
	value.map_or_else(|| String::from("none"), |value| value.to_string())
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
	now_unix_epoch: i64,
	idle_timeout: Duration,
) -> Option<String> {
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
		.is_none_or(|idle_for| {
			idle_for >= suspected_operator_run_stall_threshold(idle_timeout)
		});

	progress_is_stale.then(|| String::from("protocol_only_activity"))
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

	if let Some(status_source) = snapshot.status_source.as_deref() {
		output.push_str(&format!("Status source: {status_source}\n"));
	}
	if let Some(snapshot_age_seconds) = snapshot.snapshot_age_seconds {
		output.push_str(&format!("Snapshot age: {snapshot_age_seconds}s\n"));
	}

	output.push_str(&format!("Warnings: {}\n", snapshot.warnings.len()));

	if !snapshot.warnings.is_empty() {
		output.push_str(&format!("Warning details: {}\n", render_warning_details(snapshot)));
	}

	append_rendered_github_cli_authority(&mut output, snapshot);

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
	output.push_str(&format!(
		"Execution programs: {}\n",
		snapshot.execution_programs.len()
	));
	output.push_str(&format!("Recovery worktrees: {}\n", recovery_worktrees.len()));
	output.push_str(&format!("Post-review lanes: {}\n", snapshot.post_review_lanes.len()));

	append_rendered_attention_summary(&mut output, snapshot);
	append_rendered_execution_programs(&mut output, snapshot);

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

	append_rendered_post_review_lanes(&mut output, snapshot);

	output
}

fn append_rendered_attention_summary(output: &mut String, snapshot: &OperatorStatusSnapshot) {
	let current_attention_count = snapshot
		.projects
		.iter()
		.find(|project| project.project_id == snapshot.project_id)
		.or_else(|| snapshot.projects.first())
		.map_or_else(|| project_attention_count(snapshot, None), |project| project.attention_count);
	let history_only_attention_count = project_history_only_attention_count(snapshot);

	output.push_str(&format!("Current attention: {current_attention_count}\n"));
	output.push_str(&format!(
		"History-only terminal attention: {history_only_attention_count}\n"
	));

	if current_attention_count == 0 && history_only_attention_count > 0 {
		output.push_str(
			"Current attention action: none; terminal attention rows below are Run Ledger history only.\n",
		);
	}
}

fn append_rendered_execution_programs(output: &mut String, snapshot: &OperatorStatusSnapshot) {
	output.push_str("\nExecution Programs\n");

	if snapshot.execution_programs.is_empty() {
		output.push_str("- none\n");

		return;
	}

	for program in &snapshot.execution_programs {
		let mapped_issues = if program.mapped_issue_identifiers.is_empty() {
			String::from("none")
		} else {
			program.mapped_issue_identifiers.join(", ")
		};
		let readback_warning = program
			.readback_warning
			.as_ref()
			.map_or_else(String::new, |warning| format!(" readback_warning={warning}"));
		let intake_kind = program.intake_kind.as_deref().unwrap_or("unknown");
		let public_summary = program.public_summary.as_deref().unwrap_or("none");

		output.push_str(&format!(
			"- program_id: {} status={} source_contract_id: {} intake_kind={} summary=\"{}\" nodes={} planned={} mapped={} ready={} queued={} blocked={} held={} active={} attention={} completed={} stale={} superseded={} dispatchable={} mapped_issues={}{}\n",
			program.program_id,
			program.status,
			program.source_contract_id.as_deref().unwrap_or("none"),
			intake_kind,
			public_summary,
			program.node_count,
			program.planned_count,
			program.mapped_count,
			program.ready_count,
			program.queued_count,
			program.blocked_count,
			program.held_count,
			program.active_count,
			program.needs_attention_count,
			program.completed_count,
			program.stale_count,
			program.superseded_count,
			program.dispatchable_count,
			mapped_issues,
			readback_warning,
		));

		for node in &program.node_readbacks {
			let issue_identifier = node.issue_identifier.as_deref().unwrap_or("unmapped");
			let issue_state = node.issue_state.as_deref().unwrap_or("none");
			let dispatch_action = node.dispatch_action.as_deref().unwrap_or("none");
			let reason_codes = if node.reason_codes.is_empty() {
				String::from("none")
			} else {
				node.reason_codes.join(",")
			};
			let reasons = if node.reasons.is_empty() {
				String::from("none")
			} else {
				node.reasons.join(" | ")
			};

			output.push_str(&format!(
				"  - node: issue={} issue_state={} lifecycle={} readiness={} dispatch_action={} reason_codes={} reasons=\"{}\" next_action=\"{}\"\n",
				issue_identifier,
				issue_state,
				node.lifecycle_state,
				node.readiness_state,
				dispatch_action,
				reason_codes,
				reasons,
				node.next_action,
			));
		}
	}
}

fn append_rendered_github_cli_authority(output: &mut String, snapshot: &OperatorStatusSnapshot) {
	if let Some(authority) = rendered_project_github_cli_authority(snapshot) {
		output.push_str(&format!(
			"GitHub CLI: tier={} available={} command_path={} resolved_path={} configured_path={} next_action={}\n",
			authority.discovery_tier,
			authority.available,
			authority.command_path,
			authority.resolved_path.as_deref().unwrap_or("none"),
			authority.configured_path.as_deref().unwrap_or("none"),
			authority.next_action
		));
	}
}

fn append_rendered_post_review_lanes(output: &mut String, snapshot: &OperatorStatusSnapshot) {
	if snapshot.post_review_lanes.is_empty() {
		output.push_str("- none\n");

		return;
	}

	for lane in &snapshot.post_review_lanes {
		let loop_status = render_loop_status_summary(lane.loop_status.as_ref());
		let loop_review = render_loop_review_summary(lane.loop_status.as_ref());
		let loop_architecture_recovery =
			render_loop_architecture_recovery_summary(lane.loop_status.as_ref());
		let loop_boundary = render_loop_boundary_summary(lane.loop_status.as_ref());

		output.push_str(&format!(
			"- issue_id: {}\n  issue: {}\n  state: {}\n  classification: {}\n  reason: {}\n  branch: {}\n  worktree_path: {}\n  pr_url: {}\n  pr_head_sha: {}\n  pr_state: {}\n  review_decision: {}\n  mergeable: {}\n  check_state: {}\n  unresolved_review_threads: {}\n  readback_warning: {}\n  readback_root_cause: {}\n  loop_status: {}\n  loop_review: {}\n  loop_architecture_recovery: {}\n  loop_boundary: {}\n",
			lane.issue_id,
			lane.issue_identifier,
			lane.issue_state,
			lane.classification,
			lane.reason,
			lane.branch_name,
			lane.worktree_path,
			lane.pr_url.as_deref().unwrap_or("none"),
			lane.pr_head_sha.as_deref().unwrap_or("none"),
			lane.pr_state.as_deref().unwrap_or("none"),
			lane.review_decision.as_deref().unwrap_or("none"),
			lane.mergeable.as_deref().unwrap_or("none"),
			lane.check_state.as_deref().unwrap_or("none"),
			lane
				.unresolved_review_threads
				.map_or_else(|| String::from("none"), |value| value.to_string()),
			lane.readback_warning.as_deref().unwrap_or("none"),
			lane.readback_root_cause.as_deref().unwrap_or("none"),
			loop_status,
			loop_review,
			loop_architecture_recovery,
			loop_boundary
		));
	}
}

fn rendered_project_github_cli_authority(
	snapshot: &OperatorStatusSnapshot,
) -> Option<&OperatorGitHubCliAuthority> {
	snapshot
		.projects
		.iter()
		.find(|project| project.project_id == snapshot.project_id)
		.or_else(|| snapshot.projects.first())
		.map(|project| &project.github_cli_authority)
}

fn render_warning_details(snapshot: &OperatorStatusSnapshot) -> String {
	snapshot
		.warnings
		.iter()
		.flat_map(|warning| {
			let details = snapshot
				.warning_details
				.iter()
				.filter(|detail| &detail.warning == warning)
				.collect::<Vec<_>>();

			if details.is_empty() {
				return vec![warning.clone()];
			}

			details.into_iter().map(format_warning_detail).collect()
		})
		.collect::<Vec<_>>()
		.join("; ")
}

fn format_warning_detail(detail: &OperatorSnapshotWarningDetail) -> String {
	let mut parts = vec![detail.warning.clone()];

	if let Some(project_id) = detail.project_id.as_deref() {
		parts.push(format!("project={project_id}"));
	}
	if let Some(repo_root) = detail.repo_root.as_deref() {
		parts.push(format!("repo_root={repo_root}"));
	}

	parts.push(format!("reason={}", detail.reason));

	if let Some(next_action) = detail.next_action.as_deref() {
		parts.push(format!("next_action={next_action}"));
	}

	parts.join(" ")
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
			"- issue_id: {}\n  issue: {}\n  state: {}\n  role: {}\n  reason: {}\n  branch: {}\n  worktree_path: {}\n  provenance_source: {}\n  provenance_created_at_unix: {}\n  provenance_updated_at_unix: {}\n  audit_required: {}\n  recovery_next_action: {}\n",
			worktree.issue_id,
			worktree.issue_identifier.as_deref().unwrap_or("none"),
			worktree.issue_state.as_deref().unwrap_or("unknown"),
			role,
			worktree.ownership_reason,
			worktree.branch_name,
			worktree.worktree_path,
			worktree.provenance.source,
			format_optional_i64(worktree.provenance.created_at_unix),
			format_optional_i64(worktree.provenance.updated_at_unix),
			worktree.provenance.audit_required,
			worktree.recovery_next_action.as_deref().unwrap_or("none")
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

fn hydrate_active_run_lifecycle_metrics(
	active_runs: &mut [OperatorRunStatus],
	recent_runs: &[OperatorRunStatus],
) {
	for active_run in active_runs {
		let group_key = operator_run_group_key(active_run);
		let active_run_id = active_run.run_id.clone();
		let active_snapshot = active_run.clone();
		let mut attempts = Vec::new();
		let mut captured_active_snapshot = false;

		for run in recent_runs
			.iter()
			.filter(|run| operator_run_group_key(run) == group_key)
		{
			if run.run_id == active_run_id {
				captured_active_snapshot = true;

				attempts.push(active_snapshot.clone());
			} else {
				attempts.push(run.clone());
			}
		}

		if !captured_active_snapshot {
			attempts.push(active_snapshot);
		}

		active_run.lifecycle_metrics = operator_lane_lifecycle_metrics(&attempts);
	}
}

fn operator_lane_lifecycle_metrics(
	attempts: &[OperatorRunStatus],
) -> OperatorLaneLifecycleMetrics {
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

		metrics.protocol_event_count = metrics
			.protocol_event_count
			.saturating_add(run.event_count.max(0));

		let Some(summary) = run.child_agent_activity.as_ref() else {
			continue;
		};

		metrics.captured_attempt_count += 1;
		metrics.child_event_count = metrics
			.child_event_count
			.saturating_add(summary.event_count.max(0));
		metrics.wall_seconds = metrics.wall_seconds.saturating_add(summary.wall_seconds.max(0));
		metrics.tool_call_count = metrics
			.tool_call_count
			.saturating_add(summary.tool_call_count.max(0));
		metrics.input_tokens_current =
			max_optional_i64(metrics.input_tokens_current, summary.input_tokens_current);
		metrics.input_tokens_peak = max_optional_i64(metrics.input_tokens_peak, summary.input_tokens_max);
		metrics.input_tokens_cumulative = metrics
			.input_tokens_cumulative
			.saturating_add(summary.input_tokens_cumulative.max(0));
		metrics.output_tokens_cumulative = metrics
			.output_tokens_cumulative
			.saturating_add(summary.output_tokens_cumulative.max(0));

		if summary
			.largest_tool_output_bytes
			.is_some_and(|bytes| metrics.largest_tool_output_bytes.is_none_or(|current| bytes > current))
		{
			metrics.largest_tool_output_bytes = summary.largest_tool_output_bytes;
			metrics.largest_tool_output_tool = summary.largest_tool_output_tool.clone();
		}

		for warning in &summary.large_output_warnings {
			if !warning.trim().is_empty() {
				warning_set.insert(warning.clone());
			}
		}
		for bucket in &summary.buckets {
			let total = bucket_totals
				.entry(bucket.name.clone())
				.or_insert_with(|| ChildAgentActivityBucket {
					name: bucket.name.clone(),
					..ChildAgentActivityBucket::default()
				});

			total.wall_seconds = total.wall_seconds.saturating_add(bucket.wall_seconds.max(0));
			total.event_count = total.event_count.saturating_add(bucket.event_count.max(0));
			total.tool_call_count = total
				.tool_call_count
				.saturating_add(bucket.tool_call_count.max(0));
			total.input_tokens = total.input_tokens.saturating_add(bucket.input_tokens.max(0));
			total.output_tokens = total.output_tokens.saturating_add(bucket.output_tokens.max(0));
			total.output_bytes = total.output_bytes.saturating_add(bucket.output_bytes.max(0));
		}
	}

	metrics.missing_attempt_count = metrics
		.attempt_count
		.saturating_sub(metrics.captured_attempt_count);
	metrics.run_count = run_ids.len();
	metrics.large_output_warnings = warning_set.into_iter().collect();

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
		let entry = groups.entry(phase.key.to_owned()).or_insert_with(|| {
			(phase.label.to_owned(), phase.rank, Vec::new())
		});

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
				},
			)
		})
		.collect::<Vec<_>>();

	phases.sort_by(|(left_rank, left), (right_rank, right)| {
		left_rank
			.cmp(right_rank)
			.then_with(|| left.phase.cmp(&right.phase))
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

	if let Some(review_phase) = run
		.loop_status
		.as_ref()
		.and_then(|status| status.review.as_ref())
		.map(|review| review.phase.as_str())
	{
		return match review_phase {
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
		let loop_status = render_loop_status_summary(attention.loop_status.as_ref());
		let loop_review = render_loop_review_summary(attention.loop_status.as_ref());
		let loop_architecture_recovery =
			render_loop_architecture_recovery_summary(attention.loop_status.as_ref());
		let loop_boundary = render_loop_boundary_summary(attention.loop_status.as_ref());

		output.push_str(&format!(
			"  attention: {}\n  attention_run: {}\n  attention_attempt: {}\n  attention_operation: {}\n  attention_thread: {}\n  attention_cause: {}\n  attention_next_action: {}\n  attention_auto_retry: {}\n  attention_retry_budget_attempts: {}\n  attention_worktree: {}\n  attention_last_activity: {}\n  loop_status: {}\n  loop_review: {}\n  loop_architecture_recovery: {}\n  loop_boundary: {}\n",
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
			loop_status,
			loop_review,
			loop_architecture_recovery,
			loop_boundary
		));

		if let Some(decision_request) = attention.decision_request.as_ref() {
			output.push_str(&format!(
				"  decision_request_phase: {}\n  decision_request_reason: {}\n  decision_request_boundary: {}\n  decision_request_id: {}\n  decision_request_next_action: {}\n",
				decision_request.phase,
				decision_request.reason,
				decision_request.boundary,
				decision_request.decision_request_id,
				decision_request.next_action
			));
		}
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
		matches!(
			candidate.reason.as_str(),
			"issue_needs_attention" | QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT
		)
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

fn render_loop_status_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(status) = status else {
		return String::from("none");
	};
	let next_action = status.next_action.as_deref().unwrap_or("none");

	format!(
		"{}; review_level={}; autonomy={}; next_action={next_action}",
		status.summary, status.review_level, status.autonomy
	)
}

fn render_loop_review_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(review) = status.and_then(|status| status.review.as_ref()) else {
		return String::from("none");
	};
	let checkpoint = review.checkpoint.as_ref().map_or_else(
		|| String::from("checkpoint=none"),
		|checkpoint| {
			format!(
				"checkpoint=head:{} round:{} updated:{}",
				checkpoint.head_sha, checkpoint.round, checkpoint.updated_at
			)
		},
	);

	format!("phase={} status={} {checkpoint}", review.phase, review.status)
}

fn render_loop_architecture_recovery_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(recovery) = status.and_then(|status| status.architecture_recovery.as_ref()) else {
		return String::from("none");
	};
	let budget = recovery.budget.as_ref().map_or_else(
		|| String::from("none"),
		|budget| format!("{}/{}", budget.attempt, budget.max_attempts),
	);

	format!(
		"status={} reason={} guardrail={} boundary={} budget={} next_action={}",
		recovery.status,
		recovery.reason_code,
		recovery.guardrail_reason.as_deref().unwrap_or("none"),
		recovery.boundary_disposition.as_deref().unwrap_or("none"),
		budget,
		recovery.next_action
	)
}

fn render_loop_boundary_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(boundary) = status.and_then(|status| status.boundary.as_ref()) else {
		return String::from("none");
	};

	format!(
		"disposition={} reason={} attempted_recovery={} changed_surfaces={} improvement_signals={}",
		boundary.disposition,
		boundary.reason.as_deref().unwrap_or("none"),
		boundary.attempted_recovery_reason.as_deref().unwrap_or("none"),
		boundary.changed_surface_count,
		boundary.improvement_signal_count
	)
}

fn render_control_capability_summary(
	capability: Option<&OperatorRunControlCapability>,
) -> String {
	let Some(capability) = capability else {
		return String::from("none");
	};
	let thread_id = capability.thread_id.as_deref().unwrap_or("none");
	let turn_id = capability.turn_id.as_deref().unwrap_or("none");

	format!(
		"status={}; transport={}; channel={}; thread_id={thread_id}; turn_id={turn_id}",
		capability.status, capability.transport, capability.channel_path
	)
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

	output.push_str(&format!(
		"  lifecycle_metrics: {}\n",
		render_lane_lifecycle_metrics(&lane.lifecycle_metrics)
	));

	if history_ledger_outcome_has_records(&lane.ledger_outcome) {
		output.push_str(&format!(
			"  local_attempts: {}\n  latest_run_id: {}\n",
			lane.attempt_count, lane.latest_run.run_id
		));
	} else {
		append_rendered_run(output, &lane.latest_run);
	}
	if lane.lifecycle_metrics.phases.is_empty() {
		return;
	}

	output.push_str("  phase_breakdown:\n");

	for phase in &lane.lifecycle_metrics.phases {
		output.push_str(&format!(
			"    - phase: {} label: {} attempts: {} captured: {}/{} protocol_events: {} child_events: {} wall: {} tool_calls: {} input_tokens: {} output_tokens: {}\n",
			phase.phase,
			phase.label,
			phase.attempt_count,
			phase.captured_attempt_count,
			phase.attempt_count,
			phase.protocol_event_count,
			phase.child_event_count,
			format_seconds_compact(phase.wall_seconds),
			phase.tool_call_count,
			phase.input_tokens_cumulative,
			phase.output_tokens_cumulative,
		));
	}
}

fn render_lane_lifecycle_metrics(metrics: &OperatorLaneLifecycleMetrics) -> String {
	format!(
		"attempts={}; captured={}/{}; missing={}; protocol_events={}; child_events={}; wall={}; tool_calls={}; input_tokens={}; output_tokens={}",
		metrics.attempt_count,
		metrics.captured_attempt_count,
		metrics.attempt_count,
		metrics.missing_attempt_count,
		metrics.protocol_event_count,
		metrics.child_event_count,
		format_seconds_compact(metrics.wall_seconds),
		metrics.tool_call_count,
		metrics.input_tokens_cumulative,
		metrics.output_tokens_cumulative,
	)
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
	let private_evidence = render_private_evidence_reference(run);
	let loop_status = render_loop_status_summary(run.loop_status.as_ref());
	let loop_review = render_loop_review_summary(run.loop_status.as_ref());
	let loop_architecture_recovery =
		render_loop_architecture_recovery_summary(run.loop_status.as_ref());
	let loop_boundary = render_loop_boundary_summary(run.loop_status.as_ref());
	let control_capability = render_control_capability_summary(run.control_capability.as_ref());

	output.push_str(&format!(
		"- run_id: {}\n  project_id: {}\n  issue_id: {}\n  issue_identifier: {}\n  title: {}\n  attempt: {}\n  status: {}\n  attempt_status: {}\n  status_projection_reason: {}\n  phase: {}\n  wait_reason: {}\n  current_operation: {}\n  active_lease: {}\n  queue_lease_state: {}\n  queue_lease: {}\n  execution_liveness: {}\n  freshness_at: {}\n  freshness_source: {}\n  timing: run_idle={} protocol_idle={} last_progress={} protocol_event={} events={}\n  account: {}\n  accounts: {}\n  child_agent_activity: {}\n  protocol_activity: {}\n  context_pressure: {}\n  private_evidence: {}\n  loop_status: {}\n  loop_review: {}\n  loop_architecture_recovery: {}\n  loop_boundary: {}\n  control_capability: {}\n  thread_id: {}\n  turn_id: {}\n  thread_status: {}\n  thread_active_flags: {}\n  interactive_requested: {}\n  continuation_pending: {}\n  branch: {}\n  worktree_path: {}\n  updated_at: {}\n  last_run_activity_at: {}\n  last_protocol_activity_at: {}\n  last_progress_at: {}\n  idle_for_seconds: {}\n  protocol_idle_for_seconds: {}\n  suspected_stall: {}\n  progress_diagnostic: {}\n  process_id: {}\n  process_alive: {}\n  process_liveness_reason: {}\n  retry_kind: {}\n  next_retry_at: {}\n  effective_model: {}\n  effective_model_provider: {}\n  effective_cwd: {}\n  effective_approval_policy: {}\n  effective_approvals_reviewer: {}\n  effective_sandbox_mode: {}\n  protocol_event: {}\n  event_count: {}\n",
		run.run_id,
		run.project_id,
		run.issue_id,
		run.issue_identifier.as_deref().unwrap_or("none"),
		run.title.as_deref().unwrap_or("none"),
		run.attempt_number,
		run.status,
		run.attempt_status,
		run.status_projection_reason.as_deref().unwrap_or("none"),
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
		private_evidence,
		loop_status,
		loop_review,
		loop_architecture_recovery,
		loop_boundary,
		control_capability,
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
		run.progress_diagnostic.as_deref().unwrap_or("none"),
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
		EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH if operator_run_has_recent_app_server_execution(run) => {
			String::from("not_held (app_server_activity keeps lane visible)")
		},
		EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH => {
			String::from("not_held (process_identity_mismatch needs attention)")
		},
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
