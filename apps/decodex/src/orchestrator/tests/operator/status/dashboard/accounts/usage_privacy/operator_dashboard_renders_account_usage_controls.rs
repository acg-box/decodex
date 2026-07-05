use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_renders_account_usage_controls() {
	let response = dashboard::dashboard_response();

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
	assert!(
		response.contains(
			"function renderCodexAccountPoolRow(account, snapshot, isLastAccount = false)"
		)
	);
	assert!(response.contains("function renderCodexAccountNameControl(account, snapshot)"));
	assert!(!response.contains("ACCOUNT_SELECTION_CONFIRMATION_MS"));
	assert!(!response.contains("accountSelectionConfirmationTimer"));
	assert!(response.contains("function accountSelectionConfirmationMatches(action, selector)"));
	assert!(response.contains("function syncAccountSelectionConfirmationDom()"));
	assert!(response.contains("clearAccountSelectionConfirmation(true);"));
	assert!(
		response.contains("function accountSelectionControlTitle(action, displayTitle, armed)")
	);
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
	assert!(
		response
			.contains("<p class=\"table-meta section-marker-meta\" id=\"account-mode-meta\"></p>")
	);
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
