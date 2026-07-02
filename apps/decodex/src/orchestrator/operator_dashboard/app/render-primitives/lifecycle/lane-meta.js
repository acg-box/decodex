
			function currentLaneTelemetryFacts(run) {
				const facts = [];
				const freshness = currentLaneFreshness(run);
				const focus = protocolActivityFocus(run);

				facts.push(["run phase", displayToken(run.run_phase || run.phase || run.status)]);
				if (freshness.timestamp) {
					facts.push([freshness.sourceLabel, formatRelativeTimestamp(freshness.timestamp)]);
				}
				if (focus) {
					facts.push(["focus", detailLabel(focus)]);
				}
				if (run.idle_for_seconds != null) {
					facts.push(["lane idle", formatDuration(run.idle_for_seconds)]);
				}
				if (run.protocol_idle_for_seconds != null) {
					facts.push(["agent idle", formatDuration(run.protocol_idle_for_seconds)]);
				}
				return facts;
			}

			function currentLaneReadbackValues(run) {
				return [
					run?.run_phase || run?.phase || run?.status,
					run?.current_operation,
					run?.active_goal_phase,
					run?.public_progress_phase,
				].filter(Boolean);
			}

			function currentLaneVisibleSummary(card, run) {
				const summary = String(card?.detail || "").trim();
				if (!summary) {
					return "";
				}

				return currentLaneReadbackValues(run).some((value) => displayTextRepeats(summary, value))
					? ""
					: summary;
			}

			function renderRunMetaFact(label, value, valueClass = "", title = "") {
				const classAttribute = valueClass ? ` class="${escapeHtml(valueClass)}"` : "";
				const titleAttribute = title ? ` title="${escapeHtml(title)}"` : "";

				return `
					<span class="run-meta-item">
						<span class="run-meta-label">${escapeHtml(detailLabel(label))}</span>
						<strong${classAttribute}${titleAttribute}>${escapeHtml(value)}</strong>
					</span>
				`;
			}

			function renderRunTelemetryMetaItems(run) {
				const facts = currentLaneTelemetryFacts(run);

				if (!facts.length) {
					return "";
				}

				return facts.map(([label, value]) => renderRunMetaFact(label, value)).join("");
			}
