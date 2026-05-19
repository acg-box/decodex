use std::collections::{self, BTreeMap};

const AGENT_HANDOFF_INDEX_SCHEMA: &str = "decodex.agent_handoff_index/1";
const AGENT_BLOCKER_SNAPSHOT_SCHEMA: &str = "decodex.blocker_snapshot/1";
const AGENT_RUN_CAPSULE_SCHEMA: &str = "decodex.run_capsule/1";
const AGENT_EVIDENCE_EVENT_SCHEMA: &str = "decodex.agent_evidence_event/1";
const HANDOFF_INDEX_FILE_NAME: &str = "handoff-index.json";
const BLOCKERS_DIR_NAME: &str = "blockers";
const RUNS_DIR_NAME: &str = "runs";
const EVENTS_FILE_NAME: &str = "events.jsonl";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentEvidenceSource {
	DiagnoseCommand,
	ServeTick,
}
impl AgentEvidenceSource {
	fn as_str(self) -> &'static str {
		match self {
			Self::DiagnoseCommand => "diagnose_command",
			Self::ServeTick => "serve_tick",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentEvidenceWriteResult {
	project_id: String,
	handoff_index_path: String,
	handoff_index: AgentHandoffIndex,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentHandoffIndex {
	schema: &'static str,
	project_id: String,
	generated_at: String,
	source: String,
	evidence_root: String,
	handoff_index_path: String,
	blockers_dir: String,
	runs_dir: String,
	events_path: String,
	summary: AgentEvidenceSummary,
	warnings: Vec<String>,
	connector_backoffs: Vec<AgentConnectorBackoff>,
	blockers: Vec<AgentBlocker>,
	run_capsules: Vec<AgentRunCapsuleRef>,
	recovery_worktrees: Vec<AgentRecoveryWorktree>,
	recovery_contracts: Vec<AgentRecoveryContract>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentEvidenceSummary {
	project_count: usize,
	active_run_count: usize,
	recent_run_count: usize,
	history_lane_count: usize,
	queued_candidate_count: usize,
	post_review_lane_count: usize,
	recovery_worktree_count: usize,
	blocker_count: usize,
	run_capsule_count: usize,
	connector_backoff_count: usize,
	warning_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentConnectorBackoff {
	evidence_ref: String,
	connector: String,
	sync_phase: String,
	quota_class: String,
	reset_at: String,
	reset_unix_epoch: i64,
	reset_source: String,
	retry_after_seconds: i64,
	warning: String,
	next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentBlocker {
	evidence_ref: String,
	project_id: String,
	surface: String,
	issue_id: Option<String>,
	issue_identifier: Option<String>,
	run_id: Option<String>,
	attempt_number: Option<i64>,
	classification: String,
	reason_code: String,
	reason: String,
	next_action: String,
	blocker_snapshot_path: String,
	related_run_capsule_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentBlockerSnapshot {
	schema: &'static str,
	project_id: String,
	generated_at: String,
	issue_id: Option<String>,
	issue_identifier: Option<String>,
	blockers: Vec<AgentBlocker>,
	related_run_capsules: Vec<AgentRunCapsuleRef>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentRunCapsuleRef {
	evidence_ref: String,
	run_id: String,
	issue_id: String,
	issue_identifier: Option<String>,
	attempt_number: i64,
	status: String,
	phase: String,
	current_operation: String,
	path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentRunCapsule {
	schema: &'static str,
	evidence_ref: String,
	project_id: String,
	generated_at: String,
	path: String,
	run_id: String,
	issue_id: String,
	issue_identifier: Option<String>,
	title: Option<String>,
	attempt_number: i64,
	status: String,
	attempt_status: String,
	phase: String,
	wait_reason: Option<String>,
	current_operation: String,
	queue_lease_state: String,
	execution_liveness: String,
	active_lease: bool,
	continuation_pending: bool,
	suspected_stall: bool,
	thread_id: Option<String>,
	turn_id: Option<String>,
	thread_status: Option<String>,
	thread_active_flags: Vec<String>,
	interactive_requested: bool,
	process_id: Option<u32>,
	process_alive: Option<bool>,
	process_liveness_reason: Option<String>,
	event_count: i64,
	last_event_type: Option<String>,
	last_event_at: Option<String>,
	last_run_activity_at: Option<String>,
	last_protocol_activity_at: Option<String>,
	last_progress_at: Option<String>,
	idle_for_seconds: Option<i64>,
	protocol_idle_for_seconds: Option<i64>,
	retry_kind: Option<String>,
	next_retry_at: Option<String>,
	effective_model: Option<String>,
	effective_model_provider: Option<String>,
	effective_cwd: Option<String>,
	effective_approval_policy: Option<String>,
	effective_approvals_reviewer: Option<String>,
	effective_sandbox_mode: Option<String>,
	branch_name: Option<String>,
	worktree_path: Option<String>,
	ledger_outcome: Option<AgentRunLedgerOutcome>,
	diagnosis: AgentRunDiagnosis,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentRunLedgerOutcome {
	ledger_status: String,
	final_outcome: String,
	final_event_type: Option<String>,
	final_event_at: Option<String>,
	summary: Option<String>,
	pr_url: Option<String>,
	commit_sha: Option<String>,
	closeout_status: Option<String>,
	needs_attention_reason: Option<String>,
	record_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentRunDiagnosis {
	attention_required: bool,
	reason_code: Option<String>,
	next_action: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentRecoveryWorktree {
	issue_id: String,
	issue_identifier: Option<String>,
	issue_state: Option<String>,
	branch_name: String,
	worktree_path: String,
	role: String,
	ownership: String,
	ownership_reason: String,
	hygiene_classification: Option<String>,
	hygiene_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentRecoveryContract {
	evidence_ref: String,
	kind: String,
	issue_identifier: Option<String>,
	reason_code: String,
	command: Option<String>,
	next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct AgentEvidenceEvent {
	schema: &'static str,
	project_id: String,
	generated_at: String,
	source: String,
	handoff_index_path: String,
	blocker_count: usize,
	run_capsule_count: usize,
	warning_count: usize,
	connector_backoff_count: usize,
}

struct AgentEvidenceFileWriteContext<'a> {
	project_id: &'a str,
	generated_at: &'a str,
	source: AgentEvidenceSource,
	handoff_index_path: &'a Path,
	blockers_dir: &'a Path,
	events_path: &'a Path,
}

struct AgentEvidenceProjectView<'a> {
	project_id: &'a str,
	warnings: Vec<String>,
	projects: Vec<&'a OperatorProjectStatus>,
	connector_backoffs: Vec<&'a OperatorConnectorBackoffStatus>,
	active_runs: Vec<&'a OperatorRunStatus>,
	recent_runs: Vec<&'a OperatorRunStatus>,
	history_lanes: Vec<&'a OperatorHistoryLaneStatus>,
	queued_candidates: Vec<&'a OperatorQueuedIssueStatus>,
	recovery_worktrees: Vec<(&'a str, &'a OperatorWorktreeStatus)>,
	post_review_lanes: Vec<&'a OperatorPostReviewLaneStatus>,
}
impl<'a> AgentEvidenceProjectView<'a> {
	fn from_snapshot(snapshot: &'a OperatorStatusSnapshot, project_id: &'a str) -> Self {
		let single_project_snapshot = snapshot.project_id == project_id;
		let projects = snapshot
			.projects
			.iter()
			.filter(|project| project.project_id == project_id)
			.collect::<Vec<_>>();
		let connector_backoffs = snapshot
			.connector_backoffs
			.iter()
			.filter(|backoff| backoff.project_id == project_id)
			.collect::<Vec<_>>();
		let active_runs = snapshot
			.active_runs
			.iter()
			.filter(|run| run.project_id == project_id)
			.collect::<Vec<_>>();
		let recent_runs = snapshot
			.recent_runs
			.iter()
			.filter(|run| run.project_id == project_id)
			.collect::<Vec<_>>();
		let history_lanes = snapshot
			.history_lanes
			.iter()
			.filter(|lane| lane.project_id == project_id)
			.collect::<Vec<_>>();
		let post_review_lanes = snapshot
			.post_review_lanes
			.iter()
			.filter(|lane| lane_issue_belongs_to_project(lane.issue_id.as_str(), project_id, snapshot))
			.collect::<Vec<_>>();
		let queued_candidates = snapshot
			.queued_candidates
			.iter()
			.filter(|candidate| {
				lane_issue_belongs_to_project(candidate.issue_id.as_str(), project_id, snapshot)
			})
			.collect::<Vec<_>>();
		let recovery_worktrees = if single_project_snapshot {
			rendered_recovery_worktrees(snapshot)
		} else {
			rendered_recovery_worktrees(snapshot)
				.into_iter()
				.filter(|(_, worktree)| {
					lane_issue_belongs_to_project(worktree.issue_id.as_str(), project_id, snapshot)
				})
				.collect()
		};

		Self {
			project_id,
			warnings: snapshot.warnings.clone(),
			projects,
			connector_backoffs,
			active_runs,
			recent_runs,
			history_lanes,
			queued_candidates,
			recovery_worktrees,
			post_review_lanes,
		}
	}
}

fn write_agent_evidence_snapshot(
	snapshot: &OperatorStatusSnapshot,
	source: AgentEvidenceSource,
) -> Result<Vec<AgentEvidenceWriteResult>> {
	let generated_at = current_timestamp();
	let month_bucket = current_month_bucket();
	let mut results = Vec::new();

	for project_id in agent_evidence_project_ids(snapshot) {
		let service_root = runtime::agent_evidence_dir()?.join(&project_id);
		let handoff_index_path = service_root.join(HANDOFF_INDEX_FILE_NAME);
		let blockers_dir = service_root.join(BLOCKERS_DIR_NAME);
		let runs_dir = service_root.join(RUNS_DIR_NAME);
		let events_path = service_root.join(EVENTS_FILE_NAME);
		let project_view = AgentEvidenceProjectView::from_snapshot(snapshot, &project_id);
		let mut run_capsules = build_run_capsules(
			&project_view,
			&generated_at,
			&runs_dir,
			&month_bucket,
		);

		run_capsules.sort_by(|left, right| {
			left
				.issue_identifier
				.cmp(&right.issue_identifier)
				.then_with(|| left.issue_id.cmp(&right.issue_id))
				.then_with(|| left.attempt_number.cmp(&right.attempt_number))
				.then_with(|| left.run_id.cmp(&right.run_id))
		});

		let run_refs = run_capsules
			.iter()
			.map(run_capsule_ref)
			.collect::<Vec<_>>();
		let blockers = build_agent_blockers(&project_view, &blockers_dir, &run_refs);
		let recovery_worktrees = project_view
			.recovery_worktrees
			.iter()
			.map(|(role, worktree)| agent_recovery_worktree(role, worktree))
			.collect::<Vec<_>>();
		let recovery_contracts = blockers.iter().filter_map(agent_recovery_contract).collect();
		let connector_backoffs = project_view
			.connector_backoffs
			.iter()
			.copied()
			.map(agent_connector_backoff)
			.collect::<Vec<_>>();
		let summary = AgentEvidenceSummary {
			project_count: project_view.projects.len(),
			active_run_count: project_view.active_runs.len(),
			recent_run_count: project_view.recent_runs.len(),
			history_lane_count: project_view.history_lanes.len(),
			queued_candidate_count: project_view.queued_candidates.len(),
			post_review_lane_count: project_view.post_review_lanes.len(),
			recovery_worktree_count: recovery_worktrees.len(),
			blocker_count: blockers.len(),
			run_capsule_count: run_refs.len(),
			connector_backoff_count: connector_backoffs.len(),
			warning_count: project_view.warnings.len(),
		};
		let index = AgentHandoffIndex {
			schema: AGENT_HANDOFF_INDEX_SCHEMA,
			project_id: project_id.clone(),
			generated_at: generated_at.clone(),
			source: source.as_str().to_owned(),
			evidence_root: service_root.display().to_string(),
			handoff_index_path: handoff_index_path.display().to_string(),
			blockers_dir: blockers_dir.display().to_string(),
			runs_dir: runs_dir.display().to_string(),
			events_path: events_path.display().to_string(),
			summary,
			warnings: project_view.warnings.clone(),
			connector_backoffs,
			blockers,
			run_capsules: run_refs,
			recovery_worktrees,
			recovery_contracts,
		};
		let write_context = AgentEvidenceFileWriteContext {
			project_id: &project_id,
			generated_at: &generated_at,
			source,
			handoff_index_path: &handoff_index_path,
			blockers_dir: &blockers_dir,
			events_path: &events_path,
		};

		write_agent_evidence_files(&write_context, &index, &run_capsules)?;

		results.push(AgentEvidenceWriteResult {
			project_id,
			handoff_index_path: handoff_index_path.display().to_string(),
			handoff_index: index,
		});
	}

	Ok(results)
}

fn write_agent_evidence_best_effort(
	snapshot: &OperatorStatusSnapshot,
	source: AgentEvidenceSource,
) {
	if let Err(error) = write_agent_evidence_snapshot(snapshot, source) {
		let _ = error;

		tracing::warn!(
			"Agent evidence write failed; sensitive runtime details were withheld from logs."
		);
	}
}

fn render_agent_evidence_write_result(result: &AgentEvidenceWriteResult) -> String {
	format!(
		"agent evidence written: project={} blockers={} run_capsules={} warnings={} index={}\n",
		result.project_id,
		result.handoff_index.summary.blocker_count,
		result.handoff_index.summary.run_capsule_count,
		result.handoff_index.summary.warning_count,
		result.handoff_index_path,
	)
}

fn lane_issue_belongs_to_project(
	issue_id: &str,
	project_id: &str,
	snapshot: &OperatorStatusSnapshot,
) -> bool {
	snapshot
		.active_runs
		.iter()
		.chain(snapshot.recent_runs.iter())
		.any(|run| run.project_id == project_id && run.issue_id == issue_id)
		|| snapshot
			.history_lanes
			.iter()
			.any(|lane| lane.project_id == project_id && lane.issue_id == issue_id)
		|| snapshot.project_id == project_id
}

fn agent_evidence_project_ids(snapshot: &OperatorStatusSnapshot) -> Vec<String> {
	let mut project_ids = collections::BTreeSet::new();

	for project in &snapshot.projects {
		project_ids.insert(project.project_id.clone());
	}
	for run in snapshot.active_runs.iter().chain(snapshot.recent_runs.iter()) {
		project_ids.insert(run.project_id.clone());
	}
	for lane in &snapshot.history_lanes {
		project_ids.insert(lane.project_id.clone());
	}
	for backoff in &snapshot.connector_backoffs {
		project_ids.insert(backoff.project_id.clone());
	}

	if project_ids.is_empty() && snapshot.project_id != "all" {
		project_ids.insert(snapshot.project_id.clone());
	}

	project_ids.into_iter().collect()
}

fn build_run_capsules(
	project_view: &AgentEvidenceProjectView<'_>,
	generated_at: &str,
	runs_dir: &Path,
	month_bucket: &str,
) -> Vec<AgentRunCapsule> {
	let mut run_ids = collections::BTreeSet::new();
	let mut capsules = Vec::new();

	for run in project_view
		.active_runs
		.iter()
		.chain(project_view.recent_runs.iter())
		.copied()
	{
		if run_ids.insert(run.run_id.clone()) {
			capsules.push(agent_run_capsule(
				project_view.project_id,
				generated_at,
				runs_dir,
				month_bucket,
				run,
				ledger_outcome_for_run(run, project_view),
			));
		}
	}
	for lane in &project_view.history_lanes {
		for run in &lane.attempts {
			if run_ids.insert(run.run_id.clone()) {
				capsules.push(agent_run_capsule(
					project_view.project_id,
					generated_at,
					runs_dir,
					month_bucket,
					run,
					Some(agent_run_ledger_outcome(&lane.ledger_outcome)),
				));
			}
		}
	}

	capsules
}

fn ledger_outcome_for_run(
	run: &OperatorRunStatus,
	project_view: &AgentEvidenceProjectView<'_>,
) -> Option<AgentRunLedgerOutcome> {
	project_view
		.history_lanes
		.iter()
		.find(|lane| lane.attempts.iter().any(|attempt| attempt.run_id == run.run_id))
		.map(|lane| agent_run_ledger_outcome(&lane.ledger_outcome))
}

fn agent_run_ledger_outcome(
	outcome: &OperatorHistoryLedgerOutcome,
) -> AgentRunLedgerOutcome {
	AgentRunLedgerOutcome {
		ledger_status: outcome.ledger_status.clone(),
		final_outcome: outcome.final_outcome.clone(),
		final_event_type: outcome.final_event_type.clone(),
		final_event_at: outcome.final_event_at.clone(),
		summary: outcome.summary.clone(),
		pr_url: outcome.pr_url.clone(),
		commit_sha: outcome.commit_sha.clone(),
		closeout_status: outcome.closeout_status.clone(),
		needs_attention_reason: outcome.needs_attention_reason.clone(),
		record_count: outcome.record_count,
	}
}

fn agent_run_capsule(
	project_id: &str,
	generated_at: &str,
	runs_dir: &Path,
	month_bucket: &str,
	run: &OperatorRunStatus,
	ledger_outcome: Option<AgentRunLedgerOutcome>,
) -> AgentRunCapsule {
	let path = run_capsule_path(runs_dir, month_bucket, &run.run_id);
	let diagnosis = agent_run_diagnosis(run);

	AgentRunCapsule {
		schema: AGENT_RUN_CAPSULE_SCHEMA,
		evidence_ref: run_evidence_ref(project_id, &run.run_id),
		project_id: project_id.to_owned(),
		generated_at: generated_at.to_owned(),
		path: path.display().to_string(),
		run_id: run.run_id.clone(),
		issue_id: run.issue_id.clone(),
		issue_identifier: run.issue_identifier.clone(),
		title: run.title.clone(),
		attempt_number: run.attempt_number,
		status: run.status.clone(),
		attempt_status: run.attempt_status.clone(),
		phase: run.phase.clone(),
		wait_reason: run.wait_reason.clone(),
		current_operation: run.current_operation.clone(),
		queue_lease_state: run.queue_lease_state.clone(),
		execution_liveness: run.execution_liveness.clone(),
		active_lease: run.active_lease,
		continuation_pending: run.continuation_pending,
		suspected_stall: run.suspected_stall,
		thread_id: run.thread_id.clone(),
		turn_id: run.turn_id.clone(),
		thread_status: run.thread_status.clone(),
		thread_active_flags: run.thread_active_flags.clone(),
			interactive_requested: run.interactive_requested,
			process_id: run.process_id,
			process_alive: run.process_alive,
			process_liveness_reason: run.process_liveness_reason.clone(),
			event_count: run.event_count,
		last_event_type: run.last_event_type.clone(),
		last_event_at: run.last_event_at.clone(),
		last_run_activity_at: run.last_run_activity_at.clone(),
		last_protocol_activity_at: run.last_protocol_activity_at.clone(),
		last_progress_at: run.last_progress_at.clone(),
		idle_for_seconds: run.idle_for_seconds,
		protocol_idle_for_seconds: run.protocol_idle_for_seconds,
		retry_kind: run.retry_kind.clone(),
		next_retry_at: run.next_retry_at.clone(),
		effective_model: run.effective_model.clone(),
		effective_model_provider: run.effective_model_provider.clone(),
		effective_cwd: run.effective_cwd.clone(),
		effective_approval_policy: run.effective_approval_policy.clone(),
		effective_approvals_reviewer: run.effective_approvals_reviewer.clone(),
		effective_sandbox_mode: run.effective_sandbox_mode.clone(),
		branch_name: run.branch_name.clone(),
		worktree_path: run.worktree_path.clone(),
		ledger_outcome,
		diagnosis,
	}
}

fn run_capsule_ref(capsule: &AgentRunCapsule) -> AgentRunCapsuleRef {
	AgentRunCapsuleRef {
		evidence_ref: capsule.evidence_ref.clone(),
		run_id: capsule.run_id.clone(),
		issue_id: capsule.issue_id.clone(),
		issue_identifier: capsule.issue_identifier.clone(),
		attempt_number: capsule.attempt_number,
		status: capsule.status.clone(),
		phase: capsule.phase.clone(),
		current_operation: capsule.current_operation.clone(),
		path: capsule.path.clone(),
	}
}

fn agent_run_diagnosis(run: &OperatorRunStatus) -> AgentRunDiagnosis {
	let reason = agent_run_blocker_reason(run);

	AgentRunDiagnosis {
		attention_required: reason.is_some(),
		reason_code: reason.map(str::to_owned),
		next_action: agent_run_next_action(run).map(str::to_owned),
	}
}

fn agent_run_blocker_reason(run: &OperatorRunStatus) -> Option<&'static str> {
	if run.suspected_stall {
		return Some("suspected_stall");
	}
	if run.phase == "stalled" {
		return Some("run_stalled");
	}
	if run.process_alive == Some(false) && matches!(run.status.as_str(), "starting" | "running") {
		return Some("process_exited_without_terminal_status");
	}
	if operator_run_has_stale_execution_without_known_process(run) {
		return Some("stale_execution_without_known_process");
	}
	if run.wait_reason.is_some() {
		return Some("run_waiting");
	}
	if run.next_retry_at.is_some() {
		return Some("retry_backoff");
	}

	None
}

fn agent_run_next_action(run: &OperatorRunStatus) -> Option<&'static str> {
	match agent_run_blocker_reason(run) {
		Some("suspected_stall" | "run_stalled" | "stale_execution_without_known_process") =>
			Some("Inspect the run capsule, retained worktree, protocol activity, and process state before retrying."),
		Some("process_exited_without_terminal_status") =>
			Some("Inspect the retained worktree and runtime markers; reconcile or retry only after preserving useful local changes."),
		Some("run_waiting") =>
			Some("Inspect wait_reason, thread status, and protocol activity before deciding whether the agent can continue."),
		Some("retry_backoff") => Some("Wait until next_retry_at or run an explicit operator retry after reviewing the retained state."),
		_ => None,
	}
}

fn build_agent_blockers(
	project_view: &AgentEvidenceProjectView<'_>,
	blockers_dir: &Path,
	run_refs: &[AgentRunCapsuleRef],
) -> Vec<AgentBlocker> {
	let mut blockers = Vec::new();

	push_run_blockers(&mut blockers, project_view, blockers_dir, run_refs);
	push_queued_candidate_blockers(&mut blockers, project_view, blockers_dir, run_refs);
	push_post_review_lane_blockers(&mut blockers, project_view, blockers_dir);
	push_recovery_worktree_blockers(&mut blockers, project_view, blockers_dir);
	push_warning_blockers(&mut blockers, project_view, blockers_dir);
	push_connector_backoff_blockers(&mut blockers, project_view, blockers_dir);
	sort_agent_blockers(&mut blockers);

	blockers
}

fn push_run_blockers(
	blockers: &mut Vec<AgentBlocker>,
	project_view: &AgentEvidenceProjectView<'_>,
	blockers_dir: &Path,
	run_refs: &[AgentRunCapsuleRef],
) {
	for run in &project_view.active_runs {
		if let Some(reason_code) = agent_run_blocker_reason(run) {
			let issue_key = issue_key(run.issue_identifier.as_deref(), &run.issue_id);

			blockers.push(AgentBlocker {
				evidence_ref: blocker_evidence_ref(project_view.project_id, &issue_key, reason_code),
				project_id: project_view.project_id.to_owned(),
				surface: String::from("running_lane"),
				issue_id: Some(run.issue_id.clone()),
				issue_identifier: run.issue_identifier.clone(),
				run_id: Some(run.run_id.clone()),
				attempt_number: Some(run.attempt_number),
				classification: String::from("attention_required"),
				reason_code: reason_code.to_owned(),
				reason: run
					.wait_reason
					.clone()
					.unwrap_or_else(|| reason_code.replace('_', " ")),
				next_action: agent_run_next_action(run).unwrap_or("Inspect the run capsule.").to_owned(),
				blocker_snapshot_path: blocker_snapshot_path(blockers_dir, &issue_key)
					.display()
					.to_string(),
				related_run_capsule_path: run_refs
					.iter()
					.find(|run_ref| run_ref.run_id == run.run_id)
					.map(|run_ref| run_ref.path.clone()),
			});
		}
	}
}

fn push_queued_candidate_blockers(
	blockers: &mut Vec<AgentBlocker>,
	project_view: &AgentEvidenceProjectView<'_>,
	blockers_dir: &Path,
	run_refs: &[AgentRunCapsuleRef],
) {
	for candidate in &project_view.queued_candidates {
		if candidate.classification != "blocked" && candidate.attention.is_none() {
			continue;
		}

		let issue_key = issue_key(Some(&candidate.issue_identifier), &candidate.issue_id);
		let reason_code = candidate
			.attention
			.as_ref()
			.and_then(|attention| attention.attention_error_class.as_deref())
			.unwrap_or(candidate.reason.as_str());

		blockers.push(AgentBlocker {
			evidence_ref: blocker_evidence_ref(project_view.project_id, &issue_key, reason_code),
			project_id: project_view.project_id.to_owned(),
			surface: String::from("intake_queue"),
			issue_id: Some(candidate.issue_id.clone()),
			issue_identifier: Some(candidate.issue_identifier.clone()),
			run_id: candidate.attention.as_ref().and_then(|attention| attention.run_id.clone()),
			attempt_number: candidate.attention.as_ref().and_then(|attention| attention.attempt_number),
			classification: candidate.classification.clone(),
			reason_code: reason_code.to_owned(),
			reason: candidate
				.attention
				.as_ref()
				.map(|attention| attention.summary.clone())
				.unwrap_or_else(|| candidate.reason.clone()),
			next_action: candidate
				.attention
				.as_ref()
				.and_then(|attention| attention.attention_next_action.clone())
				.unwrap_or_else(|| String::from("Inspect the queued candidate and retained worktree before retrying.")),
			blocker_snapshot_path: blocker_snapshot_path(blockers_dir, &issue_key)
				.display()
				.to_string(),
			related_run_capsule_path: candidate
				.attention
				.as_ref()
				.and_then(|attention| attention.run_id.as_deref())
				.and_then(|run_id| run_refs.iter().find(|run_ref| run_ref.run_id == run_id))
				.map(|run_ref| run_ref.path.clone()),
		});
	}
}

fn push_post_review_lane_blockers(
	blockers: &mut Vec<AgentBlocker>,
	project_view: &AgentEvidenceProjectView<'_>,
	blockers_dir: &Path,
) {
	for lane in &project_view.post_review_lanes {
		if !post_review_lane_requires_attention(lane) {
			continue;
		}

		let issue_key = issue_key(Some(&lane.issue_identifier), &lane.issue_id);

		blockers.push(AgentBlocker {
			evidence_ref: blocker_evidence_ref(project_view.project_id, &issue_key, &lane.reason),
			project_id: project_view.project_id.to_owned(),
			surface: String::from("review_landing"),
			issue_id: Some(lane.issue_id.clone()),
			issue_identifier: Some(lane.issue_identifier.clone()),
			run_id: None,
			attempt_number: None,
			classification: lane.classification.clone(),
			reason_code: lane.reason.clone(),
			reason: lane.reason.clone(),
			next_action: post_review_lane_next_action(lane, project_view.project_id),
			blocker_snapshot_path: blocker_snapshot_path(blockers_dir, &issue_key)
				.display()
				.to_string(),
			related_run_capsule_path: None,
		});
	}
}

fn push_recovery_worktree_blockers(
	blockers: &mut Vec<AgentBlocker>,
	project_view: &AgentEvidenceProjectView<'_>,
	blockers_dir: &Path,
) {
	for (role, worktree) in &project_view.recovery_worktrees {
		if worktree.hygiene.is_none() {
			continue;
		}

		let issue_key = issue_key(worktree.issue_identifier.as_deref(), &worktree.issue_id);
		let reason_code = worktree
			.hygiene
			.as_ref()
			.map(|hygiene| hygiene.classification.as_str())
			.unwrap_or(*role);

		blockers.push(AgentBlocker {
			evidence_ref: blocker_evidence_ref(project_view.project_id, &issue_key, reason_code),
			project_id: project_view.project_id.to_owned(),
			surface: String::from("recovery_worktree"),
			issue_id: Some(worktree.issue_id.clone()),
			issue_identifier: worktree.issue_identifier.clone(),
			run_id: None,
			attempt_number: None,
			classification: (*role).to_owned(),
			reason_code: reason_code.to_owned(),
			reason: worktree.ownership_reason.clone(),
			next_action: String::from("Inspect the retained worktree before cleanup or recovery."),
			blocker_snapshot_path: blocker_snapshot_path(blockers_dir, &issue_key)
				.display()
				.to_string(),
			related_run_capsule_path: None,
		});
	}
}

fn push_warning_blockers(
	blockers: &mut Vec<AgentBlocker>,
	project_view: &AgentEvidenceProjectView<'_>,
	blockers_dir: &Path,
) {
	for warning in &project_view.warnings {
		if warning == "external_observer_status_skipped" {
			continue;
		}

		let issue_key = format!("project-{}", sanitize_evidence_path_component(warning));

		blockers.push(AgentBlocker {
			evidence_ref: blocker_evidence_ref(project_view.project_id, &issue_key, warning),
			project_id: project_view.project_id.to_owned(),
			surface: String::from("operator_snapshot"),
			issue_id: None,
			issue_identifier: None,
			run_id: None,
			attempt_number: None,
			classification: String::from("snapshot_warning"),
			reason_code: warning.clone(),
			reason: warning.clone(),
			next_action: String::from("Regenerate diagnose output after resolving the unavailable observer or runtime warning."),
			blocker_snapshot_path: blocker_snapshot_path(blockers_dir, &issue_key)
				.display()
				.to_string(),
			related_run_capsule_path: None,
		});
	}
}

fn push_connector_backoff_blockers(
	blockers: &mut Vec<AgentBlocker>,
	project_view: &AgentEvidenceProjectView<'_>,
	blockers_dir: &Path,
) {
	for backoff in &project_view.connector_backoffs {
		let issue_key = format!("connector-{}", sanitize_evidence_path_component(&backoff.connector));

		blockers.push(AgentBlocker {
			evidence_ref: blocker_evidence_ref(
				project_view.project_id,
				&issue_key,
				&backoff.warning,
			),
			project_id: project_view.project_id.to_owned(),
			surface: String::from("connector_backoff"),
			issue_id: None,
			issue_identifier: None,
			run_id: None,
			attempt_number: None,
			classification: String::from("backoff"),
			reason_code: backoff.warning.clone(),
			reason: format!("{} {}", backoff.connector, backoff.sync_phase),
			next_action: backoff.next_action.clone(),
			blocker_snapshot_path: blocker_snapshot_path(blockers_dir, &issue_key)
				.display()
				.to_string(),
			related_run_capsule_path: None,
		});
	}
}

fn sort_agent_blockers(blockers: &mut [AgentBlocker]) {
	blockers.sort_by(|left, right| {
		left
			.issue_identifier
			.cmp(&right.issue_identifier)
			.then_with(|| left.issue_id.cmp(&right.issue_id))
			.then_with(|| left.surface.cmp(&right.surface))
			.then_with(|| left.reason_code.cmp(&right.reason_code))
	});
}

fn post_review_lane_requires_attention(lane: &OperatorPostReviewLaneStatus) -> bool {
	matches!(
		lane.classification.as_str(),
		"blocked" | "needs_review_repair" | "closeout_blocked" | "cleanup_blocked"
	) || lane.reason == "missing_review_handoff_record"
}

fn post_review_lane_next_action(
	lane: &OperatorPostReviewLaneStatus,
	project_id: &str,
) -> String {
	if lane.reason == "missing_review_handoff_record" {
		return format!(
			"Run `decodex recover review-handoff diagnose {} --json`; rebind only after PR lineage and retained worktree HEAD match.",
			lane.issue_identifier
		);
	}
	if lane.classification == "needs_review_repair" {
		return String::from("Run or inspect the retained review-repair lane before attempting land.");
	}

	format!(
		"Inspect the `{}` retained post-review lane for service `{project_id}` before retrying.",
		lane.classification
	)
}

fn agent_connector_backoff(backoff: &OperatorConnectorBackoffStatus) -> AgentConnectorBackoff {
	AgentConnectorBackoff {
		evidence_ref: format!(
			"connector:{}/{}:{}",
			backoff.project_id, backoff.connector, backoff.sync_phase
		),
		connector: backoff.connector.clone(),
		sync_phase: backoff.sync_phase.clone(),
		quota_class: backoff.quota_class.clone(),
		reset_at: backoff.reset_at.clone(),
		reset_unix_epoch: backoff.reset_unix_epoch,
		reset_source: backoff.reset_source.clone(),
		retry_after_seconds: backoff.retry_after_seconds,
		warning: backoff.warning.clone(),
		next_action: backoff.next_action.clone(),
	}
}

fn agent_recovery_worktree(
	role: &str,
	worktree: &OperatorWorktreeStatus,
) -> AgentRecoveryWorktree {
	AgentRecoveryWorktree {
		issue_id: worktree.issue_id.clone(),
		issue_identifier: worktree.issue_identifier.clone(),
		issue_state: worktree.issue_state.clone(),
		branch_name: worktree.branch_name.clone(),
		worktree_path: worktree.worktree_path.clone(),
		role: role.to_owned(),
		ownership: worktree.ownership.clone(),
		ownership_reason: worktree.ownership_reason.clone(),
		hygiene_classification: worktree
			.hygiene
			.as_ref()
			.map(|hygiene| hygiene.classification.clone()),
		hygiene_reason: worktree.hygiene.as_ref().map(|hygiene| hygiene.reason.clone()),
	}
}

fn agent_recovery_contract(blocker: &AgentBlocker) -> Option<AgentRecoveryContract> {
	let command = if blocker.reason_code == "missing_review_handoff_record" {
		blocker
			.issue_identifier
			.as_ref()
			.map(|issue| format!("decodex recover review-handoff diagnose {issue} --json"))
	} else {
		None
	};

	if command.is_none() && blocker.surface != "running_lane" && blocker.surface != "intake_queue" {
		return None;
	}

	Some(AgentRecoveryContract {
		evidence_ref: blocker.evidence_ref.clone(),
		kind: blocker.surface.clone(),
		issue_identifier: blocker.issue_identifier.clone(),
		reason_code: blocker.reason_code.clone(),
		command,
		next_action: blocker.next_action.clone(),
	})
}

fn write_agent_evidence_files(
	context: &AgentEvidenceFileWriteContext<'_>,
	index: &AgentHandoffIndex,
	run_capsules: &[AgentRunCapsule],
) -> Result<()> {
	for capsule in run_capsules {
		let path = PathBuf::from(&capsule.path);

		write_json_atomically(&path, capsule)?;
	}

	write_blocker_snapshots(
		context.project_id,
		context.generated_at,
		context.blockers_dir,
		&index.blockers,
		&index.run_capsules,
	)?;
	write_json_atomically(context.handoff_index_path, index)?;
	append_agent_evidence_event(
		context.project_id,
		context.generated_at,
		context.source,
		context.events_path,
		index,
	)?;

	Ok(())
}

fn write_blocker_snapshots(
	project_id: &str,
	generated_at: &str,
	blockers_dir: &Path,
	blockers: &[AgentBlocker],
	run_refs: &[AgentRunCapsuleRef],
) -> Result<()> {
	fs::create_dir_all(blockers_dir)?;

	let mut blockers_by_path: BTreeMap<String, Vec<AgentBlocker>> = BTreeMap::new();

	for blocker in blockers {
		blockers_by_path
			.entry(blocker.blocker_snapshot_path.clone())
			.or_default()
			.push(blocker.clone());
	}

	let mut kept_paths = collections::BTreeSet::new();

	for (path, blockers) in blockers_by_path {
		let path = PathBuf::from(path);
		let related_run_capsules = blockers
			.iter()
			.filter_map(|blocker| blocker.run_id.as_deref())
			.filter_map(|run_id| run_refs.iter().find(|run_ref| run_ref.run_id == run_id))
			.cloned()
			.collect::<Vec<_>>();
		let snapshot = AgentBlockerSnapshot {
			schema: AGENT_BLOCKER_SNAPSHOT_SCHEMA,
			project_id: project_id.to_owned(),
			generated_at: generated_at.to_owned(),
			issue_id: blockers.iter().find_map(|blocker| blocker.issue_id.clone()),
			issue_identifier: blockers
				.iter()
				.find_map(|blocker| blocker.issue_identifier.clone()),
			blockers,
			related_run_capsules,
		};

		write_json_atomically(&path, &snapshot)?;

		kept_paths.insert(path);
	}

	prune_stale_json_files(blockers_dir, &kept_paths)
}

fn append_agent_evidence_event(
	project_id: &str,
	generated_at: &str,
	source: AgentEvidenceSource,
	events_path: &Path,
	index: &AgentHandoffIndex,
) -> Result<()> {
	if let Some(parent) = events_path.parent() {
		fs::create_dir_all(parent)?;
	}

	let event = AgentEvidenceEvent {
		schema: AGENT_EVIDENCE_EVENT_SCHEMA,
		project_id: project_id.to_owned(),
		generated_at: generated_at.to_owned(),
		source: source.as_str().to_owned(),
		handoff_index_path: index.handoff_index_path.clone(),
		blocker_count: index.summary.blocker_count,
		run_capsule_count: index.summary.run_capsule_count,
		warning_count: index.summary.warning_count,
		connector_backoff_count: index.summary.connector_backoff_count,
	};
	let mut file = OpenOptions::new()
		.create(true)
		.append(true)
		.open(events_path)?;

	writeln!(file, "{}", serde_json::to_string(&event)?)?;

	Ok(())
}

fn write_json_atomically<T>(path: &Path, value: &T) -> Result<()>
where
	T: Serialize,
{
	let Some(parent) = path.parent() else {
		eyre::bail!("Agent evidence path `{}` has no parent directory.", path.display());
	};

	fs::create_dir_all(parent)?;

	let file_name = path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("Agent evidence path `{}` has no UTF-8 file name.", path.display()))?;
	let temp_path = parent.join(format!(
		".{file_name}.tmp-{}-{}",
		process::id(),
		OffsetDateTime::now_utc().unix_timestamp_nanos()
	));
	let body = serde_json::to_vec_pretty(value)?;

	fs::write(&temp_path, body)?;
	fs::rename(&temp_path, path)?;

	Ok(())
}

fn prune_stale_json_files(
	dir: &Path,
	keep_paths: &collections::BTreeSet<PathBuf>,
) -> Result<()> {
	for entry in fs::read_dir(dir)? {
		let entry = entry?;
		let path = entry.path();

		if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
			continue;
		}
		if !keep_paths.contains(&path) {
			fs::remove_file(path)?;
		}
	}

	Ok(())
}

fn issue_key(issue_identifier: Option<&str>, issue_id: &str) -> String {
	issue_identifier.map_or_else(
		|| sanitize_evidence_path_component(issue_id),
		sanitize_evidence_path_component,
	)
}

fn blocker_snapshot_path(blockers_dir: &Path, issue_key: &str) -> PathBuf {
	blockers_dir.join(format!("{issue_key}.json"))
}

fn run_capsule_path(runs_dir: &Path, month_bucket: &str, run_id: &str) -> PathBuf {
	runs_dir
		.join(month_bucket)
		.join(sanitize_evidence_path_component(run_id))
		.join("capsule.json")
}

fn run_evidence_ref(project_id: &str, run_id: &str) -> String {
	format!("run:{project_id}/{run_id}")
}

fn blocker_evidence_ref(project_id: &str, issue_key: &str, reason_code: &str) -> String {
	format!("blocker:{project_id}/{issue_key}/{reason_code}")
}

fn sanitize_evidence_path_component(raw: &str) -> String {
	let mut out = String::new();
	let mut previous_dash = false;

	for byte in raw.bytes() {
		let character = byte as char;

		if character.is_ascii_alphanumeric() {
			out.push(character.to_ascii_lowercase());

			previous_dash = false;
		} else if !previous_dash {
			out.push('-');

			previous_dash = true;
		}
	}

	let out = out.trim_matches('-').to_owned();

	if out.is_empty() {
		String::from("unknown")
	} else {
		out
	}
}

fn current_month_bucket() -> String {
	let now = OffsetDateTime::now_utc();

	format!("{:04}-{:02}", now.year(), u8::from(now.month()))
}
