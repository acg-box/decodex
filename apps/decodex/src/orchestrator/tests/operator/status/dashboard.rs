fn dashboard_response() -> String {
	String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_DASHBOARD_ENDPOINT_PATH
			)
			.as_bytes(),
			None,
			OperatorSnapshotReadiness::SnapshotUnavailable,
		)
		.expect("dashboard response should build"),
	)
	.expect("dashboard response should be utf-8")
}

#[test]
fn operator_dashboard_background_wash_stays_viewport_fixed() {
	let response = dashboard_response();

	assert!(response.contains("background-attachment: fixed, fixed, fixed, scroll;"));
	assert!(
		response.contains("background-size: 100vw 100vh, 100vw 100vh, 100vw 100vh, auto;")
	);
}

#[test]
fn operator_dashboard_child_bucket_rows_split_time_bars_from_event_diagnostics() {
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_DASHBOARD_ENDPOINT_PATH
			)
			.as_bytes(),
			None,
			OperatorSnapshotReadiness::SnapshotUnavailable,
		)
		.expect("dashboard response should build"),
	)
	.expect("dashboard response should be utf-8");

	assert!(response.contains("childBucketIsSubsecond"));
	assert!(response.contains("childBucketIsEventOnly"));
	assert!(response.contains("childBucketEventSignals"));
	assert!(response.contains("childBucketEventSummary"));
	assert!(response.contains("childBucketDiagnosticSignals"));
	assert!(response.contains("childBucketDiagnosticSummary"));
	assert!(response.contains("renderChildBucketDiagnosticSignals"));
	assert!(response.contains("childBucketHasMeaningfulWallShare"));
	assert!(response.contains("childAgentLargeOutputWarnings"));
	assert!(response.contains("childAgentLargeOutputSummary"));
	assert!(response.contains("childBucketShareLabel"));
	assert!(response.contains("childBucketWidth"));
	assert!(response.contains("function setPanelMeta(node, text, tone = \"\")"));
	assert!(response.contains("function pluralLabel(count, singular, plural = `${singular}s`)"));
	assert!(response.contains("return `${count} ${pluralLabel(count, singular, plural)}`;"));
	assert!(response.contains("pluralLabel(notices.length, \"alert\")"));
	assert!(response.contains("pluralLabel(notices.length, \"warning\")"));
	assert!(response.contains("child-bucket is-share"));
	assert!(response.contains("child-bucket is-diagnostic"));
	assert!(response.contains("child-bucket is-event-only"));
	assert!(response.contains("child-bucket-signals"));
	assert!(response.contains("child-bucket-signal"));
	assert!(response.contains("data-duration=\"wall-share\""));
	assert!(response.contains("data-duration=\"event-diagnostics\""));
	assert!(response.contains("data-duration=\"diagnostic\""));
	assert!(response.contains("function childDiagnosticBucketRank(bucket)"));
	assert!(response.contains("summary.current_detail"));
	assert!(response.contains("return `${label} · ${formatDuration(summary.current_elapsed_seconds)}`;"));
	assert!(response.contains("tool calls"));
	assert!(response.contains("output bytes"));
	assert!(response.contains("Largest single tool output. Open Debug details for tool attribution."));
	assert!(response.contains("field(\"Large outputs\", childAgentLargeOutputSummary(childAgentActivity(run)))"));
	assert!(!response.contains("events only"));
	assert!(!response.contains("child-warning"));
	assert!(!response.contains("${warnings.length ? `<div class=\"child-warning\">"));
	assert!(!response.contains("warnings.join(\" · \")"));
	assert!(!response.contains("summary.largest_tool_output_tool || \"tool\""));
	assert!(!response.contains("child-bucket is-subsecond"));
	assert!(!response.contains("data-duration=\"events-only\""));
	assert!(!response.contains("child-bucket.is-event-only .child-bucket-bar::before"));
	assert!(response.contains("--child-bucket-value-column: clamp(190px, 18vw, 230px);"));
	assert!(response.contains("grid-template-columns: 96px minmax(64px, 1fr) var(--child-bucket-value-column);"));
	assert!(response.contains("width: var(--child-bucket-value-column);"));
	assert!(response.contains("runNeedsAttention"));
	assert!(response.contains("runCountsAsRunning"));
	assert!(response.contains("runOperationRequiresLiveAgent"));
	assert!(response.contains("runProcessStoppedWithoutAttention"));
	assert!(response.contains("runStageLabel"));
	assert!(response.contains("return \"Stopped\";"));
	assert!(response.contains("Finalizing"));
	assert!(
		response.contains(
			"Agent process stopped before the lane finished; operator recovery is required."
		)
	);
	assert!(response.contains("Stopped agent process"));
	assert!(response.contains("attention stopped"));
	assert!(response.contains("recovery <strong>needed</strong>"));
	assert!(response.contains("agent <strong>done</strong>"));
	assert!(!response.contains("process <strong>stopped</strong>"));
	assert!(response.contains("runningLaneMetaText"));
	assert!(response.contains("return count === 1 ? \"1 needs attention\" : `${count} need attention`;"));
	assert!(response.contains("nodes.activeRunsMeta,"));
	assert!(!response.contains("const parts = [`${derived.liveRuns} live`];"));
	assert!(!response.contains("parts.push(`${derived.runningAttentionCount} stalled`)"));
	assert!(response.contains("runStaleWithoutKnownProcessNeedsAttention"));
	assert!(response.contains("runExecutionLivenessSummary"));
	assert!(response.contains("runQueueLeaseSummary"));
	assert!(response.contains("lease <strong>not held</strong>"));
	assert!(response.contains("field(\"Attempt status\", run.attempt_status || run.status)"));
	assert!(response.contains("field(\"Queue lease\", runQueueLeaseSummary(run))"));
	assert!(response.contains("field(\"Execution liveness\", runExecutionLivenessSummary(run))"));
	assert!(response.contains("Live, no queue lease"));
	assert!(response.contains("queue lease not held; live process keeps lane visible"));
	assert!(!response.contains("Queue ownership"));
	assert!(response.contains("attention.worktree_path"));
	assert!(response.contains("candidate.attention?.attention_error_class"));
	assert!(response.contains("facts.push([\"Cause\", humanizeToken(attention.attention_error_class)]);"));
	assert!(response.contains("queued attention"));
	assert!(response.contains("worktree.ownership_reason"));
	assert!(response.contains("const hygiene = worktree.hygiene;"));
	assert!(response.contains("hygiene.classification === \"merged_dirty_worktree\""));
	assert!(response.contains("post-land dirty worktree"));
	assert!(response.contains("post-land cleanup"));
	assert!(response.contains("hygiene.reason ||"));
	assert!(response.contains("function renderWorktreeHygieneFields(worktree)"));
	assert!(response.contains("field(\"Cleanup state\", humanizeToken(hygiene.classification || \"cleanup_pending\"))"));
	assert!(response.contains("field(\"Default branch\", hygiene.default_branch || \"unknown\")"));
	assert!(response.contains("field(\"Uncommitted changes\", hygiene.dirty ? \"yes\" : \"no\")"));
	assert!(response.contains("local cleanup"));
	assert!(response.contains(
		"Owned by an intake issue that needs attention; recover it from Intake Queue instead of cleaning it up."
	));
	assert!(response.contains(
		"No active lane, queued recovery, or PR lane owns this worktree; safe for local cleanup."
	));
}

