					const key = codexAccountProfileExpansionKey(account);
					const label = expanded ? "Hide account profile" : "Show account profile";
					if (!key) {
						return "";
					}

					return `
						<button class="account-profile-toggle" type="button" data-account-profile-toggle="${escapeHtml(key)}" aria-expanded="${expanded ? "true" : "false"}" aria-label="${escapeHtml(label)}" title="${escapeHtml(label)}">
							<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">
								<path d="m6 9 6 6 6-6"></path>
							</svg>
						</button>
					`;
				}

				function renderCodexAccountPoolRow(account, snapshot, isLastAccount = false) {
					const weight = codexAccountCapacityLabel(account);
					const statusTone = codexAccountStatusTone(account);
					const toneClass = statusTone ? ` is-${statusTone}` : "";
					const fixed = codexAccountMatchesConfiguredSelector(account, snapshot);
					const fixedClass = fixed ? " is-fixed" : "";
					const selector = codexAccountControlSelector(account);
					const action = fixed ? "clearAccountSelection" : "selectAccount";
					const armedClass =
						selector && accountSelectionConfirmationMatches(action, selector) ? " is-armed" : "";
					const selectedClass =
						String(account.status || "").toLowerCase() === "selected" ? " is-selected" : "";
					const metaFacts = codexAccountMetaFacts(account);
					const credits = codexAccountCreditsSummary(account);
					const creditTone = codexAccountCreditsTone(account);
					const creditClass = creditTone ? ` is-${creditTone}` : "";
					const identityClass = codexAccountShowsEmail(account) ? " is-machine" : "";
					const profileKey = codexAccountProfileExpansionKey(account);
					const hasProfileDetails = codexAccountHasProfileDetails(account);
					const profileExpanded = hasProfileDetails && codexAccountProfileExpanded(account);
					const profileOpenClass = profileExpanded ? " is-profile-open" : "";
					const profileToggleableClass = hasProfileDetails ? " is-profile-toggleable" : "";
					const lastAccountClass = isLastAccount ? " is-last-account" : "";
					const profileRowToggleAttribute = hasProfileDetails
						? ` data-account-profile-row-toggle="${escapeHtml(profileKey)}"`
						: "";

					return `
							<div class="account-row${selectedClass}${fixedClass}${armedClass}${toneClass}${profileOpenClass}${profileToggleableClass}${lastAccountClass}" data-render-key="account-row:${escapeHtml(profileKey)}"${profileRowToggleAttribute}>
								<div class="account-row-id${identityClass}">
									${renderCodexAccountNameControl(account, snapshot)}
									${renderCodexAccountRandomNameButton(account)}
								</div>
								<div class="account-row-plan">${escapeHtml(weight)}</div>
								${renderCodexAccountPoolWindow(account, "primary")}
								${renderCodexAccountPoolWindow(account, "secondary")}
								<div class="account-row-credit${creditClass}">
									<span>credits</span>
									<strong>${escapeHtml(credits || "-")}</strong>
								</div>
								<div class="account-row-state">
									<strong class="account-status">${escapeHtml(codexAccountStatusLabel(account))}</strong>
								</div>
								${
									metaFacts.length
										? `<div class="account-meta account-row-meta">
												${metaFacts
													.map(
														([label, value]) =>
															`<span>${escapeHtml(label)} <strong>${escapeHtml(value)}</strong></span>`,
													)
													.join("")}
											</div>`
										: ""
								}
								${hasProfileDetails ? renderCodexAccountProfileToggle(account, profileExpanded) : ""}
							</div>
							${hasProfileDetails ? renderCodexAccountProfilePanel(account, snapshot, profileKey, profileExpanded) : ""}
							`;
				}

				function codexAccountProfileFactMap(account) {
					return new Map(codexAccountProfileMetaFacts(account));
				}

				function renderCodexAccountProfilePanel(account, snapshot, profileKey, expanded) {
					const facts = codexAccountProfileFactMap(account);
					const activity = renderCodexAccountProfileActivityStrip(account);
					if (!facts.size && !activity) {
						return "";
					}

					const statusTone = codexAccountStatusTone(account);
					const toneClass = statusTone ? ` is-${statusTone}` : "";
					const fixedClass = codexAccountMatchesConfiguredSelector(account, snapshot)
						? " is-fixed"
						: "";
					const openClass = expanded ? " is-open" : "";
					const selectedClass =
						String(account.status || "").toLowerCase() === "selected" ? " is-selected" : "";
					const title = [
						codexAccountDisplayTitle(account),
						...Array.from(facts, ([label, value]) => `${label} ${value}`),
					]
						.filter(Boolean)
						.join(" · ");
					const metricFacts = [
						["Lifetime", facts.get("tok") || "-"],
						["Peak day", facts.get("peak") || "-"],
						["Streak", facts.get("streak") || "-"],
						["Longest task", facts.get("task") || "-"],
					];

					return `
						<div class="account-profile-panel${selectedClass}${fixedClass}${toneClass}${openClass}" data-render-key="account-profile-panel:${escapeHtml(profileKey)}" aria-hidden="${expanded ? "false" : "true"}" title="${escapeHtml(title)}">
							<div class="account-profile-panel-inner">
								${metricFacts
									.map(
										([label, value]) => `
											<div class="account-profile-fact">
												<span>${escapeHtml(label)}</span>
												<strong>${escapeHtml(value)}</strong>
											</div>
										`,
									)
									.join("")}
								<div class="account-profile-activity">
									<span class="account-profile-activity-label">Activity</span>
									${activity || '<strong class="account-profile-empty">-</strong>'}
								</div>
							</div>
						</div>
					`;
				}

				function renderCodexAccountPoolSortButton([key, label]) {
					const direction = accountPoolSort.key === key ? accountPoolSort.direction : "";
					const activeClass = direction ? ` is-active is-${direction}` : "";
					const current = direction
						? `currently ${direction === "asc" ? "ascending" : "descending"}`
						: "not sorted";

					return `
						<button class="account-pool-sort${activeClass}" type="button" data-account-sort-key="${escapeHtml(key)}" aria-label="Sort accounts by ${escapeHtml(label)}; ${escapeHtml(current)}" title="Sort by ${escapeHtml(label)}">
							<span class="account-pool-sort-label">${escapeHtml(label)}</span>
							<svg class="account-pool-sort-icon" aria-hidden="true" viewBox="0 0 8 12" fill="currentColor">
								<path class="account-sort-up" d="M4 1 1 5h6Z"></path>
								<path class="account-sort-down" d="M4 11 1 7h6Z"></path>
							</svg>
						</button>
					`;
				}

				function renderCodexAccountPoolGuideCell(column) {
					const [key] = column;
					const sortButton = renderCodexAccountPoolSortButton(column);
					if (key !== "account") {
						return sortButton;
					}

					return `<span class="account-pool-heading">${sortButton}${accountPrivacyToggleMarkup()}</span>`;
				}

				function codexAccountControlStatusLabel(snapshot) {
					const accountControl = snapshot?.account_control || {};
					const selector = String(accountControl.account_selector || "").trim();
					if (!selector) {
						return "Balanced";
					}

					const account = codexAccountPoolAccounts(snapshot).find(
						(candidate) =>
							selector === codexAccountEmail(candidate) ||
							selector === codexAccountFingerprint(candidate),
					);
					if (account) {
						const label = codexAccountShowsEmail(account)
							? compactAccountIdentity(selector)
							: codexAccountVisibleName(account);
						return `Fixed · ${label}`;
					}

					return `Fixed · ${codexAccountFallbackName(selector)}`;
				}

				function renderAccountModeControl(snapshot) {
					const title = codexAccountControlStatusLabel(snapshot);
					nodes.accountModeMeta.innerHTML = `<span class="account-mode-head">${escapeHtml(title)}</span>`;
					nodes.accountModeMeta.title = title;
				}

				function renderCodexAccountPoolUsageSummary(accounts) {
					const estimate = accountPoolUsageEstimate();
					if (!estimate) {
						return "";
					}

					const used = codexAccountNumber(estimate.total_used_of_capacity_percent);
					const delta = accountPoolDayDeltaPercentagePoints(accounts, estimate);
					const metrics = [
						{
							label: "Pool used",
							value: formatUsagePercent(used),
							tone: accountPoolUsageTone(used),
						},
						{
							label: "Day Δ",
							value: delta == null ? "-" : formatPercentagePointDelta(delta),
							tone: accountPoolDayDeltaTone(delta, used),
						},
						{
							label: "Daily avg",
							value: formatDailyUsageRate(estimate.average_daily_pool_percent),
							tone: "muted",
						},
					];
					const profileAggregate = codexAccountProfileAggregate(accounts);
					const profileLabel = new Map([
						["tok", "Lifetime tok"],
						["peak", "Peak day"],
						["streak", "Streak"],
						["task", "Longest task"],
					]);
					const profileMetrics = profileAggregate
						? codexAccountProfileMetaFacts(profileAggregate).map(([key, value]) => ({
								label: profileLabel.get(key) || key,
								value,
								tone: key === "tok" ? "run" : "muted",
							}))
						: [];
					metrics.push(...profileMetrics);
					const measured = Number(estimate.account_estimate_count || 0);
					const accountCount = Number(estimate.account_count || 0);
					const note =
						accountCount > 0 && measured > 0 && measured < accountCount
							? `<span class="account-pool-summary-note">${escapeHtml(`${measured}/${accountCount} accounts measured`)}</span>`
							: "";
					const activityStrip = profileAggregate
						? renderCodexAccountPoolActivityStrip(profileAggregate)
						: "";
					if (activityStrip) {
						metrics.push({
							label: "Activity",
							value: "token history",
							tone: "muted",
							valueHtml: activityStrip,
						});
					}
					const label = `Pool usage: ${metrics.map((metric) => `${metric.label} ${metric.value}`).join(", ")}`;

					return `
						<div class="account-pool-summary" data-render-key="account-pool-summary" aria-label="${escapeHtml(label)}">
							${metrics
								.map(
									(metric) => `
										<div class="account-pool-metric">
											<span class="account-pool-metric-label">${escapeHtml(metric.label)}</span>
											${
												metric.valueHtml
													? metric.valueHtml
													: `<strong class="account-pool-metric-value" data-tone="${escapeHtml(metric.tone)}">${escapeHtml(metric.value)}</strong>`
											}
										</div>
									`,
								)
								.join("")}
							${note}
						</div>
					`;
				}

				function renderCodexAccountPool(accounts, snapshot) {
					if (!accounts.length) {
						return "";
					}

					return `
						${renderCodexAccountPoolUsageSummary(accounts)}
						<div class="account-pool-list" data-render-key="account-pool-list" aria-label="Accounts">
							<div class="account-pool-guide">
								${ACCOUNT_POOL_SORT_COLUMNS.map(renderCodexAccountPoolGuideCell).join("")}
							</div>
							${accounts.map((account, index) => renderCodexAccountPoolRow(account, snapshot, index === accounts.length - 1)).join("")}
						</div>
					`;
				}

				function renderRunCodexAccountInline(run, snapshot) {
					const capturedAccount = codexAccount(run);
					const account = capturedAccount || selectedDashboardAccount(snapshot);
					if (!account) {
						return `
							<span class="run-meta-item is-account is-missing" aria-label="account">
								<span class="run-meta-icon" aria-hidden="true">
									<svg viewBox="0 0 16 16" fill="none">
										<path fill="currentColor" fill-rule="evenodd" clip-rule="evenodd" d="M3.35 2.25h9.3c.61 0 1.1.49 1.1 1.1v9.3c0 .61-.49 1.1-1.1 1.1h-9.3c-.61 0-1.1-.49-1.1-1.1v-9.3c0-.61.49-1.1 1.1-1.1ZM8 4.15a1.8 1.8 0 1 1 0 3.6 1.8 1.8 0 0 1 0-3.6Zm0 4.78c2.02 0 3.26.96 3.7 2.78.08.35-.18.67-.54.67H4.84c-.36 0-.62-.32-.54-.67.44-1.82 1.68-2.78 3.7-2.78Z"></path>
									</svg>
								</span>
								<strong>not captured</strong>
							</span>
						`;
					}

					const displayAccount = codexAccountDisplaySource(account, snapshot);
					const displayTitle = codexAccountDisplayTitle(displayAccount);
					const visibleName = codexAccountVisibleName(displayAccount);
					const identityClass = codexAccountShowsEmail(displayAccount) ? " is-machine" : "";
					const pendingTitle = capturedAccount
						? displayTitle
						: `${displayTitle || visibleName} · run account capture pending`;

					return `
						<span class="run-meta-item is-account" aria-label="account">
							<span class="run-meta-icon" aria-hidden="true">
								<svg viewBox="0 0 16 16" fill="none">
									<path fill="currentColor" fill-rule="evenodd" clip-rule="evenodd" d="M3.35 2.25h9.3c.61 0 1.1.49 1.1 1.1v9.3c0 .61-.49 1.1-1.1 1.1h-9.3c-.61 0-1.1-.49-1.1-1.1v-9.3c0-.61.49-1.1 1.1-1.1ZM8 4.15a1.8 1.8 0 1 1 0 3.6 1.8 1.8 0 0 1 0-3.6Zm0 4.78c2.02 0 3.26.96 3.7 2.78.08.35-.18.67-.54.67H4.84c-.36 0-.62-.32-.54-.67.44-1.82 1.68-2.78 3.7-2.78Z"></path>
								</svg>
							</span>
							<strong class="account-name${identityClass}" title="${escapeHtml(pendingTitle)}">${escapeHtml(visibleName)}</strong>
						</span>
					`;
				}

				function renderRunMetaLine(run, snapshot = null) {
					const items = [
						renderRunCodexAccountInline(run, snapshot),
						renderRunTelemetryMetaItems(run),
					]
						.filter(Boolean)
						.join("");

					if (!items) {
						return "";
					}

					return `<div class="run-meta-line" aria-label="Lane metadata">${items}</div>`;
				}

				function renderCodexAccountBand(run) {
					return renderRunMetaLine(run);
				}

				function codexAccountPoolAccounts() {
					return sortCodexAccountPoolAccounts(
						accountApiAccounts().map((account) => ({ ...account })),
					);
				}

				function codexAccountCreditsSortValue(account) {
					if (!account) {
						return null;
					}
					if (account.credits_unlimited === true) {
						return Number.POSITIVE_INFINITY;
					}
					if (account.credits_has_credits === false) {
						return 0;
					}

					return codexAccountNumber(account.credits_balance);
				}

				function codexAccountPoolColumnSortValue(account, key) {
					if (key === "account") {
						return codexAccountDisplayName(account).toLowerCase();
					}
					if (key === "plan") {
						return codexAccountCapacityMultiplier(account);
					}
					if (key === "primary") {
						return codexAccountWindowData(account, "primary").remainingPercent;
					}
					if (key === "secondary") {
						return codexAccountWindowData(account, "secondary").remainingPercent;
					}
					if (key === "credits") {
						return codexAccountCreditsSortValue(account);
					}
					if (key === "status") {
						return codexAccountStatusLabel(account).toLowerCase();
					}

					return "";
				}

				function compareCodexAccountPoolColumn(left, right, key, direction) {
					const leftValue = codexAccountPoolColumnSortValue(left, key);
					const rightValue = codexAccountPoolColumnSortValue(right, key);
					const leftMissing = leftValue == null || leftValue === "";
					const rightMissing = rightValue == null || rightValue === "";
					if (leftMissing && rightMissing) {
						return 0;
					}
					if (leftMissing) {
						return 1;
					}
					if (rightMissing) {
						return -1;
					}

					const delta =
						typeof leftValue === "number" && typeof rightValue === "number"
							? leftValue === rightValue
								? 0
								: leftValue < rightValue
									? -1
									: 1
							: String(leftValue).localeCompare(String(rightValue));

					return direction === "desc" ? -delta : delta;
				}

				function sortCodexAccountPoolAccounts(accounts) {
					if (!accountPoolSort.key) {
						return accounts;
					}

					return accounts.sort((left, right) => {
						const columnDelta = compareCodexAccountPoolColumn(
							left,
							right,
							accountPoolSort.key,
							accountPoolSort.direction,
						);
						if (columnDelta) {
							return columnDelta;
						}

						return 0;
					});
				}

				function renderAccountPool(snapshot) {
					const accounts = codexAccountPoolAccounts(snapshot);
					const visibleProfileKeys = new Set(accounts.map(codexAccountProfileExpansionKey));
					expandedAccountProfileKeys = new Set(
						[...expandedAccountProfileKeys].filter((key) => visibleProfileKeys.has(key)),
					);
					renderAccountModeControl(snapshot);
					renderStableList(nodes.accountPool, renderCodexAccountPool(accounts, snapshot));
					syncAccountSelectionConfirmationDom();
					renderAccountPrivacyToggle();
				}

				function codexAccountDebugSummary(account) {
					if (!account) {
						return "not captured";
					}

					return codexAccountDisplayName(account);
				}

				function codexAccountHistorySummary(account) {
					if (!account) {
						return "none";
					}

					return [
						codexAccountDisplayName(account),
						codexAccountStatusLabel(account),
						codexAccountWindowSummary(account, "primary"),
					].join("; ");
				}
