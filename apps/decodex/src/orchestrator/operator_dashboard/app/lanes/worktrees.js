			function worktreeRoleMeta(worktree, snapshot) {
				const hygiene = worktree.hygiene;
				const currentLaneMatch = snapshotCurrentLaneRuns(snapshot).find(
					(run) =>
						(run.worktree_path === worktree.worktree_path ||
							run.branch_name === worktree.branch_name ||
							run.issue_id === worktree.issue_id),
				);
				const reviewMatch = (snapshot?.post_review_lanes ?? []).find(
					(lane) =>
						lane.worktree_path === worktree.worktree_path ||
						lane.branch_name === worktree.branch_name ||
						lane.issue_id === worktree.issue_id ||
						lane.issue_identifier === worktree.issue_id,
				);

				if (currentLaneMatch) {
					return {
						sortRank: 0,
						tone: "tone-run",
						label: "current lane",
						summary: worktree.ownership_reason || "Leased by a running lane.",
					};
				}
				if (reviewMatch) {
					if (hygiene) {
						const isDirty = hygiene.classification === "merged_dirty_worktree" || hygiene.dirty === true;

						return {
							sortRank: 1,
							tone: isDirty ? "tone-wait" : "tone-retained",
							label: isDirty ? "post-review cleanup blocked" : "post-review cleanup",
							summary:
								hygiene.reason ||
								worktree.ownership_reason ||
								"Post-review cleanup pending.",
						};
					}

					return {
						sortRank: 1,
						tone: toneForLane(reviewMatch),
						label: `post-review ${displayToken(reviewMatch.classification)}`,
						summary:
							worktree.ownership_reason ||
							"Retained for review, landing, or closeout.",
					};
				}
				const queueMatch = (snapshot?.queued_candidates ?? []).find((candidate) => {
					const attentionWorktree = candidate.attention?.worktree_path;

					return (
						(candidate.reason === "issue_needs_attention" ||
							candidate.reason === "linear_active_label_present") &&
						(attentionWorktree === worktree.worktree_path ||
							candidate.issue_id === worktree.issue_id ||
							candidate.issue_identifier === worktree.issue_id)
					);
				});

				if (queueMatch) {
					return {
						sortRank: 0,
						tone: "tone-blocked",
						label: "queued attention",
						summary:
							worktree.ownership_reason ||
							"Owned by Intake Queue attention; recover there before cleanup.",
					};
				}
				if (worktree.ownership === "current_lane") {
					return {
						sortRank: 0,
						tone: "tone-run",
						label: "current lane",
						summary: worktree.ownership_reason || "Leased by a running lane.",
					};
				}
				if (worktree.ownership === "queued_attention") {
					return {
						sortRank: 0,
						tone: "tone-blocked",
						label: "queued attention",
						summary:
							worktree.ownership_reason ||
							"Owned by Intake Queue attention; recover there before cleanup.",
					};
				}
				if (worktree.ownership === "post_review_lane") {
					if (hygiene) {
						const isDirty = hygiene.classification === "merged_dirty_worktree" || hygiene.dirty === true;

						return {
							sortRank: 1,
							tone: isDirty ? "tone-wait" : "tone-retained",
							label: isDirty ? "post-review cleanup blocked" : "post-review cleanup",
							summary:
								hygiene.reason ||
								worktree.ownership_reason ||
								"Post-review cleanup pending.",
						};
					}

					return {
						sortRank: 1,
						tone: "tone-retained",
						label: "post-review retained",
						summary:
							worktree.ownership_reason ||
							"Retained for review, landing, or closeout.",
					};
				}
				if (hygiene) {
					const isDirty = hygiene.classification === "merged_dirty_worktree" || hygiene.dirty === true;

					return {
						sortRank: 2,
						tone: isDirty ? "tone-wait" : "tone-retained",
						label: isDirty ? "post-land cleanup blocked" : "post-land cleanup",
						summary:
							hygiene.reason ||
							worktree.ownership_reason ||
							"Post-land cleanup pending.",
					};
				}
				if (worktree.ownership === "post_land_cleanup") {
					return {
						sortRank: 2,
						tone: "tone-retained",
						label: "post-land cleanup",
						summary:
							worktree.ownership_reason ||
							"Post-land cleanup pending.",
					};
				}
				if (worktree.provenance?.audit_required) {
					return {
						sortRank: 2,
						tone: "tone-blocked",
						label: "legacy cleanup audit",
						summary:
							worktree.ownership_reason ||
							"Legacy worktree provenance is missing; verify terminal state before cleanup.",
					};
				}
				return {
					sortRank: 2,
					tone: "tone-recovery",
					label: "local cleanup",
					summary:
						worktree.ownership_reason ||
						"No lane owns this worktree; inspect before cleanup.",
				};
			}

			function renderWorktreeHygieneFields(worktree) {
				const hygiene = worktree.hygiene;
				if (!hygiene) {
					return "";
				}

				return `
					${field("Cleanup state", displayToken(hygiene.classification || "cleanup_pending"))}
					${field("Default branch", hygiene.default_branch || "unknown")}
					${field("Uncommitted changes", hygiene.dirty ? "yes" : "no")}
				`;
			}

			function renderWorktreeProvenanceFields(worktree) {
				const provenance = worktree.provenance;
				if (!provenance) {
					return "";
				}

				const createdAt = unixEpochSecondsToIso(provenance.created_at_unix) || "unknown";
				const updatedAt = unixEpochSecondsToIso(provenance.updated_at_unix) || "unknown";
				const audit = provenance.audit_required ? field("Audit", "required") : "";
				const nextAction = worktree.recovery_next_action
					? field("Next action", worktree.recovery_next_action)
					: "";

				return `
					${field("Provenance", displayToken(provenance.source || "unknown"))}
					${field("Recorded", createdAt)}
					${field("Refreshed", updatedAt)}
					${audit}
					${nextAction}
				`;
			}

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
