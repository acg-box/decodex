
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
