

				function renderCodexAccountPoolWindow(account, prefix) {
					const data = codexAccountWindowData(account, prefix);
					const label = codexAccountWindowLabel(data.windowSeconds);
					const remaining =
						data.remainingPercent == null ? "-" : `${data.remainingPercent}%`;
					const reset = codexAccountResetDisplay(data);
					const windowTone = codexAccountWindowTone(data.remainingPercent);
					const toneClass = windowTone ? ` is-${windowTone}` : "";
					const isUnreported = data.remainingPercent == null && reset.short === "-";
					const resetTitle = `${label} ${remaining}, ${reset.aria}`;

					if (isUnreported) {
						return `
							<div class="account-window is-${escapeHtml(prefix)}${toneClass}" aria-label="${escapeHtml(label)} usage unavailable" title="${escapeHtml(resetTitle)}">
								<span class="account-window-label" aria-hidden="true">${escapeHtml(label)}</span>
								<strong>-</strong>
							</div>
						`;
					}

					return `
						<div class="account-window is-${escapeHtml(prefix)}${toneClass}" aria-label="${escapeHtml(label)} remaining ${escapeHtml(remaining)}, ${escapeHtml(reset.aria)}" title="${escapeHtml(resetTitle)}">
							<span class="account-window-label" aria-hidden="true">${escapeHtml(label)}</span>
							<strong>${escapeHtml(remaining)}</strong>
							<span class="account-window-reset">${escapeHtml(reset.short)}</span>
							${reset.date ? `<span class="account-window-date">${escapeHtml(reset.date)}</span>` : ""}
						</div>
					`;
				}