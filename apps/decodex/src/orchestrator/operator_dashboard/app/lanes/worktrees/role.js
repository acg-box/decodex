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
