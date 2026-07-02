			function issueDisplayKey(item) {
				if (!item) {
					return "unknown";
				}
				if (item.issue_identifier) {
					return item.issue_identifier;
				}
				const runIssueIdentifier = issueIdentifierFromRunId(item.run_id);
				if (runIssueIdentifier) {
					return runIssueIdentifier;
				}
				for (const value of [item.worktree_path, item.branch_name]) {
					const identifier = issueIdentifierInText(value);
					if (identifier) {
						return identifier;
					}
				}

				return item.issue_id || "unknown";
			}

			function canonicalIssueIdentityKey(value) {
				const key = String(value ?? "").trim();

				if (!key || key.toLowerCase() === "unknown") {
					return "";
				}

				return key.toUpperCase();
			}

			function issueIdentityKeys(item) {
				if (!item) {
					return [];
				}

				const keys = [item.issue_id, item.issue_identifier, issueDisplayKey(item)]
					.map(canonicalIssueIdentityKey)
					.filter(Boolean);

				return [...new Set(keys)];
			}

			function issueMatchesKeySet(item, keySet) {
				return issueIdentityKeys(item).some((key) => keySet.has(key));
			}
