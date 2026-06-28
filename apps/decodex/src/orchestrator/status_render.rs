fn render_operator_status(snapshot: &OperatorStatusSnapshot) -> String {
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

fn render_child_agent_activity_summary(summary: Option<&ChildAgentActivitySummary>) -> String {
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
			.map(render_protocol_activity_event_summary)
			.collect::<Vec<_>>()
			.join(", ")
	};

	format!("turn={turn}; waiting={wait}; rate_limit={rate_limit}; recent={recent}")
}

fn render_protocol_activity_event_summary(event: &ProtocolActivityEventSummary) -> String {
	event.detail.as_ref().map_or_else(
		|| event.event_type.clone(),
		|detail| format!("{}:{}", event.event_type, render_protocol_activity_detail(detail)),
	)
}

fn render_protocol_activity_detail(detail: &str) -> &str {
	if operator_protocol_activity_detail_is_public(detail) {
		detail
	} else {
		"redacted_sensitive_detail"
	}
}

fn render_loop_status_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(status) = status else {
		return String::from("none");
	};
	let next_action = status.next_action.as_deref().unwrap_or("none");
	let autonomy_objective = status
		.autonomy_objective
		.as_ref()
		.map(|objective| objective.source_ref.as_str())
		.unwrap_or("none");
	let autonomy_report =
		status.autonomy_report.as_ref().map(|report| report.authority.as_str()).unwrap_or("none");

	format!(
		"{}; review_level={}; autonomy={}; autonomy_objective={autonomy_objective}; autonomy_signals={}; autonomy_proposals={}; report={autonomy_report}; next_action={next_action}",
		status.summary,
		status.review_level,
		status.autonomy,
		status.autonomy_signals.len(),
		status.autonomy_proposals.len()
	)
}

fn render_loop_autonomy_signals_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(status) = status else {
		return String::from("none");
	};

	if status.autonomy_signals.is_empty() {
		return String::from("none");
	}

	status
		.autonomy_signals
		.iter()
		.map(|signal| {
			format!(
				"{}:{}@v{} freshness={} confidence={} privacy={} sources={} completeness={} gaps={} contradictions={}",
				signal.kind,
				signal.objective_id,
				signal.objective_version,
				signal.freshness,
				signal.confidence,
				signal.privacy,
				signal.source_refs.len(),
				signal.completeness,
				signal.gaps.len(),
				signal.contradictions.len()
			)
		})
		.collect::<Vec<_>>()
		.join(";")
}

fn render_loop_review_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(review) = status.and_then(|status| status.review.as_ref()) else {
		return String::from("none");
	};
	let checkpoint = review.checkpoint.as_ref().map_or_else(
		|| String::from("checkpoint=none"),
		|checkpoint| {
			format!(
				"checkpoint=head:{} round:{} review_class:{} risk_class:{} compact_eligible:{} fallback:{} updated:{}",
				checkpoint.head_sha,
				checkpoint.round,
				checkpoint.review_class.as_deref().unwrap_or("none"),
				checkpoint.risk_class.as_deref().unwrap_or("none"),
				checkpoint
					.compact_eligible
					.map_or("none", |eligible| if eligible { "true" } else { "false" }),
				checkpoint.fallback_reason.as_deref().unwrap_or("none"),
				checkpoint.updated_at
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
		"status={} reason={} guardrail={} boundary={} policy={} enhanced_evidence={} blocks_landing={} budget={} next_action={}",
		recovery.status,
		recovery.reason_code,
		recovery.guardrail_reason.as_deref().unwrap_or("none"),
		recovery.boundary_disposition.as_deref().unwrap_or("none"),
		recovery.boundary_policy_decision.as_deref().unwrap_or("none"),
		recovery.requires_enhanced_evidence,
		recovery.blocks_landing,
		budget,
		recovery.next_action
	)
}

fn render_loop_boundary_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(boundary) = status.and_then(|status| status.boundary.as_ref()) else {
		return String::from("none");
	};

	format!(
		"disposition={} policy={} enhanced_evidence={} blocks_landing={} reason={} attempted_recovery={} changed_surfaces={} improvement_signals={}",
		boundary.disposition,
		boundary.policy_decision,
		boundary.requires_enhanced_evidence,
		boundary.blocks_landing,
		boundary.reason.as_deref().unwrap_or("none"),
		boundary.attempted_recovery_reason.as_deref().unwrap_or("none"),
		boundary.changed_surface_count,
		boundary.improvement_signal_count
	)
}

