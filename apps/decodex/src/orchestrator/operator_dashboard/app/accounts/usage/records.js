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