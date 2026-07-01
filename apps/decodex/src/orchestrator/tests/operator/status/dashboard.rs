use super::*;

fn dashboard_response() -> String {
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

#[test]
fn operator_app_snapshot_endpoint_returns_json() {
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("app snapshot response should build"),
	)
	.expect("app snapshot response should be utf-8");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(response.contains("Content-Type: application/json\r\n"));
	assert!(response.ends_with("\r\n\r\n{}"));
}

#[test]
fn operator_dashboard_surfaces_loop_status_fields() {
	let response = dashboard_response();

	assert!(response.contains("function loopStatusFacts(loopStatus)"));
	assert!(response.contains("function loopStatusInline(loopStatus)"));
	assert!(response.contains("loopStatusInline(run.loop_status)"));
	assert!(response.contains("loopStatusFacts(run.loop_status)"));
	assert!(response.contains("loopStatusFacts(lane.loop_status)"));
	assert!(response.contains("loopStatusFacts(attention.loop_status)"));
	assert!(response.contains("function autonomyReadbackHasFreshSourceRefs(loopStatus)"));
	assert!(response.contains(
		"return sourceRefs.length > 0 && String(signal?.freshness || \"\") === \"fresh\";"
	));
	assert!(response.contains("function autonomyReadbackSummary(loopStatus)"));
	assert!(
		response.contains("field(\"Autonomy readback\", autonomyReadbackSummary(run.loop_status))")
	);
	assert!(!response.contains("facts.push([\"Autonomy\", displayToken(loopStatus.autonomy)]);"));
}

#[test]
fn operator_dashboard_surfaces_program_intake_panel() {
	let response = dashboard_response();

	assert!(response.contains("id=\"programs-panel\""));
	assert!(response.contains("id=\"programs-meta\""));
	assert!(response.contains("id=\"execution-programs\""));
	assert!(response.contains("<h2 id=\"programs-title\">Program Intake</h2>"));
	assert!(response.contains("programs: document.getElementById(\"programs-panel\")"));
	assert!(
		response.contains("executionPrograms: document.getElementById(\"execution-programs\")")
	);
	assert!(response.contains("programsMeta: document.getElementById(\"programs-meta\")"));
	assert!(response.contains("function renderExecutionPrograms(snapshot, derived)"));
	assert!(response.contains("function renderProgramNodeReadbacks(program)"));
	assert!(response.contains("program.node_readbacks ?? []"));
	assert!(response.contains("program.dispatchable_count"));
	assert!(
		response
			.contains("field(\"Program stage\", displayToken(node.program_stage || \"unknown\"))")
	);
	assert!(response.contains("node.dispatch_action"));
	assert!(response.contains("renderExecutionPrograms(snapshot, derived);"));
	assert!(response.contains("primary: [\"accountPool\", \"projects\", \"currentLanes\", \"programs\", \"queue\", \"review\", \"worktrees\", \"recent\"]"));
	assert!(response.contains(
		"{ marker: \"execution\", panels: [\"currentLanes\", \"programs\", \"queue\"] }"
	));
	assert!(!response.contains("data-program-edit"));
	assert!(!response.contains("data-program-mutate"));
}

#[test]
fn operator_dashboard_background_wash_stays_viewport_fixed() {
	let response = dashboard_response();

	assert!(response.contains("background-attachment: fixed, fixed, fixed, scroll;"));
	assert!(response.contains("background-size: 100vw 100vh, 100vw 100vh, 100vw 100vh, auto;"));
}

