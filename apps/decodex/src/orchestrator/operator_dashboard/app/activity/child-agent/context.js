			function childAgentActivity(run) {
				return run?.child_agent_activity || null;
			}

			function childAgentCurrentSummary(summary) {
				if (!summary?.current_bucket) {
					return null;
				}

				const label = detailLabel(displayToken(summary.current_detail || summary.current_bucket));

				return `${label} · ${formatDuration(summary.current_elapsed_seconds)}`;
			}

			function childAgentInputWindowsMatch(summary) {
				return (
					summary?.input_tokens_current != null &&
					summary?.input_tokens_max != null &&
					Number(summary.input_tokens_current) === Number(summary.input_tokens_max)
				);
			}

				function childAgentContextFacts(summary) {
					if (!summary) {
						return [];
					}

					const latestInput = formatCompactCount(summary.input_tokens_current);
					const peakInput = formatCompactCount(summary.input_tokens_max);
					const inputWindowsMatch = childAgentInputWindowsMatch(summary);
					const facts = [
						["current window", latestInput, "Current context window from the latest child-agent event."],
					];

					if (!inputWindowsMatch) {
						facts.push([
							"peak window",
							peakInput,
							"Largest observed context window for this lane.",
						]);
					}

					return facts;
				}

			function childAgentLargeOutputWarnings(summary) {
				return Array.isArray(summary?.large_output_warnings)
					? summary.large_output_warnings.filter(Boolean)
					: [];
			}
			function renderChildContextFacts(facts) {
				return facts
					.map(([label, value, title]) => {
						const titleAttribute = title ? ` title="${escapeHtml(title)}"` : "";

						return `<span${titleAttribute}>${escapeHtml(detailLabel(label))} <strong>${escapeHtml(value)}</strong></span>`;
					})
					.join("");
			}

			function runProjectSummary(run) {
				return displayToken(run?.project_display_name || run?.project_id || "project");
			}

			function lifecycleMetricSegment(value, label = "") {
				if (value == null || value === "") {
					return null;
				}

				return {
					value: String(value),
					label: label ? String(label) : "",
				};
			}

			function appendLifecycleMetricSegment(segments, value, label = "") {
				const segment = lifecycleMetricSegment(value, label);
				if (segment) {
					segments.push(segment);
				}
			}
