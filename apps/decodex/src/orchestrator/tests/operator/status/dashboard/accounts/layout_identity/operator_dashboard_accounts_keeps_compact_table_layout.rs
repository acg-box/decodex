use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_accounts_keeps_compact_table_layout() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("run-meta-line"));
	assert!(response.contains("account-pool-list"));
	assert!(response.contains("account-pool-guide"));
	assert!(response.contains("<div class=\"account-pool-summary\""));
	assert!(response.contains("function codexAccountProfileAggregate(accounts)"));
	assert!(response.contains("function renderCodexAccountPoolActivityStrip(account"));
	assert!(response.contains("function renderCodexAccountProfileActivityStrip(account"));
	assert!(response.contains("function codexAccountProfilePeakDailyTokens(account)"));
	assert!(response.contains("function renderCodexAccountProfileToggle(account, expanded)"));
	assert!(response.contains(
		"function renderCodexAccountProfilePanel(account, snapshot, profileKey, expanded)"
	));
	assert!(response.contains("function toggleCodexAccountProfileKey(key)"));
	assert!(response.contains("function accountProfileRowClickIsSuppressed(target)"));
	assert!(response.contains("data-account-profile-toggle"));
	assert!(response.contains("data-account-profile-row-toggle"));
	assert!(response.contains("data-render-key=\"account-row:${escapeHtml(profileKey)}\""));
	assert!(
		response.contains("data-render-key=\"account-profile-panel:${escapeHtml(profileKey)}\"")
	);
	assert!(response.contains(".account-row.is-profile-toggleable"));
	assert!(response.contains(
		"const profileRow = event.target.closest(\"[data-account-profile-row-toggle]\");"
	));
	assert!(response.contains("aria-hidden=\"${expanded ? \"false\" : \"true\"}\""));
	assert!(response.contains("const openClass = expanded ? \" is-open\" : \"\";"));
	assert!(response.contains("expandedAccountProfileKeys"));
	assert!(response.contains("account-pool-activity-strip"));
	assert!(response.contains("account-pool-activity-tile"));
	assert!(response.contains("label: \"Activity\""));
	assert!(response.contains("valueHtml: activityStrip"));
	assert!(response.contains(".account-pool-metric-label {\n\t\t\t\toverflow: hidden;\n\t\t\t\tcolor: var(--muted);\n\t\t\t\tfont-family: var(--sans);"));
	assert!(response.contains(".account-pool-metric-value {\n\t\t\t\toverflow: hidden;\n\t\t\t\tcolor: var(--muted-strong);\n\t\t\t\tfont-family: var(--mono);"));
	assert!(response.contains(
		".account-pool-metric-value[data-tone=\"muted\"] {\n\t\t\t\tcolor: var(--muted-strong);"
	));
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
	assert!(
		response
			.contains("ACCOUNT_POOL_SORT_COLUMNS.map(renderCodexAccountPoolGuideCell).join(\"\")")
	);
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
	assert!(response.contains(
		"\n\t\t\t\tdisplay: grid;\n\t\t\t\tmin-width: 760px;\n\t\t\t\tbackground: transparent;"
	));
	assert!(response.contains(".account-pool-guide {\n\t\t\t\tdisplay: grid;"));
	assert!(response.contains("grid-template-columns: var(--account-grid);"));
	assert!(response.contains(".account-pool-sort {\n\t\t\t\tjustify-self: center;"));
	assert!(response.contains(".account-pool-sort-icon"));
	assert!(response.contains("background: transparent;"));
	assert!(!response.contains(
		"box-shadow: 0 8px 24px color-mix(in srgb, var(--account-accent) 7%, transparent);"
	));
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
	assert!(response.contains(
		"<span class=\"account-window-label\" aria-hidden=\"true\">${escapeHtml(label)}</span>"
	));
	assert!(response.contains("aria-label=\"${escapeHtml(label)} remaining ${escapeHtml(remaining)}, ${escapeHtml(reset.aria)}\""));
	assert!(response.contains("title=\"${escapeHtml(resetTitle)}\""));
	assert!(response.contains("account-window-date"));
	assert!(!response.contains("<span class=\"is-reset\">Reset</span>"));
}
