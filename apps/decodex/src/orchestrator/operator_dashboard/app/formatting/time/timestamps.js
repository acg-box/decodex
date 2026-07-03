			function formatTimestamp(value) {
				if (!value) {
					return "none";
				}

				const parsed = new Date(value);
				if (Number.isNaN(parsed.getTime())) {
					return String(value);
				}

				return new Intl.DateTimeFormat(undefined, {
					dateStyle: "medium",
					timeStyle: "medium",
				}).format(parsed);
			}

			function formatTimestampCompact(value) {
				if (!value) {
					return "none";
				}

				const parsed = new Date(value);
				if (Number.isNaN(parsed.getTime())) {
					return String(value);
				}

				return new Intl.DateTimeFormat(undefined, {
					dateStyle: "medium",
					timeStyle: "short",
				}).format(parsed);
			}

			function formatRelativeTimestamp(value) {
				if (!value) {
					return "none";
				}

				const parsed = new Date(value);
				if (Number.isNaN(parsed.getTime())) {
					return String(value);
				}

				const seconds = Math.max(0, Math.floor((Date.now() - parsed.getTime()) / 1000));
				if (seconds < 5) {
					return "0s";
				}
				if (seconds < 60) {
					return `${seconds}s`;
				}

				const minutes = Math.floor(seconds / 60);
				if (minutes < 60) {
					return `${minutes}m`;
				}

				const hours = Math.floor(minutes / 60);
				if (hours < 24) {
					return `${hours}h`;
				}

				const days = Math.floor(hours / 24);
				return `${days}d`;
			}
