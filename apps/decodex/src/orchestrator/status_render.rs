use super::{
	ChildAgentActivityBucket, ChildAgentActivitySummary, CodexAccountActivitySummary,
	EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH, OperatorContinuationRecoveryStatus,
	OperatorGitHubCliAuthority, OperatorHistoryLaneStatus, OperatorHistoryLedgerOutcome,
	OperatorLaneLifecycleMetrics, OperatorLoopStatus, OperatorPhaseAcceptanceStatus,
	OperatorQueuedIssueStatus, OperatorRunControlCapability, OperatorRunStatus,
	OperatorSnapshotWarningDetail, OperatorStatusSnapshot, OperatorWorktreeStatus,
	ProtocolActivityEventSummary, ProtocolActivitySummary,
	QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT, ServiceConfig, format_optional_i64,
	format_optional_unix_timestamp, format_status_no_eligible_issue_hint,
	operator_protocol_activity_detail_is_public, operator_run_counts_as_current_lane,
	operator_run_counts_as_running, operator_run_has_recent_app_server_execution,
	project_attention_count, project_history_only_attention_count,
	queued_candidate_counts_as_waiting_intake, render_private_evidence_reference,
};

mod activity;
mod run_rows;

use activity::{
	render_account_summary, render_accounts_summary, render_child_agent_activity_summary,
	render_child_agent_context_pressure, render_control_capability_summary,
	render_loop_architecture_recovery_summary, render_loop_autonomy_signals_summary,
	render_loop_boundary_summary, render_loop_review_summary, render_loop_status_summary,
	render_protocol_activity_summary,
};
use run_rows::{append_rendered_history_lane, append_rendered_run};

