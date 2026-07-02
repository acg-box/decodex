

				function codexAccountControlStatusLabel(snapshot) {
					const accountControl = snapshot?.account_control || {};
					const selector = String(accountControl.account_selector || "").trim();
					if (!selector) {
						return "Balanced";
					}

					const account = codexAccountPoolAccounts(snapshot).find(
						(candidate) =>
							selector === codexAccountEmail(candidate) ||
							selector === codexAccountFingerprint(candidate),
					);
					if (account) {
						const label = codexAccountShowsEmail(account)
							? compactAccountIdentity(selector)
							: codexAccountVisibleName(account);
						return `Fixed · ${label}`;
					}

					return `Fixed · ${codexAccountFallbackName(selector)}`;
				}

				function renderAccountModeControl(snapshot) {
					const title = codexAccountControlStatusLabel(snapshot);
					nodes.accountModeMeta.innerHTML = `<span class="account-mode-head">${escapeHtml(title)}</span>`;
					nodes.accountModeMeta.title = title;
				}