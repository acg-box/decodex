			function formatDuration(seconds) {
				if (seconds == null) {
					return "none";
				}

				const value = Math.max(0, Number(seconds));
				const hours = Math.floor(value / 3600);
				const minutes = Math.floor((value % 3600) / 60);
				const remainingSeconds = Math.floor(value % 60);
				const parts = [];

				if (hours > 0) {
					parts.push(`${hours}h`);
				}
				if (minutes > 0 || hours > 0) {
					parts.push(`${minutes}m`);
				}
				parts.push(`${remainingSeconds}s`);

				return parts.join(" ");
			}

				function formatCompactCount(value) {
					if (value == null) {
						return "none";
					}

					const number = Math.max(0, Number(value));

					if (number >= 1_000_000_000) {
						return `${(number / 1_000_000_000).toFixed(1)}B`;
					}
					if (number >= 1_000_000) {
						return `${(number / 1_000_000).toFixed(2)}M`;
					}
					if (number >= 1_000) {
						return `${(number / 1_000).toFixed(1)}k`;
					}

					return String(Math.floor(number));
				}

			function formatCompactBytes(value) {
				if (value == null) {
					return "none";
				}

				const number = Math.max(0, Number(value));

				if (number >= 1_048_576) {
					return `${(number / 1_048_576).toFixed(1)}MiB`;
				}
				if (number >= 1024) {
					return `${(number / 1024).toFixed(1)}KiB`;
				}

				return `${Math.floor(number)}B`;
			}