#[test]
fn operator_dashboard_renders_account_usage_controls() {
	let response = dashboard_response();

	assert!(response.contains("function codexAccount(run)"));
	assert!(response.contains("function codexAccounts(run)"));
	assert!(response.contains("function codexAccountDisplayName(account)"));
	assert!(response.contains("function codexAccountTokenLabel(refreshStatus)"));
	assert!(response.contains("function codexAccountWindowLabel(seconds)"));
	assert!(response.contains("function codexAccountStatusTone(account)"));
	assert!(response.contains("function renderRunCodexAccountInline(run)"));
	assert!(response.contains("function renderAccountPool(snapshot)"));
	assert!(response.contains("function codexAccountPoolAccounts(snapshot)"));
	assert!(response.contains("function codexAccountPoolRank(account)"));
	assert!(response.contains("function codexAccountPoolSortKey(account)"));
	assert!(response.contains("function renderCodexAccountPool(accounts)"));
	assert!(!response.contains("function renderCodexAccountPoolHeader(accounts)"));
	assert!(response.contains("function renderCodexAccountPoolRow(account)"));
	assert!(response.contains("function codexAccountDebugSummary(account)"));
	assert!(response.contains("function codexAccountPoolDebugSummary(accounts)"));
	assert!(response.contains("function codexAccountHistorySummary(account)"));
	assert!(response.contains("snapshot?.accounts"));
	assert!(response.contains("account?.email"));
	assert!(response.contains("run?.account || null"));
	assert!(response.contains("run?.accounts"));
	assert!(response.contains("account-pool-panel"));
	assert!(response.contains("<h2>Accounts</h2>"));
	assert!(!response.contains("<h2>Codex Accounts</h2>"));
	assert!(!response.contains(".stack > .panel + .panel"));
	assert!(response.contains("panel section-control\" id=\"account-pool-panel\""));
	assert!(response.contains("section-marker section-marker-control"));
	assert!(response.contains("section-marker section-marker-projects"));
	assert!(response.contains("section-marker section-marker-execution"));
	assert!(response.contains("section-marker section-marker-aftercare"));
	assert!(response.contains("<span>Control Plane</span>"));
	assert!(response.contains("<span>Projects</span>"));
	assert!(response.contains("<span>Execution</span>"));
	assert!(response.contains("<span>Closeout</span>"));
	assert!(response.contains("<p>Accounts</p>"));
	assert!(response.contains("All · Active"));
	assert!(response.contains("Running · Intake"));
	assert!(response.contains("Review · Recovery · History"));
	assert!(!response.contains("data-fold-key=\"panel:projects\""));
	assert!(response.contains("panel section-execution\" id=\"active-panel\""));
	assert!(response.contains("panel section-aftercare\" id=\"review-panel\""));
	assert!(!response.contains("section-group-start"));
	assert!(response.contains("#queue-panel .panel-head"));
	assert!(response.contains(".queue-group > .action-card:last-child"));
	assert!(response.contains("nodes.projectTitle.textContent = \"Decodex\""));
	assert!(!response.contains("Decodex Operator"));
	assert!(response.contains("primary: [\"accountPool\", \"projects\", \"active\", \"queue\", \"review\", \"worktrees\", \"recent\"]"));
	assert!(response.contains("#account-pool-panel {"));
	assert!(response.contains("#active-panel {\n\t\t\t\tbackground: transparent;"));
	assert!(response.contains("account-pool-title"));
	assert!(response.contains("account-privacy-toggle"));
	assert!(response.contains("account-eye-open"));
	assert!(response.contains("account-eye-off"));
	assert!(response.contains("const ACCOUNT_PRIVACY_STORAGE_KEY = \"decodex.operator.accountPrivacy\";"));
	assert!(response.contains("const ACCOUNT_NAME_OFFSET_STORAGE_KEY = \"decodex.operator.accountNameOffsets\";"));
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
	assert!(response.contains("function loadAccountNameOffsets()"));
	assert!(response.contains("function persistAccountPrivacy(hidden)"));
	assert!(response.contains("function persistAccountNameOffsets()"));
	assert!(response.contains("function renderAccountPrivacyToggle()"));
	assert!(response.contains("function codexAccountRandomNameKey(account)"));
	assert!(response.contains("function codexAccountRandomNameOffset(account)"));
	assert!(response.contains("function renderCodexAccountRandomNameButton(account)"));
	assert!(response.contains("function codexAccountShowsEmail(account)"));
	assert!(response.contains("function codexAccountVisibleName(account)"));
	assert!(response.contains("function codexAccountDisplayTitle(account)"));
	assert!(response.contains("return codexAccountShowsEmail(account) ? email : codexAccountRandomName(account);"));
	assert!(response.contains("? compactAccountEmail(email)"));
	assert!(response.contains("account-name-reroll"));
	assert!(response.contains("data-account-name-reroll"));
	assert!(response.contains("aria-label=\"Change account name\""));
	assert!(response.contains("return `${local.slice(0, 3)}...${local.slice(-3)}${domain}`;"));
	assert!(response.contains("return ACCOUNT_RANDOM_NAMES[index];"));
	assert!(response.contains("return status === \"selected\" ? 1 : 0;"));
	assert!(response.contains("return codexAccountPoolSortKey(left).localeCompare(codexAccountPoolSortKey(right));"));
	assert!(response.contains("const hash = codexAccountIdentityHash(identity).toString(16).padStart(8, \"0\");"));
	assert!(response.contains("return hash;"));
	assert!(!response.contains("const checkedAt = codexAccountNumber(account?.checked_at_unix_epoch) || 0;"));
	assert!(!response.contains("localeCompare(codexAccountDisplayName(right))"));
	assert!(!response.contains("return account.account_fingerprint;"));
	assert!(!response.contains("`fingerprint ${account.account_fingerprint || \"unknown\"}`"));
	assert!(!response.contains("const fingerprint = account.account_fingerprint || \"unknown\";"));
	assert!(!response.contains("account.account_fingerprint || \"unknown\",\n"));
	assert!(response.contains("renderAccountPrivacyToggle();"));
	assert!(response.contains("persistAccountPrivacy(accountEmailsHidden);"));
	assert!(response.contains("persistAccountNameOffsets();"));
	assert!(response.contains("let lastDashboardRender = null;"));
	assert!(response.contains("lastDashboardRender = {"));
	assert!(response.contains("function renderDashboardState({"));
	assert!(response.contains("renderDashboardState(lastDashboardRender);"));
	assert!(response.contains(".table-meta[data-tone=\"active\"]"));
	assert!(response.contains(".table-meta[data-tone=\"attention\"]"));
	assert!(response.contains("font-size: 11px;"));
	assert!(response.contains("letter-spacing: 0.06em;"));
	assert!(response.contains("text-transform: uppercase;"));
	assert!(response.contains("setPanelMeta(nodes.accountPoolMeta, meta, activeCount > 0 ? \"active\" : \"\")"));
	assert!(response.contains("${pluralize(accounts.length, \"account\")} · ${activeCount} active"));
}

