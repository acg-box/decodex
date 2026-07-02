

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
