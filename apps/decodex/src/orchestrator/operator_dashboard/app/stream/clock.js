
			function unixEpochSeconds(value) {
				const unixEpochSeconds = Number(value);
				if (!Number.isFinite(unixEpochSeconds)) {
					return null;
				}

				return unixEpochSeconds;
			}

			function unixEpochSecondsToIso(value) {
				const seconds = unixEpochSeconds(value);
				if (seconds == null) {
					return null;
				}

				const parsed = new Date(seconds * 1000);
				if (Number.isNaN(parsed.getTime())) {
					return null;
				}

				return parsed.toISOString();
			}

			function isoTimestampUnixEpoch(value) {
				const timestamp = Date.parse(value || "");
				if (!Number.isFinite(timestamp)) {
					return null;
				}

				return Math.floor(timestamp / 1000);
			}

			function parseDashboardSocketMessage(message) {
				try {
					return JSON.parse(message.data);
				} catch (_error) {
					return null;
				}
			}

			function updateDashboardStreamState(patch, shouldRender = true) {
				dashboardStreamState = {
					...dashboardStreamState,
					...patch,
				};

				if (shouldRender && lastDashboardRender) {
					renderDashboardState(lastDashboardRender);
				}
			}

			function scheduleDashboardSocketReconnect() {
				if (dashboardSocketReconnectTimer) {
					return;
				}

				dashboardSocketReconnectTimer = window.setTimeout(() => {
					dashboardSocketReconnectTimer = null;
					connectDashboardSocket();
				}, 2000);
			}

			function renderDashboardLocalClockTick() {
				if (document.hidden || !lastDashboardRender) {
					return;
				}

				refreshAccountApiSnapshot();
				renderDashboardState(lastDashboardRender, { refreshAccounts: false });
			}

			function startDashboardLocalClock() {
				if (dashboardLocalClockTimer) {
					return;
				}

				dashboardLocalClockTimer = window.setInterval(
					renderDashboardLocalClockTick,
					DASHBOARD_LOCAL_CLOCK_INTERVAL_MS,
				);
			}