#[test]
fn operator_dashboard_renders_account_sort_controls() {
	let response = dashboard_response();

	assert!(response.contains("const ACCOUNT_POOL_SORT_STORAGE_KEY = \"decodex.operator.accountSort\";"));
	assert!(response.contains("const ACCOUNT_POOL_SORT_COLUMNS = ["));
	assert!(response.contains("function loadAccountPoolSort()"));
	assert!(response.contains("function persistAccountPoolSort()"));
	assert!(response.contains("function isAccountPoolSortKey(value)"));
	assert!(response.contains("function renderCodexAccountPoolSortButton([key, label])"));
	assert!(response.contains("account-pool-sort"));
	assert!(response.contains("data-account-sort-key"));
	assert!(response.contains("aria-label=\"Sort accounts by ${escapeHtml(label)}; ${escapeHtml(current)}\""));
	assert!(response.contains("account-sort-up"));
	assert!(response.contains("account-sort-down"));
	assert!(response.contains("function codexAccountPoolColumnSortValue(account, key)"));
	assert!(response.contains("function compareCodexAccountPoolColumn(left, right, key, direction)"));
	assert!(response.contains("function compareCodexAccountPoolStable(left, right)"));
	assert!(response.contains("function sortCodexAccountPoolAccounts(accounts)"));
	assert!(response.contains("codexAccountWindowData(account, \"primary\").remainingPercent"));
	assert!(response.contains("codexAccountWindowData(account, \"secondary\").remainingPercent"));
	assert!(response.contains("codexAccountCreditsSortValue(account)"));
	assert!(response.contains("persistAccountPoolSort();"));
	assert!(response.contains("accountPoolSort.key === key && accountPoolSort.direction === \"asc\""));
}

