
			function currentLaneFreshness(run) {
				if (run?.last_run_activity_at) {
					return {
						label: "Lane activity",
						source: "last_run_activity_at",
						sourceLabel: "live activity",
						timestamp: run.last_run_activity_at,
					};
				}
				if (run?.last_progress_at) {
					return {
						label: "Last progress",
						source: "last_progress_at",
						sourceLabel: "progress",
						timestamp: run.last_progress_at,
					};
				}
				if (run?.last_protocol_activity_at) {
					return {
						label: "Protocol activity",
						source: "last_protocol_activity_at",
						sourceLabel: "protocol activity",
						timestamp: run.last_protocol_activity_at,
					};
				}
				return {
					label: "Lane activity",
					source: "none",
					sourceLabel: "activity",
					timestamp: null,
				};
			}

			function currentLaneFreshnessFact(run, formatter = formatTimestamp) {
				const freshness = currentLaneFreshness(run);
				return [
					freshness.label,
					freshness.timestamp ? formatter(freshness.timestamp) : "not captured",
				];
			}

			function historyRunTimingFacts(run) {
				return [
					["Updated", formatTimestampCompact(run.updated_at)],
					["Attempt", String(run.attempt_number ?? "none")],
					["Status", displayToken(run.status)],
					["Events", String(run.event_count ?? 0)],
				];
			}
