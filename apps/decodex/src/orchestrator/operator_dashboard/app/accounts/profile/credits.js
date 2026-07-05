

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

				function codexAccountResetCreditCards(account) {
					const credits = Array.isArray(account?.reset_credits) ? account.reset_credits : [];

					return credits
						.filter((credit) => {
							const status = String(credit?.status || "").toLowerCase();
							return !status || status === "available";
						})
						.map((credit) => ({
							grantedAt: codexAccountNumber(credit?.granted_at_unix_epoch),
							expiresAt: codexAccountNumber(credit?.expires_at_unix_epoch),
							status: String(credit?.status || "").trim(),
						}))
						.sort((left, right) => (left.expiresAt || Number.MAX_SAFE_INTEGER) - (right.expiresAt || Number.MAX_SAFE_INTEGER));
				}

				function codexAccountResetCreditsAvailableCount(account) {
					const reported = codexAccountNumber(account?.reset_credits_available_count);
					return reported ?? codexAccountResetCreditCards(account).length;
				}

				function codexAccountResetCreditsSummary(account) {
					const count = codexAccountResetCreditsAvailableCount(account);
					if (count == null || count <= 0) {
						return null;
					}

					return `${count} reset`;
				}

					const CODEX_ACCOUNT_RESET_CREDIT_LOCALE = "en-US";
					const CODEX_ACCOUNT_RESET_CREDIT_TIME_ZONE = "Asia/Shanghai";
					const CODEX_ACCOUNT_RESET_CREDIT_TIME_ZONE_LABEL = "BJT";

					function codexAccountResetCreditCompactTimestamp(value) {
						const seconds = codexAccountNumber(value);
						if (seconds == null || seconds <= 0) {
							return "-";
						}

						const date = new Date(seconds * 1000);
						if (Number.isNaN(date.getTime())) {
							return "-";
						}

						const parts = new Intl.DateTimeFormat(CODEX_ACCOUNT_RESET_CREDIT_LOCALE, {
							timeZone: CODEX_ACCOUNT_RESET_CREDIT_TIME_ZONE,
							month: "short",
							day: "numeric",
							hour: "2-digit",
							minute: "2-digit",
							hour12: false,
						})
							.formatToParts(date)
							.reduce((result, part) => ({ ...result, [part.type]: part.value }), {});

						return `${parts.month || ""} ${parts.day || ""} ${parts.hour || ""}:${parts.minute || ""}`.trim();
					}

					function codexAccountResetCreditExpiry(card) {
						return codexAccountResetCreditCompactTimestamp(card.expiresAt);
					}

					function renderCodexAccountResetCreditsStrip(account) {
						const cards = codexAccountResetCreditCards(account);
						const count = codexAccountResetCreditsAvailableCount(account);
						if (!cards.length && (count == null || count <= 0)) {
							return "";
						}

						const title = cards
							.map((card) => {
								const expires = codexAccountResetCreditCompactTimestamp(card.expiresAt);
								return `expires ${expires} ${CODEX_ACCOUNT_RESET_CREDIT_TIME_ZONE_LABEL}`;
							})
							.join(" · ");

						return `
							<div class="account-reset-credit-strip" aria-label="${escapeHtml(`${codexAccountResetCreditsSummary(account) || "reset cards"} ${CODEX_ACCOUNT_RESET_CREDIT_TIME_ZONE_LABEL}`)}" title="${escapeHtml(title || "reset cards")}">
								${cards
									.map((card) => {
										const label = codexAccountResetCreditExpiry(card);
										return `<span class="account-reset-credit-chip" title="${escapeHtml(`${label} ${CODEX_ACCOUNT_RESET_CREDIT_TIME_ZONE_LABEL}`)}">${escapeHtml(label)}</span>`;
									})
									.join("")}
							</div>
						`;
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