#[test]
fn operator_dashboard_accounts_keeps_compact_table_layout() {
	let response = dashboard_response();

	assert!(response.contains("account-use-line"));
	assert!(response.contains("account-pool-list"));
	assert!(response.contains("account-pool-guide"));
	assert!(!response.contains("account-pool-window-heads"));
	assert!(response.contains("<div class=\"account-pool-guide\">"));
	assert!(response.contains("[\"account\", \"Account\"]"));
	assert!(response.contains("[\"plan\", \"Plan\"]"));
	assert!(response.contains("[\"primary\", \"5h\"]"));
	assert!(response.contains("[\"secondary\", \"7d\"]"));
	assert!(response.contains("[\"credits\", \"Credits\"]"));
	assert!(response.contains("[\"status\", \"Status\"]"));
	assert!(response.contains("ACCOUNT_POOL_SORT_COLUMNS.map(renderCodexAccountPoolSortButton).join(\"\")"));
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
	assert!(response.contains("\n\t\t\t\tdisplay: grid;\n\t\t\t\tmin-width: 760px;\n\t\t\t\tbackground: transparent;"));
	assert!(response.contains(".account-pool-guide {\n\t\t\t\tdisplay: grid;"));
	assert!(response.contains("grid-template-columns: var(--account-grid);"));
	assert!(response.contains(".account-pool-sort {\n\t\t\t\tjustify-self: center;"));
	assert!(response.contains(".account-pool-sort-icon"));
	assert!(response.contains("background: transparent;"));
	assert!(!response.contains("box-shadow: 0 8px 24px color-mix(in srgb, var(--account-accent) 7%, transparent);"));
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
	assert!(response.contains("<span class=\"account-window-label\" aria-hidden=\"true\">${escapeHtml(label)}</span>"));
	assert!(response.contains("aria-label=\"${escapeHtml(label)} remaining ${escapeHtml(remaining)}, ${escapeHtml(reset.aria)}\""));
	assert!(response.contains("title=\"${escapeHtml(resetTitle)}\""));
	assert!(response.contains("account-window-date"));
	assert!(!response.contains("<span class=\"is-reset\">Reset</span>"));
	assert!(response.contains("is-selected"));
	assert!(response.contains("is-ready"));
	assert!(response.contains("--account-accent: var(--tone-muted);"));
	assert!(response.contains("grid-template-areas:"));
	assert!(response.contains("\"id plan primary secondary credit state\""));
	assert!(response.contains("\"meta meta meta meta meta meta\""));
	assert!(!response.contains("\"account status\""));
	assert!(!response.contains("\"windows windows\""));
	assert!(response.contains(".account-row-id {\n\t\t\t\tgrid-area: id;"));
	assert!(response.contains("justify-content: center;"));
	assert!(response.contains("text-align: center;"));
	assert!(response.contains("function codexAccountPlanLabel(account)"));
	assert!(response.contains(
		"return account?.plan_type ? humanizeToken(account.plan_type) : \"not reported\";"
	));
	assert!(response.contains("const plan = codexAccountPlanLabel(account);"));
	assert!(response.contains(".account-row-plan {\n\t\t\t\tgrid-area: plan;"));
	assert!(response.contains("<div class=\"account-row-plan\">${escapeHtml(plan)}</div>"));
	assert!(!response.contains("function codexAccountSecondaryLabel(account)"));
	assert!(response.contains("const visibleName = codexAccountVisibleName(account);"));
	assert!(response.contains("const displayTitle = codexAccountDisplayTitle(account);"));
	assert!(response.contains("title=\"${escapeHtml(displayTitle)}\">${escapeHtml(visibleName)}</strong>"));
	assert!(response.contains("text.startsWith(\"...\") && text.indexOf(\"...\", 3) === -1"));
}

