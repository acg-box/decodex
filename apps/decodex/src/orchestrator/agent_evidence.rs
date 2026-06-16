const AGENT_HANDOFF_INDEX_SCHEMA: &str = "decodex.agent_handoff_index/1";
const AGENT_BLOCKER_SNAPSHOT_SCHEMA: &str = "decodex.blocker_snapshot/1";
const AGENT_RUN_CAPSULE_SCHEMA: &str = "decodex.run_capsule/1";
const AGENT_EVIDENCE_EVENT_SCHEMA: &str = "decodex.agent_evidence_event/1";
const PRIVATE_EVIDENCE_READBACK_SCHEMA: &str = "decodex.private_execution_evidence_readback/1";
const PRIVATE_EVIDENCE_PAYLOAD_PREVIEW_LIMIT: usize = 160;
const REVIEW_CHECKPOINT_EVENT_TYPE: &str = "review_checkpoint";
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
	github_cli_authority: Option<OperatorGitHubCliAuthority>,
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
	current_lane_count: usize,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct AgentPrivateEvidenceRef {
	evidence_ref: String,
	source: String,
	default_view: String,
	read_command: String,
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
	private_evidence: AgentPrivateEvidenceRef,
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
	ownership_state: String,
	liveness_state: String,
	policy_state: String,
	terminalization_state: String,
	lane_control_next_action: String,
	lane_control_conditions: Vec<String>,
	run_lease: bool,
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
	private_evidence: AgentPrivateEvidenceRef,
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

#[derive(Clone, Debug, PartialEq, Serialize)]
struct PrivateEvidenceReadback {
	schema: &'static str,
	project_id: String,
	issue_selector: String,
	issue_id: String,
	issue_identifier: Option<String>,
	run_id: String,
	attempt_number: i64,
	source: &'static str,
	evidence_ref: String,
	read_command: String,
	payload_mode: &'static str,
	event_count: usize,
	latest_event_type: Option<String>,
	latest_event_at: Option<String>,
	review_checkpoints: Vec<PrivateEvidenceReviewCheckpointSummary>,
	boundary_checks: Vec<PrivateEvidenceBoundaryCheckSummary>,
	decision_requests: Vec<PrivateEvidenceDecisionRequestSummary>,
	architecture_recoveries: Vec<PrivateEvidenceArchitectureRecoverySummary>,
	improvement_candidates: Vec<HarnessImprovementCandidateSummary>,
	events: Vec<PrivateEvidenceReadbackEvent>,
	warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct PrivateEvidenceDecisionRequestSummary {
	decision_request_id: String,
	phase: String,
	reason: String,
	boundary: String,
	next_action: String,
	recommendation: Option<String>,
	resume_condition: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct PrivateEvidenceReviewCheckpointSummary {
	phase: String,
	status: String,
	head_sha: Option<String>,
	round: Option<u64>,
	accepted_finding_count: usize,
	rejected_finding_count: usize,
	next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct PrivateEvidenceBoundaryCheckSummary {
	disposition: String,
	reason: Option<String>,
	attempted_recovery_reason: Option<String>,
	decision_contract_count: usize,
	changed_surface_count: usize,
	improvement_signal_count: usize,
	next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct PrivateEvidenceArchitectureRecoverySummary {
	reason_code: String,
	guardrail_reason: Option<String>,
	boundary_disposition: Option<String>,
	recovery_budget_attempt: Option<u64>,
	recovery_budget_max_attempts: Option<u64>,
	next_action: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct PrivateEvidenceReadbackEvent {
	record_id: i64,
	event_type: String,
	recorded_at: String,
	payload_summary: PrivateEvidencePayloadSummary,
	#[serde(skip_serializing_if = "Option::is_none")]
	payload: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct PrivateEvidencePayloadSummary {
	kind: String,
	byte_count: usize,
	keys: Vec<String>,
	preview: Vec<String>,
	redacted_default_keys: Vec<String>,
}

struct PrivateEvidenceTarget {
	issue_id: String,
	issue_identifier: Option<String>,
	run_id: String,
	attempt_number: i64,
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
	current_lanes: Vec<&'a OperatorRunStatus>,
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
		let current_lanes = snapshot
			.current_lanes
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
			current_lanes,
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
			current_lane_count: project_view.current_lanes.len(),
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
		let github_cli_authority = project_view
			.projects
			.first()
			.map(|project| project.github_cli_authority.clone());
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
			github_cli_authority,
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

fn render_private_evidence_reference(run: &OperatorRunStatus) -> String {
	let private_evidence = agent_private_evidence_ref(run);

	format!(
		"ref={} source={} default_view={} read=`{}`",
		private_evidence.evidence_ref,
		private_evidence.source,
		private_evidence.default_view,
		private_evidence.read_command
	)
}

fn agent_private_evidence_ref(run: &OperatorRunStatus) -> AgentPrivateEvidenceRef {
	run.private_evidence.clone()
}

fn private_evidence_ref_for_run_fields(
	project_id: &str,
	issue_id: &str,
	issue_identifier: Option<&str>,
	run_id: &str,
	attempt_number: i64,
) -> AgentPrivateEvidenceRef {
	AgentPrivateEvidenceRef {
		evidence_ref: private_evidence_ref_for_parts(project_id, issue_id, run_id, attempt_number),
		source: String::from("runtime_sqlite"),
		default_view: String::from("summarized_payloads"),
		read_command: private_evidence_read_command(
			issue_identifier.unwrap_or(issue_id),
			Some(run_id),
			Some(attempt_number),
			true,
			false,
		),
	}
}

fn private_evidence_read_command(
	issue_selector: &str,
	run_id: Option<&str>,
	attempt_number: Option<i64>,
	json: bool,
	include_payload: bool,
) -> String {
	let mut command = format!("decodex evidence {}", shell_quote(issue_selector));

	if let Some(run_id) = run_id {
		command.push_str(&format!(" --run-id {}", shell_quote(run_id)));
	}
	if let Some(attempt_number) = attempt_number {
		command.push_str(&format!(" --attempt {attempt_number}"));
	}

	if json {
		command.push_str(" --json");
	}
	if include_payload {
		command.push_str(" --include-payload");
	}

	command
}

fn private_evidence_ref_for_parts(
	project_id: &str,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
) -> String {
	format!("private-evidence:{project_id}/{issue_id}/{run_id}/{attempt_number}")
}

fn shell_quote(raw: &str) -> String {
	if !raw.is_empty()
		&& raw
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':'))
	{
		return raw.to_owned();
	}

	format!("'{}'", raw.replace('\'', "'\\''"))
}

fn build_private_evidence_readback(
	state_store: &StateStore,
	project: &ServiceConfig,
	request: &EvidenceRequest<'_>,
) -> Result<PrivateEvidenceReadback> {
	let target = resolve_private_evidence_target(
		state_store,
		project,
		request.issue,
		request.run_id,
		request.attempt_number,
	)?;
	let events = state_store.list_private_execution_events(
		project.service_id(),
		&target.issue_id,
		&target.run_id,
		target.attempt_number,
	)?;
	let latest_event = events.last();
	let warnings = if events.is_empty() {
		vec![String::from("private_execution_evidence_missing")]
	} else {
		Vec::new()
	};
	let issue_selector = target
		.issue_identifier
		.as_deref()
		.unwrap_or(&target.issue_id)
		.to_owned();
	let read_command = private_evidence_read_command(
		&issue_selector,
		Some(&target.run_id),
		Some(target.attempt_number),
		true,
		request.include_payload,
	);

	Ok(PrivateEvidenceReadback {
		schema: PRIVATE_EVIDENCE_READBACK_SCHEMA,
		project_id: project.service_id().to_owned(),
		issue_selector: request.issue.to_owned(),
		issue_id: target.issue_id.clone(),
		issue_identifier: target.issue_identifier,
		run_id: target.run_id.clone(),
		attempt_number: target.attempt_number,
		source: "runtime_sqlite",
		evidence_ref: private_evidence_ref_for_parts(
			project.service_id(),
			&target.issue_id,
			&target.run_id,
			target.attempt_number,
		),
		read_command,
		payload_mode: if request.include_payload { "full_payloads" } else { "summarized_payloads" },
		event_count: events.len(),
		latest_event_type: latest_event.map(|event| event.event_type().to_owned()),
		latest_event_at: latest_event.map(|event| event.recorded_at().to_owned()),
		review_checkpoints: review_checkpoints_from_private_events(&events),
		boundary_checks: boundary_checks_from_private_events(&events),
		decision_requests: authority_decision_requests_from_private_events(&events),
		architecture_recoveries: architecture_recoveries_from_private_events(&events),
		improvement_candidates: harness_improvement_candidates_from_private_events(&events),
		events: events
			.iter()
			.map(|event| private_evidence_readback_event(event, request.include_payload))
			.collect(),
		warnings,
	})
}

fn resolve_private_evidence_target(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_selector: &str,
	run_id: Option<&str>,
	attempt_number: Option<i64>,
) -> Result<PrivateEvidenceTarget> {
	let (_, runs) = state_store.list_project_runs(project.service_id(), usize::MAX)?;
	let selector = issue_selector.trim();
	let matching_run = runs
		.iter()
		.filter(|run| private_evidence_run_matches_issue(project, run, selector))
		.filter(|run| run_id.is_none_or(|run_id| run.run_id() == run_id)).find(|run| attempt_number.is_none_or(|attempt| run.attempt_number() == attempt));

	if let Some(run) = matching_run {
		let branch_name = run.branch_name().map(str::to_owned);
		let worktree_path = run
			.worktree_path()
			.map(|path| relative_worktree_path_for_path(project, path));
		let issue_identifier = operator_run_issue_identifier_from_fields(
			run.run_id(),
			branch_name.as_deref(),
			worktree_path.as_deref(),
		);

		return Ok(PrivateEvidenceTarget {
			issue_id: run.issue_id().to_owned(),
			issue_identifier,
			run_id: run.run_id().to_owned(),
			attempt_number: run.attempt_number(),
		});
	}
	if let (Some(run_id), Some(attempt_number)) = (run_id, attempt_number) {
		let events = state_store.list_private_execution_events_for_run_attempt(
			project.service_id(),
			run_id,
			attempt_number,
		)?;

		if let Some(issue_id) = private_evidence_direct_lookup_issue_id(&events, selector)? {
			return Ok(PrivateEvidenceTarget {
				issue_identifier: (issue_id != selector).then(|| selector.to_owned()),
				issue_id,
				run_id: run_id.to_owned(),
				attempt_number,
			});
		}

		return Ok(PrivateEvidenceTarget {
			issue_id: selector.to_owned(),
			issue_identifier: None,
			run_id: run_id.to_owned(),
			attempt_number,
		});
	}

	eyre::bail!(
		"No local run matched issue `{selector}` in project `{}`. Pass --run-id and --attempt for direct runtime-store lookup, or run `decodex status --json` to find local run ids.",
		project.service_id()
	)
}

fn private_evidence_direct_lookup_issue_id(
	events: &[state::PrivateExecutionEvent],
	selector: &str,
) -> Result<Option<String>> {
	let issue_ids = events
		.iter()
		.map(state::PrivateExecutionEvent::issue_id)
		.collect::<collections::BTreeSet<_>>();

	if issue_ids.is_empty() {
		return Ok(None);
	}
	if issue_ids.len() == 1 {
		return Ok(issue_ids.iter().next().map(|issue_id| (*issue_id).to_owned()));
	}
	if issue_ids.contains(selector) {
		return Ok(Some(selector.to_owned()));
	}

	eyre::bail!(
		"Direct private evidence lookup for issue `{selector}` matched multiple local issue ids for the supplied run and attempt; pass the local issue id from `decodex status --json`."
	)
}

fn review_checkpoints_from_private_events(
	events: &[state::PrivateExecutionEvent],
) -> Vec<PrivateEvidenceReviewCheckpointSummary> {
	events
		.iter()
		.filter(|event| event.event_type() == REVIEW_CHECKPOINT_EVENT_TYPE)
		.filter_map(review_checkpoint_from_private_event)
		.collect()
}

fn review_checkpoint_from_private_event(
	event: &state::PrivateExecutionEvent,
) -> Option<PrivateEvidenceReviewCheckpointSummary> {
	let payload = event.payload();
	let phase = payload.get("phase")?.as_str()?.to_owned();
	let status = payload.get("status")?.as_str()?.to_owned();
	let head_sha = payload
		.get("head_sha")
		.and_then(Value::as_str)
		.map(str::to_owned);
	let round = payload
		.get("nonclean_rounds")
		.or_else(|| payload.get("round"))
		.and_then(Value::as_u64);
	let accepted_finding_count = payload
		.get("review")
		.and_then(|review| review.get("accepted_findings"))
		.or_else(|| payload.get("accepted_findings"))
		.and_then(Value::as_array)
		.map_or(0, Vec::len);
	let rejected_finding_count = payload
		.get("review")
		.and_then(|review| review.get("rejected_findings"))
		.or_else(|| payload.get("rejected_findings"))
		.and_then(Value::as_array)
		.map_or(0, Vec::len);
	let next_action = review_checkpoint_next_action(&status);

	Some(PrivateEvidenceReviewCheckpointSummary {
		phase,
		status,
		head_sha,
		round,
		accepted_finding_count,
		rejected_finding_count,
		next_action,
	})
}

fn review_checkpoint_next_action(status: &str) -> String {
	match status {
		"clean" => String::from("Proceed with review handoff when repo gate evidence is current."),
		"findings" => String::from("Repair accepted findings, rerun validation, and checkpoint the repaired head."),
		"blocked" => String::from("Resolve the blocking review condition before continuing."),
		"needs_architecture_review" => {
			String::from("Escalate for an architecture decision before further repair churn.")
		},
		_ => String::from("Inspect the Decodex Review checkpoint summary before continuing."),
	}
}

fn boundary_checks_from_private_events(
	events: &[state::PrivateExecutionEvent],
) -> Vec<PrivateEvidenceBoundaryCheckSummary> {
	events
		.iter()
		.filter(|event| event.event_type() == AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE)
		.filter_map(boundary_check_from_private_event)
		.collect()
}

fn boundary_check_from_private_event(
	event: &state::PrivateExecutionEvent,
) -> Option<PrivateEvidenceBoundaryCheckSummary> {
	let payload = event.payload();
	let disposition = payload.get("disposition")?.as_str()?.to_owned();
	let reason = payload
		.get("final_disposition")
		.and_then(|final_disposition| final_disposition.get("reason"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let attempted_recovery_reason = payload
		.get("attempted_recovery_reason")
		.and_then(Value::as_str)
		.map(str::to_owned);
	let decision_contract_count = payload
		.get("decision_contract_ids")
		.and_then(Value::as_array)
		.map_or(0, Vec::len);
	let changed_surface_count = payload
		.get("changed_surfaces")
		.and_then(Value::as_array)
		.map_or(0, Vec::len);
	let improvement_signal_count = payload
		.get("improvement_signals")
		.and_then(Value::as_array)
		.map_or(0, Vec::len);
	let next_action = boundary_check_next_action(&disposition);

	Some(PrivateEvidenceBoundaryCheckSummary {
		disposition,
		reason,
		attempted_recovery_reason,
		decision_contract_count,
		changed_surface_count,
		improvement_signal_count,
		next_action,
	})
}

fn boundary_check_next_action(disposition: &str) -> String {
	match disposition {
		"within_authority" => {
			String::from("Continue autonomous architecture recovery inside the accepted boundary.")
		},
		"requires_human" => String::from("Stop for a human boundary decision before continuing."),
		"insufficient_evidence" => {
			String::from("Provide boundary evidence or a Decision Contract before retrying.")
		},
		_ => String::from("Inspect the authority boundary summary before continuing."),
	}
}

fn authority_decision_requests_from_private_events(
	events: &[state::PrivateExecutionEvent],
) -> Vec<PrivateEvidenceDecisionRequestSummary> {
	events
		.iter()
		.filter(|event| event.event_type() == AUTHORITY_DECISION_REQUEST_EVENT_TYPE)
		.filter_map(authority_decision_request_from_private_event)
		.collect()
}

fn authority_decision_request_from_private_event(
	event: &state::PrivateExecutionEvent,
) -> Option<PrivateEvidenceDecisionRequestSummary> {
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

	Some(PrivateEvidenceDecisionRequestSummary {
		decision_request_id,
		phase,
		reason,
		boundary,
		next_action,
		recommendation,
		resume_condition,
	})
}

fn architecture_recoveries_from_private_events(
	events: &[state::PrivateExecutionEvent],
) -> Vec<PrivateEvidenceArchitectureRecoverySummary> {
	events
		.iter()
		.filter(|event| {
			matches!(
				event.event_type(),
				ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE
					| ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE
					| ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE
			)
		})
		.filter_map(architecture_recovery_from_private_event)
		.collect()
}

fn architecture_recovery_from_private_event(
	event: &state::PrivateExecutionEvent,
) -> Option<PrivateEvidenceArchitectureRecoverySummary> {
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
	let next_action = architecture_recovery_next_action(&reason_code);

	Some(PrivateEvidenceArchitectureRecoverySummary {
		reason_code,
		guardrail_reason,
		boundary_disposition,
		recovery_budget_attempt,
		recovery_budget_max_attempts,
		next_action,
	})
}

fn architecture_recovery_next_action(reason_code: &str) -> String {
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

fn private_evidence_run_matches_issue(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
	selector: &str,
) -> bool {
	if run.issue_id() == selector {
		return true;
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

	issue_identifier
		.as_deref()
		.is_some_and(|issue_identifier| issue_identifier.eq_ignore_ascii_case(selector))
}

fn private_evidence_readback_event(
	event: &state::PrivateExecutionEvent,
	include_payload: bool,
) -> PrivateEvidenceReadbackEvent {
	PrivateEvidenceReadbackEvent {
		record_id: event.record_id(),
		event_type: event.event_type().to_owned(),
		recorded_at: event.recorded_at().to_owned(),
		payload_summary: summarize_private_evidence_payload(event.payload()),
		payload: include_payload.then(|| event.payload().clone()),
	}
}

fn summarize_private_evidence_payload(payload: &Value) -> PrivateEvidencePayloadSummary {
	let encoded = serde_json::to_vec(payload).unwrap_or_default();
	let mut keys = Vec::new();
	let mut preview = Vec::new();
	let mut redacted_default_keys = Vec::new();
	let kind = match payload {
		Value::Object(object) => {
			for (key, value) in object {
				keys.push(key.clone());

				if private_evidence_payload_key_is_sensitive(key) {
					redacted_default_keys.push(key.clone());
					preview.push(format!("{key}=<redacted by default>"));
				} else {
					preview.push(format!("{key}={}", summarize_private_evidence_payload_value(value)));
				}
			}

			String::from("object")
		},
		Value::Array(values) => {
			preview.push(format!("array_len={}", values.len()));

			String::from("array")
		},
		Value::String(value) => {
			preview.push(truncate_private_evidence_payload_preview(value));

			String::from("string")
		},
		Value::Number(value) => {
			preview.push(value.to_string());

			String::from("number")
		},
		Value::Bool(value) => {
			preview.push(value.to_string());

			String::from("bool")
		},
		Value::Null => String::from("null"),
	};

	PrivateEvidencePayloadSummary {
		kind,
		byte_count: encoded.len(),
		keys,
		preview,
		redacted_default_keys,
	}
}

fn summarize_private_evidence_payload_value(value: &Value) -> String {
	match value {
		Value::Null => String::from("null"),
		Value::Bool(value) => value.to_string(),
		Value::Number(value) => value.to_string(),
		Value::String(value) => truncate_private_evidence_payload_preview(value),
		Value::Array(values) => format!("array(len={})", values.len()),
		Value::Object(object) => format!("object(keys={})", object.len()),
	}
}

fn private_evidence_payload_key_is_sensitive(key: &str) -> bool {
	let key = key.to_ascii_lowercase();

	key.contains("transcript")
		|| key.contains("message")
		|| key.contains("conversation")
		|| key.contains("raw")
		|| key.contains("stdout")
		|| key.contains("stderr")
		|| key.contains("log")
		|| key.contains("token")
		|| key.contains("secret")
}

fn truncate_private_evidence_payload_preview(value: &str) -> String {
	let mut preview = String::new();
	let mut truncated = false;

	for character in value.chars() {
		if preview.len() + character.len_utf8() > PRIVATE_EVIDENCE_PAYLOAD_PREVIEW_LIMIT {
			truncated = true;

			break;
		}

		preview.push(character);
	}

	if truncated {
		preview.push_str("...");
	}

	preview
}

fn render_private_evidence_readback(readback: &PrivateEvidenceReadback) -> String {
	let mut output = String::new();

	append_private_evidence_readback_header(&mut output, readback);
	append_private_evidence_decision_requests(&mut output, &readback.decision_requests);
	append_private_evidence_review_checkpoints(&mut output, &readback.review_checkpoints);
	append_private_evidence_architecture_recoveries(
		&mut output,
		&readback.architecture_recoveries,
	);
	append_private_evidence_boundary_checks(&mut output, &readback.boundary_checks);
	append_private_evidence_improvement_candidates(
		&mut output,
		&readback.improvement_candidates,
	);
	append_private_evidence_events(&mut output, &readback.events);

	output
}

fn append_private_evidence_readback_header(
	output: &mut String,
	readback: &PrivateEvidenceReadback,
) {
	output.push_str(&format!("Project: {}\n", readback.project_id));
	output.push_str("Private Execution Evidence\n");
	output.push_str(&format!("issue_selector: {}\n", readback.issue_selector));
	output.push_str(&format!("issue_id: {}\n", readback.issue_id));
	output.push_str(&format!(
		"issue_identifier: {}\n",
		readback.issue_identifier.as_deref().unwrap_or("none")
	));
	output.push_str(&format!("run_id: {}\n", readback.run_id));
	output.push_str(&format!("attempt: {}\n", readback.attempt_number));
	output.push_str(&format!("source: {}\n", readback.source));
	output.push_str(&format!("evidence_ref: {}\n", readback.evidence_ref));
	output.push_str(&format!("payload_mode: {}\n", readback.payload_mode));
	output.push_str(&format!("event_count: {}\n", readback.event_count));
	output.push_str(&format!(
		"improvement_candidate_count: {}\n",
		readback.improvement_candidates.len()
	));
	output.push_str(&format!(
		"decision_request_count: {}\n",
		readback.decision_requests.len()
	));
	output.push_str(&format!(
		"review_checkpoint_count: {}\n",
		readback.review_checkpoints.len()
	));
	output.push_str(&format!(
		"architecture_recovery_count: {}\n",
		readback.architecture_recoveries.len()
	));
	output.push_str(&format!(
		"boundary_check_count: {}\n",
		readback.boundary_checks.len()
	));
	output.push_str(&format!(
		"latest_event_type: {}\n",
		readback.latest_event_type.as_deref().unwrap_or("none")
	));
	output.push_str(&format!(
		"latest_event_at: {}\n",
		readback.latest_event_at.as_deref().unwrap_or("none")
	));

	if !readback.warnings.is_empty() {
		output.push_str(&format!("warnings: {}\n", readback.warnings.join(", ")));
	}
}

fn append_private_evidence_decision_requests(
	output: &mut String,
	decision_requests: &[PrivateEvidenceDecisionRequestSummary],
) {
	output.push_str("\nDecision Requests\n");

	if decision_requests.is_empty() {
		output.push_str("- none\n");
	} else {
		for request in decision_requests {
			output.push_str(&format!(
				"- id: {}\n  phase: {}\n  reason: {}\n  boundary: {}\n  next_action: {}\n",
				request.decision_request_id,
				request.phase,
				request.reason,
				request.boundary,
				request.next_action
			));
		}
	}
}

fn append_private_evidence_review_checkpoints(
	output: &mut String,
	review_checkpoints: &[PrivateEvidenceReviewCheckpointSummary],
) {
	output.push_str("\nReview Checkpoints\n");

	if review_checkpoints.is_empty() {
		output.push_str("- none\n");
	} else {
		for checkpoint in review_checkpoints {
			output.push_str(&format!(
				"- phase: {}\n  status: {}\n  head_sha: {}\n  round: {}\n  accepted_findings: {}\n  rejected_findings: {}\n  next_action: {}\n",
				checkpoint.phase,
				checkpoint.status,
				checkpoint.head_sha.as_deref().unwrap_or("none"),
				checkpoint
					.round
					.map_or_else(|| String::from("none"), |round| round.to_string()),
				checkpoint.accepted_finding_count,
				checkpoint.rejected_finding_count,
				checkpoint.next_action
			));
		}
	}
}

fn append_private_evidence_architecture_recoveries(
	output: &mut String,
	architecture_recoveries: &[PrivateEvidenceArchitectureRecoverySummary],
) {
	output.push_str("\nArchitecture Recoveries\n");

	if architecture_recoveries.is_empty() {
		output.push_str("- none\n");
	} else {
		for recovery in architecture_recoveries {
			output.push_str(&format!(
				"- reason_code: {}\n  guardrail_reason: {}\n  boundary_disposition: {}\n  budget: {}/{}\n  next_action: {}\n",
				recovery.reason_code,
				recovery.guardrail_reason.as_deref().unwrap_or("none"),
				recovery.boundary_disposition.as_deref().unwrap_or("none"),
				recovery
					.recovery_budget_attempt
					.map_or_else(|| String::from("none"), |attempt| attempt.to_string()),
				recovery
					.recovery_budget_max_attempts
					.map_or_else(|| String::from("none"), |max_attempts| max_attempts.to_string()),
				recovery.next_action
			));
		}
	}
}

fn append_private_evidence_boundary_checks(
	output: &mut String,
	boundary_checks: &[PrivateEvidenceBoundaryCheckSummary],
) {
	output.push_str("\nBoundary Checks\n");

	if boundary_checks.is_empty() {
		output.push_str("- none\n");
	} else {
		for boundary in boundary_checks {
			output.push_str(&format!(
				"- disposition: {}\n  reason: {}\n  attempted_recovery: {}\n  decision_contracts: {}\n  changed_surfaces: {}\n  improvement_signals: {}\n  next_action: {}\n",
				boundary.disposition,
				boundary.reason.as_deref().unwrap_or("none"),
				boundary
					.attempted_recovery_reason
					.as_deref()
					.unwrap_or("none"),
				boundary.decision_contract_count,
				boundary.changed_surface_count,
				boundary.improvement_signal_count,
				boundary.next_action
			));
		}
	}
}

fn append_private_evidence_improvement_candidates(
	output: &mut String,
	improvement_candidates: &[HarnessImprovementCandidateSummary],
) {
	output.push_str("\nImprovement Candidates\n");

	if improvement_candidates.is_empty() {
		output.push_str("- none\n");
	} else {
		for candidate in improvement_candidates {
			output.push_str(&format!(
				"- kind: {}\n  reason_code: {}\n  target: {}\n  source_event_count: {}\n  recommendation: {}\n",
				candidate.kind,
				candidate.reason_code,
				candidate.target,
				candidate.source_event_count,
				candidate.recommendation
			));
		}
	}
}

fn append_private_evidence_events(
	output: &mut String,
	events: &[PrivateEvidenceReadbackEvent],
) {
	output.push_str("\nEvents\n");

	if events.is_empty() {
		output.push_str("- none\n");

		return;
	}

	for event in events {
		output.push_str(&format!(
			"- record_id: {}\n  event_type: {}\n  recorded_at: {}\n  payload: {}\n",
			event.record_id,
			event.event_type,
			event.recorded_at,
			render_private_evidence_payload_summary(&event.payload_summary)
		));

		if let Some(payload) = &event.payload {
			output.push_str(&format!("  full_payload: {}\n", payload));
		}
	}
}

fn render_private_evidence_payload_summary(
	summary: &PrivateEvidencePayloadSummary,
) -> String {
	let keys = if summary.keys.is_empty() {
		String::from("none")
	} else {
		summary.keys.join(",")
	};
	let preview = if summary.preview.is_empty() {
		String::from("none")
	} else {
		summary.preview.join("; ")
	};
	let redacted = if summary.redacted_default_keys.is_empty() {
		String::from("none")
	} else {
		summary.redacted_default_keys.join(",")
	};

	format!(
		"kind={} bytes={} keys={} preview={} redacted_default_keys={}",
		summary.kind, summary.byte_count, keys, preview, redacted
	)
}

fn lane_issue_belongs_to_project(
	issue_id: &str,
	project_id: &str,
	snapshot: &OperatorStatusSnapshot,
) -> bool {
	snapshot
		.current_lanes
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
	for run in snapshot.current_lanes.iter().chain(snapshot.recent_runs.iter()) {
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
		.current_lanes
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
	let private_evidence = agent_private_evidence_ref(run);

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
		ownership_state: run.ownership_state.clone(),
		liveness_state: run.liveness_state.clone(),
		policy_state: run.policy_state.clone(),
		terminalization_state: run.terminalization_state.clone(),
		lane_control_next_action: run.lane_control_next_action.clone(),
		lane_control_conditions: run.lane_control_conditions.clone(),
		run_lease: run.run_lease,
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
		private_evidence,
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
		private_evidence: capsule.private_evidence.clone(),
	}
}

fn agent_run_diagnosis(run: &OperatorRunStatus) -> AgentRunDiagnosis {
	let reason = agent_run_blocker_reason(run);

	AgentRunDiagnosis {
		attention_required: reason.is_some(),
		reason_code: reason.map(str::to_owned),
		next_action: agent_run_next_action(run),
	}
}

fn agent_run_blocker_reason(run: &OperatorRunStatus) -> Option<&'static str> {
	if run.policy_state == "review_churn_exceeded" {
		return Some("review_churn_exceeded");
	}
	if run.ownership_state == "retained_attention" {
		return Some("retained_attention");
	}
	if run.ownership_state == "orphaned_live_thread" {
		return Some("orphaned_live_thread");
	}
	if run.ownership_state == "terminalizing" {
		return Some("terminalizing");
	}
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

fn agent_run_next_action(run: &OperatorRunStatus) -> Option<String> {
	if !run.lane_control_next_action.trim().is_empty() {
		return Some(run.lane_control_next_action.clone());
	}

	match agent_run_blocker_reason(run) {
		Some("suspected_stall" | "run_stalled" | "stale_execution_without_known_process") =>
			Some(String::from("Inspect the run capsule, retained worktree, protocol activity, and process state before retrying.")),
		Some("process_exited_without_terminal_status") =>
			Some(String::from("Inspect the retained worktree and runtime markers; reconcile or retry only after preserving useful local changes.")),
		Some("run_waiting") =>
			Some(String::from("Inspect wait_reason, thread status, and protocol activity before deciding whether the agent can continue.")),
		Some("retry_backoff") => Some(String::from("Wait until next_retry_at or run an explicit operator retry after reviewing the retained state.")),
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
	for run in &project_view.current_lanes {
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
					.unwrap_or_else(|| reason_code.to_owned()),
				next_action: agent_run_next_action(run)
					.unwrap_or_else(|| String::from("Inspect the run capsule.")),
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
