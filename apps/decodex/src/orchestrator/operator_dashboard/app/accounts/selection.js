
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