#[test]
fn operator_dashboard_accounts_keeps_window_status_and_credit_copy_compact() {
	let response = dashboard_response();

	assert!(response.contains("ACCOUNT_IDENTITY_MIN_EDGE_CHARS,"));
	assert!(response.contains("Math.min(ACCOUNT_IDENTITY_EDGE_CHARS, Math.floor(text.length / 2)),"));
	assert!(response.contains("return `${text.slice(0, headLength)}...${text.slice(-tailLength)}`;"));
	assert!(response.contains("grid-area: primary;"));
	assert!(response.contains("grid-area: secondary;"));
	assert!(response.contains("justify-self: stretch;"));
	assert!(!response.contains("max-width: 190px;"));
	assert!(!response.contains("max-width: 142px;"));
	assert!(response.contains("min-height: 42px;"));
	assert!(response.contains("padding: 10px 0 10px 18px;"));
	assert!(response.contains("border-bottom: 1px solid var(--line);"));
	assert!(response.contains(".account-pool-list > .account-row:last-child"));
	assert!(response.contains("account-row-credit"));
	assert!(response.contains(".account-row-credit {\n\t\t\t\tgrid-area: credit;"));
	assert!(response.contains(".account-row-credit {\n\t\t\t\tgrid-area: credit;\n\t\t\t\tjustify-self: center;"));
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
	assert!(response.contains("background: linear-gradient(90deg, var(--hover), transparent 78%);"));
	assert!(response.contains("box-shadow: 0 0 18px color-mix(in srgb, var(--account-accent) 42%, transparent);"));
	assert!(response.contains(".account-row:hover .account-window"));
	assert!(response.contains(".account-row:focus-within .account-window"));
	assert!(response.contains(".account-status::before"));
	assert!(response.contains(".account-row.is-selected .account-status"));
	assert!(response.contains(".account-row.is-ready .account-status"));
	assert!(response.contains(".account-row.is-warn .account-status"));
	assert!(response.contains(".account-row.is-danger .account-status"));
	assert!(response.contains(".account-row:hover .account-status::before"));
	assert!(response.contains(".account-row:focus-within .account-status::before"));
	assert!(!response.contains("@keyframes account-active"));
	assert!(!response.contains("account-active-glow"));
	assert!(!response.contains("account-active-sweep"));
	assert!(!response.contains("account-active-dot"));
	assert!(response.contains("aria-label=\"Account used by this lane\""));
	assert!(response.contains("<span class=\"account-use-label\">Account</span>"));
	assert!(!response.contains("<span class=\"account-use-label\">Codex account</span>"));
	assert!(response.contains("aria-label=\"Accounts\""));
	assert!(response.contains("ACCOUNT_PRIVACY_STORAGE_KEY"));
	assert!(response.contains("function codexAccountWindowData(account, prefix)"));
	assert!(response.contains("renderCodexAccountPoolWindow(account, \"primary\")"));
	assert!(response.contains("renderCodexAccountPoolWindow(account, \"secondary\")"));
	assert!(!response.contains("<div class=\"account-quota-line\">"));
	assert!(response.contains("<div class=\"account-window is-${escapeHtml(prefix)}${toneClass}\""));
	assert!(response.contains("codexAccountStatusBit(account)"));
	assert!(response.contains("renderRunCodexAccountInline(run)"));
}

#[test]
fn operator_dashboard_accounts_keeps_debug_credit_and_reset_copy_compact() {
	let response = dashboard_response();

	assert!(response.contains("field(\"Account\", codexAccountDebugSummary(account))"));
	assert!(response.contains(
		"field(\"Accounts\", codexAccountPoolDebugSummary(codexAccounts(run)))"
	));
	assert!(response.contains("field(\"Account\", codexAccountDebugSummary(codexAccount(run)))"));
	assert!(response.contains("facts.push([\"Account\", codexAccountHistorySummary(codexAccount(run))])"));
	assert!(!response.contains("facts.push([\"Codex pool\""));
	assert!(response.contains("account <strong>"));
	assert!(response.contains("credits_unlimited"));
	assert!(response.contains("function formatCodexAccountCreditsBalance(value)"));
	assert!(response.contains("const balance = formatCodexAccountCreditsBalance(account.credits_balance);"));
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
	assert!(response.contains("<strong>${escapeHtml(credits || \"not reported\")}</strong>"));

	let account_credit_index = response
		.find("<div class=\"account-row-credit${creditClass}\">")
		.expect("account credit cell render");
	let account_status_index = response
		.find("<div class=\"account-row-state\">")
		.expect("account status cell render");

	assert!(account_credit_index < account_status_index);
	assert!(response.contains("return \"0.00\";"));
	assert!(!response.contains("return \"No Credits\";"));
	assert!(response.contains("return \"Unlimited\";"));
	assert!(response.contains("return \"Ready\";"));
	assert!(response.contains("return \"not reported\";"));
	assert!(!response.contains("depleted"));
	assert!(response.contains("rate_limit_reached_type"));
	assert!(response.contains("if (normalizedStatus === \"available\")"));
	assert!(response.contains("if (codexAccountUsageLimited(account))"));
	assert!(response.contains("if (normalizedStatus.includes(\"limit\"))"));
	assert!(response.contains("return \"Limited\";"));
	assert!(response.contains("cooldown_until_unix_epoch"));
	assert!(response.contains("`${prefix}_remaining_percent`"));
	assert!(response.contains("`${prefix}_resets_at_unix_epoch`"));
	assert!(response.contains("value === 18_000"));
	assert!(response.contains("value === 604_800"));
	assert!(response.contains("function formatCodexAccountResetDuration(seconds)"));
	assert!(response.contains("function codexAccountResetDistance(value)"));
	assert!(response.contains("function codexAccountResetDisplay(data)"));
	assert!(!response.contains("const shortWindow = windowSeconds === 18_000;"));
	assert!(response.contains("return { short: \"0m\", phrase: \"reset due now\", isPast: true };"));
	assert!(response.contains("return { short, phrase: `resets in ${short}`, isPast: false };"));
	assert!(response.contains("date: \"\","));
	assert!(response.contains("date: resetAt,"));
	assert!(response.contains("aria: \"reset not reported\","));
	assert!(response.contains("reset at ${resetAt}, ${distance.phrase}"));
	assert!(response.contains("data.remainingPercent == null ? \"not reported\" : `${data.remainingPercent}%`;"));
	assert!(response.contains("aria-label=\"${escapeHtml(label)} usage not reported\""));
	assert!(response.contains("const resetTitle = `${label} ${remaining}, ${reset.aria}`;"));
	assert!(response.contains("<span class=\"account-window-reset\">${escapeHtml(reset.short)}</span>"));
	assert!(response.contains("${reset.date ? `<span class=\"account-window-date\">${escapeHtml(reset.date)}</span>` : \"\"}"));
	assert!(!response.contains("<strong>${escapeHtml(reset.main)}</strong>"));
	assert!(!response.contains("<span>${escapeHtml(reset.detail)}</span>"));
	assert!(response.contains("class=\"account-status\""));
	assert!(response.contains("function codexAccountWindowTone(percent)"));
	assert!(response.contains(".account-window.is-warn > strong"));
	assert!(response.contains(".account-window.is-danger > strong"));
	assert!(!response.contains("account-meter"));
	assert!(!response.contains("lowestRemaining}%"));
	assert!(response.contains("nodes.accountPool.innerHTML = renderCodexAccountPool(accounts)"));
	assert!(response.contains("setPanelMeta(nodes.accountPoolMeta, meta, activeCount > 0 ? \"active\" : \"\")"));
	assert!(!response.contains("nodes.accountPoolMeta.textContent = snapshot"));
	assert!(!response.contains("account-row-windows"));
	assert!(!response.contains("account-mini-window"));
	assert!(!response.contains("account-mini-label"));
	assert!(!response.contains("grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));"));
	assert!(!response.contains("grid-template-columns: minmax(170px, 1fr) minmax(360px, 1.7fr) 118px;"));
	assert!(!response.contains("border-right: 1px solid var(--line);"));
	assert!(!response.contains("box-shadow: inset 3px 0 0 var(--success)"));
	assert!(!response.contains(">Emails</span>"));
	assert!(!response.contains("[\"checked\""));
}

