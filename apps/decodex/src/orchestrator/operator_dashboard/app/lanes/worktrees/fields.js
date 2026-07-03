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