#[test]
fn operator_dashboard_uses_shared_type_scale_for_operator_rows() {
	let response = dashboard_response();
	let section_marker_title = response
		.split(".section-marker > span {")
		.nth(1)
		.expect("section marker title style should exist")
		.split(".section-marker > span::before")
		.next()
		.expect("section marker title style should end before marker rule");
	let panel_title = response
		.split(".panel-head h2 {")
		.nth(1)
		.expect("panel title style should exist")
		.split(".panel:hover .panel-head h2")
		.next()
		.expect("panel title style should end before hover rule");
	let section_marker = response
		.split(".section-marker {")
		.nth(1)
		.expect("section marker style should exist")
		.split(".section-marker:first-child")
		.next()
		.expect("section marker style should end before first child rule");
	let section_marker_bar = response
		.split(".section-marker > span::before {")
		.nth(1)
		.expect("section marker bar style should exist")
		.split(".section-marker-control")
		.next()
		.expect("section marker bar style should end before meta rule");
	let flow_step_label = response
		.split(".flow-step span {")
		.nth(1)
		.expect("flow step label style should exist")
		.split(".flow-step-labels")
		.next()
		.expect("flow step label style should end before grid rule");
	let table_meta = response
		.split(".table-meta {")
		.nth(1)
		.expect("table meta style should exist")
		.split(".table-meta:empty")
		.next()
		.expect("table meta style should end before empty rule");
	let metric_number = response
		.split(".metric-number {")
		.nth(1)
		.expect("metric number style should exist")
		.split(".metric-label")
		.next()
		.expect("metric number style should end before metric label rule");

	assert!(response.contains("--type-micro: 10px;"));
	assert!(response.contains("--type-caption: 11px;"));
	assert!(response.contains("--type-label: 12px;"));
	assert!(response.contains("--type-body: 13px;"));
	assert!(response.contains("--type-row-title: 13px;"));
	assert!(response.contains("--type-section-title: 13px;"));
	assert!(response.contains("--weight-label: 500;"));
	assert!(response.contains("--weight-strong: 600;"));
	assert!(response.contains("--tracking-caps: 0;"));
	assert!(response.contains("--tone-ready: #1d8968;"));
	assert!(response.contains("--tone-ready: #5cc59f;"));
	assert!(response.contains(".tone-ready"));
	assert!(response.contains("-apple-system, BlinkMacSystemFont"));
	assert!(response.contains("\"Menlo\", \"Monaco\", \"SFMono-Regular\", \"SF Mono\""));
	assert!(response.contains("ui-monospace, \"Cascadia Mono\","));
	assert!(response.contains("--space-panel-head-y: 12px;"));
	assert!(response.contains("--space-row-y: 12px;"));
	assert!(response.contains("--space-card-y: 16px;"));
	assert!(response.contains("--space-row-indent: 18px;"));
	assert!(section_marker_title.contains("font-size: var(--type-section-title);"));
	assert!(section_marker_title.contains("font-weight: var(--weight-label);"));
	assert!(section_marker.contains("font-family: var(--sans);"));
	assert!(!section_marker_title.contains("text-transform: uppercase;"));
	assert!(section_marker_bar.contains("height: 14px;"));
	assert!(!response.contains(".section-marker > .table-meta {"));
	assert!(flow_step_label.contains("font-family: var(--mono);"));
	assert!(flow_step_label.contains("font-size: var(--type-label);"));
	assert!(flow_step_label.contains("font-weight: var(--weight-label);"));
	assert!(!flow_step_label.contains("text-transform: uppercase;"));
	assert!(flow_step_label.contains("color: var(--muted-strong);"));
	assert!(panel_title.contains("font-size: var(--type-section-title);"));
	assert!(panel_title.contains("font-weight: var(--weight-label);"));
	assert!(panel_title.contains("font-family: var(--sans);"));
	assert!(!panel_title.contains("text-transform: uppercase;"));
	assert!(table_meta.contains("font-family: var(--mono);"));
	assert!(table_meta.contains("font-weight: var(--weight-label);"));
	assert!(!table_meta.contains("text-transform: uppercase;"));
	assert!(metric_number.contains("font-size: 0.92em;"));
	assert!(metric_number.contains("font-weight: var(--weight-label);"));
	assert!(response.contains("padding: var(--space-panel-head-y) 0 var(--space-md);"));
	assert!(
		response
			.contains("padding: var(--space-row-y) 0 var(--space-row-y) var(--space-row-indent);")
	);
	assert!(
		response.contains(
			"padding: var(--space-card-y) 0 var(--space-card-y) var(--space-row-indent);"
		)
	);
	assert!(response.contains(".project-title-line strong"));
	assert!(response.contains(".transport-meta[data-kind=\"endpoint\"] strong"));
	assert!(response.contains(".run-meta-item.is-missing strong"));
	assert!(response.contains(".project-work-ratio strong"));
	assert!(response.contains(".metric-number"));
	assert!(response.contains(".metric-label"));
	assert!(response.contains(".metric-group"));
	assert!(response.contains("gap: 4px;"));
	assert!(response.contains("font-variant-numeric: tabular-nums;"));
	assert!(response.contains(".account-row-id strong"));
	assert!(response.contains("font-size: var(--type-body);"));
	assert!(!response.contains("font-size: 17px;"));
	assert!(!response.contains("letter-spacing: 0.14em;"));
	assert!(!response.contains("font-weight: 700;"));
	assert!(!response.contains("--weight-label: 650;"));
	assert!(!response.contains("--weight-strong: 650;"));
	assert!(!response.contains("padding: 18px 0 18px 18px;"));
	assert!(!response.contains("padding: 20px 0 12px;"));
}

