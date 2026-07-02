

				function accountPoolUsageEstimate() {
					return accountApiSnapshot?.usage_estimate || null;
				}

				function accountPoolDayDeltaPercentagePoints(accounts, estimate) {
					const measuredAccounts = accounts.filter(
						(account) => codexAccountNumber(account?.seven_day_used_percent) != null,
					);
					const totalCapacity = codexAccountNumber(estimate?.total_capacity_percent);
					const currentPoolUsed = codexAccountNumber(estimate?.total_used_of_capacity_percent);
					if (!measuredAccounts.length || !totalCapacity || currentPoolUsed == null) {
						return null;
					}

					const latestDate = measuredAccounts
						.flatMap(codexAccountUsageRecords)
						.map((record) => record.date)
						.sort()
						.at(-1);
					if (!latestDate) {
						return currentPoolUsed;
					}

					const previousDate = previousUsageDate(latestDate);
					if (!previousDate) {
						return currentPoolUsed;
					}

					const previousUsedPercent = measuredAccounts.reduce((total, account) => {
						const record = usageRecordForDate(account, previousDate);
						const used = codexAccountNumber(record?.used_percent) || 0;

						return (
							total + used * codexAccountUsageRecordCapacityMultiplier(account, record)
						);
					}, 0);
					const previousPoolPercent = (previousUsedPercent / totalCapacity) * 100;

					return currentPoolUsed - previousPoolPercent;
				}

				function accountPoolUsageTone(used) {
					if (used == null || used === "") {
						return "muted";
					}
					const value = Number(used);
					if (!Number.isFinite(value)) {
						return "muted";
					}
					if (value >= 90) {
						return "danger";
					}
					if (value >= 75) {
						return "warning";
					}

					return "run";
				}

				function accountPoolDayDeltaTone(delta, used) {
					if (delta == null || delta === "") {
						return "muted";
					}
					const value = Number(delta);
					if (!Number.isFinite(value) || Math.abs(value) <= 0.05) {
						return "muted";
					}
					if (value < -0.05) {
						return "muted";
					}

					return accountPoolUsageTone(used);
				}