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
