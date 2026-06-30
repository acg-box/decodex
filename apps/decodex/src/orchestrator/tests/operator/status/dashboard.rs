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
	assert!(response.contains("field(\"Autonomy readback\", autonomyReadbackSummary(run.loop_status))"));
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
	assert!(response.contains("executionPrograms: document.getElementById(\"execution-programs\")"));
	assert!(response.contains("programsMeta: document.getElementById(\"programs-meta\")"));
	assert!(response.contains("function renderExecutionPrograms(snapshot, derived)"));
	assert!(response.contains("function renderProgramNodeReadbacks(program)"));
	assert!(response.contains("program.node_readbacks ?? []"));
	assert!(response.contains("program.dispatchable_count"));
	assert!(response.contains("field(\"Program stage\", displayToken(node.program_stage || \"unknown\"))"));
	assert!(response.contains("node.dispatch_action"));
	assert!(response.contains("renderExecutionPrograms(snapshot, derived);"));
	assert!(response.contains("primary: [\"accountPool\", \"projects\", \"currentLanes\", \"programs\", \"queue\", \"review\", \"worktrees\", \"recent\"]"));
	assert!(response.contains("{ marker: \"execution\", panels: [\"currentLanes\", \"programs\", \"queue\"] }"));
	assert!(!response.contains("data-program-edit"));
	assert!(!response.contains("data-program-mutate"));
}

#[test]
fn operator_dashboard_background_wash_stays_viewport_fixed() {
	let response = dashboard_response();

	assert!(response.contains("background-attachment: fixed, fixed, fixed, scroll;"));
	assert!(
		response.contains("background-size: 100vw 100vh, 100vw 100vh, 100vw 100vh, auto;")
	);
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
	assert!(response.contains("padding: var(--space-row-y) 0 var(--space-row-y) var(--space-row-indent);"));
	assert!(response.contains("padding: var(--space-card-y) 0 var(--space-card-y) var(--space-row-indent);"));
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
	assert!(response.contains("function renderValueLink(label, value, className = \"value-link\")"));
	assert!(response.contains("function localPathHref(value)"));
	assert!(response.contains("return `file://${encodeLocalPath(rawValue)}`;"));
	assert!(response.contains("const pullRequestMatch = text.match(/\\/pull\\/(\\d+)(?:$|[/?#])/);"));
	assert!(response.contains("return `#${pullRequestMatch[1]}`;"));
	assert!(response.contains("text-decoration: none;"));
	assert!(response.contains("background: color-mix(in srgb, currentColor 10%, transparent);"));
	assert!(response.contains("box-shadow: 0 0 0 1px color-mix(in srgb, currentColor 18%, transparent);"));
	assert!(response.contains("function renderField(label, value, valueClass, labelFormatter, fieldClass = \"\")"));
	assert!(response.contains("const fieldClassName = [\"field\", fieldClass].filter(Boolean).join(\" \");"));
	assert!(response.contains("<div class=\"${fieldClassName}\">"));
	assert!(response.contains("<div class=\"field-label\">${escapeHtml(labelFormatter(label))}</div>"));
	assert!(response.contains("return renderField(label, value, valueClass, detailLabel);"));
	assert!(response.contains("return renderField(label, value, valueClass, titleCaseLabel, \"card-field\");"));
	assert!(response.contains("const valueHtml = renderValueLink(label, value) || escapeHtml(value);"));
	assert!(!response.contains("<div class=\"field-label\">${escapeHtml(titleCaseLabel(label))}</div>"));
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
	assert!(response.contains("function patchChildNodes(current, next, animateInsertions = false)"));
	assert!(response.contains("function currentLaneRenderKey(run)"));
	assert!(response.contains(
		"const issueKey =\n\t\t\t\t\tcanonicalIssueIdentityKey(run?.issue_id) ||\n\t\t\t\t\tcanonicalIssueIdentityKey(issueDisplayKey(run));"
	));
	assert!(response.contains("data-render-key=\"${escapeHtml(renderKey)}\""));
	assert!(response.contains("renderStableList(\n\t\t\t\t\tnodes.currentLanes,"));
	assert!(response.contains("patchChildNodes(container, template.content, true);"));
	assert!(response.contains("patchChildNodes(current, next, false);"));
	assert!(response.contains("if (animateInsertions) {\n\t\t\t\t\t\t\tmarkStableListEnter(clone);\n\t\t\t\t\t\t}"));
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
		assert!(
			response.contains(snippet),
			"dashboard response should contain {snippet:?}"
		);
	}
}

