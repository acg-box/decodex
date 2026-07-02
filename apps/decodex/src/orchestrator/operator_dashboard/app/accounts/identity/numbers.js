

				function codexAccountNumber(value) {
					if (value == null) {
						return null;
					}

					const number = Number(value);
					return Number.isFinite(number) ? number : null;
				}

				function codexAccountPercent(value) {
					const number = codexAccountNumber(value);
					if (number == null) {
						return null;
					}

					return Math.max(0, Math.min(100, Math.round(number)));
				}

				function formatUsagePercent(value) {
					if (value == null || value === "") {
						return "-";
					}
					const number = Number(value);
					if (!Number.isFinite(number)) {
						return "-";
					}

					const rounded = Math.round(number);
					if (Math.abs(number - rounded) < 0.05) {
						return `${rounded}%`;
					}

					return `${number.toFixed(1)}%`;
				}

				function formatDailyUsageRate(value) {
					const percent = formatUsagePercent(value);
					return percent === "-" ? "-" : `${percent}/d`;
				}

				function formatPercentagePointDelta(value) {
					if (value == null || value === "") {
						return "-";
					}
					const number = Number(value);
					if (!Number.isFinite(number)) {
						return "-";
					}

					const absValue = Math.abs(number);
					const sign = number > 0.05 ? "+" : number < -0.05 ? "-" : "";
					const rounded = Math.round(absValue);
					if (Math.abs(absValue - rounded) < 0.05) {
						return `${sign}${rounded}pp`;
					}

					return `${sign}${absValue.toFixed(1)}pp`;
				}
