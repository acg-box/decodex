

				function codexAccountShowsEmail(account) {
					return Boolean(codexAccountEmail(account) && !accountEmailsHidden);
				}

				function renderCodexAccountRandomNameButton(account) {
					if (codexAccountShowsEmail(account)) {
						return "";
					}

					return `
						<button class="account-name-reroll" type="button" data-account-name-reroll="${escapeHtml(codexAccountRandomNameKey(account))}" aria-label="Change account name" title="Change account name">
							<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">
								<path d="m18 14 4 4-4 4"></path>
								<path d="m18 2 4 4-4 4"></path>
								<path d="M2 18h1.6c1.4 0 2.7-.7 3.5-1.8l5.8-8.4C13.7 6.7 15 6 16.4 6H22"></path>
								<path d="M2 6h1.6c1.4 0 2.7.7 3.5 1.8l.7 1"></path>
								<path d="M12.8 16.2c.8 1.1 2.1 1.8 3.5 1.8H22"></path>
							</svg>
						</button>
					`;
				}

				function renderCodexAccountNameControl(account, snapshot) {
					const selector = codexAccountControlSelector(account);
					const displayTitle = codexAccountDisplayTitle(account);
					const visibleName = codexAccountVisibleName(account);
					if (!selector) {
						return `<strong class="account-name" title="${escapeHtml(displayTitle)}">${escapeHtml(visibleName)}</strong>`;
					}

					const fixed = codexAccountMatchesConfiguredSelector(account, snapshot);
					const action = fixed ? "clearAccountSelection" : "selectAccount";
					const armed = accountSelectionConfirmationMatches(action, selector);
					const fixedClass = fixed ? " is-fixed" : "";
					const armedClass = armed ? " is-armed" : "";
					const title = accountSelectionControlTitle(action, displayTitle, armed);

					return `
						<button class="account-name-button${fixedClass}${armedClass}" type="button" data-account-confirm-action="${escapeHtml(action)}" data-account-selector="${escapeHtml(selector)}" data-account-display-title="${escapeHtml(displayTitle)}" aria-pressed="${fixed ? "true" : "false"}" aria-label="${escapeHtml(title)}" title="${escapeHtml(title)}">
							<span class="account-name">${escapeHtml(visibleName)}</span>
						</button>
					`;
				}

				function codexAccountDisplayName(account) {
					const email = codexAccountEmail(account);
					return codexAccountShowsEmail(account) ? email : codexAccountRandomName(account);
				}

				function codexAccountVisibleName(account) {
					const email = codexAccountEmail(account);
					return codexAccountShowsEmail(account)
						? compactAccountEmail(email)
						: codexAccountRandomName(account);
				}

				function codexAccountDisplayTitle(account) {
					if (codexAccountShowsEmail(account)) {
						return codexAccountEmail(account);
					}

					const identity = codexAccountIdentity(account);
					return identity
						? compactAccountIdentity(identity)
						: codexAccountDisplayName(account);
				}

				function codexAccountFallbackName(value) {
					const selector = String(value || "").trim();
					if (!selector) {
						return "account";
					}

					return accountEmailsHidden
						? codexAccountRandomName({ account_fingerprint: selector })
						: compactAccountIdentity(selector);
				}