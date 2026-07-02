

				function renderCodexAccountPool(accounts, snapshot) {
					if (!accounts.length) {
						return "";
					}

					return `
						${renderCodexAccountPoolUsageSummary(accounts)}
						<div class="account-pool-list" data-render-key="account-pool-list" aria-label="Accounts">
							<div class="account-pool-guide">
								${ACCOUNT_POOL_SORT_COLUMNS.map(renderCodexAccountPoolGuideCell).join("")}
							</div>
							${accounts.map((account, index) => renderCodexAccountPoolRow(account, snapshot, index === accounts.length - 1)).join("")}
						</div>
					`;
				}