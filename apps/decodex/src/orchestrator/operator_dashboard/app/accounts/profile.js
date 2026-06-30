				function renderCodexAccountActivityStrip(
					account,
					labelPrefix,
					stripClass,
					tileClass,
				) {
					const records = codexAccountProfileDailyUsage(account).slice(-35);
					if (!records.length) {
						return "";
					}
					const peak = records.reduce((max, record) => Math.max(max, record.tokens), 0);
					const title = `${labelPrefix}: ${records.length} days, peak ${formatCompactCount(peak)} tokens`;

					return `
						<div class="${escapeHtml(stripClass)}" aria-label="${escapeHtml(title)}" title="${escapeHtml(title)}">
							${records
								.map((record) => {
									const ratio = peak > 0 ? record.tokens / peak : 0;
									const opacity = record.tokens > 0 ? 0.24 + ratio * 0.76 : 0.12;
									const tileTitle = `${record.date}: ${formatCompactCount(record.tokens)} tokens`;
									return `<span class="${escapeHtml(tileClass)}" style="--account-activity-opacity: ${opacity.toFixed(2)}" title="${escapeHtml(tileTitle)}" aria-hidden="true"></span>`;
								})
								.join("")}
						</div>
					`;
				}

				function renderCodexAccountPoolActivityStrip(account) {
					return renderCodexAccountActivityStrip(
						account,
						"Pool token activity",
						"account-pool-activity-strip",
						"account-pool-activity-tile",
					);
				}

				function renderCodexAccountProfileActivityStrip(account) {
					return renderCodexAccountActivityStrip(
						account,
						"Account token activity",
						"account-profile-activity-strip",
						"account-profile-activity-tile",
					);
				}

				function codexAccountProfileExpansionKey(account) {
					return codexAccountRandomNameKey(account);
				}

				function codexAccountHasProfileDetails(account) {
					return (
						codexAccountProfileMetaFacts(account).length > 0 ||
						codexAccountProfileDailyUsage(account).length > 0
					);
				}

				function codexAccountProfileExpanded(account) {
					const key = codexAccountProfileExpansionKey(account);
					return Boolean(key && expandedAccountProfileKeys.has(key));
				}

				function toggleCodexAccountProfileKey(key) {
					if (!key) {
						return false;
					}

					expandedAccountProfileKeys = new Set(expandedAccountProfileKeys);
					if (expandedAccountProfileKeys.has(key)) {
						expandedAccountProfileKeys.delete(key);
					} else {
						expandedAccountProfileKeys.add(key);
					}
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
					}

					return true;
				}

				function accountProfileRowClickIsSuppressed(target) {
					return Boolean(
						target.closest(
							[
								"button",
								"a",
								"input",
								"select",
								"textarea",
								"summary",
								"details",
								"[contenteditable='true']",
								"[data-account-sort-key]",
								"[data-account-privacy-toggle]",
								"[data-account-confirm-action]",
								"[data-account-name-reroll]",
								"[data-account-profile-toggle]",
							].join(","),
						),
					);
				}

				function codexAccountResetDistance(value) {
					const seconds = codexAccountNumber(value);
					if (seconds == null || seconds <= 0) {
						return { short: "unknown", phrase: "remaining unknown", isPast: false };
					}

					const resetAt = new Date(seconds * 1000);
					if (Number.isNaN(resetAt.getTime())) {
						return { short: "unknown", phrase: "remaining unknown", isPast: false };
					}

					const distanceSeconds = Math.floor((resetAt.getTime() - Date.now()) / 1000);
					if (distanceSeconds <= 0) {
						return { short: "0m", phrase: "reset due now", isPast: true };
					}

					const short = formatCodexAccountResetDuration(distanceSeconds);
					return { short, phrase: `resets in ${short}`, isPast: false };
				}

				function codexAccountResetDisplay(data) {
					const resetAt = codexAccountUnixTimestamp(data.resetAt);
					const distance = codexAccountResetDistance(data.resetAt);
					if (resetAt === "unknown" && distance.short === "unknown") {
						return {
							short: "-",
							date: "",
							aria: "reset unavailable",
						};
					}

					return {
						short: distance.short,
						date: resetAt,
						aria:
							distance.short === "unknown"
								? `reset at ${resetAt}, remaining unknown`
								: `reset at ${resetAt}, ${distance.phrase}`,
					};
				}

				function codexAccountWindowLabel(seconds) {
					const value = codexAccountNumber(seconds);
					if (value == null) {
						return "window";
					}
					if (value === 18_000) {
						return "5h";
					}
					if (value === 604_800) {
						return "7d";
					}

					return formatDuration(value);
				}

				function codexAccountWindowData(account, prefix) {
					return {
						windowSeconds: account?.[`${prefix}_window_seconds`],
						remainingPercent: codexAccountPercent(account?.[`${prefix}_remaining_percent`]),
						resetAt: account?.[`${prefix}_resets_at_unix_epoch`],
					};
				}

				function codexAccountWindowTone(percent) {
					if (percent == null) {
						return "";
					}
					if (percent <= 10) {
						return "danger";
					}
					if (percent <= 25) {
						return "warn";
					}

					return "";
				}

				function codexAccountReachedType(account) {
					const reached = String(account?.rate_limit_reached_type || "").toLowerCase();
					return reached && reached !== "none" ? reached : "";
				}

				function codexAccountUsageLimited(account) {
					if (!account) {
						return false;
					}

					const status = String(account.status || "").toLowerCase();
					const primary = codexAccountWindowData(account, "primary");
					const secondary = codexAccountWindowData(account, "secondary");

					return Boolean(
						codexAccountReachedType(account) ||
							status.includes("limit") ||
							primary.remainingPercent === 0 ||
							secondary.remainingPercent === 0,
					);
				}

				function codexAccountStatusTone(account) {
					if (!account) {
						return "";
					}

					const status = String(account.status || "").toLowerCase();
					const refresh = String(account.refresh_status || "").toLowerCase();
					const refreshNeedsAttention = codexAccountRefreshStatusNeedsAttention(refresh);

					if (
						codexAccountUsageLimited(account) ||
						status.includes("failed") ||
						status.includes("unusable") ||
						refresh.includes("failed")
					) {
						return "danger";
					}
					if (refreshNeedsAttention) {
						return "warn";
					}
					if (status === "available") {
						return "ready";
					}

					return "";
				}

				function codexAccountStatusLabel(account) {
					if (!account) {
						return "none";
					}

					const reached = codexAccountReachedType(account);
					const status = reached || account.status || "selected";
					const refresh = String(account.refresh_status || "").toLowerCase();
					if (codexAccountUsageLimited(account)) {
						return reached || (String(status).trim() && status !== "available" ? status : "usage_limited");
					}
					if (refresh.includes("failed")) {
						return refresh;
					}
					if (refresh && !["not_needed", "refreshed", "succeeded", "none"].includes(refresh)) {
						return codexAccountTokenValue(account.refresh_status);
					}

					return displayToken(status);
				}

				function codexAccountCreditsSummary(account) {
					if (!account) {
						return null;
					}
					const balance = formatCodexAccountCreditsBalance(account.credits_balance);
					if (account.credits_unlimited === true) {
						return "Unlimited";
					}
					if (account.credits_has_credits === false) {
						return "0.00";
					}
					if (balance) {
						return balance;
					}
					if (account.credits_has_credits === true) {
						return "-";
					}

					return null;
				}

				function formatCodexAccountCreditsBalance(value) {
					if (value == null) {
						return "";
					}
					const raw = String(value).trim();
					if (!raw) {
						return "";
					}
					const number = Number(raw);
					if (!Number.isFinite(number)) {
						return raw;
					}
					return number.toFixed(2);
				}

			function codexAccountCreditsTone(account) {
				if (!account) {
					return "";
				}
				if (codexAccountReachedType(account).includes("credit")) {
					return "danger";
				}

					return "";
				}

				function codexAccountWindowSummary(account, prefix) {
					const data = codexAccountWindowData(account, prefix);
					const label = codexAccountWindowLabel(data.windowSeconds);
					const remaining = data.remainingPercent == null ? "unknown" : `${data.remainingPercent}%`;
					const reset = codexAccountUnixTimestamp(data.resetAt);

					return `${label} ${remaining} reset ${reset}`;
				}

				function codexAccountMetaFacts(account) {
					if (!account) {
						return [];
					}
					const refreshStatus = String(account.refresh_status || "").toLowerCase();
					const note = codexAccountNote(account);
					const noteLooksRoutine = codexAccountNoteLooksRoutine(note);
					const noteLooksError = codexAccountNoteLooksError(note);

					const facts = [
						codexAccountRefreshStatusNeedsAttention(refreshStatus) &&
							!codexAccountRefreshFailed(account)
							? ["token", codexAccountTokenValue(account.refresh_status)]
							: null,
						account.cooldown_until_unix_epoch
							? ["cooldown", codexAccountUnixTimestamp(account.cooldown_until_unix_epoch)]
							: null,
						note && !noteLooksRoutine && !noteLooksError
							? ["note", codexAccountPrivacyText(account, note)]
							: null,
					];

					return facts.filter(Boolean);
				}

				function renderCodexAccountPoolWindow(account, prefix) {
					const data = codexAccountWindowData(account, prefix);
					const label = codexAccountWindowLabel(data.windowSeconds);
					const remaining =
						data.remainingPercent == null ? "-" : `${data.remainingPercent}%`;
					const reset = codexAccountResetDisplay(data);
					const windowTone = codexAccountWindowTone(data.remainingPercent);
					const toneClass = windowTone ? ` is-${windowTone}` : "";
					const isUnreported = data.remainingPercent == null && reset.short === "-";
					const resetTitle = `${label} ${remaining}, ${reset.aria}`;

					if (isUnreported) {
						return `
							<div class="account-window is-${escapeHtml(prefix)}${toneClass}" aria-label="${escapeHtml(label)} usage unavailable" title="${escapeHtml(resetTitle)}">
								<span class="account-window-label" aria-hidden="true">${escapeHtml(label)}</span>
								<strong>-</strong>
							</div>
						`;
					}

					return `
						<div class="account-window is-${escapeHtml(prefix)}${toneClass}" aria-label="${escapeHtml(label)} remaining ${escapeHtml(remaining)}, ${escapeHtml(reset.aria)}" title="${escapeHtml(resetTitle)}">
							<span class="account-window-label" aria-hidden="true">${escapeHtml(label)}</span>
							<strong>${escapeHtml(remaining)}</strong>
							<span class="account-window-reset">${escapeHtml(reset.short)}</span>
							${reset.date ? `<span class="account-window-date">${escapeHtml(reset.date)}</span>` : ""}
						</div>
					`;
				}

				function renderCodexAccountProfileToggle(account, expanded) {
