			function pluralLabel(count, singular, plural = `${singular}s`) {
				return count === 1 ? singular : plural;
			}

			function pluralize(count, singular, plural = `${singular}s`) {
				return `${count} ${pluralLabel(count, singular, plural)}`;
			}

			function rawSessionHistoryRuns(snapshot) {
				const currentLaneRuns = snapshotCurrentLaneRuns(snapshot);
				const currentLaneIds = new Set(currentLaneRuns.map((run) => run.run_id));
				const currentLaneIssueKeys = new Set(currentLaneRuns.flatMap(issueIdentityKeys));
				return (snapshot?.recent_runs ?? []).filter(
					(run) => !currentLaneIds.has(run.run_id) && !issueMatchesKeySet(run, currentLaneIssueKeys),
				);
			}

			function issueIdentifierFromRunId(runId) {
				const match = String(runId || "").match(/^([a-z][a-z0-9]*-\d+)-attempt-\d+(?:-\d+)?$/i);
				if (match) {
					return match[1].toUpperCase();
				}

				const recovered = String(runId || "").match(/^recovered-([a-z][a-z0-9]*-\d+)$/i);
				return recovered ? recovered[1].toUpperCase() : "";
			}

			function attemptNumberFromRun(run) {
				if (run?.attempt_number != null) {
					return String(run.attempt_number);
				}

				const match = String(run?.run_id || "").match(/-attempt-(\d+)(?:-\d+)?$/i);
				return match ? match[1] : "";
			}

			function issueIdentifierInText(value) {
				const match = String(value || "").match(/(?:^|[^A-Za-z0-9])([A-Za-z]+-\d+)(?=$|[^A-Za-z0-9])/);
				return match ? match[1].toUpperCase() : "";
			}

			function runGroupKey(run) {
				return canonicalIssueIdentityKey(run?.issue_id) || issueDisplayKey(run);
			}

			function isSuccessfulTerminalRun(run) {
				return ["succeeded", "completed", "merged"].includes(run?.status);
			}

			function sessionHistoryRuns(snapshot) {
				return sessionHistoryLanes(snapshot).map((lane) => lane.latest_run).filter(Boolean);
			}
