			function startDashboardStream() {
				applyTheme(themeSelection, false);
				renderAccountPrivacyToggle();
				renderProjectFilterToggle();
				renderProjectLocationToggle();
				renderProjectWorkInfoState();
				applyDashboardLayout();
					lastDashboardRender = {
						snapshot: null,
						snapshotError: "",
						snapshotPublishedAt: null,
				};
				renderDashboardState(lastDashboardRender);
				connectDashboardSocket();
				startDashboardLocalClock();
			}

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

			nodes.projectOverview.addEventListener("click", (event) => {
				if (!(event.target instanceof Element)) {
					return;
				}

				const sortButton = event.target.closest("[data-project-sort-key]");
				if (sortButton) {
					event.preventDefault();
					const key = sortButton.dataset.projectSortKey;
					if (!isProjectSortKey(key)) {
						return;
					}

					projectSort = {
						key,
						direction:
							projectSort.key === key
								? projectSort.direction === "asc"
									? "desc"
									: "asc"
								: projectSortDefaultDirection(key),
					};
					persistProjectSort();
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
					}
					return;
				}

				const locationToggle = event.target.closest("[data-project-location-toggle]");
				if (locationToggle) {
					event.preventDefault();
					projectLocationsHidden = !projectLocationsHidden;
					persistProjectLocationPrivacy(projectLocationsHidden);
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
						return;
					}
					renderProjectLocationToggle();
					return;
				}

				const workInfo = event.target.closest("[data-project-work-info]");
				if (!workInfo) {
					return;
				}

				event.preventDefault();
				projectWorkInfoOpen = !projectWorkInfoOpen;
				renderProjectWorkInfoState();
			});

			if (typeof themeMediaQuery.addEventListener === "function") {
				themeMediaQuery.addEventListener("change", () => {
					if (themeSelection === "system") {
						applyTheme("system", false);
					}
				});
			} else if (typeof themeMediaQuery.addListener === "function") {
				themeMediaQuery.addListener(() => {
					if (themeSelection === "system") {
						applyTheme("system", false);
					}
				});
			}

			document.addEventListener("click", (event) => {
				if (!(event.target instanceof Element)) {
					return;
				}

				if (
					accountSelectionConfirmation &&
					!event.target.closest("[data-account-confirm-action]")
				) {
					clearAccountSelectionConfirmation(true);
				}

				if (!event.target.closest("[data-project-work-info]")) {
					projectWorkInfoOpen = false;
					renderProjectWorkInfoState();
				}

				const noticeAck = event.target.closest("[data-notice-ack]");
				if (noticeAck) {
					event.preventDefault();
					const ackKey = noticeAck.dataset.noticeAck;
					dashboardControlEvents = dashboardControlEvents.filter(
						(controlEvent) => controlEvent.key !== ackKey,
					);
					sendDashboardControl("ack", { key: ackKey });
					if (lastDashboardRender) {
						renderDashboardState(lastDashboardRender);
					}
					return;
				}

				const summary = event.target.closest("summary");
				if (!summary) {
					return;
				}

				const details = summary.parentElement;
				if (!(details instanceof HTMLDetailsElement)) {
					return;
				}

				event.preventDefault();
				animateDetail(details, !details.open);
			});

			document.addEventListener("visibilitychange", () => {
				if (document.hidden) {
					return;
				}

				if (lastDashboardRender) {
					renderDashboardState(lastDashboardRender);
				}
				if (!dashboardSocketIsOpen()) {
					connectDashboardSocket();
				}
			});

			startDashboardStream();
