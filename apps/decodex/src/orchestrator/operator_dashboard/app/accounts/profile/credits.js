

				function codexAccountCreditsSummary(account) {
					if (!account) {
						return null;
					}
					const balance = formatCodexAccountCreditsBalance(account.credits_balance);
					if (account.credits_unlimited === true) {
						return "Unlimited";
					}
					if (account.credits_has_credits === false) {
						return "0.00";
					}
					if (balance) {
						return balance;
					}
					if (account.credits_has_credits === true) {
						return "-";
					}

					return null;
				}

				function formatCodexAccountCreditsBalance(value) {
					if (value == null) {
						return "";
					}
					const raw = String(value).trim();
					if (!raw) {
						return "";
					}
					const number = Number(raw);
					if (!Number.isFinite(number)) {
						return raw;
					}
					return number.toFixed(2);
				}

			function codexAccountCreditsTone(account) {
				if (!account) {
					return "";
				}
				if (codexAccountReachedType(account).includes("credit")) {
					return "danger";
				}

					return "";
				}