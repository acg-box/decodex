			function recoveryWorktreeShouldDefaultOpen(renderedWorktree) {
				const role = renderedWorktree.role;

				return role.tone === "tone-blocked";
			}

			function renderWorktrees(snapshot) {
				const worktrees = snapshot?.worktrees ?? [];
				const renderedWorktrees = worktrees
					.map((worktree) => ({ worktree, role: worktreeRoleMeta(worktree, snapshot) }))
					.sort((left, right) => {
						const rankDelta = left.role.sortRank - right.role.sortRank;
						if (rankDelta) {
							return rankDelta;
						}
						return (
							String(left.worktree.issue_id).localeCompare(String(right.worktree.issue_id)) ||
							String(left.worktree.branch_name).localeCompare(String(right.worktree.branch_name)) ||
							String(left.worktree.worktree_path).localeCompare(String(right.worktree.worktree_path))
						);
					});
				const retainedWorktrees = renderedWorktrees.filter(({ role }) => role.sortRank > 0);
				setFoldPanelEmpty(nodes.panels.worktrees, !retainedWorktrees.length);
				syncDefaultDetailOpenState(
					nodes.panels.worktrees,
					retainedWorktrees.some(recoveryWorktreeShouldDefaultOpen),
				);

				setPanelMeta(
					nodes.worktreesMeta,
					retainedWorktrees.length
						? pluralize(retainedWorktrees.length, "worktree")
						: "0 worktrees",
				);

				if (!retainedWorktrees.length) {
					nodes.worktrees.innerHTML = "";
					return;
				}

				nodes.worktrees.innerHTML = retainedWorktrees
					.map(({ worktree, role }) => {
						const issueKey = issueDisplayKey(worktree);

						return `
							<article class="worktree-card ${role.tone}">
								<div class="row-head">
									<div class="row-title">
										<div class="kicker">
											<span>Issue</span>
											<span class="mono">${escapeHtml(issueKey)}</span>
										</div>
										<h4>${escapeHtml(worktree.branch_name)}</h4>
									</div>
								</div>
								<div class="status-line">
									${statusLabel(role.label, role.tone)}
								</div>
								<p class="row-summary">${escapeHtml(role.summary)}</p>
								<div class="grid two">
									${field("Issue state", worktree.issue_state || "unknown")}
									${field("Ownership", displayToken(worktree.ownership || role.label))}
									${field("Branch", worktree.branch_name)}
									${field("Worktree path", worktree.worktree_path)}
									${renderWorktreeProvenanceFields(worktree)}
									${renderWorktreeHygieneFields(worktree)}
								</div>
							</article>
						`;
					})
					.join("");
			}
