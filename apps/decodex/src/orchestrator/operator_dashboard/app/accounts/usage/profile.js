

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
