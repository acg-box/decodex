use super::*;

#[test]
fn operator_dashboard_renders_account_usage_controls() {
	let response = dashboard_response();

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

#[test]
fn operator_dashboard_account_privacy_controls_use_compact_identities() {
	let response = dashboard_response();

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

#[test]
fn operator_dashboard_account_errors_route_to_notice_dock_with_privacy() {
	let response = dashboard_response();

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
	let response = dashboard_response();

	assert!(response.contains("<h2 id=\"current-lanes-title\">Current Lanes</h2>"));
	assert!(response.contains("<h2 id=\"queue-title\">Intake Queue</h2>"));
	assert!(response.contains("<h2>Review &amp; Landing</h2>"));
	assert!(response.contains("<h2 id=\"worktrees-title\">Recovery Worktrees</h2>"));
	assert!(response.contains("<h2 id=\"recent-title\">Run History</h2>"));
}

#[test]
fn operator_dashboard_renders_account_sort_controls() {
	let response = dashboard_response();

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
	let response = dashboard_response();

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

#[test]
fn operator_dashboard_accounts_keeps_compact_table_layout() {
	let response = dashboard_response();

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
	let response = dashboard_response();

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
	let response = dashboard_response();

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
	let response = dashboard_response();

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

#[test]
fn operator_dashboard_accounts_keeps_debug_credit_and_reset_copy_compact() {
	let response = dashboard_response();

	assert_account_debug_and_credit_copy(&response);
	assert_account_reset_copy(&response);
	assert_account_pool_render_copy(&response);
}

fn assert_account_debug_and_credit_copy(response: &str) {
	let active_debug = response
		.split("<summary>Debug Details</summary>")
		.nth(1)
		.expect("active debug details should exist")
		.split("</details>")
		.next()
		.expect("active debug details should end");

	assert!(!active_debug.contains("field(\"Account\", codexAccountDebugSummary(account))"));
	assert!(!active_debug.contains("field(\"Freshness source\","));
	assert!(!active_debug.contains("field(\"Lane activity\","));
	assert!(!active_debug.contains("field(\"Last protocol activity\","));
	assert!(
		!response.contains("field(\"Accounts\", codexAccountPoolDebugSummary(codexAccounts(run)))")
	);
	assert!(
		response
			.contains("field(\"Account\", codexAccountDebugSummary(codexAccount(run, snapshot)))")
	);
	assert!(
		response
			.contains("facts.push([\"Account\", codexAccountHistorySummary(codexAccount(run))])")
	);
	assert!(!response.contains("facts.push([\"Codex pool\""));
	assert!(!response.contains("account <strong>"));
	assert!(response.contains("credits_unlimited"));
	assert!(response.contains("function formatCodexAccountCreditsBalance(value)"));
	assert!(
		response
			.contains("const balance = formatCodexAccountCreditsBalance(account.credits_balance);")
	);
	assert!(response.contains("return number.toFixed(2);"));
	assert!(!response.contains(".replace(/\\.00$/, \"\")"));
	assert!(!response.contains(".replace(/(\\.\\d)0$/, \"$1\")"));
	assert!(response.contains("function codexAccountCreditsTone(account)"));
	assert!(response.contains("function codexAccountUsageLimited(account)"));
	assert!(response.contains("if (status === \"available\")"));
	assert!(response.contains("return \"ready\";"));
	assert!(response.contains("codexAccountReachedType(account).includes(\"credit\")"));
	assert!(response.contains("const credits = codexAccountCreditsSummary(account);"));
	assert!(response.contains("const creditTone = codexAccountCreditsTone(account);"));
	assert!(response.contains("<span>credits</span>"));
	assert!(response.contains("<strong>${escapeHtml(credits || \"-\")}</strong>"));

	let account_credit_index = response
		.find("<div class=\"account-row-credit${creditClass}\">")
		.expect("account credit cell render");
	let account_status_index =
		response.find("<div class=\"account-row-state\">").expect("account status cell render");

	assert!(account_credit_index < account_status_index);
	assert!(response.contains("return \"0.00\";"));
	assert!(!response.contains("return \"No Credits\";"));
	assert!(response.contains("return \"Unlimited\";"));

	let account_status_label = response
		.split("function codexAccountStatusLabel(account)")
		.nth(1)
		.expect("account status label function should exist")
		.split("function codexAccountCreditsSummary(account)")
		.next()
		.expect("account status label function should have an end");

	assert!(account_status_label.contains("return refresh;"));
	assert!(account_status_label.contains("return displayToken(status);"));
	assert!(!account_status_label.contains("Refresh failed"));
	assert!(!account_status_label.contains("Ready"));
	assert!(response.contains("return codexAccountTokenValue(account.refresh_status);"));
	assert!(response.contains("return \"-\";"));
	assert!(!response.contains("depleted"));
	assert!(response.contains("rate_limit_reached_type"));
	assert!(response.contains("if (codexAccountUsageLimited(account))"));
	assert!(account_status_label.contains("return reached || (String(status).trim() && status !== \"available\" ? status : \"usage_limited\");"));
}

fn assert_account_reset_copy(response: &str) {
	assert!(response.contains("cooldown_until_unix_epoch"));
	assert!(response.contains("`${prefix}_remaining_percent`"));
	assert!(response.contains("`${prefix}_resets_at_unix_epoch`"));
	assert!(response.contains("value === 18_000"));
	assert!(response.contains("value === 604_800"));
	assert!(response.contains("function formatCodexAccountResetDuration(seconds)"));
	assert!(response.contains("function codexAccountResetDistance(value)"));
	assert!(response.contains("function codexAccountResetDisplay(data)"));
	assert!(!response.contains("const shortWindow = windowSeconds === 18_000;"));
	assert!(
		response.contains("return { short: \"0m\", phrase: \"reset due now\", isPast: true };")
	);
	assert!(response.contains("return { short, phrase: `resets in ${short}`, isPast: false };"));
	assert!(response.contains("date: \"\","));
	assert!(response.contains("date: resetAt,"));
	assert!(response.contains("aria: \"reset unavailable\","));
	assert!(response.contains("reset at ${resetAt}, ${distance.phrase}"));
	assert!(
		response.contains("data.remainingPercent == null ? \"-\" : `${data.remainingPercent}%`;")
	);
	assert!(response.contains("aria-label=\"${escapeHtml(label)} usage unavailable\""));
	assert!(response.contains("const resetTitle = `${label} ${remaining}, ${reset.aria}`;"));
	assert!(
		response.contains("<span class=\"account-window-reset\">${escapeHtml(reset.short)}</span>")
	);
	assert!(response.contains(
		"${reset.date ? `<span class=\"account-window-date\">${escapeHtml(reset.date)}</span>` : \"\"}"
	));
	assert!(!response.contains("<strong>${escapeHtml(reset.main)}</strong>"));
	assert!(!response.contains("<span>${escapeHtml(reset.detail)}</span>"));
	assert!(response.contains("class=\"account-status\""));
	assert!(response.contains("function codexAccountWindowTone(percent)"));
	assert!(response.contains(".account-window.is-warn > strong"));
	assert!(response.contains(".account-window.is-danger > strong"));
	assert!(!response.contains("function codexAccountLowestRemaining(account)"));
	assert!(!response.contains("lowestRemaining <= 20"));
	assert!(!response.contains("account-meter"));
	assert!(!response.contains("lowestRemaining}%"));
}

fn assert_account_pool_render_copy(response: &str) {
	assert!(response.contains(
		"renderStableList(nodes.accountPool, renderCodexAccountPool(accounts, snapshot));"
	));
	assert!(response.contains("syncAccountSelectionConfirmationDom();"));
	assert!(
		!response
			.contains("nodes.accountPool.innerHTML = renderCodexAccountPool(accounts, snapshot)")
	);
	assert!(response.contains("renderAccountPrivacyToggle();"));
	assert!(!response.contains("setPanelMeta(nodes.accountPoolMeta"));
	assert!(!response.contains("nodes.accountPoolMeta.textContent = snapshot"));
	assert!(!response.contains("nodes.accountPoolMeta"));
	assert!(!response.contains("account-row-windows"));
	assert!(!response.contains("account-mini-window"));
	assert!(!response.contains("account-mini-label"));
	assert!(!response.contains("grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));"));
	assert!(
		!response.contains("grid-template-columns: minmax(170px, 1fr) minmax(360px, 1.7fr) 118px;")
	);
	assert!(!response.contains("border-right: 1px solid var(--line);"));
	assert!(!response.contains("box-shadow: inset 3px 0 0 var(--success)"));
	assert!(!response.contains(">Emails</span>"));
	assert!(!response.contains("[\"checked\""));
}
