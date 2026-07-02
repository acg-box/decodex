

				function codexAccountReachedType(account) {
					const reached = String(account?.rate_limit_reached_type || "").toLowerCase();
					return reached && reached !== "none" ? reached : "";
				}

				function codexAccountUsageLimited(account) {
					if (!account) {
						return false;
					}

					const status = String(account.status || "").toLowerCase();
					const primary = codexAccountWindowData(account, "primary");
					const secondary = codexAccountWindowData(account, "secondary");

					return Boolean(
						codexAccountReachedType(account) ||
							status.includes("limit") ||
							primary.remainingPercent === 0 ||
							secondary.remainingPercent === 0,
					);
				}

				function codexAccountStatusTone(account) {
					if (!account) {
						return "";
					}

					const status = String(account.status || "").toLowerCase();
					const refresh = String(account.refresh_status || "").toLowerCase();
					const refreshNeedsAttention = codexAccountRefreshStatusNeedsAttention(refresh);

					if (
						codexAccountUsageLimited(account) ||
						status.includes("failed") ||
						status.includes("unusable") ||
						refresh.includes("failed")
					) {
						return "danger";
					}
					if (refreshNeedsAttention) {
						return "warn";
					}
					if (status === "available") {
						return "ready";
					}

					return "";
				}

				function codexAccountStatusLabel(account) {
					if (!account) {
						return "none";
					}

					const reached = codexAccountReachedType(account);
					const status = reached || account.status || "selected";
					const refresh = String(account.refresh_status || "").toLowerCase();
					if (codexAccountUsageLimited(account)) {
						return reached || (String(status).trim() && status !== "available" ? status : "usage_limited");
					}
					if (refresh.includes("failed")) {
						return refresh;
					}
					if (refresh && !["not_needed", "refreshed", "succeeded", "none"].includes(refresh)) {
						return codexAccountTokenValue(account.refresh_status);
					}

					return displayToken(status);
				}