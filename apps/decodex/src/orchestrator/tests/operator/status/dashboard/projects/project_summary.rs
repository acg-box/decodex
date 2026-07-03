use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_projects_keep_status_summary_compact() {
	let response = dashboard::dashboard_response();

	assert_dashboard_project_summary_helpers(&response);
	assert_dashboard_queue_summary_contract(&response);
	assert_dashboard_project_table_contract(&response);
}

fn assert_dashboard_project_summary_helpers(response: &str) {
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
}

fn assert_dashboard_queue_summary_contract(response: &str) {
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
}

fn assert_dashboard_project_table_contract(response: &str) {
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
