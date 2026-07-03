use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_review_cards_omit_static_summary_copy() {
	let response = dashboard::dashboard_response();

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
	let response = dashboard::dashboard_response();

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
