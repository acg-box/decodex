			function dashboardNotices(readiness, snapshotError, snapshot) {
				const notices = [];
				const warnings = snapshot?.warnings ?? [];
				const backoffs = connectorBackoffs(snapshot);

				if (readiness.tone === "danger") {
					notices.push({
						tone: "danger",
						title: readiness.label,
						copy: snapshotError
							? `${readiness.copy} Snapshot stream also failed: ${snapshotError}`
							: readiness.copy,
					});
				} else if (snapshotError) {
					notices.push({
						tone: "danger",
						title: "Snapshot stream",
						copy: snapshotError,
					});
				}

				if (
					readiness.tone === "warning" &&
					!snapshotError &&
					warnings.length === 0 &&
					backoffs.length === 0
				) {
					notices.push({
						tone: "warning",
						title: readiness.label,
						copy: readiness.copy,
					});
				}

				for (const backoff of backoffs) {
					notices.push(connectorBackoffNotice(backoff));
				}

				for (const warning of warnings) {
					if (warning === "tracker_rate_limited" && backoffs.length) {
						continue;
					}
					if (
						warning === "external_observer_status_skipped" &&
						backoffs.length &&
						warnings.includes("tracker_rate_limited")
					) {
						continue;
					}
					const message = warningNotice(warning, snapshot);
					notices.push({
						tone: message.tone,
						title: message.title,
						copy: message.copy,
					});
				}

				for (const accountNotice of codexAccountNotices(snapshot)) {
					notices.push(accountNotice);
				}

				for (const controlEvent of dashboardControlEvents) {
					notices.push({
						tone: controlEvent.accepted ? "warning" : "danger",
						title: controlEvent.accepted ? "Control accepted" : "Control failed",
						copy: `${dashboardControlActionLabel(controlEvent.action)}: ${controlEvent.message}`,
						ackKey: controlEvent.key,
					});
				}

				return notices;
			}