fn assert_excludes_all(response: &str, snippets: &[&str]) {
	for snippet in snippets {
		assert!(
			!response.contains(snippet),
			"dashboard response should not contain {snippet:?}"
		);
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
		response
			.matches("data-render-key=\"${escapeHtml(childBucketRenderKey(bucket))}\"")
			.count(),
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
	assert!(!response.contains("return run.current_operation || run.run_phase || run.phase || \"process_stopped\";"));
	assert!(!response.contains("displayToken(run.current_operation || run.run_phase || run.phase)"));
	assert!(response.contains("Stopped agent process"));
	assert!(response.contains("attention stopped"));
	assert!(response.contains("inlineStatusFact(\"Agent\", \"Done\")"));
	assert!(response.contains("const waitReason = displayToken(run.wait_reason);"));
	assert!(response.contains("if (!displayTextRepeats(summary, waitReason))"));
	assert!(response.contains("displayTextRepeats(summary, \"operator input\")"));
	assert!(response.contains("function currentLaneVisibleSummary(card, run)"));
	assert!(response.contains("currentLaneReadbackValues(run).some((value) => displayTextRepeats(summary, value))"));
	assert!(response.contains("const issueTitle = card.title || \"Run\";"));
	assert!(response.contains("const summary = currentLaneVisibleSummary(card, run);"));
	assert!(!response.contains("const summary = card.detail || \"\";"));
	assert!(!response.contains("Operator input needed."));
	assert!(!response.contains("Protocol idle."));
	assert!(response.contains("status: \"waiting\","));
	assert!(!response.contains(
		"status: run.wait_reason ? `wait ${displayToken(run.wait_reason)}`"
	));
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
	assert!(response.contains("run.interactive_requested && !runStoppedProcessNeedsAttention(run)"));
	assert!(!response.contains(&["Process stopped;", " recovery needed."].concat()));
}

#[test]
fn operator_dashboard_renders_account_usage_controls() {
	let response = dashboard_response();

	assert!(response.contains("function codexAccount(run, snapshot = null)"));
	assert!(response.contains("function codexAccounts(run)"));
	assert!(response.contains("function selectedDashboardAccount(snapshot)"));
	assert!(!response.contains("function runAuthor(run)"));
	assert!(!response.contains("function renderRunAuthorInline(run)"));
	assert!(response.contains("function codexAccountDisplayName(account)"));
	assert!(response.contains("function codexAccountTokenLabel(refreshStatus)"));
	assert!(response.contains("function codexAccountWindowLabel(seconds)"));
	assert!(response.contains("function codexAccountStatusTone(account)"));
	assert!(response.contains("function renderCodexAccountPoolUsageSummary(accounts)"));
	assert!(response.contains("function accountPoolDayDeltaPercentagePoints(accounts, estimate)"));
	assert!(response.contains("accountApiSnapshot?.usage_estimate"));
	assert!(!response.contains("snapshot?.usage_estimate"));
	assert!(response.contains("Pool used"));
	assert!(response.contains("Day Δ"));
	assert!(response.contains("Daily avg"));
	assert!(response.contains("accounts measured"));
	assert!(response.contains("function renderRunCodexAccountInline(run, snapshot)"));
	assert!(response.contains("function renderRunMetaLine(run, snapshot = null)"));
	assert!(response.contains("run account capture pending"));
	assert!(response.contains("function renderAccountPool(snapshot)"));
	assert!(response.contains("function renderAccountModeControl(snapshot)"));
	assert!(response.contains("nodes.accountModeMeta.innerHTML = `<span class=\"account-mode-head\">${escapeHtml(title)}</span>`;"));
	assert!(response.contains("nodes.accountModeMeta.title = title;"));
	assert!(response.contains("function codexAccountPoolAccounts()"));
	assert!(response.contains("accountApiAccounts().map((account) => ({ ...account }))"));
	assert!(!response.contains("function configuredDashboardAccounts(snapshot)"));
	assert!(!response.contains("function codexAccountPoolMergeRank(account)"));
	assert!(response.contains("function renderCodexAccountPool(accounts, snapshot)"));
	assert!(!response.contains("function renderCodexAccountPoolHeader(accounts)"));
	assert!(response.contains(
		"function renderCodexAccountPoolRow(account, snapshot, isLastAccount = false)"
	));
	assert!(response.contains("function renderCodexAccountNameControl(account, snapshot)"));
	assert!(!response.contains("ACCOUNT_SELECTION_CONFIRMATION_MS"));
	assert!(!response.contains("accountSelectionConfirmationTimer"));
	assert!(response.contains("function accountSelectionConfirmationMatches(action, selector)"));
	assert!(response.contains("function syncAccountSelectionConfirmationDom()"));
	assert!(response.contains("clearAccountSelectionConfirmation(true);"));
	assert!(response.contains("function accountSelectionControlTitle(action, displayTitle, armed)"));
	assert!(response.contains("function handleAccountSelectionConfirmation(action, selector)"));
	assert!(response.contains("data-account-confirm-action=\"${escapeHtml(action)}\""));
	assert!(response.contains("data-account-display-title=\"${escapeHtml(displayTitle)}\""));
	assert!(!response.contains("data-account-select=\""));
	assert!(!response.contains("dataset.accountSelect;"));
	assert!(!response.contains("account-select-button"));
	assert!(!response.contains("data-account-project-select"));
	assert!(response.contains("sendDashboardControl(action, { accountSelector: selector });"));
	assert!(response.contains("sendDashboardControl(action);"));
	assert!(response.contains("clearAccountSelection"));
	assert!(response.contains("selectAccount"));
	assert!(response.contains("function codexAccountDebugSummary(account)"));
	assert!(!response.contains("function codexAccountPoolDebugSummary(accounts)"));
	assert!(response.contains("return \"not captured\";"));
	assert!(response.contains("function codexAccountHistorySummary(account)"));
	assert!(!response.contains("snapshot?.accounts"));
	assert!(response.contains("account?.account_email || account?.email"));
	assert!(response.contains("run?.account || run?.codex_account || null"));
	assert!(response.contains("run?.accounts"));
	assert!(response.contains("run?.codex_accounts"));
	assert!(response.contains("account-pool-panel"));
	assert!(!response.contains("<h2>Accounts</h2>"));
	assert!(!response.contains("<h2>Codex Accounts</h2>"));
	assert!(!response.contains(".stack > .panel + .panel"));
	assert!(response.contains("panel section-control\" id=\"account-pool-panel\""));
	assert!(response.contains("section-marker section-marker-control"));
	assert!(response.contains("section-marker section-marker-projects"));
	assert!(response.contains("aria-label=\"Accounts group\""));
	assert!(response.contains("<span>Accounts</span>"));
	assert!(response.contains("<p class=\"table-meta section-marker-meta\" id=\"account-mode-meta\"></p>"));
	assert!(response.contains("accountModeMeta: document.getElementById(\"account-mode-meta\")"));
	assert!(!response.contains("Accounts\n\t\t\t\t\t\t\t<button class=\"account-privacy-toggle\""));
	assert!(!response.contains("account-mode-control"));
	assert!(!response.contains("account-mode-status"));
	assert!(!response.contains("<span>Control Plane</span>"));
	assert!(response.contains("Projects\n\t\t\t\t\t\t\t<button class=\"project-filter-toggle\""));
	assert!(!response.contains("id=\"account-pool-meta\""));
	assert!(!response.contains("id=\"projects-meta\""));
	assert!(!response.contains("<p>All · Active</p>"));
	assert!(response.contains("<span>Execution</span>"));
	assert!(response.contains("<span>Closeout</span>"));
	assert!(!response.contains("<p>Accounts</p>"));
	assert!(!response.contains("All · Active"));
	assert!(!response.contains("Running · Intake"));
	assert!(!response.contains("Review · Recovery · History"));
	assert!(!response.contains("data-fold-key=\"panel:projects\""));
	assert!(response.contains("panel section-execution\" id=\"current-lanes-panel\""));
	assert!(response.contains("panel section-aftercare\" id=\"review-panel\""));
	assert!(!response.contains("section-group-start"));
	assert!(response.contains("#queue-panel .panel-head"));
	assert!(!response.contains("queue-group"));
	assert!(!response.contains("queue-group-header"));
	assert!(!response.contains("queue-group-count"));
	assert!(response.contains("nodes.projectTitle.textContent = \"Decodex\""));
	assert!(!response.contains("Decodex Operator"));
	assert!(response.contains("primary: [\"accountPool\", \"projects\", \"currentLanes\", \"programs\", \"queue\", \"review\", \"worktrees\", \"recent\"]"));
	assert!(!response.contains("#account-pool-panel {"));
	assert!(!response.contains("No accounts"));
	assert!(response.contains("#current-lanes-panel {\n\t\t\t\tbackground: transparent;"));
	assert!(!response.contains("account-pool-title"));
	assert!(response.contains("account-privacy-toggle"));
	assert!(response.contains("account-eye-open"));
	assert!(response.contains("account-eye-off"));
}

#[test]
fn operator_dashboard_account_privacy_controls_use_compact_identities() {
	let response = dashboard_response();

	assert!(response.contains("const ACCOUNT_PRIVACY_STORAGE_KEY = \"decodex.operator.accountPrivacy\";"));
	assert!(!response.contains("const ACCOUNT_NAME_OFFSET_STORAGE_KEY = \"decodex.operator.accountNameOffsets\";"));
	assert!(response.contains("const ACCOUNT_IDENTITY_EDGE_CHARS = 6;"));
	assert!(response.contains("const ACCOUNT_IDENTITY_MIN_EDGE_CHARS = 3;"));
	assert!(!response.contains("const ACCOUNT_EMAIL_LOCAL_HEAD_CHARS = 5;"));
	assert!(!response.contains("const ACCOUNT_EMAIL_LOCAL_TAIL_CHARS = 4;"));
	assert!(response.contains("const ACCOUNT_RANDOM_NAMES = ["));
	assert!(!response.contains("const ACCOUNT_RANDOM_NAME_PREFIXES = ["));
	assert!(!response.contains("const ACCOUNT_RANDOM_NAME_SUFFIXES = ["));
	assert!(response.contains("function trimLeadingEllipsis(value)"));
	assert!(response.contains("function compactAccountIdentity(value)"));
	assert!(!response.contains("function compactAccountEmailIdentity(value)"));
	assert!(response.contains("function codexAccountIdentityHash(value)"));
	assert!(response.contains("function codexAccountRandomName(account)"));
	assert!(response.contains("function codexAccountEmail(account)"));
	assert!(response.contains("function compactAccountEmail(email)"));
	assert!(response.contains("function loadAccountPrivacy()"));
	assert!(!response.contains("function loadAccountNameOffsets()"));
	assert!(response.contains("function persistAccountPrivacy(hidden)"));
	assert!(!response.contains("function persistAccountNameOffsets()"));
	assert!(!response.contains("function configuredDashboardAccounts(snapshot)"));
	assert!(response.contains("function renderAccountPrivacyToggle()"));
	assert!(response.contains("function codexAccountRandomNameKey(account)"));
	assert!(response.contains("function codexAccountRandomNameOffset(account)"));
	assert!(response.contains("function codexAccountPendingRandomNameOffset(account)"));
	assert!(response.contains("let pendingAccountNameOffsets = {};"));
	assert!(!response.contains("function codexAccountStoredRandomNameOffset(account)"));
	assert!(!response.contains("function syncStoredAccountNameOffsets(accounts)"));
	assert!(response.contains("function codexAccountDisplaySource(account, snapshot)"));
	assert!(response.contains("function renderCodexAccountRandomNameButton(account)"));
	assert!(response.contains("function codexAccountShowsEmail(account)"));
	assert!(response.contains("function codexAccountPrivacyLabel(account)"));
	assert!(response.contains("function codexAccountPrivacyText(account, value)"));
	assert!(response.contains("function codexAccountVisibleName(account)"));
	assert!(response.contains("function codexAccountDisplayTitle(account)"));
	assert!(response.contains("function codexAccountControlStatusLabel(snapshot)"));
	assert!(response.contains("text = replaceLiteral(text, codexAccountEmail(account), replacement);"));
	assert!(response.contains("/[A-Z0-9._%+-]+@[A-Z0-9.-]+\\.[A-Z]{2,}/gi"));
	assert!(response.contains("return codexAccountShowsEmail(account) ? email : codexAccountRandomName(account);"));
	assert!(response.contains("? compactAccountEmail(email)"));
	assert!(response.contains("const account = codexAccountPoolAccounts(snapshot).find("));
	assert!(response.contains("? compactAccountIdentity(selector)"));
	assert!(response.contains(": codexAccountVisibleName(account);"));
	assert!(response.contains("return \"Balanced\";"));
	assert!(response.contains("return `Fixed · ${label}`;"));
	assert!(response.contains("function codexAccountFallbackName(value)"));
	assert!(response.contains("return `Fixed · ${codexAccountFallbackName(selector)}`;"));
	assert!(response.contains("const title = codexAccountControlStatusLabel(snapshot);"));
	assert!(!response.contains("const title = `Mode ${modeLabel}`;"));
	assert!(response.contains("account-name-reroll"));
	assert!(response.contains("data-account-name-reroll"));
	assert!(response.contains("aria-label=\"Change account name\""));
	assert!(response.contains("\"Alex\""));
	assert!(response.contains("return `${local.slice(0, 3)}...${local.slice(-3)}${domain}`;"));
	assert!(response.contains("return ACCOUNT_RANDOM_NAMES[index];"));
	assert!(response.contains("return accounts;"));
	assert!(!response.contains("function codexAccountPoolSortKey(account)"));
	assert!(!response.contains("return codexAccountPoolSortKey(left).localeCompare(codexAccountPoolSortKey(right));"));
	assert!(!response.contains("const checkedAt = codexAccountNumber(account?.checked_at_unix_epoch) || 0;"));
	assert!(!response.contains("localeCompare(codexAccountDisplayName(right))"));
	assert!(!response.contains("return account.account_fingerprint;"));
	assert!(!response.contains("`fingerprint ${account.account_fingerprint || \"unknown\"}`"));
	assert!(!response.contains("const fingerprint = account.account_fingerprint || \"unknown\";"));
	assert!(!response.contains("account.account_fingerprint || \"unknown\",\n"));
	assert!(response.contains("renderAccountPrivacyToggle();"));
	assert!(response.contains("renderAccountModeControl(snapshot);"));
	assert!(response.contains("persistAccountPrivacy(accountEmailsHidden);"));
	assert!(response.contains("let lastDashboardRender = null;"));
	assert!(response.contains("lastDashboardRender = {"));
	assert!(response.contains("function renderDashboardState({"));
	assert!(response.contains("renderDashboardState(lastDashboardRender);"));
	assert!(response.contains(".table-meta .metric-number"));
	assert!(response.contains(".table-meta[data-tone=\"active\"] .metric-number"));
	assert!(response.contains("font-size: var(--type-label);"));
	assert!(response.contains("letter-spacing: var(--tracking-caps);"));
	assert!(!response.contains("text-transform: uppercase;"));
	assert!(response.contains("function renderCodexAccountPoolGuideCell(column)"));
	assert!(response.contains("return `<span class=\"account-pool-heading\">${sortButton}${accountPrivacyToggleMarkup()}</span>`;"));
	assert!(response.contains(".section-marker-meta {"));
	assert!(response.contains("text-align: right;"));
	assert!(!response.contains("setPanelMeta(nodes.accountPoolMeta"));
	assert!(!response.contains("${pluralize(accounts.length, \"account\")} · ${activeCount} active"));
}

#[test]
fn operator_dashboard_account_errors_route_to_notice_dock_with_privacy() {
	let response = dashboard_response();

	assert!(response.contains("function codexAccountNotices(snapshot)"));
	assert!(response.contains("for (const accountNotice of codexAccountNotices(snapshot))"));
	assert!(response.contains("notices.push(accountNotice);"));
	assert!(response.contains("function codexAccountHasNotice(account)"));
	assert!(response.contains("function codexAccountNoticeCopy(account)"));
	assert!(response.contains("return `${codexAccountPrivacyLabel(account)}: ${parts.join(\"; \")}`;"));
	assert!(response.contains("codexAccountRefreshFailed(account) && !noteIncludesRefreshFailure"));
	assert!(response.contains("codexAccountRefreshStatusNeedsAttention(refreshStatus) &&"));
	assert!(response.contains("!codexAccountRefreshFailed(account)"));
	assert!(response.contains("note && !noteLooksRoutine && !noteLooksError"));
	assert!(response.contains("codexAccountPrivacyText(account, note)"));
}

#[test]
fn operator_dashboard_uses_expanded_section_titles() {
	let response = dashboard_response();

	assert!(response.contains("<h2 id=\"current-lanes-title\">Current Lanes</h2>"));
	assert!(response.contains("<h2 id=\"queue-title\">Intake Queue</h2>"));
	assert!(response.contains("<h2>Review &amp; Landing</h2>"));
	assert!(response.contains("<h2 id=\"worktrees-title\">Recovery Worktrees</h2>"));
	assert!(response.contains("<h2 id=\"recent-title\">Run History</h2>"));
}

#[test]
fn operator_dashboard_renders_account_sort_controls() {
	let response = dashboard_response();

	assert!(response.contains("const ACCOUNT_POOL_SORT_STORAGE_KEY = \"decodex.operator.accountSort\";"));
	assert!(response.contains("const ACCOUNT_POOL_SORT_COLUMNS = ["));
	assert!(response.contains("function loadAccountPoolSort()"));
	assert!(response.contains("function persistAccountPoolSort()"));
	assert!(response.contains("function isAccountPoolSortKey(value)"));
	assert!(response.contains("function renderCodexAccountPoolSortButton([key, label])"));
	assert!(response.contains("account-pool-sort"));
	assert!(response.contains("data-account-sort-key"));
	assert!(response.contains("aria-label=\"Sort accounts by ${escapeHtml(label)}; ${escapeHtml(current)}\""));
	assert!(response.contains("account-sort-up"));
	assert!(response.contains("account-sort-down"));
	assert!(response.contains("function codexAccountPoolColumnSortValue(account, key)"));
	assert!(response.contains("function compareCodexAccountPoolColumn(left, right, key, direction)"));
	assert!(!response.contains("function compareCodexAccountPoolStable(left, right)"));
	assert!(response.contains("function sortCodexAccountPoolAccounts(accounts)"));
	assert!(response.contains("if (!accountPoolSort.key)"));
	assert!(response.contains("return 0;"));
	assert!(response.contains("codexAccountWindowData(account, \"primary\").remainingPercent"));
	assert!(response.contains("codexAccountWindowData(account, \"secondary\").remainingPercent"));
	assert!(response.contains("codexAccountCreditsSortValue(account)"));
	assert!(response.contains("persistAccountPoolSort();"));
	assert!(response.contains("accountPoolSort.key === key && accountPoolSort.direction === \"asc\""));
}

#[test]
fn operator_dashboard_renders_project_sort_controls() {
	let response = dashboard_response();

	assert!(response.contains("const PROJECT_SORT_STORAGE_KEY = \"decodex.operator.projectSort\";"));
	assert!(response.contains("const PROJECT_SORT_COLUMNS = ["));
	assert!(response.contains("[\"project\", \"Project\"]"));
	assert!(response.contains("[\"location\", \"Location\"]"));
	assert!(response.contains("[\"activity\", \"Activity\"]"));
	assert!(response.contains("[\"work\", \"Work\"]"));
	assert!(response.contains("function loadProjectSort()"));
	assert!(response.contains("function persistProjectSort()"));
	assert!(response.contains("function isProjectSortKey(value)"));
	assert!(response.contains("function projectSortDefaultDirection(key)"));
	assert!(response.contains("return [\"activity\", \"work\"].includes(key) ? \"desc\" : \"asc\";"));
	assert!(response.contains("function renderProjectSortButton([key, label])"));
	assert!(response.contains("project-table-sort"));
	assert!(response.contains("data-project-sort-key"));
	assert!(response.contains("aria-label=\"Sort projects by ${escapeHtml(label)}; ${escapeHtml(current)}\""));
	assert!(response.contains("project-sort-up"));
	assert!(response.contains("project-sort-down"));
	assert!(response.contains("aria-sort=\"${direction === \"asc\" ? \"ascending\" : \"descending\"}\""));
	assert!(response.contains("function projectColumnSortValue(project, key)"));
	assert!(response.contains("function compareProjectRowsByColumn(left, right, key, direction)"));
	assert!(response.contains("function compareProjectRowsStable(left, right)"));
	assert!(response.contains("function sortProjectRows(rows)"));
	assert!(response.contains("projectSort.key === key"));
	assert!(response.contains("projectSortDefaultDirection(key)"));
	assert!(response.contains("persistProjectSort();"));
	assert!(response.contains("sortProjectRows(projectFilterRows(projects, activeProjectRows))"));
}

#[test]
fn operator_dashboard_accounts_keeps_compact_table_layout() {
	let response = dashboard_response();

	assert!(response.contains("run-meta-line"));
	assert!(response.contains("account-pool-list"));
	assert!(response.contains("account-pool-guide"));
	assert!(response.contains("<div class=\"account-pool-summary\""));
	assert!(response.contains("function codexAccountProfileAggregate(accounts)"));
	assert!(response.contains("function renderCodexAccountPoolActivityStrip(account"));
	assert!(response.contains("function renderCodexAccountProfileActivityStrip(account"));
	assert!(response.contains("function codexAccountProfilePeakDailyTokens(account)"));
	assert!(response.contains("function renderCodexAccountProfileToggle(account, expanded)"));
	assert!(response.contains("function renderCodexAccountProfilePanel(account, snapshot, profileKey, expanded)"));
	assert!(response.contains("function toggleCodexAccountProfileKey(key)"));
	assert!(response.contains("function accountProfileRowClickIsSuppressed(target)"));
	assert!(response.contains("data-account-profile-toggle"));
	assert!(response.contains("data-account-profile-row-toggle"));
	assert!(response.contains("data-render-key=\"account-row:${escapeHtml(profileKey)}\""));
	assert!(response.contains("data-render-key=\"account-profile-panel:${escapeHtml(profileKey)}\""));
	assert!(response.contains(".account-row.is-profile-toggleable"));
	assert!(response.contains("const profileRow = event.target.closest(\"[data-account-profile-row-toggle]\");"));
	assert!(response.contains("aria-hidden=\"${expanded ? \"false\" : \"true\"}\""));
	assert!(response.contains("const openClass = expanded ? \" is-open\" : \"\";"));
	assert!(response.contains("expandedAccountProfileKeys"));
	assert!(response.contains("account-pool-activity-strip"));
	assert!(response.contains("account-pool-activity-tile"));
	assert!(response.contains("label: \"Activity\""));
	assert!(response.contains("valueHtml: activityStrip"));
	assert!(response.contains(".account-pool-metric-label {\n\t\t\t\toverflow: hidden;\n\t\t\t\tcolor: var(--muted);\n\t\t\t\tfont-family: var(--sans);"));
	assert!(response.contains(".account-pool-metric-value {\n\t\t\t\toverflow: hidden;\n\t\t\t\tcolor: var(--muted-strong);\n\t\t\t\tfont-family: var(--mono);"));
	assert!(response.contains(".account-pool-metric-value[data-tone=\"muted\"] {\n\t\t\t\tcolor: var(--muted-strong);"));
	assert!(!response.contains(".account-pool-activity-strip {\n\t\t\t\tgrid-column: 1 / -1;"));
	assert!(response.contains("account-profile-activity-strip"));
	assert!(response.contains("account-profile-toggle"));
	assert!(response.contains("account-profile-panel"));
	assert!(response.contains(".account-profile-panel.is-open"));
	assert!(response.contains("grid-template-columns: repeat(5, minmax(0, 1fr));"));
	assert!(response.contains("account-profile-fact"));
	assert!(response.contains("account-profile-activity"));
	assert!(response.contains("[\"Lifetime\", facts.get(\"tok\") || \"-\"]"));
	assert!(response.contains("Lifetime tok"));
	assert!(response.contains("Peak day"));
	assert!(response.contains("Longest task"));
	assert!(!response.contains("account-profile-table"));
	assert!(!response.contains("account-profile-guide"));
	assert!(!response.contains(".account-profile-row"));
	assert!(!response.contains("account-profile-lane"));
	assert!(!response.contains("account-profile-head"));
	assert!(!response.contains("account-pool-summary is-profile"));
	assert!(!response.contains("account-pool-window-heads"));
	assert!(!response.contains("account-pool-summary-head"));
	assert!(!response.contains("account-pool-track"));
	assert!(response.contains("<div class=\"account-pool-guide\">"));
	assert!(response.contains("[\"account\", \"Account\"]"));
	assert!(response.contains("[\"plan\", \"Weight\"]"));
	assert!(response.contains("[\"primary\", \"5h\"]"));
	assert!(response.contains("[\"secondary\", \"7d\"]"));
	assert!(response.contains("[\"credits\", \"Credits\"]"));
	assert!(response.contains("[\"status\", \"Status\"]"));
	assert!(response.contains("ACCOUNT_POOL_SORT_COLUMNS.map(renderCodexAccountPoolGuideCell).join(\"\")"));
	assert!(response.contains(".account-pool-heading"));
	assert!(!response.contains("account-table-head"));
	assert!(response.contains(
		"--account-grid: minmax(220px, 1.12fr) minmax(56px, 0.42fr) repeat(4, minmax(0, 1fr));"
	));
	assert!(response.contains(
		"--account-grid: minmax(150px, 1fr) minmax(44px, 0.44fr) repeat(4, minmax(0, 1fr));"
	));
	assert!(!response.contains("--account-grid: repeat(6, minmax(0, 1fr));"));
	assert!(!response.contains("--account-grid: minmax(112px, 1fr)"));
	assert!(response.contains(".account-pool-list {\n\t\t\t\t--account-grid:"));
	assert!(response.contains(".account-pool {\n\t\t\t\tdisplay: grid;"));
	assert!(response.contains("\n\t\t\t\toverflow-x: auto;"));
	assert!(response.contains("\n\t\t\t\tdisplay: grid;\n\t\t\t\tmin-width: 760px;\n\t\t\t\tbackground: transparent;"));
	assert!(response.contains(".account-pool-guide {\n\t\t\t\tdisplay: grid;"));
	assert!(response.contains("grid-template-columns: var(--account-grid);"));
	assert!(response.contains(".account-pool-sort {\n\t\t\t\tjustify-self: center;"));
	assert!(response.contains(".account-pool-sort-icon"));
	assert!(response.contains("background: transparent;"));
	assert!(!response.contains("box-shadow: 0 8px 24px color-mix(in srgb, var(--account-accent) 7%, transparent);"));
	assert!(!response.contains("account-quota-line"));
	assert!(!response.contains("account-window-value"));
	assert!(response.contains("account-window-reset"));
	assert!(!response.contains(".account-window-reset strong"));
	assert!(!response.contains(".account-window-reset span"));
	assert!(response.contains("account-status"));
	assert!(!response.contains("account-status-pill"));
	assert!(response.contains("account-window-label"));
	assert!(response.contains(".account-window {\n\t\t\t\tdisplay: inline-grid;"));
	assert!(response.contains("grid-template-columns: max-content max-content;"));
	assert!(response.contains("justify-content: center;"));
	assert!(response.contains("justify-items: center;"));
	assert!(response.contains("width: 100%;"));
	assert!(response.contains("text-align: center;"));
	assert!(response.contains(".account-window-label {\n\t\t\t\tdisplay: none;"));
	assert!(response.contains("<span class=\"account-window-label\" aria-hidden=\"true\">${escapeHtml(label)}</span>"));
	assert!(response.contains("aria-label=\"${escapeHtml(label)} remaining ${escapeHtml(remaining)}, ${escapeHtml(reset.aria)}\""));
	assert!(response.contains("title=\"${escapeHtml(resetTitle)}\""));
	assert!(response.contains("account-window-date"));
	assert!(!response.contains("<span class=\"is-reset\">Reset</span>"));
}

#[test]
fn operator_dashboard_accounts_renders_fixed_selection_affordance() {
	let response = dashboard_response();

	assert!(response.contains("is-selected"));
	assert!(response.contains("is-ready"));
	assert!(response.contains("is-armed"));
	assert!(response.contains("--account-accent: var(--tone-muted);"));
	assert!(response.contains("--account-confirm-accent: var(--tone-run);"));
	assert!(response.contains(".account-row.is-ready {\n\t\t\t\t--account-accent: var(--success);"));
	assert!(response.contains(".account-row.is-fixed {\n\t\t\t\t--account-accent: var(--info);"));
	assert!(!response.contains(".account-row.is-armed {\n\t\t\t\t--account-accent: var(--warning);"));
	assert!(response.contains("--account-confirm-cycle: 1.45s;"));
	assert!(!response.contains("--account-confirm-color-cycle"));
	assert!(!response.contains("account-confirm-bar-breathe"));
	assert!(response.contains("@keyframes account-confirm-name-breathe"));
	assert!(response.contains("@keyframes account-confirm-bracket-left"));
	assert!(response.contains("@keyframes account-confirm-bracket-right"));
	assert!(response.contains("color: var(--account-confirm-accent);"));
	assert!(!response.contains("12.5%"));
	assert!(!response.contains("37.5%"));
	assert!(!response.contains("62.5%"));
	assert!(!response.contains("87.5%"));
	assert!(response.contains(
		"color: color-mix(in srgb, var(--account-confirm-accent) 46%, var(--muted));"
	));
	assert!(response.contains("text-shadow: none;"));
	assert!(response.contains(".account-name-button.is-fixed::before"));
	assert!(response.contains(".account-name-button.is-fixed::after"));
	assert!(response.contains(
		".account-name-button.is-fixed {\n\t\t\t\tcolor: var(--account-confirm-accent);"
	));
	assert!(response.contains(".account-name-button + .account-name-reroll"));
	assert!(response.contains("margin-left: 8px;"));
	assert!(response.contains("opacity: 0.72;"));
	assert!(response.contains("animation: account-confirm-name-breathe var(--account-confirm-cycle) var(--ease) infinite;"));
	assert!(response.contains("animation: account-confirm-bracket-left var(--account-confirm-cycle) var(--ease) infinite;"));
	assert!(response.contains("animation: account-confirm-bracket-right var(--account-confirm-cycle) var(--ease) infinite;"));
	assert!(!response.contains("infinite alternate;"));
}

#[test]
fn operator_dashboard_accounts_keeps_identity_rows_compact() {
	let response = dashboard_response();

	assert!(response.contains("grid-template-areas:"));
	assert!(response.contains("\"id plan primary secondary credit state\""));
	assert!(response.contains("\"meta meta meta meta meta meta\""));
	assert!(!response.contains("\"account status\""));
	assert!(!response.contains("\"windows windows\""));
	assert!(response.contains(".account-row-id {\n\t\t\t\tgrid-area: id;"));
	assert!(response.contains("justify-content: center;"));
	assert!(response.contains("text-align: center;"));
	assert!(response.contains("function codexAccountCapacityLabel(account)"));
	assert!(response.contains("function codexAccountCapacityMultiplier(account)"));
	assert!(response.contains("const planType = String(account?.plan_type || \"\").trim().toLowerCase();"));
	assert!(response.contains("return planType === \"pro\" ? 20 : 1;"));
	assert!(response.contains("const weight = codexAccountCapacityLabel(account);"));
	assert!(response.contains("const identityClass = codexAccountShowsEmail(account) ? \" is-machine\" : \"\";"));
	assert!(response.contains(".account-row-plan {\n\t\t\t\tgrid-area: plan;"));
	assert!(response.contains("<div class=\"account-row-id${identityClass}\">"));
	assert!(response.contains("<div class=\"account-row-plan\">${escapeHtml(weight)}</div>"));
	assert!(response.contains("<button class=\"account-name-button${fixedClass}${armedClass}\""));
	assert!(response.contains("<span class=\"account-name\">${escapeHtml(visibleName)}</span>"));
	assert!(response.contains("<span class=\"run-meta-icon\" aria-hidden=\"true\">"));
	assert!(response.contains("<svg viewBox=\"0 0 16 16\" fill=\"none\">"));
	assert!(response.contains("<path fill=\"currentColor\" fill-rule=\"evenodd\" clip-rule=\"evenodd\" d=\"M3.35 2.25h9.3"));
	assert!(response.contains("M8 4.15a1.8 1.8"));
	assert!(response.contains("c.61 0 1.1.49 1.1 1.1v9.3"));
	assert!(!response.contains("d=\"M8 1.65a6.35"));
	assert!(!response.contains("<path fill=\"currentColor\" d=\"M8 7.3a2.55"));
	assert!(!response.contains("<path fill=\"currentColor\" d=\"M3.25 13.15c.48-2.65"));
	assert!(!response.contains("fill-rule=\"evenodd\" clip-rule=\"evenodd\" d=\"M3.9 2.2h8.2"));
	assert!(!response.contains("<circle cx=\"8\" cy=\"5.1\""));
	assert!(response.contains("<strong class=\"account-name${identityClass}\" title=\"${escapeHtml(pendingTitle)}\">${escapeHtml(visibleName)}</strong>"));
	assert!(!response.contains("<strong class=\"machine-text\">${escapeHtml(`${value}%`)}</strong>"));
	assert!(!response.contains("function codexAccountSecondaryLabel(account)"));
	assert!(response.contains("const visibleName = codexAccountVisibleName(account);"));
	assert!(response.contains("const displayTitle = codexAccountDisplayTitle(account);"));
	assert!(response.contains("title=\"${escapeHtml(pendingTitle)}\">${escapeHtml(visibleName)}</strong>"));
	assert!(response.contains("text.startsWith(\"...\") && text.indexOf(\"...\", 3) === -1"));
}

#[test]
fn operator_dashboard_accounts_keeps_window_status_and_credit_copy_compact() {
	let response = dashboard_response();

	assert!(response.contains("ACCOUNT_IDENTITY_MIN_EDGE_CHARS,"));
	assert!(response.contains("Math.min(ACCOUNT_IDENTITY_EDGE_CHARS, Math.floor(text.length / 2)),"));
	assert!(response.contains("return `${text.slice(0, headLength)}...${text.slice(-tailLength)}`;"));
	assert!(response.contains("grid-area: primary;"));
	assert!(response.contains("grid-area: secondary;"));
	assert!(response.contains("justify-self: stretch;"));
	assert!(!response.contains("max-width: 190px;"));
	assert!(!response.contains("max-width: 142px;"));
	assert!(response.contains("min-height: 42px;"));
	assert!(response.contains("padding: var(--space-account-row-y) 28px var(--space-account-row-y) var(--space-row-indent);"));
	assert!(response.contains("border-bottom: 1px solid var(--line);"));
	assert!(response.contains(".account-pool-list > .account-row.is-last-account"));
	assert!(response.contains("const lastAccountClass = isLastAccount ? \" is-last-account\" : \"\";"));
	assert!(response.contains(
		"accounts.map((account, index) => renderCodexAccountPoolRow(account, snapshot, index === accounts.length - 1))"
	));
	assert!(!response.contains(".account-pool-list > .account-row:last-child"));
	assert!(response.contains("account-row-credit"));
	assert!(response.contains(".account-row-credit {\n\t\t\t\tgrid-area: credit;"));
	assert!(response.contains(".account-row-credit {\n\t\t\t\tgrid-area: credit;\n\t\t\t\tjustify-self: center;"));
	assert!(response.contains(".account-row-credit.is-danger strong"));
	assert!(!response.contains("grid-template-columns: minmax(116px, 0.58fr) minmax(190px, 1fr);"));
	assert!(!response.contains("grid-template-columns: minmax(34px, max-content) minmax(0, 1fr);"));
	assert!(response.contains(".account-window-reset {\n\t\t\t\tdisplay: inline;"));
	assert!(response.contains(".account-row::after"));
	assert!(response.contains(".account-row::before"));
	assert!(response.contains(".account-row:hover::before"));
	assert!(response.contains(".account-row:focus-within::before"));
	assert!(response.contains(".account-row:hover::after"));
	assert!(response.contains(".account-row:focus-within::after"));
	assert!(response.contains("background: linear-gradient(90deg, var(--hover), transparent 78%);"));
	assert!(response.contains("box-shadow: 0 0 18px color-mix(in srgb, var(--account-accent) 42%, transparent);"));
	assert!(response.contains(".account-row:hover .account-window"));
	assert!(response.contains(".account-row:focus-within .account-window"));
	assert!(response.contains(".account-status::before"));
	assert!(response.contains(".account-row.is-selected .account-status"));
	assert!(response.contains(".account-row.is-fixed .account-status"));
	assert!(!response.contains(".account-row.is-armed .account-status"));
	assert!(response.contains(".account-row.is-ready .account-status"));
	assert!(response.contains(".account-row.is-warn .account-status"));
	assert!(response.contains(".account-row.is-danger .account-status"));
	assert!(response.contains(".account-row:hover .account-status::before"));
	assert!(response.contains(".account-row:focus-within .account-status::before"));
	assert!(!response.contains("@keyframes account-active"));
	assert!(!response.contains("account-active-glow"));
	assert!(!response.contains("account-active-sweep"));
	assert!(!response.contains("account-active-dot"));
	assert!(response.contains("aria-label=\"Lane metadata\""));
	assert!(response.contains("<span class=\"run-meta-item is-account\" aria-label=\"account\">"));
	assert!(!response.contains("<span>account</span>"));
	assert!(response.contains("<strong>not captured</strong>"));
	assert!(!response.contains("<span class=\"account-use-label\">Account</span>"));
	assert!(!response.contains("<span class=\"account-use-label\">Codex account</span>"));
	assert!(response.contains("aria-label=\"Accounts\""));
	assert!(response.contains("ACCOUNT_PRIVACY_STORAGE_KEY"));
	assert!(response.contains("function codexAccountWindowData(account, prefix)"));
	assert!(response.contains("renderCodexAccountPoolWindow(account, \"primary\")"));
	assert!(response.contains("renderCodexAccountPoolWindow(account, \"secondary\")"));
	assert!(!response.contains("<div class=\"account-quota-line\">"));
	assert!(response.contains("<div class=\"account-window is-${escapeHtml(prefix)}${toneClass}\""));
	assert!(!response.contains("codexAccountStatusBit(account)"));
	assert!(response.contains("renderRunCodexAccountInline(run, snapshot)"));
	assert!(response.contains("function renderRunMetaLine(run, snapshot = null)"));
}

#[test]
fn operator_dashboard_accounts_keeps_debug_credit_and_reset_copy_compact() {
	let response = dashboard_response();
	let active_debug = response
		.split("<summary>Debug Details</summary>")
		.nth(1)
		.expect("active debug details should exist")
		.split("</details>")
		.next()
		.expect("active debug details should end");

	assert!(!active_debug.contains("field(\"Account\", codexAccountDebugSummary(account))"));
	assert!(!active_debug.contains("field(\"Freshness source\","));
	assert!(!active_debug.contains("field(\"Lane activity\","));
	assert!(!active_debug.contains("field(\"Last protocol activity\","));
	assert!(!response.contains(
		"field(\"Accounts\", codexAccountPoolDebugSummary(codexAccounts(run)))"
	));
	assert!(response.contains(
		"field(\"Account\", codexAccountDebugSummary(codexAccount(run, snapshot)))"
	));
	assert!(response.contains("facts.push([\"Account\", codexAccountHistorySummary(codexAccount(run))])"));
	assert!(!response.contains("facts.push([\"Codex pool\""));
	assert!(!response.contains("account <strong>"));
	assert!(response.contains("credits_unlimited"));
	assert!(response.contains("function formatCodexAccountCreditsBalance(value)"));
	assert!(response.contains("const balance = formatCodexAccountCreditsBalance(account.credits_balance);"));
	assert!(response.contains("return number.toFixed(2);"));
	assert!(!response.contains(".replace(/\\.00$/, \"\")"));
	assert!(!response.contains(".replace(/(\\.\\d)0$/, \"$1\")"));
	assert!(response.contains("function codexAccountCreditsTone(account)"));
	assert!(response.contains("function codexAccountUsageLimited(account)"));
	assert!(response.contains("if (status === \"available\")"));
	assert!(response.contains("return \"ready\";"));
	assert!(response.contains("codexAccountReachedType(account).includes(\"credit\")"));
	assert!(response.contains("const credits = codexAccountCreditsSummary(account);"));
	assert!(response.contains("const creditTone = codexAccountCreditsTone(account);"));
	assert!(response.contains("<span>credits</span>"));
	assert!(response.contains("<strong>${escapeHtml(credits || \"-\")}</strong>"));

	let account_credit_index = response
		.find("<div class=\"account-row-credit${creditClass}\">")
		.expect("account credit cell render");
	let account_status_index = response
		.find("<div class=\"account-row-state\">")
		.expect("account status cell render");

	assert!(account_credit_index < account_status_index);
	assert!(response.contains("return \"0.00\";"));
	assert!(!response.contains("return \"No Credits\";"));
	assert!(response.contains("return \"Unlimited\";"));

	let account_status_label = response
		.split("function codexAccountStatusLabel(account)")
		.nth(1)
		.expect("account status label function should exist")
		.split("function codexAccountCreditsSummary(account)")
		.next()
		.expect("account status label function should have an end");

	assert!(account_status_label.contains("return refresh;"));
	assert!(account_status_label.contains("return displayToken(status);"));
	assert!(!account_status_label.contains("Refresh failed"));
	assert!(!account_status_label.contains("Ready"));
	assert!(response.contains("return codexAccountTokenValue(account.refresh_status);"));
	assert!(response.contains("return \"-\";"));
	assert!(!response.contains("depleted"));
	assert!(response.contains("rate_limit_reached_type"));
	assert!(response.contains("if (codexAccountUsageLimited(account))"));
	assert!(account_status_label.contains("return reached || (String(status).trim() && status !== \"available\" ? status : \"usage_limited\");"));
	assert!(response.contains("cooldown_until_unix_epoch"));
	assert!(response.contains("`${prefix}_remaining_percent`"));
	assert!(response.contains("`${prefix}_resets_at_unix_epoch`"));
	assert!(response.contains("value === 18_000"));
	assert!(response.contains("value === 604_800"));
	assert!(response.contains("function formatCodexAccountResetDuration(seconds)"));
	assert!(response.contains("function codexAccountResetDistance(value)"));
	assert!(response.contains("function codexAccountResetDisplay(data)"));
	assert!(!response.contains("const shortWindow = windowSeconds === 18_000;"));
	assert!(response.contains("return { short: \"0m\", phrase: \"reset due now\", isPast: true };"));
	assert!(response.contains("return { short, phrase: `resets in ${short}`, isPast: false };"));
	assert!(response.contains("date: \"\","));
	assert!(response.contains("date: resetAt,"));
	assert!(response.contains("aria: \"reset unavailable\","));
	assert!(response.contains("reset at ${resetAt}, ${distance.phrase}"));
	assert!(response.contains("data.remainingPercent == null ? \"-\" : `${data.remainingPercent}%`;"));
	assert!(response.contains("aria-label=\"${escapeHtml(label)} usage unavailable\""));
	assert!(response.contains("const resetTitle = `${label} ${remaining}, ${reset.aria}`;"));
	assert!(response.contains("<span class=\"account-window-reset\">${escapeHtml(reset.short)}</span>"));
	assert!(response.contains("${reset.date ? `<span class=\"account-window-date\">${escapeHtml(reset.date)}</span>` : \"\"}"));
	assert!(!response.contains("<strong>${escapeHtml(reset.main)}</strong>"));
	assert!(!response.contains("<span>${escapeHtml(reset.detail)}</span>"));
	assert!(response.contains("class=\"account-status\""));
	assert!(response.contains("function codexAccountWindowTone(percent)"));
	assert!(response.contains(".account-window.is-warn > strong"));
	assert!(response.contains(".account-window.is-danger > strong"));
	assert!(!response.contains("function codexAccountLowestRemaining(account)"));
	assert!(!response.contains("lowestRemaining <= 20"));
	assert!(!response.contains("account-meter"));
	assert!(!response.contains("lowestRemaining}%"));
	assert!(response.contains(
		"renderStableList(nodes.accountPool, renderCodexAccountPool(accounts, snapshot));"
	));
	assert!(response.contains("syncAccountSelectionConfirmationDom();"));
	assert!(!response.contains("nodes.accountPool.innerHTML = renderCodexAccountPool(accounts, snapshot)"));
	assert!(response.contains("renderAccountPrivacyToggle();"));
	assert!(!response.contains("setPanelMeta(nodes.accountPoolMeta"));
	assert!(!response.contains("nodes.accountPoolMeta.textContent = snapshot"));
	assert!(!response.contains("nodes.accountPoolMeta"));
	assert!(!response.contains("account-row-windows"));
	assert!(!response.contains("account-mini-window"));
	assert!(!response.contains("account-mini-label"));
	assert!(!response.contains("grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));"));
	assert!(!response.contains("grid-template-columns: minmax(170px, 1fr) minmax(360px, 1.7fr) 118px;"));
	assert!(!response.contains("border-right: 1px solid var(--line);"));
	assert!(!response.contains("box-shadow: inset 3px 0 0 var(--success)"));
	assert!(!response.contains(">Emails</span>"));
	assert!(!response.contains("[\"checked\""));
}

#[test]
fn operator_dashboard_omits_lane_mutation_controls() {
	let response = dashboard_response();

	assert!(!response.contains("function dashboardSubscriptionMatches(subscription)"));
	assert!(!response.contains("function clearDashboardSubscription(shouldSend = true)"));
	assert!(!response.contains("function toggleDashboardSubscription(subscription)"));
	assert!(!response.contains("toggleDashboardSubscription({ projectId })"));
	assert!(!response.contains("toggleDashboardSubscription({ projectId, issueId, runId })"));
	assert!(!response.contains("data-dashboard-control=\"focusProject\""));
	assert!(!response.contains("data-dashboard-control=\"focusRun\""));
	assert!(!response.contains("data-dashboard-control=\"pauseProject\""));
	assert!(!response.contains("data-dashboard-control=\"resumeProject\""));
	assert!(!response.contains(">Watch</button>"));
	assert!(!response.contains(">Watching</button>"));
	assert!(!response.contains(">Pause</button>"));
	assert!(!response.contains(">Resume</button>"));
	assert!(!response.contains("data-dashboard-control=\"retryRun\""));
	assert!(!response.contains(">Retry now</button>"));
	assert!(!response.contains("data-dashboard-control=\"interruptRun\""));
	assert!(!response.contains("aria-label=\"Stop this active Decodex work\""));
	assert!(!response.contains("runInterruptControlEnabled"));
	assert!(!response.contains("renderRunStopControl"));
	assert!(!response.contains("const statusLineParts = [...statusBits];"));
	assert!(!response.contains("statusLineParts.splice(1, 0, stopControl);"));
	assert!(!response.contains(".status-line .run-stop-button {"));
	assert!(!response.contains("action === \"interruptRun\""));
	assert!(!response.contains("case \"interruptRun\""));
	assert!(response.contains("<div class=\"status-line\">${statusBits.join(\"\")}</div>"));
	assert!(!response.contains("<rect x=\"4.2\" y=\"3.2\" width=\"2.9\" height=\"9.6\""));
	assert!(!response.contains("class=\"row-head run-row-head\""));
	assert!(!response.contains("class=\"run-head-aside\""));
	assert!(!response.contains("class=\"run-actions\""));
	assert!(!response.contains("data-tone=\"danger\" title=\"Stop this active Decodex work.\""));
	assert!(!response.contains("run-stop-button {\n\t\t\t\tposition: absolute;"));
}

#[test]
fn operator_dashboard_projects_keep_status_summary_compact() {
	let response = dashboard_response();

	assert!(response.contains("function projectCapacitySummary(project)"));
	assert!(response.contains("function renderProjectStats(project)"));
	assert!(response.contains("function projectHasActiveWork(project)"));
	assert!(!response.contains("function projectHasVisibleWork(project)"));
	assert!(response.contains("function activeProjects(projects)"));
	assert!(response.contains("function renderProjectEntry(project, selectedId)"));
	assert!(response.contains("function renderProjectTable(projects, activeProjectRows, selectedId)"));
	assert!(response.contains("function projectFilterRows(projects, activeProjectRows)"));
	assert!(response.contains("function renderEmptyState(title, copy = \"\")"));
	assert!(response.contains("function renderRoutineEmptyList(container)"));
	assert!(response.contains("nodes.projectOverview.innerHTML = \"\";"));
	assert!(response.contains("renderRoutineEmptyList("));
	assert!(!response.contains("Appears after /state publishes a snapshot."));
	assert!(response.contains("renderQueuedCandidates("));
	assert!(response.contains("function formatDetailToken(value)"));
	assert!(response.contains("return token || \"NONE\";"));
	assert!(!response.contains("return token ? token.toUpperCase() : \"NONE\";"));
	assert!(response.contains("return priority == null ? \"NONE\" : `P${priority}`;"));
	assert!(response.contains("function queuedCandidateSummaryIsNoise(summary)"));
	assert!(response.contains("normalized.includes(\"systemerror\")"));
	assert!(response.contains("function displayToken(value)"));
	assert!(response.contains("return token || \"none\";"));
	assert!(!response.contains(".replace(/_/g, \" \")"));
	assert!(!response.contains("External sync skipped"));
	assert!(response.contains("function displayTextRepeats(left, right)"));
	assert!(response.contains("function inlineStatusFact(label, value)"));
	assert!(response.contains("titleCaseLabel(label)"));
	assert!(response.contains("const summary = summarizeQueuedCandidate(candidate);"));
	assert!(response.contains("const reason = queuedCandidateInlineReason(candidate);"));
	assert!(response.contains("bits.push(inlineStatusFact(\"History\", displayToken(outcome.ledger_status)))"));
	assert!(!response.contains("facts.push([\"History\", displayToken(outcome.ledger_status)])"));
	assert!(!response.contains("facts.push([\"Closeout\", displayToken(outcome.closeout_status)])"));
	assert!(response.contains("<div class=\"grid two card-facts\">"));
	assert!(!response.contains("queue-facts"));
	assert!(response.contains("cardField(\"State\", formatDetailToken(candidate.state))"));
	assert!(response.contains("cardField(\"Priority\", formatPriority(candidate.priority))"));
	assert!(response.contains(": \"NONE\";"));
	assert!(response.contains("cardField(\"Blockers\", blockers, blockers === \"NONE\" ? \"is-muted\" : \"\")"));
	assert!(response.contains("${summary ? `<p class=\"row-summary\">${escapeHtml(summary)}</p>` : \"\"}"));
	assert!(response.contains("${reason ? inlineStatusFact(\"Reason\", reason) : \"\"}"));
	assert!(!response.contains("<span>reason <strong>"));
	assert!(!response.contains("<span>wait <strong>"));
	assert!(!response.contains("<span>metadata <strong>"));
	assert!(!response.contains("<span>telemetry <strong>"));
	assert!(response.contains("renderActionCards("));
	assert!(response.contains("function cardFactValueClass(value, explicitClass = \"\")"));
	assert!(response.contains("String(value || \"\").trim() === \"NONE\" ? \"is-muted\" : \"\""));
	assert!(response.contains("${item.facts.map(([label, value, valueClass]) => cardField(label, value, cardFactValueClass(value, valueClass))).join(\"\")}"));
	assert!(!response.contains("${item.facts.map(([label, value]) => field(label, value)).join(\"\")}"));
	assert!(!response.contains("No running lanes"));
	assert!(!response.contains("No queued issues"));
	assert!(!response.contains("No PR lanes"));
	assert!(!response.contains(&["Ready to", " start."].concat()));
	assert!(!response.contains(&["Waiting for a", " free agent slot."].concat()));
	assert!(!response.contains(&["App-server thread", " ended with systemError."].concat()));
	assert!(!response.contains("return \"Capacity full\";"));
	assert!(response.contains("function projectKicker(project)"));
	assert!(response.contains("return \"Disabled\";"));
	assert!(!response.contains("function projectScopeKicker"));
	assert!(!response.contains("return projects.length === 1 ? \"Current\" : \"Selected\";"));
	assert!(response.contains("return \"\";"));
	assert!(!response.contains("<h2 id=\"projects-title\">Projects</h2>"));
	assert!(!response.contains("id=\"projects-meta\""));
	assert!(!response.contains("project-panel-head"));
	assert!(!response.contains("nodes.projectsMeta"));
	assert!(response.contains("role=\"group\" aria-label=\"Projects\""));
	assert!(response.contains("const activeProjectRows = activeProjects(projects);"));
	assert!(response.contains("const visibleProjectRows = projectFilterRows(projects, activeProjectRows);"));
	assert!(response.contains(": \"\";"));
	assert!(!response.contains("No active project work"));
	assert!(!response.contains("Open All when you need the full registry."));
	assert!(!response.contains("function projectOverviewSummary(projects, activeProjectRows)"));
	assert!(!response.contains("setPanelMeta(nodes.projectsMeta"));
	assert!(response.contains("class=\"project-table\" role=\"table\" aria-label=\"${escapeHtml(label)}\""));
	assert!(response.contains(".project-table-guide span {\n\t\t\t\tmin-width: 0;\n\t\t\t\ttext-align: center;"));
	assert!(response.contains(".project-table-guide .project-location-head"));
	assert!(!response.contains(".project-table-guide span:first-child"));
	assert!(response.contains("renderProjectColumnHead(PROJECT_SORT_COLUMNS[0])"));
	assert!(response.contains("renderProjectColumnHead(PROJECT_SORT_COLUMNS[1], {"));
	assert!(response.contains("after: projectLocationToggleMarkup()"));
	assert!(response.contains("renderProjectColumnHead(PROJECT_SORT_COLUMNS[2])"));
	assert!(response.contains("renderProjectColumnHead(PROJECT_SORT_COLUMNS[3], {"));
	assert!(response.contains("after: projectWorkInfoMarkup()"));
	assert!(!response.contains("<span role=\"columnheader\">Project</span>"));
	assert!(!response.contains("<span role=\"columnheader\">Activity</span>"));
	assert!(!response.contains("<span role=\"columnheader\">Status</span>"));
	assert!(!response.contains("<span role=\"columnheader\">Running</span>"));
	assert!(!response.contains("<span role=\"columnheader\">Waiting</span>"));
	assert!(!response.contains("<span role=\"columnheader\">Attention</span>"));
	assert!(
		response.contains(
			"nodes.projectOverview.classList.toggle(\"has-registered-projects\", visibleProjectRows.length > 0);",
		)
	);
	assert!(response.contains("role=\"row\""));
	assert!(response.contains("role=\"cell\""));
}

#[test]
fn operator_dashboard_projects_show_compact_activity_work_and_location() {
	let response = dashboard_response();

	assert!(!response.contains("<h2>Active</h2>"));
	assert!(!response.contains("<h2>All</h2>"));
	assert!(response.contains("return projects.filter(projectHasActiveWork);"));
	assert!(response.contains("project.queued_candidate_count ?? 0"));
	assert!(response.contains("project.post_review_lane_count ?? 0"));
	assert!(response.contains("return workCount > 0;"));
	assert!(!response.contains("syncNeedsAttention"));
	assert!(!response.contains("project.retained_worktree_count ?? 0);"));
	assert!(!response.contains("projectHasRecentActivity(project)"));
	assert!(response.contains("class=\"project-activity\""));
	assert!(response.contains("const activityCopy = lastActivity === \"none\" ? \"-\" : lastActivity;"));
	assert!(!response.contains("`activity ${lastActivity}`"));
	assert!(!response.contains("`active ${lastActivity}`"));
	assert!(response.contains("project.retained_worktree_count ?? 0"));
	assert!(response.contains("return pluralize(project.warning_count, \"warning\");"));
	assert!(response.contains("return `${pluralize(project.retained_worktree_count, \"worktree\")} retained`;"));
	assert!(response.contains("return { label: \"running\", tone: \"tone-run\""));
	assert!(response.contains("return { label: \"needs attention\", tone: \"tone-blocked\""));
	assert!(response.contains("return { label: \"waiting\", tone: \"tone-wait\""));
	assert!(response.contains("return { label: \"cleanup blocked\", tone: \"tone-wait\""));
	assert!(response.contains("return { label: \"cleanup pending\", tone: \"tone-retained\""));
	assert!(response.contains("label: \"sync backoff\""));
	assert!(response.contains("label: \"config error\""));
	assert!(response.contains("label: \"sync degraded\""));
	assert!(response.contains("label: \"sync degraded\", tone: \"tone-muted\""));
	assert!(response.contains("project.connector_state === \"config_error\""));
	assert!(response.contains("function warningDetailsFor(warning, snapshot)"));
	assert!(response.contains("function warningNotice(warning, snapshot)"));
	assert!(response.contains("title: \"Worktree hygiene unavailable\""));
	assert!(response.contains("worktree_hygiene_unavailable"));
	assert!(response.contains("copy: displayToken(warning)"));
	assert!(!response.contains("title: projectSummary"));
	assert!(response.contains("const nextAction = detail.next_action ?"));
	assert!(response.contains("return { label: \"ok\", tone: \"tone-ready\""));
	assert!(!response.contains("function projectSyncMeta(project, health)"));
	assert!(!response.contains("const connectorCopy = projectSyncMeta(project, health);"));
	assert!(!response.contains("const prefix = `${activeCount} active · ${projects.length} all`;"));
	assert!(response.contains("return \"ok\";"));
	assert!(response.contains("const kicker = projectKicker(project);"));
	assert!(response.contains("${kicker ? `<span class=\"project-kicker\">${escapeHtml(kicker)}</span>` : \"\"}"));
	assert!(!response.contains("projectScopeKicker(project"));
	assert!(!response.contains("renderProjectEntry(project, selectedId, projects)"));
	assert!(!response.contains("const connectorCopy = `connector ${connector}`;"));
	assert!(!response.contains("const connectorCopy = `sync ${connector}`;"));
	assert!(!response.contains("? pluralize(project.warning_count, \"warning\")"));
	assert!(!response.contains("explicitly registered"));
	assert!(!response.contains("Current registration"));
	assert!(!response.contains("Selected registration"));
	assert!(!response.contains("Registry snapshot pending"));
	assert!(!response.contains("Registered projects appear after the first operator state snapshot."));
	assert!(!response.contains("return \"Registered project\";"));
	assert!(!response.contains("Disabled registration"));
	assert!(response.contains("aria-label=\"Project status summary\""));
	assert!(response.contains("function projectRunningLaneCount(project)"));
	assert!(response.contains("const running = projectRunningLaneCount(project);"));
	assert!(response.contains("const waiting = project.waiting_lane_count ?? 0;"));
	assert!(response.contains("const attention = project.attention_count ?? 0;"));
	assert!(response.contains("const cleanup = (project.cleanup_blocked_count ?? 0) + (project.cleanup_pending_count ?? 0);"));
	assert!(response.contains("`${projectRunningLaneCount(project)} running`"));
	assert!(response.contains("`${project.waiting_lane_count ?? 0} waiting`"));
	assert!(response.contains("`${project.attention_count ?? 0} attention`"));
	assert!(response.contains("`${cleanup} cleanup`"));
	assert!(response.contains("run.process_alive !== false"));
	assert!(!response.contains("(run.process_alive !== false || runHasFreshExecution(run))"));
	assert!(!response.contains("run.process_alive === false &&\n\t\t\t\t\t!run.wait_reason &&\n\t\t\t\t\t!runHasFreshExecution(run)"));
	assert!(response.contains("return toneForRun(run);"));
	assert!(response.contains("return project.running_lane_count ?? project.current_lane_count ?? 0;"));
	assert!(response.contains("run: derived.currentLaneCount > 0,"));
	assert!(!response.contains("const running = project.current_lane_count ?? 0;"));
	assert!(!response.contains("`${project.current_lane_count ?? 0} running`"));
	assert!(response.contains("projectNumber(project.cleanup_blocked_count)"));
	assert!(response.contains("projectNumber(project.cleanup_pending_count)"));
	assert!(!response.contains("[project.post_review_lane_count ?? 0, \"review/land\"]"));
	assert!(!response.contains("[project.retained_worktree_count, \"recovery\"]"));
	assert!(response.contains("function compactProjectLocation(projectPath)"));
	assert!(response.contains("function projectLocationMarkup(projectPath)"));
	assert!(response.contains("projectLocationsHidden ? \"-\" : compactProjectLocation(projectPath)"));
	assert!(response.contains("projectLocationsHidden ? \"Project location hidden\" : projectPath"));
	assert!(response.contains("class=\"project-path-prefix\""));
	assert!(response.contains("class=\"project-path-tail\""));
	assert!(response.contains("class=\"project-work-ratio\""));
	assert!(response.contains("function projectWorkInfoMarkup()"));
	assert!(response.contains("data-project-work-info"));
	assert!(response.contains("Work format: running / waiting / attention / cleanup"));
	assert!(response.contains("class=\"project-work-tooltip\" role=\"tooltip\""));
}

#[test]
fn operator_dashboard_normalizes_review_state_tokens() {
	let response = dashboard_response();

	assert!(response.contains("function compactStateToken(value)"));
	assert!(response.contains("return formatDetailToken(value);"));
	assert!(response.contains("function reviewThreadToken(count)"));
	assert!(
		response.contains(
			"return Number.isFinite(numericCount) && numericCount > 0 ? String(numericCount) : \"NONE\";",
		)
	);
	assert!(response.contains("function optionalCardToken(value)"));
	assert!(response.contains("return token || \"NONE\";"));
	assert!(response.contains("if (/^[A-Z0-9]+$/.test(word) && /[A-Z]/.test(word))"));
	assert!(response.contains(
		"status: lane.mergeable ? `merge ${compactStateToken(lane.mergeable)}` : \"ready\","
	));
	assert!(response.contains(
		"status: lane.check_state ? `checks ${compactStateToken(lane.check_state)}` : \"waiting\","
	));
	assert!(response.contains("`review ${compactStateToken(lane.review_decision)}`"));
	assert!(response.contains("[\"Checks\", compactStateToken(lane.check_state)]"));
	assert!(response.contains("[\"Threads\", reviewThreadToken(lane.unresolved_review_threads)]"));
	assert!(response.contains("[\"Review decision\", compactStateToken(lane.review_decision)]"));
	assert!(response.contains("[\"PR\", optionalCardToken(lane.pr_url)]"));
	assert!(!response.contains("`merge ${displayToken(lane.mergeable)}`"));
	assert!(!response.contains("`checks ${displayToken(lane.check_state)}`"));
	assert!(!response.contains("[\"Checks\", lane.check_state || \"none\"]"));
	assert!(!response.contains("lane.unresolved_review_threads == null ? \"none\""));
	assert!(!response.contains("lane.pr_url || \"none\""));
}

#[test]
fn operator_dashboard_review_cards_omit_static_summary_copy() {
	let response = dashboard_response();

	assert!(response.contains("const shadowedByCurrentLane ="));
	assert!(response.contains("`run phase ${displayToken(currentLane.run_phase || currentLane.phase)}`"));
	assert!(response.contains("function postReviewBlockerStatus(lane, blockerScope)"));
	assert!(response.contains("status: postReviewBlockerStatus(lane, blockerScope)"));
	assert!(response.contains("summary: \"\",\n\t\t\t\t\t\t\tstatus: lane.check_state"));
	assert!(response.contains("summary: \"\",\n\t\t\t\t\t\t\tstatus: lane.mergeable"));
	assert!(!response.contains("status: lane.review_decision && blockerScope === \"Review\""));
	assert!(
		response.contains("${item.summary ? `<p class=\"row-summary\">${escapeHtml(item.summary)}</p>` : \"\"}")
	);
	assert!(!response.contains(&["Repair lane", " already active."].concat()));
	assert!(!response.contains(&["Needs attention before", " retained lane can continue."].concat()));
	assert!(!response.contains(&["Waiting on review", " or checks."].concat()));
	assert!(!response.contains(&["Approvals and required", " checks complete."].concat()));
}

#[test]
fn operator_dashboard_projects_filter_uses_icon_toggle() {
	let response = dashboard_response();

	assert!(response.contains("const PROJECT_FILTER_STORAGE_KEY = \"decodex.operator.projectFilter\";"));
	assert!(response.contains("projectFilterToggle: document.getElementById(\"project-filter-toggle\")"));
	assert!(response.contains("let projectFilterMode = loadProjectFilterMode();"));
	assert!(response.contains("function loadProjectFilterMode()"));
	assert!(response.contains("function persistProjectFilterMode()"));
	assert!(response.contains("function renderProjectFilterToggle(projects = [])"));
	assert!(response.contains("class=\"project-filter-toggle\" id=\"project-filter-toggle\""));
	assert!(response.contains("role=\"switch\" aria-checked=\"false\" aria-label=\"Show all projects\""));
	assert!(response.contains("M3 4h10l-4 4.6v3.1l-2 1V8.6L3 4Z"));
	assert!(response.contains("projectFilterMode = projectFilterMode === \"all\" ? \"active\" : \"all\";"));
	assert!(response.contains("persistProjectFilterMode();"));
	assert!(response.contains("renderProjectFilterToggle(projects);"));
	assert!(response.contains(
		"const PROJECT_LOCATION_PRIVACY_STORAGE_KEY = \"decodex.operator.projectLocationPrivacy\";",
	));
	assert!(response.contains("let projectLocationsHidden = loadProjectLocationPrivacy();"));
	assert!(response.contains("function loadProjectLocationPrivacy()"));
	assert!(response.contains("function persistProjectLocationPrivacy(hidden)"));
	assert!(response.contains("function renderProjectLocationToggle()"));
	assert!(response.contains("data-project-location-toggle"));
	assert!(response.contains("projectLocationsHidden = !projectLocationsHidden;"));
	assert!(response.contains("persistProjectLocationPrivacy(projectLocationsHidden);"));
	assert!(response.contains("let projectWorkInfoOpen = false;"));
	assert!(response.contains("function renderProjectWorkInfoState()"));
	assert!(response.contains("data-project-work-info"));
	assert!(response.contains("projectWorkInfoOpen = !projectWorkInfoOpen;"));
	assert!(response.contains("button.setAttribute(\"aria-expanded\", projectWorkInfoOpen ? \"true\" : \"false\");"));
}

#[test]
fn operator_dashboard_empty_lane_meta_uses_counts() {
	let response = dashboard_response();

	assert!(!response.contains("Snapshot pending"));
	assert!(!response.contains("COPY.waitingSnapshot"));
	assert!(response.contains("runningLaneMetaText(derived),"));
	assert!(response.contains(": \"0 issues · 0 attempts\","));
	assert!(response.contains(": \"0 PRs · 0 need attention · 0 ready · 0 waiting · 0 cleanup\","));
	assert!(response.contains("const parts = [`${derived.liveRuns ?? 0} running`];"));
	assert!(response.contains("const parts = [`${derived.queueBacklogCandidates.length} queued`];"));
	assert!(response.contains("return \"0 queued\";"));
	assert!(response.contains("setPanelMeta(nodes.queuedMeta, backlogMetaText(snapshot, derived));"));
	assert!(response.contains(": \"0 worktrees\","));
	assert!(!response.contains("queue empty"));
	assert!(!response.contains("No running lanes"));
	assert!(!response.contains("No queued issues"));
	assert!(!response.contains("No PR lanes"));
	assert!(!response.contains("No recovery worktrees"));
}

#[test]
fn operator_dashboard_flow_counts_distinguish_intake_attention() {
	let response = dashboard_response();

	assert!(response.contains("queuedCandidateNeedsAttention"));
	assert!(response.contains("intakeAttentionCount"));
	assert!(response.contains("queuedBlockedWithoutAttention"));
	assert!(response.contains("attention.thread_status && attention.thread_status !== \"systemError\""));
	assert!(response.contains("queueBacklogCandidates.filter(queuedCandidateNeedsAttention).length"));
	assert!(response.contains(
		"${pluralize(derived.postReviewLanes.length, \"PR\")} · ${pluralize(derived.reviewBlockerCount, \"needs attention\", \"need attention\")} · ${derived.readyItems.length} ready · ${derived.reviewWaitingCount} waiting · ${derived.cleanupCount} cleanup"
	));
	assert!(response.contains("const cleanupIssueKeys = new Set();"));
	assert!(response.contains("const cleanupCount = cleanupIssueKeys.size;"));
	assert!(response.contains("? pluralize(retainedWorktrees.length, \"worktree\")"));
	assert!(!response.contains("retained or cleanup"));
	assert!(response.contains("function recoveryWorktreeShouldDefaultOpen(renderedWorktree)"));
	assert!(response.contains("role.tone === \"tone-blocked\""));
	assert!(!response.contains("role.label.includes(\"cleanup\")"));
	assert!(response.contains("label: isDirty ? \"post-review cleanup blocked\" : \"post-review cleanup\""));
	assert!(response.contains("retainedWorktrees.some(recoveryWorktreeShouldDefaultOpen)"));
	assert!(!response.contains("syncDefaultDetailOpenState(nodes.panels.worktrees, retainedWorktrees.length > 0);"));
	assert!(!response.contains("claimed without local lane"));
	assert!(!response.contains("const repairCount = attentionItems.length;"));
}

#[test]
fn operator_dashboard_does_not_hide_claimed_queue_without_local_lane() {
	let response = dashboard_response();

	assert!(response.contains("const currentLaneByIssue = new Map();"));
	assert!(response.contains("for (const key of issueIdentityKeys(run))"));
	assert!(response.contains("const currentLane = issueIdentityKeys(candidate)"));
	assert!(response.contains("if (currentLane) {"));
	assert!(!response.contains("currentLane && candidate.classification === \"claimed\""));
	assert!(!response.contains("candidate.classification !== \"claimed\" &&"));
}

#[test]
fn operator_dashboard_prioritizes_needs_attention_reason_over_retry_count() {
	let response = dashboard_response();
	let reason_text = response
		.split("function queuedCandidateReasonText(candidate)")
		.nth(1)
		.expect("queued candidate reason function should exist")
		.split("function queuedCandidateNeedsAttention(candidate)")
		.next()
		.expect("queued candidate reason function should have an end");

	assert!(reason_text.contains("return displayToken(candidate.reason);"));
	assert!(
		response.contains("facts.push([\"Attempt status\", displayToken(attention.attempt_status)]);")
	);
	assert!(response.contains(
		"facts.push([\"Failed attempts\", `${attention.retry_budget_attempt_count}${retryMax}`]);"
	));
	assert!(response.contains(
		"facts.push([\"Auto retry\", autoRetryBlockedReasonText(attention.auto_retry_blocked_reason)]);"
	));
	assert!(response.contains("return displayToken(reason);"));
	assert!(reason_text.contains("return \"retry_budget_attempt_count\";"));
	assert!(response.contains("function queuedCandidateInlineReason(candidate)"));
	assert!(response.contains("displayTextRepeats(reason, displayToken(candidate.attention.attention_error_class))"));
	assert!(response.contains("displayTextRepeats(reason, \"worktree_has_tracked_changes\")"));
	assert!(!response.contains("return \"blocked by needs-attention\";"));
	assert!(!reason_text.contains("return \"Retry budget held\";"));
	assert!(!response.contains(
		"facts.push([\"Retry\", String(attention.retry_budget_attempt_count)]);"
	));
	assert!(
		reason_text
			.find("if (candidate.attention?.attention_error_class)")
			.expect("attention error-class reason should exist")
			< reason_text
				.find("return \"retry_budget_attempt_count\";")
				.expect("retry-budget reason should exist")
	);
}

#[test]
fn operator_dashboard_header_shows_endpoint_and_snapshot_freshness() {
	let response = dashboard_response();

	assert!(response.contains("const DASHBOARD_WEBSOCKET_ENDPOINT = \"/dashboard/control\";"));
	assert!(!response.contains("SNAPSHOT_PUBLISHED_HEADER"));
	assert!(!response.contains("x-decodex-snapshot-unix-epoch"));
	assert!(!response.contains("function dashboardEndpointMeta(path)"));
	assert!(response.contains("function dashboardSocketUrl()"));
	assert!(!response.contains("function snapshotPublishedAtFromResponse(response)"));
	assert!(response.contains("function snapshotAgeSeconds(snapshotPublishedAt)"));
	assert!(response.contains("function snapshotFreshnessMeta("));
	assert!(response.contains("function topbarReadinessLabel(label)"));
	assert!(!response.contains("function topbarStreamLabel(label)"));
	assert!(response.contains("window.location.protocol === \"https:\" ? \"wss:\" : \"ws:\""));
	assert!(response.contains("<span>Transport</span>"));
	assert!(response.contains("topbarReadinessLabel(readiness.label)"));
	assert!(response.contains("<span class=\"transport-meta\" data-kind=\"endpoint\" data-tone=\"${escapeHtml(stream.tone)}\""));
	assert!(response.contains("<span>Transport</span><strong>${renderValueLink(\"WebSocket\", dashboardSocketUrl(), \"transport-link\") || escapeHtml(dashboardSocketUrl())}</strong>"));
	assert!(!response.contains("topbarStreamLabel(stream.label)"));
	assert!(!response.contains("Poll fallback"));
	assert!(response.contains("<span class=\"transport-meta\" data-kind=\"snapshot\" data-tone=\"${escapeHtml(snapshotFreshness.tone)}\""));
	assert!(response.contains("<span>Snapshot</span>"));
	assert!(response.contains("case \"Snapshot ready\":"));
	assert!(response.contains("return \"Ready\";"));
	assert!(!response.contains("return \"Connected\";"));
	assert!(response.contains("label: \"Unavailable\""));
	assert!(response.contains("label: \"Pending\""));
	assert!(response.contains("WebSocket connected."));
	assert!(response.contains("const snapshotFreshnessRow = snapshotFreshness"));
	assert!(response.contains("return null;"));
	assert!(response.contains("const staleByAge = ageSeconds != null && ageSeconds >= 30;"));
	assert!(!response.contains("const staleByReadiness"));
	assert!(response.contains("data-tone=\"${escapeHtml(snapshotFreshness.tone)}\""));
	assert!(response.contains("Published ${formatTimestamp(snapshotPublishedAt)}"));
	assert!(response.contains("formatRelativeTimestamp(snapshotPublishedAt)"));
	assert!(response.contains("unixEpochSecondsToIso(payload.snapshotPublishedAtUnixEpoch)"));
	assert!(!response.contains("stateResult.value.snapshotPublishedAt"));
	assert!(!response.contains("readinessResponse"));
	assert!(!response.contains("body: \"ready\""));
	assert!(!response.contains("polling active"));
	assert!(
		response.contains("renderHeader(snapshot, readiness, notices, snapshotPublishedAt, snapshotError)")
	);
	assert!(response.contains(".transport-meta"));
	assert!(response.contains("max-width: min(42vw, 320px);"));
	assert!(!response.contains("Auto-refresh"));
	assert!(!response.contains("Diagnostics"));
}

#[test]
fn operator_dashboard_active_freshness_prefers_live_activity_source() {
	let response = dashboard_response();

	assert!(response.contains("function currentLaneFreshness(run)"));
	assert!(response.contains("source: \"last_run_activity_at\""));
	assert!(response.contains("source: \"none\""));
	assert!(!response.contains("source: \"updated_at\""));
	assert!(response.contains("function formatRelativeTimestamp(value)"));
	assert!(response.contains("return \"0s\";"));
	assert!(response.contains("return `${seconds}s`;"));
	assert!(response.contains("return `${minutes}m`;"));
	assert!(response.contains("return `${hours}h`;"));
	assert!(response.contains("return `${days}d`;"));
	assert!(response.contains("sourceLabel: \"live activity\""));
	assert!(response.contains("sourceLabel: \"protocol activity\""));
	assert!(response.contains("facts.push([\"lane idle\", formatDuration(run.idle_for_seconds)]);"));
	assert!(response.contains("facts.push([\"agent idle\", formatDuration(run.protocol_idle_for_seconds)]);"));
	assert!(response.contains("facts.push([\"focus\", detailLabel(focus)]);"));
	assert!(response.contains("function currentLaneLifecycleMetrics(run, summary = childAgentActivity(run))"));
	assert!(response.contains("function lifecycleMetricFacts(metrics, { includeAttempts = false } = {})"));
	assert!(response.contains("facts.push([\"run phase\", displayToken(run.run_phase || run.phase || run.status)]);"));
	assert!(!response.contains("facts.push([\"current operation\", displayToken(run.current_operation)]);"));
	assert!(!response.contains("facts.push([\"active goal phase\", displayToken(run.active_goal_phase)]);"));
	assert!(!response.contains("facts.push([\"public progress phase\", displayToken(run.public_progress_phase)]);"));
	assert!(response.contains("function lifecycleRecoveryDebugSummary(metrics)"));
	assert!(response.contains("function lifecycleEvidenceDebugSummary(metrics)"));
	assert!(response.contains("${field(\"Lifecycle recovery\", lifecycleRecoveryDebugSummary(currentLaneLifecycleMetrics(run)))}"));
	assert!(response.contains("${field(\"Lifecycle evidence\", lifecycleEvidenceDebugSummary(currentLaneLifecycleMetrics(run)))}"));
	assert!(response.contains("${field(\"Run phase\", capturedValue(run.run_phase || run.phase))}"));
	assert!(response.contains("${field(\"Current operation\", capturedValue(run.current_operation))}"));
	assert!(response.contains("${field(\"Active goal phase\", capturedValue(run.active_goal_phase))}"));
	assert!(response.contains("${field(\"Public progress phase\", capturedValue(run.public_progress_phase))}"));
	assert!(response.contains("facts.push([\"tokens\", tokenSummary]);"));
	assert!(response.contains("facts.push([\"tools\", formatCompactCount(metrics.tool_call_count)]);"));
	assert!(response.contains("\"max output\","));
	assert!(response.contains("function childAgentContextRows(run, summary, lifecycle = currentLaneLifecycleMetrics(run, summary))"));
	assert!(response.contains("renderChildLifecycleOverview(lifecycle, contextFacts)"));
	assert!(response.contains("renderChildLifecyclePhaseTable(lifecycle.phases || [])"));
	assert!(!response.contains("rows.push(renderChildContextRow(\"Total\", totalFacts, \"is-total\"));"));
	assert!(response.contains("<div class=\"child-context-group\" aria-label=\"Context lifecycle metrics\">"));
	assert!(response.contains(".child-phase-table {\n\t\t\t\tdisplay: inline-grid;\n\t\t\t\tgrid-template-columns:\n\t\t\t\t\tmax-content"));
	assert!(!response.contains("function childAgentUsageFacts(summary)"));
	assert!(!response.contains("<span class=\"child-context-label\">Usage</span>"));
	assert!(response.contains("renderRunMetaFact(label, value)"));
	assert!(!response.contains("sourceLabel: \"Live Activity\""));
	assert!(!response.contains("facts.push([\"Lane Idle\", formatDuration(run.idle_for_seconds)]);"));
	assert!(!response.contains("facts.push([\"Agent Idle\", formatDuration(run.protocol_idle_for_seconds)]);"));
	assert!(!response.contains("${inlineStatusFact(label, value)}"));
	assert!(!response.contains("just now"));
	assert!(!response.contains("s ago"));
	assert!(!response.contains("m ago"));
	assert!(!response.contains("h ago"));
	assert!(!response.contains("d ago"));
	assert!(response.contains("function currentLaneTelemetryFacts(run)"));
	assert!(response.contains("function renderRunTelemetryMetaItems(run)"));
	assert!(response.contains("function renderRunMetaFact(label, value, valueClass = \"\", title = \"\")"));
	assert!(!response.contains("renderCurrentLaneActivityStrip(run)"));
	assert!(!response.contains("run-activity-strip"));
	assert!(!response.contains("function renderActiveTelemetryLine(run)"));
	assert!(!response.contains("activity-line"));
	assert!(
		response
			.contains("freshness.timestamp ? formatter(freshness.timestamp) : \"not captured\"")
	);
	assert!(!response.contains("Last ${freshness.sourceLabel}"));
	assert!(!response.contains("Latest ${freshness.sourceLabel}"));
	assert!(!response.contains("renderTimingStrip(run)"));
	assert!(!response.contains("currentLaneFreshnessSource(run)"));
	assert!(!response.contains("field(\"Freshness source\", currentLaneFreshnessSource(run))"));
	assert!(response.contains("field(\"Updated\", formatTimestamp(run.updated_at))"));
}

#[test]
fn operator_dashboard_uses_shared_protocol_activity_summary() {
	let response = dashboard_response();

	assert!(response.contains("function protocolActivity(run)"));
	assert!(response.contains("function protocolActivityFocus(run)"));
	assert!(response.contains("function protocolActivityRecentSummary(run)"));
	assert!(response.contains("function protocolActivityDebugSummary(run)"));
	assert!(!response.contains("function normalizedProtocolRateLimitStatus(value)"));
	assert!(!response.contains("status.includes(\"/\") || status.includes(\" \")"));
	assert!(!response.contains("protocolActivityRateLimitDisplay(run, \"\")"));
	assert!(!response.contains("parts.splice(2, 0, `rate limit ${rateLimit}`);"));
	assert!(!response.contains("`rate ${protocolActivityRateLimitDisplay(run)}`"));
	assert!(response.contains("facts.push([\"focus\", detailLabel(focus)]);"));
	assert!(response.contains("return \"approval/user input\";"));
	assert!(response.contains("return \"protocol idleness\";"));
	assert!(response.contains("field(\"Protocol activity\", protocolActivityDebugSummary(run))"));
	assert!(!response.contains("field(\"Rate limit\", protocolActivityRateLimitDisplay(run))"));
}

#[test]
fn operator_dashboard_run_activity_consumes_server_presentation() {
	let response = dashboard_response();

	assert!(response.contains("function presentationCurrentLaneCards(presentation)"));
	assert!(response.contains("function snapshotCurrentLaneCards(snapshot)"));
	assert!(response.contains("return presentationCurrentLaneCards(snapshot?.presentation);"));
	assert!(response.contains("function currentLaneRunsFromCards(cards)"));
	assert!(response.contains("function currentLaneCardToneClass(card)"));
	assert!(response.contains("if (card?.is_waiting === true || card?.tone === \"waiting\")"));
	assert!(response.contains("if (card?.counts_as_running === true || runCountsAsRunning(run))"));
	assert!(response.contains("function dashboardRunActivityIsStale(payload)"));
	assert!(response.contains("emittedAt < snapshotPublishedAt"));
	assert!(response.contains("if (dashboardRunActivityIsStale(payload))"));
	assert!(response.contains("let dashboardLivePresentation = null;"));
	assert!(response.contains("let dashboardLiveRunActivitySeen = false;"));
	assert!(!response.contains("let dashboardLiveAccounts = null;"));
	assert!(response.contains("let dashboardLiveAccountControl = null;"));
	assert!(response
		.contains("function dashboardLiveRunActivityHasOverlay({ includeCompletedEmpty = false } = {})"));
	assert!(response.contains("function clearDashboardLiveRunActivityOverlayIfCompleteEmpty()"));
	assert!(response.contains("return includeCompletedEmpty;"));
	assert!(response.contains("function clearDashboardLiveRunActivityOverlay()"));
	assert!(response.contains("function snapshotWithLiveRunActivity(snapshot, options = {})"));
	assert!(response.contains("if (!dashboardLiveRunActivityHasOverlay(options))"));
	assert!(!response.contains("field(\"Author\","));
	assert!(!response.contains("\"author\",\n"));
	assert!(response.contains("payload.accountControl"));
	assert!(!response.contains("activityPayload.accounts"));
	assert!(!response.contains("payload.accounts"));
	assert!(!response.contains("dashboardLiveAccounts"));
	assert!(response.contains("dashboardLiveAccountControl ="));
	assert!(response.contains("dashboardLivePresentation = {"));
	assert!(response.contains("current_lane_cards: presentationCurrentLaneCards(payload.presentation),"));
	assert!(response.contains("dashboardLiveRunActivitySeen = true;"));
	assert!(response.contains("clearDashboardLiveRunActivityOverlay();"));
	assert!(response.contains("snapshot: payload.snapshot,"));
	assert!(response.contains(
		"snapshot: snapshotWithLiveRunActivity(lastDashboardRender.snapshot, {\n\t\t\t\t\t\tincludeCompletedEmpty: true,\n\t\t\t\t\t}),"
	));
	assert!(response.contains("clearDashboardLiveRunActivityOverlayIfCompleteEmpty();"));
	assert!(response.contains("account_control:"));
	assert!(!response.contains("accounts: dashboardLiveAccounts"));
	assert!(response.contains("current_lanes: liveRuns,"));
	assert!(response.contains("presentation: dashboardLivePresentation,"));
	assert!(!response.contains("snapshot?.current_lanes ?? []"));
	assert!(!response.contains("issueDisplayKey(run),\n\t\t\t\t\t\t\trun_id: run.run_id"));
	assert!(!response.contains("function currentLaneSummary"));
	assert!(!response.contains("function runIssueTitle"));
	assert!(!response.contains("card.title || runIssueTitle"));
	assert!(!response.contains("currentLaneSummary(run) || card.detail"));
	assert!(!response.contains("function currentLaneCardToneClass(card, run)"));
	assert!(!response.contains("function mergeDashboardRunRecord"));
	assert!(!response.contains("function mergeDashboardCurrentLanes"));
	assert!(!response.contains("function mergeDashboardRunActivity"));
}

#[test]
fn operator_dashboard_uses_websocket_without_http_state_fallback() {
	let response = dashboard_response();

	assert!(response.contains("connectDashboardSocket();"));
	assert!(response.contains("function startDashboardStream()"));
	assert!(response.contains("startDashboardStream();"));
	assert!(response.contains("document.addEventListener(\"visibilitychange\", () => {"));
	assert!(response.contains("if (document.hidden) {\n\t\t\t\t\treturn;\n\t\t\t\t}"));
	assert!(response.contains("if (!dashboardSocketIsOpen()) {\n\t\t\t\t\tconnectDashboardSocket();"));
	assert!(response.contains("function renderDashboardLocalClockTick()"));
	assert!(response.contains("const ACCOUNT_API_REFRESH_INTERVAL_MS = 15_000;"));
	assert!(response.contains("now - accountApiRefreshedAt < ACCOUNT_API_REFRESH_INTERVAL_MS"));
	assert!(response.contains("const response = await fetch(\"/api/accounts?refresh=1\""));
	assert!(response.contains("refreshAccountApiSnapshot();"));
	assert!(response.contains("renderDashboardState(lastDashboardRender, { refreshAccounts: false });"));
	assert!(response.contains("const shouldRefreshAccounts = options.refreshAccounts !== false;"));
	assert!(!response.contains("function scheduleDashboardHttpFallback"));
	assert!(!response.contains("clearDashboardHttpFallback();"));
	assert!(!response.contains("requestJson("));
	assert!(!response.contains("requestText("));
	assert!(!response.contains("\"/state\""));
	assert!(!response.contains("\"/readyz\""));
	assert!(!response.contains("window.setInterval(refreshDashboard"));
	assert!(!response.contains("function refreshDashboard"));
	assert!(!response.contains("const REFRESH_INTERVAL_MS"));
}