pub(crate) fn render_operator_status(snapshot: &OperatorStatusSnapshot) -> String {
	let session_history_attempt_count =
		snapshot.history_lanes.iter().map(|lane| lane.attempt_count).sum::<usize>();
	let hides_current_lanes = session_history_attempt_count < snapshot.recent_runs.len();
	let (current_lane_claims, backlog_or_stale_queue_candidates): (Vec<_>, Vec<_>) = snapshot
		.queued_candidates
		.iter()
		.partition(|queued_issue| queue_claim_belongs_to_current_lane(queued_issue, snapshot));
	let (stale_closed_queue_labels, backlog_candidates) =
		rendered_backlog_queue_groups(backlog_or_stale_queue_candidates);
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

	let running_lane_count =
		snapshot.current_lanes.iter().filter(|run| operator_run_counts_as_running(run)).count();

	output.push_str(&format!("Current lanes: {}\n", snapshot.current_lanes.len()));
	output.push_str(&format!("Running lanes: {running_lane_count}\n"));
	output.push_str(&format!(
		"Run ledger shown: {} issue lanes from {} history attempts{}\n",
		snapshot.history_lanes.len(),
		session_history_attempt_count,
		if hides_current_lanes { " (current lanes inline)" } else { "" },
	));
	output.push_str(&format!("Backlog: {}\n", backlog_candidates.len()));
	output.push_str(&format!("Claimed queue echoes: {}\n", current_lane_claims.len()));
	output.push_str(&format!("Stale closed queue labels: {}\n", stale_closed_queue_labels.len()));
	output.push_str(&format!("Execution programs: {}\n", snapshot.execution_programs.len()));
	output.push_str(&format!("Recovery worktrees: {}\n", recovery_worktrees.len()));
	output.push_str(&format!("Post-review lanes: {}\n", snapshot.post_review_lanes.len()));

	append_rendered_attention_summary(&mut output, snapshot);
	append_rendered_execution_programs(&mut output, snapshot);

	output.push_str("\nCurrent Lanes\n");

	if snapshot.current_lanes.is_empty() {
		output.push_str("- none\n");
	} else {
		for run in &snapshot.current_lanes {
			append_rendered_run(&mut output, run);
		}
	}

	output.push_str("\nRun Ledger\n");

	if snapshot.history_lanes.is_empty() {
		if hides_current_lanes {
			output.push_str("- none (current lanes are shown above)\n");
		} else {
			output.push_str("- none\n");
		}
	} else {
		for lane in &snapshot.history_lanes {
			append_rendered_history_lane(&mut output, lane);
		}
	}

	append_rendered_queued_issue_section(
		&mut output,
		"Backlog",
		&backlog_candidates,
		snapshot,
		false,
	);
	append_rendered_queued_issue_section(
		&mut output,
		"Claimed Queue Echoes",
		&current_lane_claims,
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
	output.push_str(&format!("History-only terminal attention: {history_only_attention_count}\n"));

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
				"  - node: issue={} issue_state={} program_stage={} lifecycle={} readiness={} dispatch_action={} reason_codes={} reasons=\"{}\" next_action=\"{}\"\n",
				issue_identifier,
				issue_state,
				node.program_stage,
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
			"- issue_id: {}\n  issue: {}\n  state: {}\n  classification: {}\n  reason: {}\n  shadowed_by_current_lane: {}\n  branch: {}\n  worktree_path: {}\n  pr_url: {}\n  pr_head_sha: {}\n  pr_state: {}\n  review_decision: {}\n  mergeable: {}\n  check_state: {}\n  unresolved_review_threads: {}\n  readback_warning: {}\n  readback_root_cause: {}\n  loop_status: {}\n  loop_review: {}\n  loop_architecture_recovery: {}\n  loop_boundary: {}\n",
			lane.issue_id,
			lane.issue_identifier,
			lane.issue_state,
			lane.classification,
			lane.reason,
			if lane.shadowed_by_current_lane { "yes" } else { "no" },
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

pub(crate) fn render_queue_explain(
	config: &ServiceConfig,
	queued_candidates: &[OperatorQueuedIssueStatus],
) -> String {
	let mut output = String::new();

	output.push_str(&format!("Project: {}\n", config.service_id()));
	output.push_str("Mode: dry-run queue explain\n");
	output.push_str(&format!("Queued candidates: {}\n", queued_candidates.len()));
	output.push_str(&format!(
		"Ready: {}\n",
		queued_candidates.iter().filter(|candidate| candidate.classification == "ready").count()
	));
	output.push_str(&format!(
		"Waiting: {}\n",
		queued_candidates.iter().filter(|candidate| candidate.classification == "waiting").count()
	));
	output.push_str(&format!(
		"Blocked: {}\n",
		queued_candidates.iter().filter(|candidate| candidate.classification == "blocked").count()
	));
	output.push_str(&format!(
		"Claimed: {}\n",
		queued_candidates.iter().filter(|candidate| candidate.classification == "claimed").count()
	));
	output.push_str(&format!(
		"Closed: {}\n",
		queued_candidates.iter().filter(|candidate| candidate.classification == "closed").count()
	));
	output.push_str("\nQueued Candidate Reasons\n");

	if queued_candidates.is_empty() {
		output.push_str("- none\n");
		output.push_str(&format!(
			"  {}\n",
			format_status_no_eligible_issue_hint(config.service_id())
		));

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
		queued_candidates
			.into_iter()
			.partition(|queued_issue| queued_issue.classification == "closed");
	let backlog_candidates = non_closed_queue_candidates
		.into_iter()
		.filter(|queued_issue| queued_candidate_counts_as_waiting_intake(queued_issue))
		.collect::<Vec<_>>();

	(stale_closed_queue_labels, backlog_candidates)
}

pub(in crate::orchestrator) fn rendered_recovery_worktrees(
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
			.then(|| current_lane_run_id_for_queue_candidate(queued_issue, snapshot))
			.flatten();

		append_rendered_queued_issue(output, queued_issue, running_owner);
	}
}

fn queue_claim_belongs_to_current_lane(
	queued_issue: &OperatorQueuedIssueStatus,
	snapshot: &OperatorStatusSnapshot,
) -> bool {
	queued_issue.classification == "claimed"
		&& current_lane_run_id_for_queue_candidate(queued_issue, snapshot).is_some()
}

fn current_lane_run_id_for_queue_candidate<'a>(
	queued_issue: &OperatorQueuedIssueStatus,
	snapshot: &'a OperatorStatusSnapshot,
) -> Option<&'a str> {
	snapshot
		.current_lanes
		.iter()
		.find(|run| run.issue_id == queued_issue.issue_id && run.counts_as_running)
		.map(|run| run.run_id.as_str())
}

fn append_rendered_queued_issue(
	output: &mut String,
	queued_issue: &OperatorQueuedIssueStatus,
	current_lane_run_id: Option<&str>,
) {
	let priority =
		queued_issue.priority.map_or_else(|| String::from("none"), |value| value.to_string());
	let blockers = if queued_issue.blocker_identifiers.is_empty() {
		String::from("none")
	} else {
		queued_issue.blocker_identifiers.join(", ")
	};
	let running_owner = current_lane_run_id.unwrap_or("none");

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
	if snapshot.current_lanes.iter().any(|run| {
		run.ownership_state == "leased_run"
			&& (run.worktree_path.as_deref() == Some(worktree.worktree_path.as_str())
				|| run.branch_name.as_deref() == Some(worktree.branch_name.as_str())
				|| run.issue_id == worktree.issue_id)
	}) {
		return "current_lane";
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
		) && (candidate.attention.as_ref().and_then(|attention| attention.worktree_path.as_deref())
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
		"current_lane"
		| "running_lane"
		| "blocked_queue_issue"
		| "queued_attention"
		| "continuation_pending" => 0,
		"post_review_lane" => 1,
		_ => 2,
	}
}
