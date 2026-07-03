			for (const button of nodes.themeButtons) {
				button.addEventListener("click", () => {
					applyTheme(button.dataset.themeChoice);
				});
			}

			nodes.projectFilterToggle.addEventListener("click", () => {
				projectFilterMode = projectFilterMode === "all" ? "active" : "all";
				persistProjectFilterMode();
				if (lastDashboardRender) {
					renderDashboardState(lastDashboardRender);
					return;
				}
				renderProjectFilterToggle();
			});

			nodes.accountPool.addEventListener("click", (event) => {
				if (!(event.target instanceof Element)) {
					return;
				}

				const privacyButton = event.target.closest("[data-account-privacy-toggle]");
				if (privacyButton) {
					event.preventDefault();
					accountEmailsHidden = !accountEmailsHidden;
					persistAccountPrivacy(accountEmailsHidden);
					renderAccountPrivacyToggle();
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
					}
					return;
				}

				const sortButton = event.target.closest("[data-account-sort-key]");
				if (sortButton) {
					event.preventDefault();
					const key = sortButton.dataset.accountSortKey;
					if (!isAccountPoolSortKey(key)) {
						return;
					}

					accountPoolSort = {
						key,
						direction:
							accountPoolSort.key === key && accountPoolSort.direction === "asc"
								? "desc"
								: "asc",
					};
					persistAccountPoolSort();
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
					}
					return;
				}

				const profileToggle = event.target.closest("[data-account-profile-toggle]");
				if (profileToggle) {
					event.preventDefault();
					toggleCodexAccountProfileKey(profileToggle.dataset.accountProfileToggle);
					return;
				}

				const confirmButton = event.target.closest("[data-account-confirm-action]");
				if (confirmButton) {
					event.preventDefault();
					const action = confirmButton.dataset.accountConfirmAction;
					const accountSelector = confirmButton.dataset.accountSelector || null;
					handleAccountSelectionConfirmation(action, accountSelector);
					return;
				}

				const button = event.target.closest("[data-account-name-reroll]");
				if (button) {
					event.preventDefault();
					const key = button.dataset.accountNameReroll;
					if (!key) {
						return;
					}

					const account = lastDashboardRender
						? codexAccountPoolAccounts(lastDashboardRender.snapshot).find(
								(candidate) => codexAccountRandomNameKey(candidate) === key,
							)
						: null;
					const current = account
						? codexAccountRandomNameOffset(account)
						: normalizeAccountNameOffset(pendingAccountNameOffsets[key]);
					const next = normalizeAccountNameOffset(current + 1);
					pendingAccountNameOffsets = {
						...pendingAccountNameOffsets,
						[key]: next,
					};
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
					}
					if (account) {
						postAccountNameOffset(account, next)
							.then((updated) => {
								if (updated) {
									delete pendingAccountNameOffsets[key];
									if (lastDashboardRender) {
										renderDashboardState(lastDashboardRender);
									}
								}
							})
							.catch(() => {});
					}
					return;
				}

				const profileRow = event.target.closest("[data-account-profile-row-toggle]");
				if (profileRow && !accountProfileRowClickIsSuppressed(event.target)) {
					event.preventDefault();
					toggleCodexAccountProfileKey(profileRow.dataset.accountProfileRowToggle);
				}
			});
