

				function codexAccountPoolAccounts() {
					return sortCodexAccountPoolAccounts(
						accountApiAccounts().map((account) => ({ ...account })),
					);
				}

				function codexAccountCreditsSortValue(account) {
					if (!account) {
						return null;
					}
					if (account.credits_unlimited === true) {
						return Number.POSITIVE_INFINITY;
					}
					if (account.credits_has_credits === false) {
						return 0;
					}

					return codexAccountNumber(account.credits_balance);
				}

				function codexAccountPoolColumnSortValue(account, key) {
					if (key === "account") {
						return codexAccountDisplayName(account).toLowerCase();
					}
					if (key === "plan") {
						return codexAccountCapacityMultiplier(account);
					}
					if (key === "primary") {
						return codexAccountWindowData(account, "primary").remainingPercent;
					}
					if (key === "secondary") {
						return codexAccountWindowData(account, "secondary").remainingPercent;
					}
					if (key === "credits") {
						return codexAccountCreditsSortValue(account);
					}
					if (key === "status") {
						return codexAccountStatusLabel(account).toLowerCase();
					}

					return "";
				}

				function compareCodexAccountPoolColumn(left, right, key, direction) {
					const leftValue = codexAccountPoolColumnSortValue(left, key);
					const rightValue = codexAccountPoolColumnSortValue(right, key);
					const leftMissing = leftValue == null || leftValue === "";
					const rightMissing = rightValue == null || rightValue === "";
					if (leftMissing && rightMissing) {
						return 0;
					}
					if (leftMissing) {
						return 1;
					}
					if (rightMissing) {
						return -1;
					}

					const delta =
						typeof leftValue === "number" && typeof rightValue === "number"
							? leftValue === rightValue
								? 0
								: leftValue < rightValue
									? -1
									: 1
							: String(leftValue).localeCompare(String(rightValue));

					return direction === "desc" ? -delta : delta;
				}

				function sortCodexAccountPoolAccounts(accounts) {
					if (!accountPoolSort.key) {
						return accounts;
					}

					return accounts.sort((left, right) => {
						const columnDelta = compareCodexAccountPoolColumn(
							left,
							right,
							accountPoolSort.key,
							accountPoolSort.direction,
						);
						if (columnDelta) {
							return columnDelta;
						}

						return 0;
					});
				}