fn render_control_capability_summary(capability: Option<&OperatorRunControlCapability>) -> String {
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
		summary.account_fingerprint, summary.status,
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
	let remaining =
		remaining_percent.map_or_else(|| String::from("unknown"), |value| format!("{value}%"));
	let reset = format_optional_unix_timestamp(resets_at_unix_epoch)
		.unwrap_or_else(|| String::from("unknown"));

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

fn render_child_agent_context_pressure(summary: Option<&ChildAgentActivitySummary>) -> String {
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

fn render_child_agent_bucket_distribution(buckets: &[ChildAgentActivityBucket]) -> String {
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

	output.push_str("  lifecycle_bucket_breakdown:\n");

	for phase in &lane.lifecycle_metrics.phases {
		output.push_str(&format!(
			"    - lifecycle_bucket: {} lifecycle_bucket_key: {} attempts: {} sources: recorded={} recovered={} current_snapshot={} captured: {}/{} protocol_events: {} child_events: {} wall: {} tool_calls: {} input_tokens: {} output_tokens: {}\n",
			phase.label,
			phase.phase,
			phase.attempt_count,
			phase.recorded_attempt_count,
			phase.recovered_attempt_count,
			phase.current_snapshot_attempt_count,
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
		"attempts={}; sources=recorded:{},recovered:{},current_snapshot:{}; captured={}/{}; missing={}; protocol_events={}; child_events={}; wall={}; tool_calls={}; input_tokens={}; output_tokens={}",
		metrics.attempt_count,
		metrics.recorded_attempt_count,
		metrics.recovered_attempt_count,
		metrics.current_snapshot_attempt_count,
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

fn render_lane_lifecycle_evidence(metrics: &OperatorLaneLifecycleMetrics) -> String {
	if metrics.attempt_evidence.is_empty() && metrics.recovery_gaps.is_empty() {
		return String::from("none");
	}

	let mut lines = metrics
		.attempt_evidence
		.iter()
		.map(|attempt| {
			let evidence = if attempt.evidence.is_empty() {
				String::from("none")
			} else {
				attempt.evidence.join(",")
			};
			let gaps = if attempt.gaps.is_empty() {
				String::from("none")
			} else {
				attempt.gaps.join(",")
			};

			format!(
				"run={} attempt={} phase={} source={} evidence={} gaps={} protocol_events={} child_events={} updated_at={}",
				attempt.run_id,
				attempt.attempt_number,
				attempt.phase,
				attempt.source,
				evidence,
				gaps,
				attempt.protocol_event_count,
				attempt.child_event_count,
				attempt.updated_at
			)
		})
		.collect::<Vec<_>>();

	if !metrics.recovery_gaps.is_empty() {
		lines.push(format!("aggregate_gaps={}", metrics.recovery_gaps.join(",")));
	}

	lines.join(" | ")
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

fn operator_run_phase_readback(run: &OperatorRunStatus) -> &str {
	if run.run_phase.trim().is_empty() { &run.phase } else { &run.run_phase }
}

fn append_rendered_run(output: &mut String, run: &OperatorRunStatus) {
	let (freshness_source, freshness_at) = operator_run_freshness(run);
	let protocol_event = render_run_protocol_event(run);
	let thread_id = run.thread_id.as_deref().unwrap_or("none");
	let turn_id = run.turn_id.as_deref().unwrap_or("none");
	let thread_status = run.thread_status.as_deref().unwrap_or("none");
	let thread_active_flags = render_run_thread_active_flags(run);
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
	let loop_autonomy_signals = render_loop_autonomy_signals_summary(run.loop_status.as_ref());
	let loop_review = render_loop_review_summary(run.loop_status.as_ref());
	let loop_architecture_recovery =
		render_loop_architecture_recovery_summary(run.loop_status.as_ref());
	let loop_boundary = render_loop_boundary_summary(run.loop_status.as_ref());
	let control_capability = render_control_capability_summary(run.control_capability.as_ref());
	let continuation_recovery =
		render_continuation_recovery_summary(run.continuation_recovery.as_ref());
	let phase_acceptance = render_phase_acceptance_summary(run.phase_acceptance.as_ref());

	output.push_str(&format!(
		"- run_id: {}\n  project_id: {}\n  issue_id: {}\n  issue_identifier: {}\n  title: {}\n  attempt: {}\n  status: {}\n  attempt_status: {}\n  status_projection_reason: {}\n  ownership_state: {}\n  liveness_state: {}\n  policy_state: {}\n  terminalization_state: {}\n  lane_control_next_action: {}\n  lane_control_conditions: {}\n  run_phase: {}\n  wait_reason: {}\n  current_operation: {}\n  active_goal_phase: {}\n  public_progress_phase: {}\n  run_lease: {}\n  queue_lease_state: {}\n  queue_lease: {}\n  execution_liveness: {}\n  has_fresh_execution: {}\n  counts_as_running: {}\n  needs_attention: {}\n  freshness_at: {}\n  freshness_source: {}\n  timing: run_idle={} protocol_idle={} last_progress={} protocol_event={} events={}\n  account: {}\n  accounts: {}\n  child_agent_activity: {}\n  protocol_activity: {}\n  context_pressure: {}\n  lifecycle_metrics: {}\n  lifecycle_evidence: {}\n  private_evidence: {}\n  loop_status: {}\n  loop_autonomy_signals: {}\n  loop_review: {}\n  loop_architecture_recovery: {}\n  loop_boundary: {}\n  control_capability: {}\n  thread_id: {}\n  turn_id: {}\n  thread_status: {}\n  thread_active_flags: {}\n  interactive_requested: {}\n  continuation_pending: {}\n  continuation_recovery: {}\n  phase_acceptance: {}\n  branch: {}\n  worktree_path: {}\n  updated_at: {}\n  last_run_activity_at: {}\n  last_protocol_activity_at: {}\n  last_progress_at: {}\n  idle_for_seconds: {}\n  protocol_idle_for_seconds: {}\n  suspected_stall: {}\n  progress_diagnostic: {}\n  process_id: {}\n  process_alive: {}\n  process_liveness_reason: {}\n  retry_kind: {}\n  next_retry_at: {}\n  effective_model: {}\n  effective_model_provider: {}\n  effective_cwd: {}\n  effective_approval_policy: {}\n  effective_approvals_reviewer: {}\n  effective_sandbox_mode: {}\n  protocol_event: {}\n  event_count: {}\n",
		run.run_id,
		run.project_id,
		run.issue_id,
		run.issue_identifier.as_deref().unwrap_or("none"),
		run.title.as_deref().unwrap_or("none"),
		run.attempt_number,
		run.status,
		run.attempt_status,
		run.status_projection_reason.as_deref().unwrap_or("none"),
		run.ownership_state,
		run.liveness_state,
		run.policy_state,
		run.terminalization_state,
		run.lane_control_next_action,
		render_lane_control_conditions(run),
		operator_run_phase_readback(run),
		run.wait_reason.as_deref().unwrap_or("none"),
		run.current_operation,
		run.active_goal_phase.as_deref().unwrap_or("none"),
		run.public_progress_phase.as_deref().unwrap_or("none"),
		if run.run_lease { "yes" } else { "no" },
		run.queue_lease_state,
		queue_lease,
		run.execution_liveness,
		if run.has_fresh_execution { "yes" } else { "no" },
		if run.counts_as_running { "yes" } else { "no" },
		if run.needs_attention { "yes" } else { "no" },
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
		render_lane_lifecycle_metrics(&run.lifecycle_metrics),
		render_lane_lifecycle_evidence(&run.lifecycle_metrics),
		private_evidence,
		loop_status,
		loop_autonomy_signals,
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
		continuation_recovery,
		phase_acceptance,
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

fn render_run_protocol_event(run: &OperatorRunStatus) -> String {
	match (&run.last_event_type, &run.last_event_at) {
		(Some(event_type), Some(timestamp)) => format!("{event_type} @ {timestamp}"),
		(Some(event_type), None) => event_type.clone(),
		(None, Some(timestamp)) => timestamp.clone(),
		(None, None) => String::from("none"),
	}
}

fn render_run_thread_active_flags(run: &OperatorRunStatus) -> String {
	if run.thread_active_flags.is_empty() {
		String::from("none")
	} else {
		run.thread_active_flags.join(",")
	}
}

fn render_lane_control_conditions(run: &OperatorRunStatus) -> String {
	if run.lane_control_conditions.is_empty() {
		String::from("none")
	} else {
		run.lane_control_conditions.join(",")
	}
}

fn render_continuation_recovery_summary(
	recovery: Option<&OperatorContinuationRecoveryStatus>,
) -> String {
	let Some(recovery) = recovery else {
		return String::from("none");
	};
	let message = recovery
		.source_error_message
		.as_deref()
		.map(single_line_status_value)
		.unwrap_or_else(|| String::from("none"));

	format!(
		"state={} source_phase={} next_phase={} source_error_class={} source_error_message={} count={}/{} budget_exceeded={} recorded_at={} run_id={} attempt={} next_action={}",
		recovery.state,
		recovery.source_phase,
		recovery.next_phase,
		recovery.source_error_class,
		message,
		recovery.recovery_count,
		recovery.automatic_continuation_limit,
		if recovery.budget_exceeded { "yes" } else { "no" },
		recovery.recorded_at,
		recovery.run_id,
		recovery.attempt_number,
		recovery.next_action,
	)
}

fn render_phase_acceptance_summary(acceptance: Option<&OperatorPhaseAcceptanceStatus>) -> String {
	let Some(acceptance) = acceptance else {
		return String::from("none");
	};
	let surfaces = if acceptance.changed_surfaces.is_empty() {
		String::from("none")
	} else {
		acceptance.changed_surfaces.join(",")
	};

	format!(
		"phase={} decision={} reason={} objective_covered={} effective_delta={} surfaces={} non_goal_passed={} validation_passed={} recorded_at={} run_id={} attempt={} next_action={}",
		acceptance.phase,
		acceptance.decision,
		acceptance.reason_code,
		if acceptance.objective_covered { "yes" } else { "no" },
		if acceptance.effective_delta_present { "yes" } else { "no" },
		surfaces,
		if acceptance.non_goal_passed { "yes" } else { "no" },
		if acceptance.validation_passed { "yes" } else { "no" },
		acceptance.recorded_at,
		acceptance.run_id,
		acceptance.attempt_number,
		acceptance.next_action
	)
}

fn single_line_status_value(value: &str) -> String {
	value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn operator_run_queue_lease_summary(run: &OperatorRunStatus) -> String {
	if run.run_lease {
		return String::from("held");
	}

	match run.execution_liveness.as_str() {
		"process_alive" => String::from("not_held (process_alive keeps lane visible)"),
		"thread_active" => String::from("not_held (thread_active keeps lane visible)"),
		"protocol_observed" => String::from("not_held (protocol_observed keeps lane visible)"),
		"process_stopped" => String::from("not_held (process_stopped needs attention)"),
		EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH
			if operator_run_has_recent_app_server_execution(run) =>
			String::from("not_held (app_server_activity keeps lane visible)"),
		EXECUTION_LIVENESS_PROCESS_IDENTITY_MISMATCH =>
			String::from("not_held (process_identity_mismatch needs attention)"),
		_ => String::from("not_held"),
	}
}

fn operator_run_freshness(run: &OperatorRunStatus) -> (&'static str, &str) {
	if operator_run_counts_as_current_lane(run) {
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
