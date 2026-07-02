

				function codexAccountTokenLabel(refreshStatus) {
					const status = String(refreshStatus || "").toLowerCase();
					return status || "token_unknown";
				}

				function codexAccountTokenValue(refreshStatus) {
					return codexAccountTokenLabel(refreshStatus);
				}

				function codexAccountRefreshStatusNeedsAttention(refreshStatus) {
					const status = String(refreshStatus || "").toLowerCase();
					return Boolean(
						status &&
							!["not_needed", "refreshed", "succeeded", "none"].includes(status),
					);
				}

				function codexAccountRefreshFailed(account) {
					return String(account?.refresh_status || "").toLowerCase().includes("failed");
				}

				function codexAccountNote(account) {
					return String(account?.note || "").trim();
				}

				function codexAccountNoteLooksRoutine(note) {
					return String(note || "").trim().toLowerCase() === "usage probe ok";
				}

				function codexAccountNoteLooksError(note) {
					return /\b(failed|error|unauthorized|forbidden|invalid|missing|unusable)\b/i.test(
						String(note || ""),
					);
				}