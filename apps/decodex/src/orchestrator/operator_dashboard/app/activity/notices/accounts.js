				function codexAccountHasNotice(account) {
					if (!account) {
						return false;
					}

					const status = String(account.status || "").toLowerCase();
					const note = codexAccountNote(account);
					return Boolean(
						codexAccountRefreshFailed(account) ||
							codexAccountNoteLooksError(note) ||
							status.includes("failed") ||
							status.includes("unusable"),
					);
				}

				function codexAccountNoticeTitle(account) {
					if (codexAccountRefreshFailed(account)) {
						return "Codex account token";
					}
					if (codexAccountNoteLooksError(codexAccountNote(account))) {
						return "Codex account usage";
					}

					return "Codex account";
				}

				function codexAccountNoticeCopy(account) {
					const note = codexAccountNote(account);
					const parts = [];
					const noteIncludesRefreshFailure = /refresh failed|token refresh failed/i.test(note);
					if (note && !codexAccountNoteLooksRoutine(note)) {
						parts.push(codexAccountPrivacyText(account, note));
					}
					if (codexAccountRefreshFailed(account) && !noteIncludesRefreshFailure) {
						parts.unshift(codexAccountTokenLabel(account.refresh_status));
					}
					if (!parts.length) {
						parts.push(codexAccountStatusLabel(account));
					}

					return `${codexAccountPrivacyLabel(account)}: ${parts.join("; ")}`;
				}

				function codexAccountNotices(snapshot) {
					const notices = [];
					const seen = new Set();
					for (const account of codexAccountPoolAccounts(snapshot)) {
						if (!codexAccountHasNotice(account)) {
							continue;
						}
						const notice = {
							tone: "danger",
							title: codexAccountNoticeTitle(account),
							copy: codexAccountNoticeCopy(account),
						};
						const key = `${notice.title}:${notice.copy}`;
						if (seen.has(key)) {
							continue;
						}
						seen.add(key);
						notices.push(notice);
					}

					return notices;
				}
