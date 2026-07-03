			function snapshotIsIdle(snapshot) {
				if (!snapshot) {
					return false;
				}

				return (
					snapshotCurrentLaneCards(snapshot).length === 0 &&
					(snapshot.recent_runs?.length ?? 0) === 0 &&
					(snapshot.worktrees?.length ?? 0) === 0 &&
					(snapshot.post_review_lanes?.length ?? 0) === 0
				);
			}

			function connectorBackoffs(snapshot) {
				return Array.isArray(snapshot?.connector_backoffs) ? snapshot.connector_backoffs : [];
			}

			function hasConnectorBackoff(snapshot) {
				return connectorBackoffs(snapshot).length > 0 || (snapshot?.warnings ?? []).includes("tracker_rate_limited");
			}

			function connectorBackoffNotice(backoff) {
				const project = backoff.project_id || "project";
				const connector = displayToken(backoff.connector || "tracker");
				const phase = displayToken(backoff.sync_phase || "external sync");
				const quota = displayToken(backoff.quota_class || "api quota");
				const retryAfter = backoff.retry_after_seconds == null ? "unknown" : formatDuration(backoff.retry_after_seconds);
				const resetAt = formatTimestamp(backoff.reset_at);
				const nextAction = backoff.next_action || "Monitor local lanes.";

				return {
					tone: "warning",
					title: `Sync backoff · ${project}`,
					copy: `${connector} ${phase} paused by ${quota}. Retry in ${retryAfter} at ${resetAt}. ${nextAction}`,
				};
			}

			function summarizeReadiness(snapshotError, snapshot) {
				const warnings = snapshot?.warnings ?? [];
				const trackerBackoff = hasConnectorBackoff(snapshot);

				if (!dashboardStreamState.supported) {
					return {
						tone: "danger",
						label: "WebSocket unavailable",
						copy: "This browser cannot open the dashboard WebSocket.",
					};
				}

				if (dashboardStreamState.error) {
					return {
						tone: "danger",
						label: "WebSocket disconnected",
						copy: "Dashboard stream disconnected; reconnecting.",
					};
				}

				if (snapshot) {
					if (trackerBackoff && !snapshotError) {
						return {
							tone: "warning",
							label: "Tracker sync paused",
							copy: "Serving local state; Linear sync is paused.",
						};
					}

					return {
						tone: snapshotError || warnings.length ? "warning" : "success",
						label: snapshotError || warnings.length ? "State degraded" : "Snapshot ready",
						copy: snapshotError
							? "WebSocket did not deliver a usable snapshot."
							: warnings.length
								? `warnings: ${warnings.map(displayToken).join(", ")}`
								: "Fresh operator snapshot published.",
					};
				}

				return {
					tone: "warning",
					label: "No snapshot",
					copy: dashboardStreamState.connected
						? "WebSocket connected; waiting for operator snapshot."
						: "Connecting to dashboard WebSocket.",
				};
			}
