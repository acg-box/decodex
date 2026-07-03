use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_projects_show_compact_activity_work_and_location() {
	let response = dashboard::dashboard_response();

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
