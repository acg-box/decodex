use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_account_errors_route_to_notice_dock_with_privacy() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("function codexAccountNotices(snapshot)"));
	assert!(response.contains("for (const accountNotice of codexAccountNotices(snapshot))"));
	assert!(response.contains("notices.push(accountNotice);"));
	assert!(response.contains("function codexAccountHasNotice(account)"));
	assert!(response.contains("function codexAccountNoticeCopy(account)"));
	assert!(
		response.contains("return `${codexAccountPrivacyLabel(account)}: ${parts.join(\"; \")}`;")
	);
	assert!(response.contains("codexAccountRefreshFailed(account) && !noteIncludesRefreshFailure"));
	assert!(response.contains("codexAccountRefreshStatusNeedsAttention(refreshStatus) &&"));
	assert!(response.contains("!codexAccountRefreshFailed(account)"));
	assert!(response.contains("note && !noteLooksRoutine && !noteLooksError"));
	assert!(response.contains("codexAccountPrivacyText(account, note)"));
}

#[test]
fn operator_dashboard_uses_expanded_section_titles() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("<h2 id=\"current-lanes-title\">Current Lanes</h2>"));
	assert!(response.contains("<h2 id=\"queue-title\">Intake Queue</h2>"));
	assert!(response.contains("<h2>Review &amp; Landing</h2>"));
	assert!(response.contains("<h2 id=\"worktrees-title\">Recovery Worktrees</h2>"));
	assert!(response.contains("<h2 id=\"recent-title\">Run History</h2>"));
}

#[test]
fn operator_dashboard_renders_account_sort_controls() {
	let response = dashboard::dashboard_response();

	assert!(
		response
			.contains("const ACCOUNT_POOL_SORT_STORAGE_KEY = \"decodex.operator.accountSort\";")
	);
	assert!(response.contains("const ACCOUNT_POOL_SORT_COLUMNS = ["));
	assert!(response.contains("function loadAccountPoolSort()"));
	assert!(response.contains("function persistAccountPoolSort()"));
	assert!(response.contains("function isAccountPoolSortKey(value)"));
	assert!(response.contains("function renderCodexAccountPoolSortButton([key, label])"));
	assert!(response.contains("account-pool-sort"));
	assert!(response.contains("data-account-sort-key"));
	assert!(
		response.contains(
			"aria-label=\"Sort accounts by ${escapeHtml(label)}; ${escapeHtml(current)}\""
		)
	);
	assert!(response.contains("account-sort-up"));
	assert!(response.contains("account-sort-down"));
	assert!(response.contains("function codexAccountPoolColumnSortValue(account, key)"));
	assert!(
		response.contains("function compareCodexAccountPoolColumn(left, right, key, direction)")
	);
	assert!(!response.contains("function compareCodexAccountPoolStable(left, right)"));
	assert!(response.contains("function sortCodexAccountPoolAccounts(accounts)"));
	assert!(response.contains("if (!accountPoolSort.key)"));
	assert!(response.contains("return 0;"));
	assert!(response.contains("codexAccountWindowData(account, \"primary\").remainingPercent"));
	assert!(response.contains("codexAccountWindowData(account, \"secondary\").remainingPercent"));
	assert!(response.contains("codexAccountCreditsSortValue(account)"));
	assert!(response.contains("persistAccountPoolSort();"));
	assert!(
		response.contains("accountPoolSort.key === key && accountPoolSort.direction === \"asc\"")
	);
}

#[test]
fn operator_dashboard_renders_project_sort_controls() {
	let response = dashboard::dashboard_response();

	assert!(
		response.contains("const PROJECT_SORT_STORAGE_KEY = \"decodex.operator.projectSort\";")
	);
	assert!(response.contains("const PROJECT_SORT_COLUMNS = ["));
	assert!(response.contains("[\"project\", \"Project\"]"));
	assert!(response.contains("[\"location\", \"Location\"]"));
	assert!(response.contains("[\"activity\", \"Activity\"]"));
	assert!(response.contains("[\"work\", \"Work\"]"));
	assert!(response.contains("function loadProjectSort()"));
	assert!(response.contains("function persistProjectSort()"));
	assert!(response.contains("function isProjectSortKey(value)"));
	assert!(response.contains("function projectSortDefaultDirection(key)"));
	assert!(
		response.contains("return [\"activity\", \"work\"].includes(key) ? \"desc\" : \"asc\";")
	);
	assert!(response.contains("function renderProjectSortButton([key, label])"));
	assert!(response.contains("project-table-sort"));
	assert!(response.contains("data-project-sort-key"));
	assert!(
		response.contains(
			"aria-label=\"Sort projects by ${escapeHtml(label)}; ${escapeHtml(current)}\""
		)
	);
	assert!(response.contains("project-sort-up"));
	assert!(response.contains("project-sort-down"));
	assert!(
		response
			.contains("aria-sort=\"${direction === \"asc\" ? \"ascending\" : \"descending\"}\"")
	);
	assert!(response.contains("function projectColumnSortValue(project, key)"));
	assert!(response.contains("function compareProjectRowsByColumn(left, right, key, direction)"));
	assert!(response.contains("function compareProjectRowsStable(left, right)"));
	assert!(response.contains("function sortProjectRows(rows)"));
	assert!(response.contains("projectSort.key === key"));
	assert!(response.contains("projectSortDefaultDirection(key)"));
	assert!(response.contains("persistProjectSort();"));
	assert!(response.contains("sortProjectRows(projectFilterRows(projects, activeProjectRows))"));
}
