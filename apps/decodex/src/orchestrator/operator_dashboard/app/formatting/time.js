			function formatTimestamp(value) {
				if (!value) {
					return "none";
				}

				const parsed = new Date(value);
				if (Number.isNaN(parsed.getTime())) {
					return String(value);
				}

				return new Intl.DateTimeFormat(undefined, {
					dateStyle: "medium",
					timeStyle: "medium",
				}).format(parsed);
			}

			function formatTimestampCompact(value) {
				if (!value) {
					return "none";
				}

				const parsed = new Date(value);
				if (Number.isNaN(parsed.getTime())) {
					return String(value);
				}

				return new Intl.DateTimeFormat(undefined, {
					dateStyle: "medium",
					timeStyle: "short",
				}).format(parsed);
			}

			function formatRelativeTimestamp(value) {
				if (!value) {
					return "none";
				}

				const parsed = new Date(value);
				if (Number.isNaN(parsed.getTime())) {
					return String(value);
				}

				const seconds = Math.max(0, Math.floor((Date.now() - parsed.getTime()) / 1000));
				if (seconds < 5) {
					return "0s";
				}
				if (seconds < 60) {
					return `${seconds}s`;
				}

				const minutes = Math.floor(seconds / 60);
				if (minutes < 60) {
					return `${minutes}m`;
				}

				const hours = Math.floor(minutes / 60);
				if (hours < 24) {
					return `${hours}h`;
				}

				const days = Math.floor(hours / 24);
				return `${days}d`;
			}

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

			function formatDuration(seconds) {
				if (seconds == null) {
					return "none";
				}

				const value = Math.max(0, Number(seconds));
				const hours = Math.floor(value / 3600);
				const minutes = Math.floor((value % 3600) / 60);
				const remainingSeconds = Math.floor(value % 60);
				const parts = [];

				if (hours > 0) {
					parts.push(`${hours}h`);
				}
				if (minutes > 0 || hours > 0) {
					parts.push(`${minutes}m`);
				}
				parts.push(`${remainingSeconds}s`);

				return parts.join(" ");
			}

				function formatCompactCount(value) {
					if (value == null) {
						return "none";
					}

					const number = Math.max(0, Number(value));

					if (number >= 1_000_000_000) {
						return `${(number / 1_000_000_000).toFixed(1)}B`;
					}
					if (number >= 1_000_000) {
						return `${(number / 1_000_000).toFixed(2)}M`;
					}
					if (number >= 1_000) {
						return `${(number / 1_000).toFixed(1)}k`;
					}

					return String(Math.floor(number));
				}

			function formatCompactBytes(value) {
				if (value == null) {
					return "none";
				}

				const number = Math.max(0, Number(value));

				if (number >= 1_048_576) {
					return `${(number / 1_048_576).toFixed(1)}MiB`;
				}
				if (number >= 1024) {
					return `${(number / 1024).toFixed(1)}KiB`;
				}

				return `${Math.floor(number)}B`;
			}
