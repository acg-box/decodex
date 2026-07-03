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

#[test]
fn operator_dashboard_accounts_renders_fixed_selection_affordance() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("is-selected"));
	assert!(response.contains("is-ready"));
	assert!(response.contains("is-armed"));
	assert!(response.contains("--account-accent: var(--tone-muted);"));
	assert!(response.contains("--account-confirm-accent: var(--tone-run);"));
	assert!(
		response.contains(".account-row.is-ready {\n\t\t\t\t--account-accent: var(--success);")
	);
	assert!(response.contains(".account-row.is-fixed {\n\t\t\t\t--account-accent: var(--info);"));
	assert!(
		!response.contains(".account-row.is-armed {\n\t\t\t\t--account-accent: var(--warning);")
	);
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
	assert!(
		response.contains(
			"color: color-mix(in srgb, var(--account-confirm-accent) 46%, var(--muted));"
		)
	);
	assert!(response.contains("text-shadow: none;"));
	assert!(response.contains(".account-name-button.is-fixed::before"));
	assert!(response.contains(".account-name-button.is-fixed::after"));
	assert!(response.contains(
		".account-name-button.is-fixed {\n\t\t\t\tcolor: var(--account-confirm-accent);"
	));
	assert!(response.contains(".account-name-button + .account-name-reroll"));
	assert!(response.contains("margin-left: 8px;"));
	assert!(response.contains("opacity: 0.72;"));
	assert!(response.contains(
		"animation: account-confirm-name-breathe var(--account-confirm-cycle) var(--ease) infinite;"
	));
	assert!(response.contains(
		"animation: account-confirm-bracket-left var(--account-confirm-cycle) var(--ease) infinite;"
	));
	assert!(response.contains(
		"animation: account-confirm-bracket-right var(--account-confirm-cycle) var(--ease) infinite;"
	));
	assert!(!response.contains("infinite alternate;"));
}

#[test]
fn operator_dashboard_accounts_keeps_identity_rows_compact() {
	let response = dashboard::dashboard_response();

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
	assert!(
		response
			.contains("const planType = String(account?.plan_type || \"\").trim().toLowerCase();")
	);
	assert!(response.contains("return planType === \"pro\" ? 20 : 1;"));
	assert!(response.contains("const weight = codexAccountCapacityLabel(account);"));
	assert!(response.contains(
		"const identityClass = codexAccountShowsEmail(account) ? \" is-machine\" : \"\";"
	));
	assert!(response.contains(".account-row-plan {\n\t\t\t\tgrid-area: plan;"));
	assert!(response.contains("<div class=\"account-row-id${identityClass}\">"));
	assert!(response.contains("<div class=\"account-row-plan\">${escapeHtml(weight)}</div>"));
	assert!(response.contains("<button class=\"account-name-button${fixedClass}${armedClass}\""));
	assert!(response.contains("<span class=\"account-name\">${escapeHtml(visibleName)}</span>"));
	assert!(response.contains("<span class=\"run-meta-icon\" aria-hidden=\"true\">"));
	assert!(response.contains("<svg viewBox=\"0 0 16 16\" fill=\"none\">"));
	assert!(response.contains(
		"<path fill=\"currentColor\" fill-rule=\"evenodd\" clip-rule=\"evenodd\" d=\"M3.35 2.25h9.3"
	));
	assert!(response.contains("M8 4.15a1.8 1.8"));
	assert!(response.contains("c.61 0 1.1.49 1.1 1.1v9.3"));
	assert!(!response.contains("d=\"M8 1.65a6.35"));
	assert!(!response.contains("<path fill=\"currentColor\" d=\"M8 7.3a2.55"));
	assert!(!response.contains("<path fill=\"currentColor\" d=\"M3.25 13.15c.48-2.65"));
	assert!(!response.contains("fill-rule=\"evenodd\" clip-rule=\"evenodd\" d=\"M3.9 2.2h8.2"));
	assert!(!response.contains("<circle cx=\"8\" cy=\"5.1\""));
	assert!(response.contains("<strong class=\"account-name${identityClass}\" title=\"${escapeHtml(pendingTitle)}\">${escapeHtml(visibleName)}</strong>"));
	assert!(
		!response.contains("<strong class=\"machine-text\">${escapeHtml(`${value}%`)}</strong>")
	);
	assert!(!response.contains("function codexAccountSecondaryLabel(account)"));
	assert!(response.contains("const visibleName = codexAccountVisibleName(account);"));
	assert!(response.contains("const displayTitle = codexAccountDisplayTitle(account);"));
	assert!(
		response
			.contains("title=\"${escapeHtml(pendingTitle)}\">${escapeHtml(visibleName)}</strong>")
	);
	assert!(response.contains("text.startsWith(\"...\") && text.indexOf(\"...\", 3) === -1"));
}

#[test]
fn operator_dashboard_accounts_keeps_window_status_and_credit_copy_compact() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("ACCOUNT_IDENTITY_MIN_EDGE_CHARS,"));
	assert!(
		response.contains("Math.min(ACCOUNT_IDENTITY_EDGE_CHARS, Math.floor(text.length / 2)),")
	);
	assert!(
		response.contains("return `${text.slice(0, headLength)}...${text.slice(-tailLength)}`;")
	);
	assert!(response.contains("grid-area: primary;"));
	assert!(response.contains("grid-area: secondary;"));
	assert!(response.contains("justify-self: stretch;"));
	assert!(!response.contains("max-width: 190px;"));
	assert!(!response.contains("max-width: 142px;"));
	assert!(response.contains("min-height: 42px;"));
	assert!(response.contains(
		"padding: var(--space-account-row-y) 28px var(--space-account-row-y) var(--space-row-indent);"
	));
	assert!(response.contains("border-bottom: 1px solid var(--line);"));
	assert!(response.contains(".account-pool-list > .account-row.is-last-account"));
	assert!(
		response.contains("const lastAccountClass = isLastAccount ? \" is-last-account\" : \"\";")
	);
	assert!(response.contains(
		"accounts.map((account, index) => renderCodexAccountPoolRow(account, snapshot, index === accounts.length - 1))"
	));
	assert!(!response.contains(".account-pool-list > .account-row:last-child"));
	assert!(response.contains("account-row-credit"));
	assert!(response.contains(".account-row-credit {\n\t\t\t\tgrid-area: credit;"));
	assert!(response.contains(
		".account-row-credit {\n\t\t\t\tgrid-area: credit;\n\t\t\t\tjustify-self: center;"
	));
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
	assert!(
		response.contains("background: linear-gradient(90deg, var(--hover), transparent 78%);")
	);
	assert!(response.contains(
		"box-shadow: 0 0 18px color-mix(in srgb, var(--account-accent) 42%, transparent);"
	));
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
	assert!(
		response.contains("<div class=\"account-window is-${escapeHtml(prefix)}${toneClass}\"")
	);
	assert!(!response.contains("codexAccountStatusBit(account)"));
	assert!(response.contains("renderRunCodexAccountInline(run, snapshot)"));
	assert!(response.contains("function renderRunMetaLine(run, snapshot = null)"));
}
