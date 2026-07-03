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
