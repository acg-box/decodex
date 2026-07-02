

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