#[test]
fn operator_dashboard_cards_and_accounts_share_running_lane_typography() {
	let response = dashboard_response();
	let row_title = response
		.split(".row-title h3,\n\t\t\t.row-title h4,\n\t\t\t.run-title {")
		.nth(1)
		.expect("shared row title style should exist")
		.split(".worktree-card .row-summary")
		.next()
		.expect("shared row title style should end before worktree summary rule");
	let run_meta_line = response
		.split(".run-meta-line {")
		.nth(1)
		.expect("run meta line style should exist")
		.split(".run-meta-item {")
		.next()
		.expect("run meta line style should end before item rule");
	let account_row_id = response
		.split(".account-row-id {")
		.nth(1)
		.expect("account row id style should exist")
		.split(".account-row-id strong")
		.next()
		.expect("account row id style should end before strong rule");
	let row_aside = response
		.split(".row-aside {")
		.nth(1)
		.expect("row aside style should exist")
		.split(".run-title-stack")
		.next()
		.expect("row aside style should end before run title stack rule");
	let run_meta_icon = response
		.split(".run-meta-icon {")
		.nth(1)
		.expect("run meta icon style should exist")
		.split(".run-meta-icon svg")
		.next()
		.expect("run meta icon style should end before svg rule");

	assert!(response.contains(".run-title"));
	assert!(response.contains("[\"codex\", \"Codex\"]"));
	assert!(response.contains("[\"prs\", \"PRs\"]"));
	assert!(response.contains("titleCaseLabel(parts.label)"));
	assert!(response.contains("function detailLabel(label)"));
	assert!(
		response.contains("function renderValueLink(label, value, className = \"value-link\")")
	);
	assert!(response.contains("function localPathHref(value)"));
	assert!(response.contains("return `file://${encodeLocalPath(rawValue)}`;"));
	assert!(
		response.contains("const pullRequestMatch = text.match(/\\/pull\\/(\\d+)(?:$|[/?#])/);")
	);
	assert!(response.contains("return `#${pullRequestMatch[1]}`;"));
	assert!(response.contains("text-decoration: none;"));
	assert!(response.contains("background: color-mix(in srgb, currentColor 10%, transparent);"));
	assert!(
		response
			.contains("box-shadow: 0 0 0 1px color-mix(in srgb, currentColor 18%, transparent);")
	);
	assert!(response.contains(
		"function renderField(label, value, valueClass, labelFormatter, fieldClass = \"\")"
	));
	assert!(
		response.contains(
			"const fieldClassName = [\"field\", fieldClass].filter(Boolean).join(\" \");"
		)
	);
	assert!(response.contains("<div class=\"${fieldClassName}\">"));
	assert!(
		response.contains("<div class=\"field-label\">${escapeHtml(labelFormatter(label))}</div>")
	);
	assert!(response.contains("return renderField(label, value, valueClass, detailLabel);"));
	assert!(
		response.contains(
			"return renderField(label, value, valueClass, titleCaseLabel, \"card-field\");"
		)
	);
	assert!(
		response.contains("const valueHtml = renderValueLink(label, value) || escapeHtml(value);")
	);
	assert!(
		!response.contains("<div class=\"field-label\">${escapeHtml(titleCaseLabel(label))}</div>")
	);
	assert!(row_title.contains("font-size: var(--type-row-title);"));
	assert!(row_title.contains("font-weight: var(--weight-strong);"));
	assert!(row_title.contains("line-height: 1.28;"));
	assert!(response.contains("--type-row-aside: 11px;"));
	assert!(row_aside.contains("font-size: var(--type-row-aside);"));
	assert!(row_aside.contains("font-family: var(--mono);"));
	assert!(row_aside.contains("font-variant-numeric: tabular-nums;"));
	assert!(row_aside.contains("line-height: 1.22;"));
	assert!(response.contains("class=\"row-aside\""));
	assert!(!response.contains("class=\"card-meta\""));
	assert!(!response.contains("font-size: var(--type-card-title);"));
	assert!(!response.contains("--type-card-title:"));
	assert!(run_meta_line.contains("font-family: var(--mono);"));
	assert!(run_meta_line.contains("font-variant-ligatures: none;"));
	assert!(run_meta_line.contains("letter-spacing: 0;"));
	assert!(run_meta_line.contains("line-height: 1.35;"));
	assert!(response.contains(".run-meta-item.is-account {\n\t\t\t\tposition: relative;"));
	assert!(response.contains("padding-left: 16px;"));
	assert!(!response.contains(".run-meta-item.is-account {\n\t\t\t\talign-items: center;"));
	assert!(response.contains(".run-meta-icon {"));
	assert!(response.contains(".run-meta-icon svg"));
	assert!(run_meta_icon.contains("position: absolute;"));
	assert!(run_meta_icon.contains("top: 50%;"));
	assert!(run_meta_icon.contains("width: 12px;"));
	assert!(run_meta_icon.contains("height: 12px;"));
	assert!(run_meta_icon.contains("transform: translateY(-50%);"));
	assert!(response.contains(".run-meta-label"));
	assert!(account_row_id.contains("font-family: var(--mono);"));
	assert!(account_row_id.contains("font-variant-ligatures: none;"));
	assert!(account_row_id.contains("letter-spacing: 0;"));
	assert!(!response.contains(".account-row-id.is-machine"));
	assert!(!response.contains(".account-use-line"));
	assert!(!response.contains(".account-use-label"));
}

