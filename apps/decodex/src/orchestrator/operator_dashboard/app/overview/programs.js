			function programMetaText(snapshot, derived) {
				if (!snapshot) {
					return "0 programs";
				}

				const parts = [pluralize(derived.executionPrograms.length, "program")];

				if (derived.programReadyCount) {
					parts.push(`${derived.programReadyCount} ready`);
				}
				if (derived.programDispatchableCount) {
					parts.push(`${derived.programDispatchableCount} dispatchable`);
				}
				if (derived.programAttentionCount) {
					parts.push(
						derived.programAttentionCount === 1
							? "1 needs attention"
							: `${derived.programAttentionCount} need attention`,
					);
				}

				return parts.join(" · ");
			}

			function toneForProgram(program) {
				switch (program.status) {
					case "ready":
					case "completed":
						return "tone-ready";
					case "queued":
						return "tone-queue";
					case "active":
						return "tone-run";
					case "blocked":
					case "attention":
					case "stale":
						return "tone-blocked";
					case "held":
						return "tone-wait";
					default:
						return "tone-muted";
				}
			}

			function programMappedIssues(program) {
				const issues = program.mapped_issue_identifiers ?? [];

				return issues.length ? issues.join(", ") : "NONE";
			}

			function programProgressFacts(program) {
				return [
					["Ready", String(program.ready_count ?? 0)],
					["Queued", String(program.queued_count ?? 0)],
					["Dispatchable", String(program.dispatchable_count ?? 0)],
					["Active", String(program.active_count ?? 0)],
					["Blocked", String(program.blocked_count ?? 0)],
					["Held", String(program.held_count ?? 0)],
					["Attention", String(program.needs_attention_count ?? 0)],
					["Completed", String(program.completed_count ?? 0)],
					["Stale", String(program.stale_count ?? 0)],
				];
			}

			function programNodeReasons(node) {
				const reasons = node.reasons ?? [];
				if (reasons.length) {
					return reasons.join("; ");
				}

				const reasonCodes = node.reason_codes ?? [];
				return reasonCodes.length ? reasonCodes.map(displayToken).join(", ") : "none";
			}

			function programNodeIssue(node) {
				return node.issue_identifier || "unmapped";
			}

			function renderProgramNodeReadbacks(program) {
				const readbacks = program.node_readbacks ?? [];
				if (!readbacks.length) {
					return "";
				}

				const detailKey = `program:${program.program_id}:nodes`;

				return `
					<details data-detail-key="${escapeHtml(detailKey)}"${detailsOpenAttribute(detailKey)}>
						<summary>${escapeHtml(pluralize(readbacks.length, "node diagnostic"))}</summary>
						<div class="grid debug-grid">
							${readbacks
								.map((node) => {
									const reasonCodes = (node.reason_codes ?? []).map(displayToken).join(", ") || "none";
									return `
										${field("Issue", programNodeIssue(node))}
										${field("Program stage", displayToken(node.program_stage || "unknown"))}
										${field("Lifecycle", displayToken(node.lifecycle_state || "unknown"))}
										${field("Readiness", displayToken(node.readiness_state || "unknown"))}
										${field("Issue state", node.issue_state || "none")}
										${field("Dispatch action", node.dispatch_action || "none")}
										${field("Reason codes", reasonCodes)}
										${field("Reasons", programNodeReasons(node))}
										${field("Next action", node.next_action || "none")}
									`;
								})
								.join("")}
						</div>
					</details>
				`;
			}

			function renderExecutionPrograms(snapshot, derived) {
				const programs = derived.executionPrograms ?? [];
				setPanelMeta(
					nodes.programsMeta,
					programMetaText(snapshot, derived),
					derived.programAttentionCount ? "attention" : "",
				);

				if (!programs.length) {
					renderRoutineEmptyList(nodes.executionPrograms);
					return;
				}

				renderStableList(
					nodes.executionPrograms,
					programs
						.map((program) => {
							const tone = toneForProgram(program);
							const mappedIssues = programMappedIssues(program);
							const warning = program.readback_warning
								? inlineStatusFact("Warning", displayToken(program.readback_warning))
								: "";
							return `
								<article class="action-card ${tone}" data-render-key="program:${escapeHtml(program.program_id)}">
									<div class="row-head">
										<div class="row-title">
											<div class="kicker">
												<span>${escapeHtml(displayToken(program.intake_kind || "program"))}</span>
												<span class="mono">${escapeHtml(program.program_id)}</span>
											</div>
											<h4>${escapeHtml(program.public_summary || program.program_id)}</h4>
										</div>
									</div>
									<div class="status-line">
										${statusLabel(displayToken(program.status || "unknown"), tone)}
										${inlineStatusFact("Dispatchable", String(program.dispatchable_count ?? 0))}
										${warning}
									</div>
									<div class="grid two card-facts">
										${cardField("Mapped issues", mappedIssues, mappedIssues === "NONE" ? "is-muted" : "")}
										${cardField("Source contract", program.source_contract_id || "NONE", program.source_contract_id ? "" : "is-muted")}
										${programProgressFacts(program)
											.map(([label, value]) => cardField(label, value))
											.join("")}
									</div>
									${renderProgramNodeReadbacks(program)}
								</article>
							`;
						})
						.join(""),
				);
			}
