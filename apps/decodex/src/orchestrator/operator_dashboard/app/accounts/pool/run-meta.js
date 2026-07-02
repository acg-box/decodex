

				function renderRunCodexAccountInline(run, snapshot) {
					const capturedAccount = codexAccount(run);
					const account = capturedAccount || selectedDashboardAccount(snapshot);
					if (!account) {
						return `
							<span class="run-meta-item is-account is-missing" aria-label="account">
								<span class="run-meta-icon" aria-hidden="true">
									<svg viewBox="0 0 16 16" fill="none">
										<path fill="currentColor" fill-rule="evenodd" clip-rule="evenodd" d="M3.35 2.25h9.3c.61 0 1.1.49 1.1 1.1v9.3c0 .61-.49 1.1-1.1 1.1h-9.3c-.61 0-1.1-.49-1.1-1.1v-9.3c0-.61.49-1.1 1.1-1.1ZM8 4.15a1.8 1.8 0 1 1 0 3.6 1.8 1.8 0 0 1 0-3.6Zm0 4.78c2.02 0 3.26.96 3.7 2.78.08.35-.18.67-.54.67H4.84c-.36 0-.62-.32-.54-.67.44-1.82 1.68-2.78 3.7-2.78Z"></path>
									</svg>
								</span>
								<strong>not captured</strong>
							</span>
						`;
					}

					const displayAccount = codexAccountDisplaySource(account, snapshot);
					const displayTitle = codexAccountDisplayTitle(displayAccount);
					const visibleName = codexAccountVisibleName(displayAccount);
					const identityClass = codexAccountShowsEmail(displayAccount) ? " is-machine" : "";
					const pendingTitle = capturedAccount
						? displayTitle
						: `${displayTitle || visibleName} · run account capture pending`;

					return `
						<span class="run-meta-item is-account" aria-label="account">
							<span class="run-meta-icon" aria-hidden="true">
								<svg viewBox="0 0 16 16" fill="none">
									<path fill="currentColor" fill-rule="evenodd" clip-rule="evenodd" d="M3.35 2.25h9.3c.61 0 1.1.49 1.1 1.1v9.3c0 .61-.49 1.1-1.1 1.1h-9.3c-.61 0-1.1-.49-1.1-1.1v-9.3c0-.61.49-1.1 1.1-1.1ZM8 4.15a1.8 1.8 0 1 1 0 3.6 1.8 1.8 0 0 1 0-3.6Zm0 4.78c2.02 0 3.26.96 3.7 2.78.08.35-.18.67-.54.67H4.84c-.36 0-.62-.32-.54-.67.44-1.82 1.68-2.78 3.7-2.78Z"></path>
								</svg>
							</span>
							<strong class="account-name${identityClass}" title="${escapeHtml(pendingTitle)}">${escapeHtml(visibleName)}</strong>
						</span>
					`;
				}

				function renderRunMetaLine(run, snapshot = null) {
					const items = [
						renderRunCodexAccountInline(run, snapshot),
						renderRunTelemetryMetaItems(run),
					]
						.filter(Boolean)
						.join("");

					if (!items) {
						return "";
					}

					return `<div class="run-meta-line" aria-label="Lane metadata">${items}</div>`;
				}

				function renderCodexAccountBand(run) {
					return renderRunMetaLine(run);
				}