

				function codexAccountResetDistance(value) {
					const seconds = codexAccountNumber(value);
					if (seconds == null || seconds <= 0) {
						return { short: "unknown", phrase: "remaining unknown", isPast: false };
					}

					const resetAt = new Date(seconds * 1000);
					if (Number.isNaN(resetAt.getTime())) {
						return { short: "unknown", phrase: "remaining unknown", isPast: false };
					}

					const distanceSeconds = Math.floor((resetAt.getTime() - Date.now()) / 1000);
					if (distanceSeconds <= 0) {
						return { short: "0m", phrase: "reset due now", isPast: true };
					}

					const short = formatCodexAccountResetDuration(distanceSeconds);
					return { short, phrase: `resets in ${short}`, isPast: false };
				}

				function codexAccountResetDisplay(data) {
					const resetAt = codexAccountUnixTimestamp(data.resetAt);
					const distance = codexAccountResetDistance(data.resetAt);
					if (resetAt === "unknown" && distance.short === "unknown") {
						return {
							short: "-",
							date: "",
							aria: "reset unavailable",
						};
					}

					return {
						short: distance.short,
						date: resetAt,
						aria:
							distance.short === "unknown"
								? `reset at ${resetAt}, remaining unknown`
								: `reset at ${resetAt}, ${distance.phrase}`,
					};
				}

				function codexAccountWindowLabel(seconds) {
					const value = codexAccountNumber(seconds);
					if (value == null) {
						return "window";
					}
					if (value === 18_000) {
						return "5h";
					}
					if (value === 604_800) {
						return "7d";
					}

					return formatDuration(value);
				}

				function codexAccountWindowData(account, prefix) {
					return {
						windowSeconds: account?.[`${prefix}_window_seconds`],
						remainingPercent: codexAccountPercent(account?.[`${prefix}_remaining_percent`]),
						resetAt: account?.[`${prefix}_resets_at_unix_epoch`],
					};
				}

				function codexAccountWindowTone(percent) {
					if (percent == null) {
						return "";
					}
					if (percent <= 10) {
						return "danger";
					}
					if (percent <= 25) {
						return "warn";
					}

					return "";
				}