#[test]
fn operator_dashboard_omits_watch_and_project_pause_controls() {
	let response = dashboard_response();

	assert!(!response.contains("function dashboardSubscriptionMatches(subscription)"));
	assert!(!response.contains("function clearDashboardSubscription(shouldSend = true)"));
	assert!(!response.contains("function toggleDashboardSubscription(subscription)"));
	assert!(!response.contains("toggleDashboardSubscription({ projectId })"));
	assert!(!response.contains("toggleDashboardSubscription({ projectId, issueId, runId })"));
	assert!(!response.contains("data-dashboard-control=\"focusProject\""));
	assert!(!response.contains("data-dashboard-control=\"focusRun\""));
	assert!(!response.contains("data-dashboard-control=\"pauseProject\""));
	assert!(!response.contains("data-dashboard-control=\"resumeProject\""));
	assert!(!response.contains(">Watch</button>"));
	assert!(!response.contains(">Watching</button>"));
	assert!(!response.contains(">Pause</button>"));
	assert!(!response.contains(">Resume</button>"));
	assert!(response.contains("data-dashboard-control=\"retryRun\""));
}

#[test]
fn operator_dashboard_projects_keep_status_summary_compact() {
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_DASHBOARD_ENDPOINT_PATH
			)
			.as_bytes(),
			None,
			OperatorSnapshotReadiness::SnapshotUnavailable,
		)
		.expect("dashboard response should build"),
	)
	.expect("dashboard response should be utf-8");

	assert!(response.contains("function projectCapacitySummary(project)"));
	assert!(response.contains("function renderProjectStats(project)"));
	assert!(response.contains("function projectHasVisibleWork(project)"));
	assert!(response.contains("function activeProjects(projects)"));
	assert!(response.contains("function renderProjectEntry(project, selectedId, projects)"));
	assert!(response.contains("function renderRegisteredProjects(projects, activeProjectRows, selectedId)"));
	assert!(response.contains("function renderEmptyState(title, copy = \"\")"));
	assert!(response.contains("nodes.projectOverview.innerHTML = renderEmptyState(COPY.waitingSnapshot);"));
	assert!(response.contains("snapshot ? \"No running lanes\" : COPY.waitingSnapshot"));
	assert!(response.contains("return projects.length === 1 ? \"Current\" : \"Selected\";"));
	assert!(response.contains("return \"\";"));
	assert!(!response.contains("<h2 id=\"projects-title\">Projects</h2>"));
	assert!(!response.contains("id=\"projects-meta\""));
	assert!(!response.contains("project-panel-head"));
	assert!(!response.contains("nodes.projectsMeta"));
	assert!(response.contains("role=\"group\" aria-label=\"Projects\""));
	assert!(response.contains("const activeProjectRows = activeProjects(projects);"));
	assert!(response.contains("No active project work"));
	assert!(response.contains("Open All when you need the full registry."));
	assert!(response.contains("class=\"project-subsection\" aria-label=\"Active projects\""));
	assert!(response.contains(".project-subsection-head::after"));
	assert!(response.contains("grid-template-columns: minmax(0, 1fr) max-content 14px;"));
	assert!(response.contains("class=\"project-active-list\" role=\"list\" aria-label=\"Active projects\""));
	assert!(response.contains("<summary><span>All</span><strong>${escapeHtml(summary)}</strong></summary>"));
	assert!(response.contains("role=\"list\" aria-label=\"All projects\""));
	assert!(response.contains(".project-active-list > .project-entry:last-child"));
	assert!(response.contains(".registered-project-list > .project-entry:last-child"));
	assert!(response.contains("class=\"project-activity\""));
	assert!(response.contains("const activityCopy = lastActivity === \"none\" ? \"\" : `active ${lastActivity}`;"));
	assert!(response.contains("project.post_review_lane_count ?? 0"));
	assert!(response.contains("project.retained_worktree_count ?? 0"));
	assert!(response.contains("return pluralize(project.warning_count, \"warning\");"));
	assert!(response.contains("return `${pluralize(project.retained_worktree_count, \"worktree\")} retained`;"));
	assert!(response.contains("return { label: \"needs attention\", tone: \"tone-blocked\""));
	assert!(response.contains("label: \"sync backoff\""));
	assert!(response.contains("label: \"sync degraded\""));
	assert!(response.contains("return { label: \"ok\", tone: \"tone-land\""));
	assert!(response.contains("function projectSyncMeta(project, health)"));
	assert!(response.contains("const connectorCopy = projectSyncMeta(project, health);"));
	assert!(response.contains("if (connector === \"ok\")"));
	assert!(response.contains("return copy === health.label ? \"\" : copy;"));
	assert!(!response.contains("const prefix = `${activeCount} active · ${projects.length} all`;"));
	assert!(response.contains("return \"ok\";"));
	assert!(response.contains("${kicker ? `<span class=\"project-kicker\">${escapeHtml(kicker)}</span>` : \"\"}"));
	assert!(!response.contains("const connectorCopy = `connector ${connector}`;"));
	assert!(!response.contains("const connectorCopy = `sync ${connector}`;"));
	assert!(!response.contains("? pluralize(project.warning_count, \"warning\")"));
	assert!(!response.contains("explicitly registered"));
	assert!(!response.contains("Current registration"));
	assert!(!response.contains("Selected registration"));
	assert!(!response.contains("Registry snapshot pending"));
	assert!(!response.contains("Registered projects appear after the first operator state snapshot."));
	assert!(!response.contains("return \"Registered project\";"));
	assert!(!response.contains("Disabled registration"));
	assert!(response.contains("aria-label=\"Project status summary\""));
	assert!(response.contains("[project.active_run_count ?? 0, \"running\"]"));
	assert!(response.contains("[project.waiting_lane_count ?? 0, \"waiting\"]"));
	assert!(response.contains("[project.attention_count ?? 0, \"attention\"]"));
	assert!(response.contains("`${project.active_run_count ?? 0} running`"));
	assert!(response.contains("`${project.waiting_lane_count ?? 0} waiting`"));
	assert!(response.contains("`${project.attention_count ?? 0} attention`"));
	assert!(!response.contains("[project.post_review_lane_count ?? 0, \"review/land\"]"));
	assert!(!response.contains("[project.retained_worktree_count, \"recovery\"]"));
	assert!(!response.contains("aria-label=\"Project capacity\""));
}

