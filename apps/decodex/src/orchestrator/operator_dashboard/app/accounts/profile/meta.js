

				function codexAccountWindowSummary(account, prefix) {
					const data = codexAccountWindowData(account, prefix);
					const label = codexAccountWindowLabel(data.windowSeconds);
					const remaining = data.remainingPercent == null ? "unknown" : `${data.remainingPercent}%`;
					const reset = codexAccountUnixTimestamp(data.resetAt);

					return `${label} ${remaining} reset ${reset}`;
				}

				function codexAccountMetaFacts(account) {
					if (!account) {
						return [];
					}
					const refreshStatus = String(account.refresh_status || "").toLowerCase();
					const note = codexAccountNote(account);
					const noteLooksRoutine = codexAccountNoteLooksRoutine(note);
					const noteLooksError = codexAccountNoteLooksError(note);

					const facts = [
						codexAccountRefreshStatusNeedsAttention(refreshStatus) &&
							!codexAccountRefreshFailed(account)
							? ["token", codexAccountTokenValue(account.refresh_status)]
							: null,
						account.cooldown_until_unix_epoch
							? ["cooldown", codexAccountUnixTimestamp(account.cooldown_until_unix_epoch)]
							: null,
						note && !noteLooksRoutine && !noteLooksError
							? ["note", codexAccountPrivacyText(account, note)]
							: null,
					];

					return facts.filter(Boolean);
				}