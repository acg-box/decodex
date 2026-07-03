			function loadThemeSelection() {
				try {
					const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
					return ["system", "light", "dark"].includes(stored) ? stored : "system";
				} catch (_error) {
					return "system";
				}
			}

			function loadAccountPrivacy() {
				try {
					return window.localStorage.getItem(ACCOUNT_PRIVACY_STORAGE_KEY) === "hidden";
				} catch (_error) {
					return false;
				}
			}

			function persistAccountPrivacy(hidden) {
				try {
					window.localStorage.setItem(
						ACCOUNT_PRIVACY_STORAGE_KEY,
						hidden ? "hidden" : "visible",
					);
				} catch (_error) {
					/* Ignore storage failures and continue with the in-memory choice. */
				}
			}

			function normalizeAccountNameOffset(value) {
				const number = Number(value);
				if (!Number.isInteger(number)) {
					return 0;
				}

				return (
					(number % ACCOUNT_RANDOM_NAMES.length) + ACCOUNT_RANDOM_NAMES.length
				) % ACCOUNT_RANDOM_NAMES.length;
			}

			function accountApiAccounts() {
				return Array.isArray(accountApiSnapshot?.accounts)
					? accountApiSnapshot.accounts.filter(Boolean)
					: [];
			}

			function noteAccountApiSnapshot(response) {
				if (!response || !Array.isArray(response.accounts)) {
					return false;
				}

				accountApiSnapshot = response;
				accountApiRefreshedAt = Date.now();

				return true;
			}

			async function refreshAccountApiSnapshot(force = false) {
				const now = Date.now();
				if (
					accountApiRefreshInFlight ||
					(!force &&
						accountApiSnapshot &&
						now - accountApiRefreshedAt < ACCOUNT_API_REFRESH_INTERVAL_MS)
				) {
					return;
				}

				accountApiRefreshInFlight = true;
				try {
					const response = await fetch("/api/accounts?refresh=1", {
						headers: { Accept: "application/json" },
					});
					if (!response.ok) {
						return;
					}
					if (noteAccountApiSnapshot(await response.json()) && lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
					}
				} catch (_error) {
					/* The websocket snapshot remains usable if the account API is unavailable. */
				} finally {
					accountApiRefreshInFlight = false;
				}
			}

			async function postAccountNameOffset(account, offset = null) {
				const selector = codexAccountControlSelector(account);
				if (!selector) {
					return false;
				}

				const body = { selector };
				if (offset != null) {
					body.random_name_offset = normalizeAccountNameOffset(offset);
				}

				const response = await fetch("/api/accounts/reroll-name", {
					method: "POST",
					headers: {
						Accept: "application/json",
						"Content-Type": "application/json",
					},
					body: JSON.stringify(body),
				});
				if (!response.ok) {
					return false;
				}

				return noteAccountApiSnapshot(await response.json());
			}
