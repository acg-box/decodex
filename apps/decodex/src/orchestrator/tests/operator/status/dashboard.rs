mod accounts;
mod activity;
mod http_and_panels;
mod lane_patching;
mod lifecycle_rows;
mod projects;
mod type_scale;

use crate::orchestrator;

pub(super) fn dashboard_response() -> String {
	String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_DASHBOARD_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("dashboard response should build"),
	)
	.expect("dashboard response should be utf-8")
}

fn assert_contains_all(response: &str, snippets: &[&str]) {
	for snippet in snippets {
		assert!(response.contains(snippet), "dashboard response should contain {snippet:?}");
	}
}

fn assert_excludes_all(response: &str, snippets: &[&str]) {
	for snippet in snippets {
		assert!(!response.contains(snippet), "dashboard response should not contain {snippet:?}");
	}
}

fn assert_child_bucket_contract(response: &str) {
	assert_contains_all(
		response,
		&[
			"childBucketIsSubsecond",
			"childBucketIsEventOnly",
			"childBucketEventSignals",
			"childBucketEventSummary",
			"childBucketDiagnosticSignals",
			"childBucketDiagnosticSummary",
			"renderChildBucketDiagnosticSignals",
			"childBucketHasMeaningfulWallShare",
			"childAgentLargeOutputWarnings",
			"childAgentLargeOutputSummary",
			"childBucketShareLabel",
			"childBucketWidth",
			"function childBucketIsPrimaryShareBucket(bucket)",
			"function childBucketIsLifecycleTotalBucket(bucket)",
			"childBucketIsPrimaryShareBucket(bucket) &&",
			"!childBucketIsLifecycleTotalBucket(bucket) &&",
			"child-bucket is-share",
			"child-bucket is-diagnostic",
			"child-bucket is-event-only",
			"child-bucket-signals",
			"child-bucket-signal",
			"data-duration=\"wall-share\"",
			"data-duration=\"event-diagnostics\"",
			"data-duration=\"diagnostic\"",
			"function childDiagnosticBucketRank(bucket)",
			"--child-bucket-value-column: clamp(190px, 18vw, 230px);",
			"grid-template-columns: 96px minmax(64px, 1fr) var(--child-bucket-value-column);",
			"width: var(--child-bucket-value-column);",
		],
	);
	assert_excludes_all(
		response,
		&[
			"events only",
			"child-warning",
			"${warnings.length ? `<div class=\"child-warning\">",
			"warnings.join(\" · \")",
			"summary.largest_tool_output_tool || \"tool\"",
			"child-bucket is-subsecond",
			"data-duration=\"events-only\"",
			"child-bucket.is-event-only .child-bucket-bar::before",
		],
	);
}

fn assert_child_activity_header_contract(response: &str) {
	assert_contains_all(
		response,
		&[
			"function renderMetricText(text)",
			"function setMetricText(node, text)",
			"function setPanelMeta(node, text, tone = \"\")",
			"function pluralLabel(count, singular, plural = `${singular}s`)",
			"return `${count} ${pluralLabel(count, singular, plural)}`;",
			"pluralLabel(notices.length, \"alert\")",
			"pluralLabel(notices.length, \"warning\")",
			"summary.current_detail",
			"detailLabel(displayToken(summary.current_detail || summary.current_bucket))",
			"return `${label} · ${formatDuration(summary.current_elapsed_seconds)}`;",
			"function runProjectSummary(run)",
			"<div class=\"child-activity-head is-project\">",
			"<span>Project</span>",
			"${escapeHtml(runProjectSummary(run))}",
			"<span>Activity</span>",
			".child-activity-head {\n\t\t\t\tdisplay: grid;\n\t\t\t\tgrid-template-columns: 96px minmax(0, 1fr);\n\t\t\t\talign-items: baseline;\n\t\t\t\tgap: 10px;",
			"const current = childAgentCurrentSummary(summary) || \"none\";",
			"[\"current window\", latestInput",
			"\"peak window\",",
		],
	);
	assert_excludes_all(
		response,
		&[
			"titleCaseLabel(\"agent activity\")",
			"<span>agent activity</span>",
			"<span>Agent Activity</span>",
			"<span>Agent Now</span>",
			"No active child bucket",
		],
	);
}

