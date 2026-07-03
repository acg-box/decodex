use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_history_lifecycle_metrics_are_grouped_by_lifecycle_bucket() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("function historyLaneLifecycleMetrics(lane)"));
	assert!(response.contains("function normalizeLifecyclePhaseMetrics(phase)"));
	assert!(response.contains("function renderHistoryLifecycleFacts(lane)"));
	assert!(response.contains("function renderPhaseBreakdown(lane)"));
	assert!(response.contains("Lifecycle tokens"));
	assert!(response.contains("Captured attempts"));
	assert!(response.contains("${renderHistoryLifecycleFacts(lane)}"));
	assert!(response.contains("${renderPhaseBreakdown(lane)}"));
	assert!(response.contains("phase-timeline"));
	assert!(response.contains("phase-list"));
	assert!(response.contains("phase-row"));
	assert!(response.contains("phase-name"));
	assert!(response.contains("phase-facts"));
	assert!(response.contains("history-phases:${lane.issue_key}"));
	assert!(!response.contains(".phase-row span:nth-child(n + 4)"));
	assert!(!response.contains("function renderAttemptTimeline(lane)"));
	assert!(!response.contains("history-attempts:${lane.issue_key}"));
	assert!(!response.contains("attempt-timeline"));
}

#[test]
fn operator_dashboard_keys_child_bucket_rows_for_stable_patching() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("function childBucketRenderKey(bucket)"));
	assert_eq!(
		response.matches("data-render-key=\"${escapeHtml(childBucketRenderKey(bucket))}\"").count(),
		2
	);
}

#[test]
fn operator_dashboard_current_lane_status_copy_stays_concise() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("runNeedsAttention"));
	assert!(response.contains("runCountsAsRunning"));
	assert!(response.contains("return run.counts_as_running;"));
	assert!(response.contains("return run.needs_attention;"));
	assert!(response.contains("return run.has_fresh_execution;"));
	assert!(response.contains("runWaitReasonShowsExecutionProgress"));
	assert!(response.contains(
		"[\"model_execution\", \"tool_execution\", \"protocol_activity\"].includes(run.wait_reason)"
	));
	assert!(response.contains("run.wait_reason && !runWaitReasonShowsExecutionProgress(run)"));
	assert!(!response.contains("runOperationRequiresLiveAgent"));
	assert!(!response.contains("runProcessStoppedWithoutAttention"));
	assert!(response.contains("runPhaseLabel"));
	assert!(response.contains("return run.process_liveness_reason || \"process_stopped\";"));
	assert!(response.contains("return displayToken(run.run_phase || run.phase || run.status);"));
	assert!(!response.contains(
		"return run.current_operation || run.run_phase || run.phase || \"process_stopped\";"
	));
	assert!(
		!response.contains("displayToken(run.current_operation || run.run_phase || run.phase)")
	);
	assert!(response.contains("Stopped agent process"));
	assert!(response.contains("attention stopped"));
	assert!(response.contains("inlineStatusFact(\"Agent\", \"Done\")"));
	assert!(response.contains("const waitReason = displayToken(run.wait_reason);"));
	assert!(response.contains("if (!displayTextRepeats(summary, waitReason))"));
	assert!(response.contains("displayTextRepeats(summary, \"operator input\")"));
	assert!(response.contains("function currentLaneVisibleSummary(card, run)"));
	assert!(response.contains(
		"currentLaneReadbackValues(run).some((value) => displayTextRepeats(summary, value))"
	));
	assert!(response.contains("const issueTitle = card.title || \"Run\";"));
	assert!(response.contains("const summary = currentLaneVisibleSummary(card, run);"));
	assert!(!response.contains("const summary = card.detail || \"\";"));
	assert!(!response.contains("Operator input needed."));
	assert!(!response.contains("Protocol idle."));
	assert!(response.contains("status: \"waiting\","));
	assert!(
		!response.contains("status: run.wait_reason ? `wait ${displayToken(run.wait_reason)}`")
	);
	assert!(!response.contains("Running through ${focus}"));
	assert!(!response.contains("Running through model execution."));
	assert!(!response.contains("Time is going to ${focus}."));
	assert!(!response.contains("Running now."));
	assert!(!response.contains("Thread is ${displayToken(run.thread_status).toLowerCase()}."));
	assert!(!response.contains("Agent turn complete; Decodex is finishing"));
	assert!(!response.contains("No agent progress for"));
	assert!(!response.contains("Waiting for approval or input."));
	assert!(!response.contains("Turn complete; continuation pending."));
	assert!(!response.contains("process <strong>stopped</strong>"));
	assert!(!response.contains("recovery <strong>needed</strong>"));
	assert!(
		response.contains("run.interactive_requested && !runStoppedProcessNeedsAttention(run)")
	);
	assert!(!response.contains(&["Process stopped;", " recovery needed."].concat()));
}
