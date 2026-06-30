use super::{
	AGENT_RUN_CAPSULE_SCHEMA, AgentBlocker, AgentConnectorBackoff, AgentEvidenceProjectView,
	AgentRecoveryContract, AgentRecoveryWorktree, AgentRunCapsule, AgentRunCapsuleRef,
	AgentRunDiagnosis, AgentRunLedgerOutcome, OperatorConnectorBackoffStatus,
	OperatorHistoryLedgerOutcome, OperatorPostReviewLaneStatus, OperatorRunStatus,
	OperatorWorktreeStatus, Path, agent_private_evidence_ref, blocker_evidence_ref,
	blocker_snapshot_path, collections, issue_key,
	operator_run_has_stale_execution_without_known_process, run_capsule_path, run_evidence_ref,
	sanitize_evidence_path_component,
};

pub(in crate::orchestrator) fn build_run_capsules(
	project_view: &AgentEvidenceProjectView<'_>,
	generated_at: &str,
	runs_dir: &Path,
	month_bucket: &str,
) -> Vec<AgentRunCapsule> {
	let mut run_ids = collections::BTreeSet::new();
	let mut capsules = Vec::new();

	for run in project_view.current_lanes.iter().chain(project_view.recent_runs.iter()).copied() {
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

fn agent_run_ledger_outcome(outcome: &OperatorHistoryLedgerOutcome) -> AgentRunLedgerOutcome {
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

pub(in crate::orchestrator) fn run_capsule_ref(capsule: &AgentRunCapsule) -> AgentRunCapsuleRef {
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

	None
}

fn agent_run_next_action(run: &OperatorRunStatus) -> Option<String> {
	if !run.lane_control_next_action.trim().is_empty() {
		return Some(run.lane_control_next_action.clone());
	}

	match agent_run_blocker_reason(run) {
		Some("suspected_stall" | "run_stalled" | "stale_execution_without_known_process") => {
			Some(String::from(
				"Inspect the run capsule, retained worktree, protocol activity, and process state before retrying.",
			))
		},
		Some("process_exited_without_terminal_status") => Some(String::from(
			"Inspect the retained worktree and runtime markers; reconcile or retry only after preserving useful local changes.",
		)),
		_ => None,
	}
}

pub(in crate::orchestrator) fn build_agent_blockers(
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
				evidence_ref: blocker_evidence_ref(
					project_view.project_id,
					&issue_key,
					reason_code,
				),
				project_id: project_view.project_id.to_owned(),
				surface: String::from("running_lane"),
				issue_id: Some(run.issue_id.clone()),
				issue_identifier: run.issue_identifier.clone(),
				run_id: Some(run.run_id.clone()),
				attempt_number: Some(run.attempt_number),
				classification: String::from("attention_required"),
				reason_code: reason_code.to_owned(),
				reason: run.wait_reason.clone().unwrap_or_else(|| reason_code.to_owned()),
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
			attempt_number: candidate
				.attention
				.as_ref()
				.and_then(|attention| attention.attempt_number),
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
				.unwrap_or_else(|| {
					String::from(
						"Inspect the queued candidate and retained worktree before retrying.",
					)
				}),
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
			next_action: String::from(
				"Regenerate diagnose output after resolving the unavailable observer or runtime warning.",
			),
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
		let issue_key =
			format!("connector-{}", sanitize_evidence_path_component(&backoff.connector));

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
		left.issue_identifier
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

fn post_review_lane_next_action(lane: &OperatorPostReviewLaneStatus, project_id: &str) -> String {
	if lane.reason == "missing_review_handoff_record" {
		return format!(
			"Run `decodex recover review-handoff diagnose {} --json`; rebind only after PR lineage and retained worktree HEAD match.",
			lane.issue_identifier
		);
	}
	if lane.classification == "needs_review_repair" {
		return String::from(
			"Run or inspect the retained review-repair lane before attempting land.",
		);
	}

	format!(
		"Inspect the `{}` retained post-review lane for service `{project_id}` before retrying.",
		lane.classification
	)
}

pub(in crate::orchestrator) fn agent_connector_backoff(
	backoff: &OperatorConnectorBackoffStatus,
) -> AgentConnectorBackoff {
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

pub(in crate::orchestrator) fn agent_recovery_worktree(
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

pub(in crate::orchestrator) fn agent_recovery_contract(
	blocker: &AgentBlocker,
) -> Option<AgentRecoveryContract> {
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
