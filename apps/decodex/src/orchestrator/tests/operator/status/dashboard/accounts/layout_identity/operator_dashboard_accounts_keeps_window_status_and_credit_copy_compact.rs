use crate::orchestrator::tests::operator::status::dashboard;

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
