				function renderCodexAccountActivityStrip(
					account,
					labelPrefix,
					stripClass,
					tileClass,
				) {
					const records = codexAccountProfileDailyUsage(account).slice(-35);
					if (!records.length) {
						return "";
					}
					const peak = records.reduce((max, record) => Math.max(max, record.tokens), 0);
					const title = `${labelPrefix}: ${records.length} days, peak ${formatCompactCount(peak)} tokens`;

					return `
						<div class="${escapeHtml(stripClass)}" aria-label="${escapeHtml(title)}" title="${escapeHtml(title)}">
							${records
								.map((record) => {
									const ratio = peak > 0 ? record.tokens / peak : 0;
									const opacity = record.tokens > 0 ? 0.24 + ratio * 0.76 : 0.12;
									const tileTitle = `${record.date}: ${formatCompactCount(record.tokens)} tokens`;
									return `<span class="${escapeHtml(tileClass)}" style="--account-activity-opacity: ${opacity.toFixed(2)}" title="${escapeHtml(tileTitle)}" aria-hidden="true"></span>`;
								})
								.join("")}
						</div>
					`;
				}

				function renderCodexAccountPoolActivityStrip(account) {
					return renderCodexAccountActivityStrip(
						account,
						"Pool token activity",
						"account-pool-activity-strip",
						"account-pool-activity-tile",
					);
				}

				function renderCodexAccountProfileActivityStrip(account) {
					return renderCodexAccountActivityStrip(
						account,
						"Account token activity",
						"account-profile-activity-strip",
						"account-profile-activity-tile",
					);
				}