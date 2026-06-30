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