#[test]
fn operator_dashboard_patches_current_lane_cards_without_replacing_the_list() {
	let response = dashboard_response();

	assert!(response.contains("function renderStableList(container, html)"));
	assert!(response.contains("function animateStableListSize(container, startHeight)"));
	assert!(response.contains("function markStableListEnter(node)"));
	assert!(
		response.contains("function patchChildNodes(current, next, animateInsertions = false)")
	);
	assert!(response.contains("function currentLaneRenderKey(run)"));
	assert!(response.contains(
		"const issueKey =\n\t\t\t\t\tcanonicalIssueIdentityKey(run?.issue_id) ||\n\t\t\t\t\tcanonicalIssueIdentityKey(issueDisplayKey(run));"
	));
	assert!(response.contains("data-render-key=\"${escapeHtml(renderKey)}\""));
	assert!(response.contains("renderStableList(\n\t\t\t\t\tnodes.currentLanes,"));
	assert!(response.contains("patchChildNodes(container, template.content, true);"));
	assert!(response.contains("patchChildNodes(current, next, false);"));
	assert!(response.contains(
		"if (animateInsertions) {\n\t\t\t\t\t\t\tmarkStableListEnter(clone);\n\t\t\t\t\t\t}"
	));
	assert!(response.contains("markStableListEnter(clone);"));
	assert!(response.contains("container.style.height = `${startHeight}px`;"));
	assert!(response.contains(".is-list-entering"));
	assert!(response.contains("@keyframes stable-list-item-enter"));
	assert!(!response.contains("nodes.currentLanes.innerHTML = runs"));
	assert!(response.contains("return node.dataset.renderKey || node.dataset.detailKey || \"\";"));
	assert!(response.contains("current.closest(\"details.is-animating\")"));
	assert!(response.contains("width var(--slow) var(--ease),"));
}

#[test]
fn operator_dashboard_child_bucket_rows_split_time_bars_from_event_diagnostics() {
	let response = dashboard_response();

	assert_child_bucket_contract(&response);
	assert_child_activity_header_contract(&response);
	assert_child_lifecycle_contract(&response);
	assert_running_lane_meta_contract(&response);
	assert_liveness_and_cleanup_contract(&response);
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

#[test]
fn operator_dashboard_history_lifecycle_metrics_are_grouped_by_lifecycle_bucket() {
	let response = dashboard_response();

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
	let response = dashboard_response();

	assert!(response.contains("function childBucketRenderKey(bucket)"));
	assert_eq!(
		response.matches("data-render-key=\"${escapeHtml(childBucketRenderKey(bucket))}\"").count(),
		2
	);
}

#[test]
fn operator_dashboard_current_lane_status_copy_stays_concise() {
	let response = dashboard_response();

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

mod accounts;
mod activity;
mod projects;
