

				function replaceLiteral(value, needle, replacement) {
					const text = String(value || "");
					const target = String(needle || "");
					return target ? text.split(target).join(replacement) : text;
				}

				function codexAccountPrivacyLabel(account) {
					return codexAccountShowsEmail(account)
						? codexAccountEmail(account)
						: codexAccountRandomName(account);
				}

				function codexAccountPrivacyText(account, value) {
					let text = String(value || "");
					if (!text || !accountEmailsHidden) {
						return text;
					}

					const replacement = codexAccountPrivacyLabel(account);
					text = replaceLiteral(text, codexAccountEmail(account), replacement);
					return text.replace(
						/[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}/gi,
						replacement,
					);
				}