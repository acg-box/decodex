

				function codexAccountProfileExpansionKey(account) {
					return codexAccountRandomNameKey(account);
				}

				function codexAccountHasProfileDetails(account) {
					return (
						codexAccountProfileMetaFacts(account).length > 0 ||
						codexAccountProfileDailyUsage(account).length > 0 ||
						codexAccountResetCreditsAvailableCount(account) > 0
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
