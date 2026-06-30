#[allow(clippy::wildcard_imports)]
use super::*;

pub(in crate::orchestrator) fn operator_history_lanes(
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

pub(in crate::orchestrator) fn hydrate_history_lane_from_run(
	lane: &mut OperatorHistoryLaneStatus,
	run: &OperatorRunStatus,
) {
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

pub(in crate::orchestrator) fn hydrate_current_lane_lifecycle_metrics(
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

pub(in crate::orchestrator) fn current_lane_lifecycle_attempts(
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

pub(in crate::orchestrator) fn operator_run_current_lane_snapshot_attempt(
	run: &OperatorRunStatus,
) -> OperatorRunStatus {
	let mut snapshot = run.clone();
	let mut evidence = std::collections::BTreeSet::<String>::new();

	evidence.insert(String::from("current_lane_snapshot"));
	evidence.extend(snapshot.lifecycle_evidence.iter().cloned());

	snapshot.lifecycle_source = String::from("current_snapshot");
	snapshot.lifecycle_evidence = evidence.into_iter().collect();

	snapshot
}

pub(in crate::orchestrator) fn operator_lane_lifecycle_metrics(
	attempts: &[OperatorRunStatus],
) -> OperatorLaneLifecycleMetrics {
	let mut metrics = operator_lane_lifecycle_totals(attempts.iter());

	metrics.phases = operator_lane_lifecycle_phase_metrics(attempts);

	metrics
}

pub(in crate::orchestrator) fn operator_lane_lifecycle_totals<'a>(
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

pub(in crate::orchestrator) fn operator_lane_lifecycle_phase_metrics(
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

pub(in crate::orchestrator) fn operator_run_lifecycle_metric_phase(
	run: &OperatorRunStatus,
) -> OperatorLifecycleMetricPhase {
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

pub(in crate::orchestrator) fn operator_lane_lifecycle_attempt_evidence(
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

pub(in crate::orchestrator) fn operator_lifecycle_metric_phase(
	key: &'static str,
	label: &'static str,
	rank: u8,
) -> OperatorLifecycleMetricPhase {
	OperatorLifecycleMetricPhase { key, label, rank }
}

pub(in crate::orchestrator) fn operator_run_group_key(run: &OperatorRunStatus) -> String {
	let issue_id = run.issue_id.trim();

	if !issue_id.is_empty() && !issue_id.eq_ignore_ascii_case("unknown") {
		return issue_id.to_ascii_uppercase();
	}

	operator_run_issue_key(run)
}

pub(in crate::orchestrator) fn operator_run_issue_key(run: &OperatorRunStatus) -> String {
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

pub(in crate::orchestrator) fn operator_run_issue_identifier_from_fields(
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

pub(in crate::orchestrator) fn issue_identifier_from_run_id(run_id: &str) -> Option<String> {
	if let Some((candidate, _attempt_suffix)) = run_id.split_once("-attempt-") {
		return issue_identifier_in_text(candidate);
	}
	if let Some(candidate) = run_id.strip_prefix("recovered-") {
		return issue_identifier_in_text(candidate);
	}

	None
}

pub(in crate::orchestrator) fn issue_identifier_in_text(value: &str) -> Option<String> {
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