#[test]
fn operator_dashboard_flow_counts_distinguish_intake_attention() {
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_DASHBOARD_ENDPOINT_PATH
			)
			.as_bytes(),
			None,
			OperatorSnapshotReadiness::SnapshotUnavailable,
		)
		.expect("dashboard response should build"),
	)
	.expect("dashboard response should be utf-8");

	assert!(response.contains("queuedCandidateNeedsAttention"));
	assert!(response.contains("intakeAttentionCount"));
	assert!(response.contains("queuedBlockedWithoutAttention"));
	assert!(response.contains("candidate.classification !== \"claimed\""));
	assert!(response.contains("queueBacklogCandidates.filter(queuedCandidateNeedsAttention).length"));
	assert!(response.contains(
		"${pluralize(derived.postReviewLanes.length, \"PR\")} · ${pluralize(derived.reviewBlockerCount, \"needs attention\", \"need attention\")}"
	));
	assert!(response.contains("${pluralize(retainedWorktrees.length, \"worktree\")} · retained or cleanup"));
	assert!(
		response.contains("Ready, capacity-limited, or blocked issues appear here before they start.")
	);
	assert!(!response.contains("claimed without local lane"));
	assert!(!response.contains("const repairCount = attentionItems.length;"));
}

#[test]
fn operator_dashboard_prioritizes_needs_attention_reason_over_retry_count() {
	let response = dashboard_response();
	let reason_text = response
		.split("function queuedCandidateReasonText(candidate)")
		.nth(1)
		.expect("queued candidate reason function should exist")
		.split("function queuedCandidateNeedsAttention(candidate)")
		.next()
		.expect("queued candidate reason function should have an end");

	assert!(reason_text.contains("return \"Needs attention\";"));
	assert!(
		response.contains("facts.push([\"Attempt status\", humanizeToken(attention.attempt_status)]);")
	);
	assert!(response.contains(
		"facts.push([\"Failed attempts\", `${attention.retry_budget_attempt_count}${retryMax}`]);"
	));
	assert!(response.contains(
		"facts.push([\"Auto retry\", autoRetryBlockedReasonText(attention.auto_retry_blocked_reason)]);"
	));
	assert!(response.contains("return \"needs-attention label set\";"));
	assert!(reason_text.contains("return \"Auto retry paused\";"));
	assert!(!response.contains("return \"blocked by needs-attention\";"));
	assert!(!reason_text.contains("return \"Retry budget held\";"));
	assert!(!response.contains(
		"facts.push([\"Retry\", String(attention.retry_budget_attempt_count)]);"
	));
	assert!(
		reason_text
			.find("return \"Needs attention\";")
			.expect("needs-attention reason should exist")
			< reason_text
				.find("return \"Auto retry paused\";")
				.expect("retry-budget reason should exist")
	);
}

