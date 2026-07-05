use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_account_privacy_controls_use_compact_identities() {
	let response = dashboard::dashboard_response();

	assert!(
		response
			.contains("const ACCOUNT_PRIVACY_STORAGE_KEY = \"decodex.operator.accountPrivacy\";")
	);
	assert!(!response.contains(
		"const ACCOUNT_NAME_OFFSET_STORAGE_KEY = \"decodex.operator.accountNameOffsets\";"
	));
	assert!(response.contains("const ACCOUNT_IDENTITY_EDGE_CHARS = 6;"));
	assert!(response.contains("const ACCOUNT_IDENTITY_MIN_EDGE_CHARS = 3;"));
	assert!(!response.contains("const ACCOUNT_EMAIL_LOCAL_HEAD_CHARS = 5;"));
	assert!(!response.contains("const ACCOUNT_EMAIL_LOCAL_TAIL_CHARS = 4;"));
	assert!(response.contains("const ACCOUNT_RANDOM_NAMES = ["));
	assert!(!response.contains("const ACCOUNT_RANDOM_NAME_PREFIXES = ["));
	assert!(!response.contains("const ACCOUNT_RANDOM_NAME_SUFFIXES = ["));
	assert!(response.contains("function trimLeadingEllipsis(value)"));
	assert!(response.contains("function compactAccountIdentity(value)"));
	assert!(!response.contains("function compactAccountEmailIdentity(value)"));
	assert!(response.contains("function codexAccountIdentityHash(value)"));
	assert!(response.contains("function codexAccountRandomName(account)"));
	assert!(response.contains("function codexAccountEmail(account)"));
	assert!(response.contains("function compactAccountEmail(email)"));
	assert!(response.contains("function loadAccountPrivacy()"));
	assert!(!response.contains("function loadAccountNameOffsets()"));
	assert!(response.contains("function persistAccountPrivacy(hidden)"));
	assert!(!response.contains("function persistAccountNameOffsets()"));
	assert!(!response.contains("function configuredDashboardAccounts(snapshot)"));
	assert!(response.contains("function renderAccountPrivacyToggle()"));
	assert!(response.contains("function codexAccountRandomNameKey(account)"));
	assert!(response.contains("function codexAccountRandomNameOffset(account)"));
	assert!(response.contains("function codexAccountPendingRandomNameOffset(account)"));
	assert!(response.contains("let pendingAccountNameOffsets = {};"));
	assert!(!response.contains("function codexAccountStoredRandomNameOffset(account)"));
	assert!(!response.contains("function syncStoredAccountNameOffsets(accounts)"));
	assert!(response.contains("function codexAccountDisplaySource(account, snapshot)"));
	assert!(response.contains("function renderCodexAccountRandomNameButton(account)"));
	assert!(response.contains("function codexAccountShowsEmail(account)"));
	assert!(response.contains("function codexAccountPrivacyLabel(account)"));
	assert!(response.contains("function codexAccountPrivacyText(account, value)"));
	assert!(response.contains("function codexAccountVisibleName(account)"));
	assert!(response.contains("function codexAccountDisplayTitle(account)"));
	assert!(response.contains("function codexAccountControlStatusLabel(snapshot)"));
	assert!(
		response.contains("text = replaceLiteral(text, codexAccountEmail(account), replacement);")
	);
	assert!(response.contains("/[A-Z0-9._%+-]+@[A-Z0-9.-]+\\.[A-Z]{2,}/gi"));
	assert!(response.contains(
		"return codexAccountShowsEmail(account) ? email : codexAccountRandomName(account);"
	));
	assert!(response.contains("? compactAccountEmail(email)"));
	assert!(response.contains("const account = codexAccountPoolAccounts(snapshot).find("));
	assert!(response.contains("? compactAccountIdentity(selector)"));
	assert!(response.contains(": codexAccountVisibleName(account);"));
	assert!(response.contains("return \"Balanced\";"));
	assert!(response.contains("return `Fixed · ${label}`;"));
	assert!(response.contains("function codexAccountFallbackName(value)"));
	assert!(response.contains("return `Fixed · ${codexAccountFallbackName(selector)}`;"));
	assert!(response.contains("const title = codexAccountControlStatusLabel(snapshot);"));
	assert!(!response.contains("const title = `Mode ${modeLabel}`;"));
	assert!(response.contains("account-name-reroll"));
	assert!(response.contains("data-account-name-reroll"));
	assert!(response.contains("aria-label=\"Change account name\""));
	assert!(response.contains("\"Alex\""));
	assert!(response.contains("return `${local.slice(0, 3)}...${local.slice(-3)}${domain}`;"));
	assert!(response.contains("return ACCOUNT_RANDOM_NAMES[index];"));
	assert!(response.contains("return accounts;"));
	assert!(!response.contains("function codexAccountPoolSortKey(account)"));
	assert!(!response.contains(
		"return codexAccountPoolSortKey(left).localeCompare(codexAccountPoolSortKey(right));"
	));
	assert!(
		!response
			.contains("const checkedAt = codexAccountNumber(account?.checked_at_unix_epoch) || 0;")
	);
	assert!(!response.contains("localeCompare(codexAccountDisplayName(right))"));
	assert!(!response.contains("return account.account_fingerprint;"));
	assert!(!response.contains("`fingerprint ${account.account_fingerprint || \"unknown\"}`"));
	assert!(!response.contains("const fingerprint = account.account_fingerprint || \"unknown\";"));
	assert!(!response.contains("account.account_fingerprint || \"unknown\",\n"));
	assert!(response.contains("renderAccountPrivacyToggle();"));
	assert!(response.contains("renderAccountModeControl(snapshot);"));
	assert!(response.contains("persistAccountPrivacy(accountEmailsHidden);"));
	assert!(response.contains("let lastDashboardRender = null;"));
	assert!(response.contains("lastDashboardRender = {"));
	assert!(response.contains("function renderDashboardState({"));
	assert!(response.contains("renderDashboardState(lastDashboardRender);"));
	assert!(response.contains(".table-meta .metric-number"));
	assert!(response.contains(".table-meta[data-tone=\"active\"] .metric-number"));
	assert!(response.contains("font-size: var(--type-label);"));
	assert!(response.contains("letter-spacing: var(--tracking-caps);"));
	assert!(!response.contains("text-transform: uppercase;"));
	assert!(response.contains("function renderCodexAccountPoolGuideCell(column)"));
	assert!(response.contains("return `<span class=\"account-pool-heading\">${sortButton}${accountPrivacyToggleMarkup()}</span>`;"));
	assert!(response.contains(".section-marker-meta {"));
	assert!(response.contains("text-align: right;"));
	assert!(!response.contains("setPanelMeta(nodes.accountPoolMeta"));
	assert!(
		!response.contains("${pluralize(accounts.length, \"account\")} · ${activeCount} active")
	);
}
