				function codexAccountUsageRecords(account) {
					return Array.isArray(account?.usage_records)
						? account.usage_records.filter((record) => record?.date)
						: [];
				}

				function codexAccountProfileDailyUsage(account) {
					return Array.isArray(account?.profile_daily_usage)
						? account.profile_daily_usage
								.filter((record) => record?.date && codexAccountNumber(record?.tokens) != null)
								.map((record) => ({
									date: String(record.date),
									tokens: Math.max(0, codexAccountNumber(record.tokens) || 0),
								}))
						: [];
				}

				function codexAccountProfilePeakDailyTokens(account) {
					const explicitPeak = codexAccountNumber(account?.profile_peak_daily_tokens);
					if (explicitPeak != null) {
						return explicitPeak;
					}

					return codexAccountProfileDailyUsage(account).reduce(
						(peak, record) => Math.max(peak, record.tokens),
						0,
					) || null;
				}

				function previousUsageDate(value) {
					const match = String(value || "").match(/^(\d{4})-(\d{2})-(\d{2})$/);
					if (!match) {
						return "";
					}

					const date = new Date(Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3])));
					if (Number.isNaN(date.getTime())) {
						return "";
					}
					date.setUTCDate(date.getUTCDate() - 1);

					return date.toISOString().slice(0, 10);
				}

				function usageRecordForDate(account, date) {
					return codexAccountUsageRecords(account)
						.filter((record) => record.date === date)
						.sort(
							(left, right) =>
								Number(right.checked_at_unix_epoch || 0) -
								Number(left.checked_at_unix_epoch || 0),
						)[0] || null;
				}

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

				function codexAccountUnixTimestamp(value) {
					const seconds = codexAccountNumber(value);
					if (seconds == null || seconds <= 0) {
						return "unknown";
					}

					return formatTimestampCompact(new Date(seconds * 1000).toISOString());
				}

				function formatCodexAccountResetDuration(seconds) {
					const value = Math.max(0, Number(seconds));
					if (!Number.isFinite(value)) {
						return "unknown";
					}
					if (value < 60) {
						return "<1m";
					}

					const days = Math.floor(value / 86_400);
					const hours = Math.floor((value % 86_400) / 3_600);
					const minutes = Math.floor((value % 3_600) / 60);
					const parts = [];

					if (days > 0) {
						parts.push(`${days}d`);
						if (hours > 0) {
							parts.push(`${hours}h`);
						}
						return parts.join(" ");
					}

					if (hours > 0) {
						parts.push(`${hours}h`);
					}
					if (minutes > 0 || hours > 0) {
						parts.push(`${minutes}m`);
					}

					return parts.join(" ") || "<1m";
				}

				function formatCodexAccountProfileDuration(seconds) {
					const value = codexAccountNumber(seconds);
					if (value == null) {
						return "";
					}

					return formatCodexAccountResetDuration(value);
				}

				function codexAccountProfileMetaFacts(account) {
					if (!account) {
						return [];
					}
					const currentStreak = codexAccountNumber(account.profile_current_streak_days);
					const longestStreak = codexAccountNumber(account.profile_longest_streak_days);
					const streak =
						currentStreak != null && longestStreak != null
							? `${currentStreak}/${longestStreak}d`
							: currentStreak != null
								? `${currentStreak}d`
								: longestStreak != null
									? `${longestStreak}d`
									: "";
					const task = formatCodexAccountProfileDuration(account.profile_longest_task_seconds);
					const peakDailyTokens = codexAccountProfilePeakDailyTokens(account);
					const facts = [
						codexAccountNumber(account.profile_lifetime_tokens) != null
							? ["tok", formatCompactCount(account.profile_lifetime_tokens)]
							: null,
						peakDailyTokens != null
							? ["peak", formatCompactCount(peakDailyTokens)]
							: null,
						streak ? ["streak", streak] : null,
						task ? ["task", task] : null,
					];

					return facts.filter(Boolean);
				}

				function codexAccountProfileAggregate(accounts) {
					const dailyUsageByDate = new Map();
					let lifetimeTokens = null;
					let peakTokensFallback = null;
					let longestTaskSeconds = null;
					let currentStreakDays = null;
					let longestStreakDays = null;

					for (const account of accounts) {
						const lifetime = codexAccountNumber(account?.profile_lifetime_tokens);
						if (lifetime != null) {
							lifetimeTokens = (lifetimeTokens || 0) + lifetime;
						}
						const peak = codexAccountProfilePeakDailyTokens(account);
						if (peak != null) {
							peakTokensFallback = (peakTokensFallback || 0) + peak;
						}
						const task = codexAccountNumber(account?.profile_longest_task_seconds);
						if (task != null) {
							longestTaskSeconds = Math.max(longestTaskSeconds || 0, task);
						}
						const currentStreak = codexAccountNumber(account?.profile_current_streak_days);
						if (currentStreak != null) {
							currentStreakDays = Math.max(currentStreakDays || 0, currentStreak);
						}
						const longestStreak = codexAccountNumber(account?.profile_longest_streak_days);
						if (longestStreak != null) {
							longestStreakDays = Math.max(longestStreakDays || 0, longestStreak);
						}
						for (const record of codexAccountProfileDailyUsage(account)) {
							dailyUsageByDate.set(record.date, (dailyUsageByDate.get(record.date) || 0) + record.tokens);
						}
					}

					const dailyUsage = Array.from(dailyUsageByDate, ([date, tokens]) => ({ date, tokens }))
						.sort((left, right) => String(left.date).localeCompare(String(right.date)));
					const peakFromDailyUsage = dailyUsage.reduce(
						(peak, record) => Math.max(peak, record.tokens),
						0,
					);
					const peakDailyTokens = peakFromDailyUsage > 0 ? peakFromDailyUsage : peakTokensFallback;
					const aggregate = {
						profile_lifetime_tokens: lifetimeTokens,
						profile_peak_daily_tokens: peakDailyTokens,
						profile_longest_task_seconds: longestTaskSeconds,
						profile_current_streak_days: currentStreakDays,
						profile_longest_streak_days: longestStreakDays,
						profile_daily_usage: dailyUsage,
					};
					const hasMetric = codexAccountProfileMetaFacts(aggregate).length > 0;

					return hasMetric || dailyUsage.length ? aggregate : null;
				}
