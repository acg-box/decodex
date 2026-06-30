
			function codexAccount(run, snapshot = null) {
				const selected = run?.account || run?.codex_account || null;
				if (selected) {
					return selected;
				}

				const accounts = codexAccounts(run);

				return (
					accounts.find((account) => {
						const status = String(account?.status || "").toLowerCase();
						return status === "selected";
					}) ||
					accounts[0] ||
					selectedDashboardAccount(snapshot)
				);
			}

			function selectedDashboardAccount(snapshot) {
				if (!snapshot) {
					return null;
				}

				const accounts = accountApiAccounts();
				if (!accounts.length) {
					return null;
				}

				const selector = codexAccountConfiguredSelector(snapshot);
				if (selector) {
					const fixed = accounts.find(
						(account) =>
							selector === codexAccountEmail(account) ||
							selector === codexAccountFingerprint(account),
					);
					if (fixed) {
						return fixed;
					}
				}

				return (
					accounts.find((account) => {
						const status = String(account?.status || "").toLowerCase();
						return status === "selected";
					}) || null
				);
			}

			function codexAccounts(run) {
				const accounts = Array.isArray(run?.accounts)
					? run.accounts.filter(Boolean)
					: Array.isArray(run?.codex_accounts)
						? run.codex_accounts.filter(Boolean)
						: [];
				const selected = run?.account || run?.codex_account || null;

				if (!selected) {
					return accounts;
				}
				if (
					accounts.some(
						(account) => codexAccountIdentity(account) === codexAccountIdentity(selected),
					)
				) {
					return accounts;
				}

				return [selected, ...accounts];
			}

				function codexAccountFingerprint(account) {
					return String(account?.account_fingerprint || "").trim();
				}

				function codexAccountIdentity(account) {
					const fingerprint = codexAccountFingerprint(account);
					if (fingerprint) {
						return fingerprint;
					}

					const email = codexAccountEmail(account);
					if (email) {
						return email;
					}

					return account?.plan_type || "";
				}

				function codexAccountControlSelector(account) {
					return codexAccountEmail(account) || codexAccountFingerprint(account);
				}

				function codexAccountConfiguredSelector(snapshot) {
					const accountControl = snapshot?.account_control || {};

					return String(accountControl.account_selector || "").trim();
				}

				function codexAccountMatchesConfiguredSelector(account, snapshot) {
					const selector = codexAccountConfiguredSelector(snapshot);
					if (!selector) {
						return false;
					}

					return (
						selector === codexAccountEmail(account) ||
						selector === codexAccountFingerprint(account)
					);
				}

				function configuredCodexAccountFor(account, snapshot) {
					const identity = codexAccountIdentity(account);
					const email = codexAccountEmail(account);
					const fingerprint = codexAccountFingerprint(account);
					if (!identity && !email && !fingerprint) {
						return null;
					}

					return (
						accountApiAccounts().find(
							(candidate) =>
								(identity && codexAccountIdentity(candidate) === identity) ||
								(email && codexAccountEmail(candidate) === email) ||
								(fingerprint && codexAccountFingerprint(candidate) === fingerprint),
						) || null
					);
				}

				function codexAccountDisplaySource(account, snapshot) {
					const configured = configuredCodexAccountFor(account, snapshot);
					if (!configured) {
						return account;
					}

					const merged = { ...configured, ...account };
					if (!String(merged.random_name || "").trim()) {
						merged.random_name = configured.random_name;
					}
					if (!String(merged.random_name_key || "").trim()) {
						merged.random_name_key = configured.random_name_key;
					}
					const accountOffset = account?.random_name_offset;
					const accountHasOffset =
						accountOffset != null &&
						!(typeof accountOffset === "string" && !accountOffset.trim()) &&
						Number.isInteger(Number(accountOffset));
					if (
						!accountHasOffset &&
						Number.isInteger(Number(configured.random_name_offset))
					) {
						merged.random_name_offset = configured.random_name_offset;
					}

					return merged;
				}

				function accountSelectionConfirmationKey(action, selector) {
					return `${String(action || "").trim()}:${String(selector || "").trim()}`;
				}

				function accountSelectionConfirmationMatches(action, selector) {
					if (!accountSelectionConfirmation) {
						return false;
					}

					return (
						accountSelectionConfirmation.key ===
						accountSelectionConfirmationKey(action, selector)
					);
				}

				function accountSelectionControlTitle(action, displayTitle, armed) {
					const prefix =
						action === "clearAccountSelection"
							? armed
								? "Click again to return the global account pool to balanced selection"
								: "Click once, then again to return the global account pool to balanced selection"
							: armed
								? "Click again to use this account for new global runs"
								: "Click once, then again to use this account for new global runs";

					return displayTitle ? `${prefix}: ${displayTitle}` : prefix;
				}

				function syncAccountSelectionConfirmationDom() {
					for (const button of nodes.accountPool.querySelectorAll("[data-account-confirm-action]")) {
						const action = button.dataset.accountConfirmAction;
						const selector = button.dataset.accountSelector || "";
						const armed = accountSelectionConfirmationMatches(action, selector);
						const row = button.closest(".account-row");
						const title = accountSelectionControlTitle(
							action,
							button.dataset.accountDisplayTitle || "",
							armed,
						);

						button.classList.toggle("is-armed", armed);
						button.setAttribute("aria-label", title);
						button.setAttribute("title", title);
						if (row) {
							row.classList.toggle("is-armed", armed);
						}
					}
				}

				function clearAccountSelectionConfirmation(syncDom = true) {
					accountSelectionConfirmation = null;
					if (syncDom) {
						syncAccountSelectionConfirmationDom();
					}
				}

				function armAccountSelectionConfirmation(action, selector) {
					accountSelectionConfirmation = {
						key: accountSelectionConfirmationKey(action, selector),
						action,
						selector,
					};
					syncAccountSelectionConfirmationDom();
				}

				function confirmAccountSelection(action, selector) {
					clearAccountSelectionConfirmation(false);
					if (action === "selectAccount") {
						sendDashboardControl(action, { accountSelector: selector });
					} else if (action === "clearAccountSelection") {
						sendDashboardControl(action);
					}
					syncAccountSelectionConfirmationDom();
				}

				function handleAccountSelectionConfirmation(action, selector) {
					if (!selector || !["selectAccount", "clearAccountSelection"].includes(action)) {
						return;
					}

					if (accountSelectionConfirmationMatches(action, selector)) {
						confirmAccountSelection(action, selector);
						return;
					}

					armAccountSelectionConfirmation(action, selector);
				}

				function codexAccountEmail(account) {
					return String(account?.account_email || account?.email || "").trim();
				}

				function compactAccountEmail(email) {
					const text = String(email || "").trim();
					const atIndex = text.indexOf("@");
					if (atIndex <= 0) {
						return compactAccountIdentity(text);
					}

					const local = text.slice(0, atIndex);
					const domain = text.slice(atIndex);
					if (local.length <= 6) {
						return `${local}${domain}`;
					}

					return `${local.slice(0, 3)}...${local.slice(-3)}${domain}`;
				}

				function trimLeadingEllipsis(value) {
					const text = String(value || "").trim();
					if (text.startsWith("...") && text.indexOf("...", 3) === -1) {
						return text.slice(3);
					}

					return text;
				}

				function compactAccountIdentity(value) {
					const text = trimLeadingEllipsis(value);
					if (!text || text === "unknown") {
						return text;
					}

					const edgeLength = Math.max(
						ACCOUNT_IDENTITY_MIN_EDGE_CHARS,
						Math.min(ACCOUNT_IDENTITY_EDGE_CHARS, Math.floor(text.length / 2)),
					);
					const headLength = edgeLength;
					const tailLength = edgeLength;
					return `${text.slice(0, headLength)}...${text.slice(-tailLength)}`;
				}

				function codexAccountIdentityHash(value) {
					const text = String(value || "account");
					let hash = 2_166_136_261;
					for (let index = 0; index < text.length; index += 1) {
						hash ^= text.charCodeAt(index);
						hash = Math.imul(hash, 16_777_619);
					}

					return hash >>> 0;
				}

				function codexAccountRandomNameKey(account) {
					const serverKey = String(account?.random_name_key || "").trim();
					if (serverKey) {
						return serverKey;
					}

					const identity =
						codexAccountIdentity(account) ||
						codexAccountEmail(account) ||
						account?.plan_type ||
						"account";

					return codexAccountIdentityHash(identity).toString(16).padStart(8, "0");
				}

				function codexAccountPendingRandomNameOffset(account) {
					const key = codexAccountRandomNameKey(account);
					if (!Object.prototype.hasOwnProperty.call(pendingAccountNameOffsets, key)) {
						return null;
					}

					return normalizeAccountNameOffset(pendingAccountNameOffsets[key]);
				}

				function codexAccountServerRandomNameOffset(account) {
					const value = Number(account?.random_name_offset);

					return Number.isInteger(value) ? normalizeAccountNameOffset(value) : null;
				}

				function codexAccountRandomNameOffset(account) {
					return (
						codexAccountPendingRandomNameOffset(account) ??
						codexAccountServerRandomNameOffset(account) ??
						0
					);
				}

				function codexAccountRandomName(account) {
					const pendingOffset = codexAccountPendingRandomNameOffset(account);
					const serverName = String(account?.random_name || "").trim();
					if (pendingOffset == null && serverName) {
						return serverName;
					}

					const seed =
						codexAccountIdentity(account) ||
						codexAccountEmail(account) ||
						account?.plan_type ||
						"account";
					const hash = codexAccountIdentityHash(seed);
					const index =
						(hash + codexAccountRandomNameOffset(account)) % ACCOUNT_RANDOM_NAMES.length;

					return ACCOUNT_RANDOM_NAMES[index];
				}

				function codexAccountShowsEmail(account) {
					return Boolean(codexAccountEmail(account) && !accountEmailsHidden);
				}

				function renderCodexAccountRandomNameButton(account) {
					if (codexAccountShowsEmail(account)) {
						return "";
					}

					return `
						<button class="account-name-reroll" type="button" data-account-name-reroll="${escapeHtml(codexAccountRandomNameKey(account))}" aria-label="Change account name" title="Change account name">
							<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">
								<path d="m18 14 4 4-4 4"></path>
								<path d="m18 2 4 4-4 4"></path>
								<path d="M2 18h1.6c1.4 0 2.7-.7 3.5-1.8l5.8-8.4C13.7 6.7 15 6 16.4 6H22"></path>
								<path d="M2 6h1.6c1.4 0 2.7.7 3.5 1.8l.7 1"></path>
								<path d="M12.8 16.2c.8 1.1 2.1 1.8 3.5 1.8H22"></path>
							</svg>
						</button>
					`;
				}

				function renderCodexAccountNameControl(account, snapshot) {
					const selector = codexAccountControlSelector(account);
					const displayTitle = codexAccountDisplayTitle(account);
					const visibleName = codexAccountVisibleName(account);
					if (!selector) {
						return `<strong class="account-name" title="${escapeHtml(displayTitle)}">${escapeHtml(visibleName)}</strong>`;
					}

					const fixed = codexAccountMatchesConfiguredSelector(account, snapshot);
					const action = fixed ? "clearAccountSelection" : "selectAccount";
					const armed = accountSelectionConfirmationMatches(action, selector);
					const fixedClass = fixed ? " is-fixed" : "";
					const armedClass = armed ? " is-armed" : "";
					const title = accountSelectionControlTitle(action, displayTitle, armed);

					return `
						<button class="account-name-button${fixedClass}${armedClass}" type="button" data-account-confirm-action="${escapeHtml(action)}" data-account-selector="${escapeHtml(selector)}" data-account-display-title="${escapeHtml(displayTitle)}" aria-pressed="${fixed ? "true" : "false"}" aria-label="${escapeHtml(title)}" title="${escapeHtml(title)}">
							<span class="account-name">${escapeHtml(visibleName)}</span>
						</button>
					`;
				}

				function codexAccountDisplayName(account) {
					const email = codexAccountEmail(account);
					return codexAccountShowsEmail(account) ? email : codexAccountRandomName(account);
				}

				function codexAccountVisibleName(account) {
					const email = codexAccountEmail(account);
					return codexAccountShowsEmail(account)
						? compactAccountEmail(email)
						: codexAccountRandomName(account);
				}

				function codexAccountDisplayTitle(account) {
					if (codexAccountShowsEmail(account)) {
						return codexAccountEmail(account);
					}

					const identity = codexAccountIdentity(account);
					return identity
						? compactAccountIdentity(identity)
						: codexAccountDisplayName(account);
				}

				function codexAccountFallbackName(value) {
					const selector = String(value || "").trim();
					if (!selector) {
						return "account";
					}

					return accountEmailsHidden
						? codexAccountRandomName({ account_fingerprint: selector })
						: compactAccountIdentity(selector);
				}

				function codexAccountCapacityMultiplier(account) {
					const explicit = codexAccountNumber(account?.capacity_multiplier);
					if (explicit != null && explicit > 0) {
						return explicit;
					}

					const planType = String(account?.plan_type || "").trim().toLowerCase();
					return planType === "pro" ? 20 : 1;
				}

				function codexAccountCapacityLabel(account) {
					return `${codexAccountCapacityMultiplier(account)}x`;
				}

				function codexAccountUsageRecordCapacityMultiplier(account, record) {
					const explicit = codexAccountNumber(record?.capacity_multiplier);

					return explicit != null && explicit > 0
						? explicit
						: codexAccountCapacityMultiplier(account);
				}

				function codexAccountTokenLabel(refreshStatus) {
					const status = String(refreshStatus || "").toLowerCase();
					return status || "token_unknown";
				}

				function codexAccountTokenValue(refreshStatus) {
					return codexAccountTokenLabel(refreshStatus);
				}

				function codexAccountRefreshStatusNeedsAttention(refreshStatus) {
					const status = String(refreshStatus || "").toLowerCase();
					return Boolean(
						status &&
							!["not_needed", "refreshed", "succeeded", "none"].includes(status),
					);
				}

				function codexAccountRefreshFailed(account) {
					return String(account?.refresh_status || "").toLowerCase().includes("failed");
				}

				function codexAccountNote(account) {
					return String(account?.note || "").trim();
				}

				function codexAccountNoteLooksRoutine(note) {
					return String(note || "").trim().toLowerCase() === "usage probe ok";
				}

				function codexAccountNoteLooksError(note) {
					return /\b(failed|error|unauthorized|forbidden|invalid|missing|unusable)\b/i.test(
						String(note || ""),
					);
				}

				function replaceLiteral(value, needle, replacement) {
					const text = String(value || "");
					const target = String(needle || "");
					return target ? text.split(target).join(replacement) : text;
				}

				function codexAccountPrivacyLabel(account) {
					return codexAccountShowsEmail(account)
						? codexAccountEmail(account)
						: codexAccountRandomName(account);
				}

				function codexAccountPrivacyText(account, value) {
					let text = String(value || "");
					if (!text || !accountEmailsHidden) {
						return text;
					}

					const replacement = codexAccountPrivacyLabel(account);
					text = replaceLiteral(text, codexAccountEmail(account), replacement);
					return text.replace(
						/[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}/gi,
						replacement,
					);
				}

				function codexAccountNumber(value) {
					if (value == null) {
						return null;
					}

					const number = Number(value);
					return Number.isFinite(number) ? number : null;
				}

				function codexAccountPercent(value) {
					const number = codexAccountNumber(value);
					if (number == null) {
						return null;
					}

					return Math.max(0, Math.min(100, Math.round(number)));
				}

				function formatUsagePercent(value) {
					if (value == null || value === "") {
						return "-";
					}
					const number = Number(value);
					if (!Number.isFinite(number)) {
						return "-";
					}

					const rounded = Math.round(number);
					if (Math.abs(number - rounded) < 0.05) {
						return `${rounded}%`;
					}

					return `${number.toFixed(1)}%`;
				}

				function formatDailyUsageRate(value) {
					const percent = formatUsagePercent(value);
					return percent === "-" ? "-" : `${percent}/d`;
				}

				function formatPercentagePointDelta(value) {
					if (value == null || value === "") {
						return "-";
					}
					const number = Number(value);
					if (!Number.isFinite(number)) {
						return "-";
					}

					const absValue = Math.abs(number);
					const sign = number > 0.05 ? "+" : number < -0.05 ? "-" : "";
					const rounded = Math.round(absValue);
					if (Math.abs(absValue - rounded) < 0.05) {
						return `${sign}${rounded}pp`;
					}

					return `${sign}${absValue.toFixed(1)}pp`;
				}

				function codexAccountUsageRecords(account) {
					return Array.isArray(account?.usage_records)
						? account.usage_records.filter((record) => record?.date)
						: [];
				}

				function codexAccountProfileDailyUsage(account) {
					return Array.isArray(account?.profile_daily_usage)
						? account.profile_daily_usage
								.filter((record) => record?.date && codexAccountNumber(record?.tokens) != null)
								.map((record) => ({
									date: String(record.date),
									tokens: Math.max(0, codexAccountNumber(record.tokens) || 0),
								}))
						: [];
				}

				function codexAccountProfilePeakDailyTokens(account) {
					const explicitPeak = codexAccountNumber(account?.profile_peak_daily_tokens);
					if (explicitPeak != null) {
						return explicitPeak;
					}

					return codexAccountProfileDailyUsage(account).reduce(
						(peak, record) => Math.max(peak, record.tokens),
						0,
					) || null;
				}

				function previousUsageDate(value) {
					const match = String(value || "").match(/^(\d{4})-(\d{2})-(\d{2})$/);
					if (!match) {
						return "";
					}

					const date = new Date(Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3])));
					if (Number.isNaN(date.getTime())) {
						return "";
					}
					date.setUTCDate(date.getUTCDate() - 1);

					return date.toISOString().slice(0, 10);
				}

				function usageRecordForDate(account, date) {
					return codexAccountUsageRecords(account)
						.filter((record) => record.date === date)
						.sort(
							(left, right) =>
								Number(right.checked_at_unix_epoch || 0) -
								Number(left.checked_at_unix_epoch || 0),
						)[0] || null;
				}

				function accountPoolUsageEstimate() {
					return accountApiSnapshot?.usage_estimate || null;
				}

				function accountPoolDayDeltaPercentagePoints(accounts, estimate) {
					const measuredAccounts = accounts.filter(
						(account) => codexAccountNumber(account?.seven_day_used_percent) != null,
					);
					const totalCapacity = codexAccountNumber(estimate?.total_capacity_percent);
					const currentPoolUsed = codexAccountNumber(estimate?.total_used_of_capacity_percent);
					if (!measuredAccounts.length || !totalCapacity || currentPoolUsed == null) {
						return null;
					}

					const latestDate = measuredAccounts
						.flatMap(codexAccountUsageRecords)
						.map((record) => record.date)
						.sort()
						.at(-1);
					if (!latestDate) {
						return currentPoolUsed;
					}

					const previousDate = previousUsageDate(latestDate);
					if (!previousDate) {
						return currentPoolUsed;
					}

					const previousUsedPercent = measuredAccounts.reduce((total, account) => {
						const record = usageRecordForDate(account, previousDate);
						const used = codexAccountNumber(record?.used_percent) || 0;

						return (
							total + used * codexAccountUsageRecordCapacityMultiplier(account, record)
						);
					}, 0);
					const previousPoolPercent = (previousUsedPercent / totalCapacity) * 100;

					return currentPoolUsed - previousPoolPercent;
				}

				function accountPoolUsageTone(used) {
					if (used == null || used === "") {
						return "muted";
					}
					const value = Number(used);
					if (!Number.isFinite(value)) {
						return "muted";
					}
					if (value >= 90) {
						return "danger";
					}
					if (value >= 75) {
						return "warning";
					}

					return "run";
				}

				function accountPoolDayDeltaTone(delta, used) {
					if (delta == null || delta === "") {
						return "muted";
					}
					const value = Number(delta);
					if (!Number.isFinite(value) || Math.abs(value) <= 0.05) {
						return "muted";
					}
					if (value < -0.05) {
						return "muted";
					}

					return accountPoolUsageTone(used);
				}

				function codexAccountUnixTimestamp(value) {
					const seconds = codexAccountNumber(value);
					if (seconds == null || seconds <= 0) {
						return "unknown";
					}

					return formatTimestampCompact(new Date(seconds * 1000).toISOString());
				}

				function formatCodexAccountResetDuration(seconds) {
					const value = Math.max(0, Number(seconds));
					if (!Number.isFinite(value)) {
						return "unknown";
					}
					if (value < 60) {
						return "<1m";
					}

					const days = Math.floor(value / 86_400);
					const hours = Math.floor((value % 86_400) / 3_600);
					const minutes = Math.floor((value % 3_600) / 60);
					const parts = [];

					if (days > 0) {
						parts.push(`${days}d`);
						if (hours > 0) {
							parts.push(`${hours}h`);
						}
						return parts.join(" ");
					}

					if (hours > 0) {
						parts.push(`${hours}h`);
					}
					if (minutes > 0 || hours > 0) {
						parts.push(`${minutes}m`);
					}

					return parts.join(" ") || "<1m";
				}

				function formatCodexAccountProfileDuration(seconds) {
					const value = codexAccountNumber(seconds);
					if (value == null) {
						return "";
					}

					return formatCodexAccountResetDuration(value);
				}

				function codexAccountProfileMetaFacts(account) {
					if (!account) {
						return [];
					}
					const currentStreak = codexAccountNumber(account.profile_current_streak_days);
					const longestStreak = codexAccountNumber(account.profile_longest_streak_days);
					const streak =
						currentStreak != null && longestStreak != null
							? `${currentStreak}/${longestStreak}d`
							: currentStreak != null
								? `${currentStreak}d`
								: longestStreak != null
									? `${longestStreak}d`
									: "";
					const task = formatCodexAccountProfileDuration(account.profile_longest_task_seconds);
					const peakDailyTokens = codexAccountProfilePeakDailyTokens(account);
					const facts = [
						codexAccountNumber(account.profile_lifetime_tokens) != null
							? ["tok", formatCompactCount(account.profile_lifetime_tokens)]
							: null,
						peakDailyTokens != null
							? ["peak", formatCompactCount(peakDailyTokens)]
							: null,
						streak ? ["streak", streak] : null,
						task ? ["task", task] : null,
					];

					return facts.filter(Boolean);
				}

				function codexAccountProfileAggregate(accounts) {
					const dailyUsageByDate = new Map();
					let lifetimeTokens = null;
					let peakTokensFallback = null;
					let longestTaskSeconds = null;
					let currentStreakDays = null;
					let longestStreakDays = null;

					for (const account of accounts) {
						const lifetime = codexAccountNumber(account?.profile_lifetime_tokens);
						if (lifetime != null) {
							lifetimeTokens = (lifetimeTokens || 0) + lifetime;
						}
						const peak = codexAccountProfilePeakDailyTokens(account);
						if (peak != null) {
							peakTokensFallback = (peakTokensFallback || 0) + peak;
						}
						const task = codexAccountNumber(account?.profile_longest_task_seconds);
						if (task != null) {
							longestTaskSeconds = Math.max(longestTaskSeconds || 0, task);
						}
						const currentStreak = codexAccountNumber(account?.profile_current_streak_days);
						if (currentStreak != null) {
							currentStreakDays = Math.max(currentStreakDays || 0, currentStreak);
						}
						const longestStreak = codexAccountNumber(account?.profile_longest_streak_days);
						if (longestStreak != null) {
							longestStreakDays = Math.max(longestStreakDays || 0, longestStreak);
						}
						for (const record of codexAccountProfileDailyUsage(account)) {
							dailyUsageByDate.set(record.date, (dailyUsageByDate.get(record.date) || 0) + record.tokens);
						}
					}

					const dailyUsage = Array.from(dailyUsageByDate, ([date, tokens]) => ({ date, tokens }))
						.sort((left, right) => String(left.date).localeCompare(String(right.date)));
					const peakFromDailyUsage = dailyUsage.reduce(
						(peak, record) => Math.max(peak, record.tokens),
						0,
					);
					const peakDailyTokens = peakFromDailyUsage > 0 ? peakFromDailyUsage : peakTokensFallback;
					const aggregate = {
						profile_lifetime_tokens: lifetimeTokens,
						profile_peak_daily_tokens: peakDailyTokens,
						profile_longest_task_seconds: longestTaskSeconds,
						profile_current_streak_days: currentStreakDays,
						profile_longest_streak_days: longestStreakDays,
						profile_daily_usage: dailyUsage,
					};
					const hasMetric = codexAccountProfileMetaFacts(aggregate).length > 0;

					return hasMetric || dailyUsage.length ? aggregate : null;
				}

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
