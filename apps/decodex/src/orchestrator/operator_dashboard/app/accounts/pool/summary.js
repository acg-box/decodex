

				function renderCodexAccountPoolUsageSummary(accounts) {
					const estimate = accountPoolUsageEstimate();
					if (!estimate) {
						return "";
					}

					const used = codexAccountNumber(estimate.total_used_of_capacity_percent);
					const delta = accountPoolDayDeltaPercentagePoints(accounts, estimate);
					const metrics = [
						{
							label: "Pool used",
							value: formatUsagePercent(used),
							tone: accountPoolUsageTone(used),
						},
						{
							label: "Day Δ",
							value: delta == null ? "-" : formatPercentagePointDelta(delta),
							tone: accountPoolDayDeltaTone(delta, used),
						},
						{
							label: "Daily avg",
							value: formatDailyUsageRate(estimate.average_daily_pool_percent),
							tone: "muted",
						},
					];
					const profileAggregate = codexAccountProfileAggregate(accounts);
					const profileLabel = new Map([
						["tok", "Lifetime tok"],
						["peak", "Peak day"],
						["streak", "Streak"],
						["task", "Longest task"],
					]);
					const profileMetrics = profileAggregate
						? codexAccountProfileMetaFacts(profileAggregate).map(([key, value]) => ({
								label: profileLabel.get(key) || key,
								value,
								tone: key === "tok" ? "run" : "muted",
							}))
						: [];
					metrics.push(...profileMetrics);
					const measured = Number(estimate.account_estimate_count || 0);
					const accountCount = Number(estimate.account_count || 0);
					const note =
						accountCount > 0 && measured > 0 && measured < accountCount
							? `<span class="account-pool-summary-note">${escapeHtml(`${measured}/${accountCount} accounts measured`)}</span>`
							: "";
					const activityStrip = profileAggregate
						? renderCodexAccountPoolActivityStrip(profileAggregate)
						: "";
					if (activityStrip) {
						metrics.push({
							label: "Activity",
							value: "token history",
							tone: "muted",
							valueHtml: activityStrip,
						});
					}
					const label = `Pool usage: ${metrics.map((metric) => `${metric.label} ${metric.value}`).join(", ")}`;

					return `
						<div class="account-pool-summary" data-render-key="account-pool-summary" aria-label="${escapeHtml(label)}">
							${metrics
								.map(
									(metric) => `
										<div class="account-pool-metric">
											<span class="account-pool-metric-label">${escapeHtml(metric.label)}</span>
											${
												metric.valueHtml
													? metric.valueHtml
													: `<strong class="account-pool-metric-value" data-tone="${escapeHtml(metric.tone)}">${escapeHtml(metric.value)}</strong>`
											}
										</div>
									`,
								)
								.join("")}
							${note}
						</div>
					`;
				}