fn assert_child_lifecycle_contract(response: &str) {
	assert_contains_all(
		response,
		&[
			"function renderChildLifecycleOverview(lifecycle, contextFacts)",
			"function renderChildLifecyclePhaseTable(phases)",
			"function lifecycleRecoveryDebugSummary(metrics)",
			"function lifecycleEvidenceDebugSummary(metrics)",
			"<div class=\"child-total-overview\" aria-label=\"Lifecycle total metrics\">",
			"<span class=\"child-total-segment\">",
			"repeat(4, max-content max-content);",
			".child-total-segment {\n\t\t\t\tdisplay: contents;",
			"<div class=\"child-phase-table\" role=\"table\" aria-label=\"Lifecycle bucket metrics\">",
			".child-phase-table {\n\t\t\t\tdisplay: inline-grid;\n\t\t\t\tgrid-template-columns:\n\t\t\t\t\tmax-content",
			"gap: 4px clamp(24px, 2vw, 34px);",
			"overflow: hidden;",
			"text-overflow: ellipsis;",
			"function formatLargestOutputValue(bytes)",
			"return formatCompactBytes(bytes);",
			"const header = [\"Lifecycle bucket\", \"attempts\", \"inference\", \"input\", \"output\", \"tools\", \"max output\"];",
			"pluralize(phases.length, \"lifecycle bucket\")",
			"const alignRight = new Set([1, 2, 3, 4, 5, 6]);",
			"width: fit-content;\n\t\t\t\tmax-width: 100%;",
			"\"tools\"",
			"output bytes",
			"field(\"Large outputs\", childAgentLargeOutputSummary(childAgentActivity(run)))",
		],
	);
	assert_excludes_all(
		response,
		&[
			"--child-total-segment-width",
			"--child-total-column-template",
			"child-total-control",
			"child-total-separator",
			".child-phase-table-cell:nth-child(7n)",
			"input / output",
			"tools / largest output",
			"rows.push(renderChildContextRow(\"Total\", totalFacts, \"is-total\"));",
			"\"cumulative input\",",
			"[\"tool calls\", String(summary.tool_call_count ?? 0)]",
			"[\"largest output\",",
			"\"model time\"",
			"\"input tokens\"",
			"\"output tokens\"",
			"${output}(${source})",
			"function largestOutputHelp(bytes, tool = \"\")",
			"const titleAttribute = segment.help ? ` title=\"${escapeHtml(segment.help)}\"` : \"\";",
			"largestOutputHelp(largestOutput, lifecycle?.largest_tool_output_tool)",
			"formatLargestOutputValue(largestOutput, lifecycle?.largest_tool_output_tool)",
			"const toolSegments = [];",
			"rows.push({ label: \"Tools\", segments: toolSegments });",
		],
	);
}

fn assert_running_lane_meta_contract(response: &str) {
	assert_contains_all(
		response,
		&[
			"runningLaneMetaText",
			"const parts = [`${derived.liveRuns ?? 0} running`];",
			"attentionCount === 1",
			"nodes.currentLanesMeta,",
			"runningLaneMetaText(derived),",
		],
	);
	assert_excludes_all(
		response,
		&[
			"Snapshot pending",
			"COPY.waitingSnapshot",
			"const parts = [`${derived.liveRuns} live`];",
			"parts.push(`${derived.runningAttentionCount} stalled`)",
		],
	);
}

fn assert_liveness_and_cleanup_contract(response: &str) {
	assert_contains_all(
		response,
		&[
			"runHasFreshExecution",
			"typeof run?.has_fresh_execution === \"boolean\"",
			"typeof run?.needs_attention === \"boolean\"",
			"typeof run?.counts_as_running === \"boolean\"",
			"runStaleWithoutKnownProcessNeedsAttention",
			"runExecutionLivenessSummary",
			"runOwnershipSummary",
			"runLivenessStateSummary",
			"runPolicyStateSummary",
			"runTerminalizationSummary",
			"runLaneControlConditionsSummary",
			"runQueueLeaseSummary",
			"return displayToken(run.execution_liveness || \"liveness_unknown\");",
			"return displayToken(run.ownership_state || (runCountsAsRunning(run) ? \"leased_run\" : \"unknown\"));",
			"field(\"Attempt status\", run.attempt_status || run.status)",
			"field(\"Queue lease\", runQueueLeaseSummary(run))",
			"field(\"Execution liveness\", runExecutionLivenessSummary(run))",
			"field(\"Ownership\", runOwnershipSummary(run))",
			"field(\"Liveness state\", runLivenessStateSummary(run))",
			"field(\"Policy state\", runPolicyStateSummary(run))",
			"field(\"Terminalization\", runTerminalizationSummary(run))",
			"field(\"Lane next action\", capturedValue(run.lane_control_next_action))",
			"field(\"Lane conditions\", runLaneControlConditionsSummary(run))",
			"inlineStatusFact(\"Owner\", displayToken(run.ownership_state))",
			"inlineStatusFact(\"Policy\", displayToken(run.policy_state))",
			"live_no_queue_lease",
			"return `${leaseState}; ${displayToken(run.execution_liveness || \"liveness_unknown\")}`;",
			"attention.worktree_path",
			"candidate.attention?.attention_error_class",
			"facts.push([\"Cause\", displayToken(attention.attention_error_class)]);",
			"queued attention",
			"worktree.ownership_reason",
			"const hygiene = worktree.hygiene;",
			"hygiene.classification === \"merged_dirty_worktree\"",
			"post-land cleanup blocked",
			"post-land cleanup",
			"post-review cleanup blocked",
			"hygiene.reason ||",
			"function renderWorktreeHygieneFields(worktree)",
			"field(\"Cleanup state\", displayToken(hygiene.classification || \"cleanup_pending\"))",
			"field(\"Default branch\", hygiene.default_branch || \"unknown\")",
			"field(\"Uncommitted changes\", hygiene.dirty ? \"yes\" : \"no\")",
			"local cleanup",
			"Owned by Intake Queue attention; recover there before cleanup.",
			"No lane owns this worktree; inspect before cleanup.",
		],
	);
	assert_excludes_all(
		response,
		&["return \"Process alive\";", "lease <strong>not held</strong>", "Queue ownership"],
	);
}