#[test]
fn operator_dashboard_header_shows_endpoint_and_snapshot_freshness() {
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_DASHBOARD_ENDPOINT_PATH
			)
			.as_bytes(),
			None,
			OperatorSnapshotReadiness::SnapshotUnavailable,
		)
		.expect("dashboard response should build"),
	)
	.expect("dashboard response should be utf-8");

	assert!(response.contains("SNAPSHOT_PUBLISHED_HEADER"));
	assert!(response.contains("x-decodex-snapshot-unix-epoch"));
	assert!(!response.contains("function dashboardEndpointMeta(path)"));
	assert!(response.contains("function dashboardSocketUrl()"));
	assert!(response.contains("function snapshotPublishedAtFromResponse(response)"));
	assert!(response.contains("function snapshotAgeSeconds(snapshotPublishedAt)"));
	assert!(response.contains("function snapshotFreshnessMeta("));
	assert!(response.contains("window.location.protocol === \"https:\" ? \"wss:\" : \"ws:\""));
	assert!(response.contains("<span>transport</span>"));
	assert!(response.contains("${escapeHtml(dashboardSocketUrl())} · ${escapeHtml(stream.label)}"));
	assert!(response.contains("Poll fallback: ${escapeHtml(ENDPOINTS.state)}"));
	assert!(response.contains("<span>snapshot</span>"));
	assert!(response.contains("Dashboard WebSocket connected."));
	assert!(response.contains("const snapshotFreshnessRow = snapshotFreshness"));
	assert!(response.contains("return null;"));
	assert!(response.contains("const staleByAge = ageSeconds != null && ageSeconds >= 30;"));
	assert!(response.contains("const staleByReadiness = readiness.label === \"Snapshot stale\";"));
	assert!(response.contains("data-tone=\"${escapeHtml(snapshotFreshness.tone)}\""));
	assert!(response.contains("Published ${formatTimestamp(snapshotPublishedAt)}"));
	assert!(response.contains("formatRelativeTimestamp(snapshotPublishedAt)"));
	assert!(response.contains("snapshotPublishedAt = stateResult.value.snapshotPublishedAt"));
	assert!(
		response.contains("renderHeader(snapshot, readiness, notices, snapshotPublishedAt, snapshotError)")
	);
	assert!(response.contains(".transport-meta"));
	assert!(response.contains("max-width: min(42vw, 320px);"));
	assert!(!response.contains("Auto-refresh"));
	assert!(!response.contains("Diagnostics"));
}

#[test]
fn operator_dashboard_active_freshness_prefers_live_activity_source() {
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_DASHBOARD_ENDPOINT_PATH
			)
			.as_bytes(),
			None,
			OperatorSnapshotReadiness::SnapshotUnavailable,
		)
		.expect("dashboard response should build"),
	)
	.expect("dashboard response should be utf-8");

	assert!(response.contains("function activeRunFreshness(run)"));
	assert!(response.contains("source: \"last_run_activity_at\""));
	assert!(response.contains("source: \"none\""));
	assert!(!response.contains("source: \"updated_at\""));
	assert!(response.contains("function formatRelativeTimestamp(value)"));
	assert!(response.contains("function activeRunTelemetryFacts(run)"));
	assert!(response.contains("function renderActiveTelemetryLine(run)"));
	assert!(response.contains("activity-line"));
	assert!(
		response
			.contains("freshness.timestamp ? formatter(freshness.timestamp) : \"not captured\"")
	);
	assert!(!response.contains("Last ${freshness.sourceLabel}"));
	assert!(!response.contains("Latest ${freshness.sourceLabel}"));
	assert!(!response.contains("renderTimingStrip(run)"));
	assert!(response.contains("field(\"Freshness source\", activeRunFreshnessSource(run))"));
	assert!(
		response.contains("field(\"Lane activity\", formatTimestamp(run.last_run_activity_at))")
	);
	assert!(response.contains("field(\"Updated\", formatTimestamp(run.updated_at))"));
}

#[test]
fn operator_dashboard_uses_shared_protocol_activity_summary() {
	let response = dashboard_response();

	assert!(response.contains("function protocolActivity(run)"));
	assert!(response.contains("function protocolActivityFocus(run)"));
	assert!(response.contains("function protocolActivityRecentSummary(run)"));
	assert!(response.contains("function protocolActivityDebugSummary(run)"));
	assert!(response.contains("facts.push([\"time going to\", focus]);"));
	assert!(response.contains("return \"approval/user input\";"));
	assert!(response.contains("return \"protocol idleness\";"));
	assert!(response.contains("field(\"Protocol activity\", protocolActivityDebugSummary(run))"));
	assert!(response.contains("field(\"Rate limit\", protocolActivityRateLimit(run))"));
}
