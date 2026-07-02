

				function renderCodexAccountPoolSortButton([key, label]) {
					const direction = accountPoolSort.key === key ? accountPoolSort.direction : "";
					const activeClass = direction ? ` is-active is-${direction}` : "";
					const current = direction
						? `currently ${direction === "asc" ? "ascending" : "descending"}`
						: "not sorted";

					return `
						<button class="account-pool-sort${activeClass}" type="button" data-account-sort-key="${escapeHtml(key)}" aria-label="Sort accounts by ${escapeHtml(label)}; ${escapeHtml(current)}" title="Sort by ${escapeHtml(label)}">
							<span class="account-pool-sort-label">${escapeHtml(label)}</span>
							<svg class="account-pool-sort-icon" aria-hidden="true" viewBox="0 0 8 12" fill="currentColor">
								<path class="account-sort-up" d="M4 1 1 5h6Z"></path>
								<path class="account-sort-down" d="M4 11 1 7h6Z"></path>
							</svg>
						</button>
					`;
				}

				function renderCodexAccountPoolGuideCell(column) {
					const [key] = column;
					const sortButton = renderCodexAccountPoolSortButton(column);
					if (key !== "account") {
						return sortButton;
					}

					return `<span class="account-pool-heading">${sortButton}${accountPrivacyToggleMarkup()}</span>`;
				}