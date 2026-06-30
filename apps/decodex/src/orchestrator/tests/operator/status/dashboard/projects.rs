use super::*;

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
	assert!(
		response.contains("function renderProjectTable(projects, activeProjectRows, selectedId)")
	);
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
	assert!(
		response.contains(
			"bits.push(inlineStatusFact(\"History\", displayToken(outcome.ledger_status)))"
		)
	);
	assert!(!response.contains("facts.push([\"History\", displayToken(outcome.ledger_status)])"));
	assert!(
		!response.contains("facts.push([\"Closeout\", displayToken(outcome.closeout_status)])")
	);
	assert!(response.contains("<div class=\"grid two card-facts\">"));
	assert!(!response.contains("queue-facts"));
	assert!(response.contains("cardField(\"State\", formatDetailToken(candidate.state))"));
	assert!(response.contains("cardField(\"Priority\", formatPriority(candidate.priority))"));
	assert!(response.contains(": \"NONE\";"));
	assert!(response.contains(
		"cardField(\"Blockers\", blockers, blockers === \"NONE\" ? \"is-muted\" : \"\")"
	));
	assert!(
		response
			.contains("${summary ? `<p class=\"row-summary\">${escapeHtml(summary)}</p>` : \"\"}")
	);
	assert!(response.contains("${reason ? inlineStatusFact(\"Reason\", reason) : \"\"}"));
	assert!(!response.contains("<span>reason <strong>"));
	assert!(!response.contains("<span>wait <strong>"));
	assert!(!response.contains("<span>metadata <strong>"));
	assert!(!response.contains("<span>telemetry <strong>"));
	assert!(response.contains("renderActionCards("));
	assert!(response.contains("function cardFactValueClass(value, explicitClass = \"\")"));
	assert!(response.contains("String(value || \"\").trim() === \"NONE\" ? \"is-muted\" : \"\""));
	assert!(response.contains("${item.facts.map(([label, value, valueClass]) => cardField(label, value, cardFactValueClass(value, valueClass))).join(\"\")}"));
	assert!(
		!response.contains("${item.facts.map(([label, value]) => field(label, value)).join(\"\")}")
	);
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
	assert!(
		response
			.contains("const visibleProjectRows = projectFilterRows(projects, activeProjectRows);")
	);
	assert!(response.contains(": \"\";"));
	assert!(!response.contains("No active project work"));
	assert!(!response.contains("Open All when you need the full registry."));
	assert!(!response.contains("function projectOverviewSummary(projects, activeProjectRows)"));
	assert!(!response.contains("setPanelMeta(nodes.projectsMeta"));
	assert!(
		response
			.contains("class=\"project-table\" role=\"table\" aria-label=\"${escapeHtml(label)}\"")
	);
	assert!(response.contains(
		".project-table-guide span {\n\t\t\t\tmin-width: 0;\n\t\t\t\ttext-align: center;"
	));
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
	assert!(
		response.contains("const activityCopy = lastActivity === \"none\" ? \"-\" : lastActivity;")
	);
	assert!(!response.contains("`activity ${lastActivity}`"));
	assert!(!response.contains("`active ${lastActivity}`"));
	assert!(response.contains("project.retained_worktree_count ?? 0"));
	assert!(response.contains("return pluralize(project.warning_count, \"warning\");"));
	assert!(response.contains(
		"return `${pluralize(project.retained_worktree_count, \"worktree\")} retained`;"
	));
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
	assert!(response.contains(
		"${kicker ? `<span class=\"project-kicker\">${escapeHtml(kicker)}</span>` : \"\"}"
	));
	assert!(!response.contains("projectScopeKicker(project"));
	assert!(!response.contains("renderProjectEntry(project, selectedId, projects)"));
	assert!(!response.contains("const connectorCopy = `connector ${connector}`;"));
	assert!(!response.contains("const connectorCopy = `sync ${connector}`;"));
	assert!(!response.contains("? pluralize(project.warning_count, \"warning\")"));
	assert!(!response.contains("explicitly registered"));
	assert!(!response.contains("Current registration"));
	assert!(!response.contains("Selected registration"));
	assert!(!response.contains("Registry snapshot pending"));
	assert!(
		!response.contains("Registered projects appear after the first operator state snapshot.")
	);
	assert!(!response.contains("return \"Registered project\";"));
	assert!(!response.contains("Disabled registration"));
	assert!(response.contains("aria-label=\"Project status summary\""));
	assert!(response.contains("function projectRunningLaneCount(project)"));
	assert!(response.contains("const running = projectRunningLaneCount(project);"));
	assert!(response.contains("const waiting = project.waiting_lane_count ?? 0;"));
	assert!(response.contains("const attention = project.attention_count ?? 0;"));
	assert!(response.contains(
		"const cleanup = (project.cleanup_blocked_count ?? 0) + (project.cleanup_pending_count ?? 0);"
	));
	assert!(response.contains("`${projectRunningLaneCount(project)} running`"));
	assert!(response.contains("`${project.waiting_lane_count ?? 0} waiting`"));
	assert!(response.contains("`${project.attention_count ?? 0} attention`"));
	assert!(response.contains("`${cleanup} cleanup`"));
	assert!(response.contains("run.process_alive !== false"));
	assert!(!response.contains("(run.process_alive !== false || runHasFreshExecution(run))"));
	assert!(!response.contains("run.process_alive === false &&\n\t\t\t\t\t!run.wait_reason &&\n\t\t\t\t\t!runHasFreshExecution(run)"));
	assert!(response.contains("return toneForRun(run);"));
	assert!(
		response.contains("return project.running_lane_count ?? project.current_lane_count ?? 0;")
	);
	assert!(response.contains("run: derived.currentLaneCount > 0,"));
	assert!(!response.contains("const running = project.current_lane_count ?? 0;"));
	assert!(!response.contains("`${project.current_lane_count ?? 0} running`"));
	assert!(response.contains("projectNumber(project.cleanup_blocked_count)"));
	assert!(response.contains("projectNumber(project.cleanup_pending_count)"));
	assert!(!response.contains("[project.post_review_lane_count ?? 0, \"review/land\"]"));
	assert!(!response.contains("[project.retained_worktree_count, \"recovery\"]"));
	assert!(response.contains("function compactProjectLocation(projectPath)"));
	assert!(response.contains("function projectLocationMarkup(projectPath)"));
	assert!(
		response.contains("projectLocationsHidden ? \"-\" : compactProjectLocation(projectPath)")
	);
	assert!(
		response.contains("projectLocationsHidden ? \"Project location hidden\" : projectPath")
	);
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
	assert!(response.contains(
		"return Number.isFinite(numericCount) && numericCount > 0 ? String(numericCount) : \"NONE\";",
	));
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
	assert!(
		response
			.contains("`run phase ${displayToken(currentLane.run_phase || currentLane.phase)}`")
	);
	assert!(response.contains("function postReviewBlockerStatus(lane, blockerScope)"));
	assert!(response.contains("status: postReviewBlockerStatus(lane, blockerScope)"));
	assert!(response.contains("summary: \"\",\n\t\t\t\t\t\t\tstatus: lane.check_state"));
	assert!(response.contains("summary: \"\",\n\t\t\t\t\t\t\tstatus: lane.mergeable"));
	assert!(!response.contains("status: lane.review_decision && blockerScope === \"Review\""));
	assert!(response.contains(
		"${item.summary ? `<p class=\"row-summary\">${escapeHtml(item.summary)}</p>` : \"\"}"
	));
	assert!(!response.contains(&["Repair lane", " already active."].concat()));
	assert!(
		!response.contains(&["Needs attention before", " retained lane can continue."].concat())
	);
	assert!(!response.contains(&["Waiting on review", " or checks."].concat()));
	assert!(!response.contains(&["Approvals and required", " checks complete."].concat()));
}

