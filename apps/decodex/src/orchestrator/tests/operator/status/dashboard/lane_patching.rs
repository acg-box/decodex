use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_patches_current_lane_cards_without_replacing_the_list() {
	let response = dashboard::dashboard_response();

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
	let response = dashboard::dashboard_response();

	dashboard::assert_child_bucket_contract(&response);
	dashboard::assert_child_activity_header_contract(&response);
	dashboard::assert_child_lifecycle_contract(&response);
	dashboard::assert_running_lane_meta_contract(&response);
	dashboard::assert_liveness_and_cleanup_contract(&response);
}
