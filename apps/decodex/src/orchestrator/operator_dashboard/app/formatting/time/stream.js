			function dashboardSocketUrl() {
				const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
				const host = window.location.host;

				if (!host) {
					return DASHBOARD_WEBSOCKET_ENDPOINT;
				}

				return `${protocol}//${host}${DASHBOARD_WEBSOCKET_ENDPOINT}`;
			}

			function snapshotAgeSeconds(snapshotPublishedAt) {
				if (!snapshotPublishedAt) {
					return null;
				}

				const parsed = new Date(snapshotPublishedAt);
				if (Number.isNaN(parsed.getTime())) {
					return null;
				}

				return Math.max(0, Math.floor((Date.now() - parsed.getTime()) / 1000));
			}

			function snapshotFreshnessMeta(snapshotPublishedAt, readiness, snapshotError) {
				if (snapshotPublishedAt) {
					const ageSeconds = snapshotAgeSeconds(snapshotPublishedAt);
					const staleByAge = ageSeconds != null && ageSeconds >= 30;
					if (
						!snapshotError &&
						readiness.tone !== "danger" &&
						!staleByAge
					) {
						return null;
					}

					return {
						label: formatRelativeTimestamp(snapshotPublishedAt),
						tone:
							snapshotError || readiness.tone === "danger"
								? "danger"
								: staleByAge
									? "warning"
									: readiness.tone,
						title: `Published ${formatTimestamp(snapshotPublishedAt)}`,
					};
				}

				if (snapshotError || readiness.tone === "danger") {
					return {
						label: "Unavailable",
						tone: "danger",
						title: snapshotError || readiness.copy,
					};
				}

				return {
					label: "Pending",
					tone: readiness.tone === "warning" ? "warning" : "muted",
					title: readiness.copy,
				};
			}

			function topbarReadinessLabel(label) {
				switch (label) {
					case "Snapshot ready":
						return "Ready";
					case "State degraded":
						return "Degraded";
					case "Tracker sync paused":
						return "Sync paused";
					case "Listener down":
						return "Listener down";
					case "No snapshot":
						return "No snapshot";
					default:
						return label;
				}
			}

			function dashboardStreamMeta() {
				if (!dashboardStreamState.supported) {
					return {
						label: "unavailable",
						tone: "danger",
						title: "WebSocket unavailable.",
					};
				}

				if (dashboardStreamState.connected) {
					return {
						label: dashboardStreamState.lastEventAt ? "live" : "connected",
						tone: "success",
						title: dashboardStreamState.lastEventAt
							? `Last event ${formatRelativeTimestamp(dashboardStreamState.lastEventAt)}`
							: "WebSocket connected.",
					};
				}

				if (dashboardStreamState.error) {
					return {
						label: "reconnecting",
						tone: "warning",
						title: "WebSocket reconnecting.",
					};
				}

				return {
					label: "starting",
					tone: "muted",
					title: "WebSocket connecting.",
				};
			}