#[test]
fn operator_dashboard_projects_filter_uses_icon_toggle() {
	let response = dashboard_response();

	assert!(
		response.contains("const PROJECT_FILTER_STORAGE_KEY = \"decodex.operator.projectFilter\";")
	);
	assert!(
		response
			.contains("projectFilterToggle: document.getElementById(\"project-filter-toggle\")")
	);
	assert!(response.contains("let projectFilterMode = loadProjectFilterMode();"));
	assert!(response.contains("function loadProjectFilterMode()"));
	assert!(response.contains("function persistProjectFilterMode()"));
	assert!(response.contains("function renderProjectFilterToggle(projects = [])"));
	assert!(response.contains("class=\"project-filter-toggle\" id=\"project-filter-toggle\""));
	assert!(
		response
			.contains("role=\"switch\" aria-checked=\"false\" aria-label=\"Show all projects\"")
	);
	assert!(response.contains("M3 4h10l-4 4.6v3.1l-2 1V8.6L3 4Z"));
	assert!(
		response
			.contains("projectFilterMode = projectFilterMode === \"all\" ? \"active\" : \"all\";")
	);
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
	assert!(response.contains(
		"button.setAttribute(\"aria-expanded\", projectWorkInfoOpen ? \"true\" : \"false\");"
	));
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
	assert!(
		response.contains("const parts = [`${derived.queueBacklogCandidates.length} queued`];")
	);
	assert!(response.contains("return \"0 queued\";"));
	assert!(
		response.contains("setPanelMeta(nodes.queuedMeta, backlogMetaText(snapshot, derived));")
	);
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
	assert!(
		response.contains("attention.thread_status && attention.thread_status !== \"systemError\"")
	);
	assert!(
		response.contains("queueBacklogCandidates.filter(queuedCandidateNeedsAttention).length")
	);
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
	assert!(
		response
			.contains("label: isDirty ? \"post-review cleanup blocked\" : \"post-review cleanup\"")
	);
	assert!(response.contains("retainedWorktrees.some(recoveryWorktreeShouldDefaultOpen)"));
	assert!(!response.contains(
		"syncDefaultDetailOpenState(nodes.panels.worktrees, retainedWorktrees.length > 0);"
	));
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
		response
			.contains("facts.push([\"Attempt status\", displayToken(attention.attempt_status)]);")
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
	assert!(response.contains(
		"displayTextRepeats(reason, displayToken(candidate.attention.attention_error_class))"
	));
	assert!(response.contains("displayTextRepeats(reason, \"worktree_has_tracked_changes\")"));
	assert!(!response.contains("return \"blocked by needs-attention\";"));
	assert!(!reason_text.contains("return \"Retry budget held\";"));
	assert!(
		!response
			.contains("facts.push([\"Retry\", String(attention.retry_budget_attempt_count)]);")
	);
	assert!(
		reason_text
			.find("if (candidate.attention?.attention_error_class)")
			.expect("attention error-class reason should exist")
			< reason_text
				.find("return \"retry_budget_attempt_count\";")
				.expect("retry-budget reason should exist")
	);
}
