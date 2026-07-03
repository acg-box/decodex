				function warningDetailsFor(warning, snapshot) {
					return (snapshot?.warning_details ?? []).filter((detail) => detail?.warning === warning);
				}

				function warningNotice(warning, snapshot) {
					const details = warningDetailsFor(warning, snapshot);
					if (warning === "worktree_hygiene_unavailable" && details.length) {
						return {
							tone: "warning",
							title: "Worktree hygiene unavailable",
							copy: details.map(worktreeHygieneWarningCopy).join(" "),
						};
					}

					return {
						tone: "warning",
						title: "Snapshot warning",
						copy: displayToken(warning),
					};
				}

				function worktreeHygieneWarningCopy(detail) {
					const project = detail.project_id || "project";
					const repo = detail.repo_root ? ` Repo: ${detail.repo_root}.` : "";
					const reason = detail.reason || "Worktree hygiene scan failed.";
					const nextAction = detail.next_action ? ` ${detail.next_action}` : "";

					return `${project}: ${reason}.${repo}${nextAction}`;
				}
