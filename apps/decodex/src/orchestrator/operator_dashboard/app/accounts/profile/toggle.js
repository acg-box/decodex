

				function renderCodexAccountProfileToggle(account, expanded) {
					const key = codexAccountProfileExpansionKey(account);
					const label = expanded ? "Hide account profile" : "Show account profile";
					if (!key) {
						return "";
					}

					return `
						<button class="account-profile-toggle" type="button" data-account-profile-toggle="${escapeHtml(key)}" aria-expanded="${expanded ? "true" : "false"}" aria-label="${escapeHtml(label)}" title="${escapeHtml(label)}">
							<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">
								<path d="m6 9 6 6 6-6"></path>
							</svg>
						</button>
					`;
				}