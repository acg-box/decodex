			function renderAttentionFacts(candidate) {
				const attention = candidate.attention;
				if (!attention) {
					return "";
				}

				const facts = [];
				if (attention.run_id) {
					const attempt = attention.attempt_number == null ? "" : ` · attempt ${attention.attempt_number}`;
					facts.push(["Run", `${attention.run_id}${attempt}`]);
				}
				if (attention.current_operation && attention.current_operation !== "agent_run") {
					facts.push(["Op", displayToken(attention.current_operation)]);
				}
				if (attention.thread_status && attention.thread_status !== "systemError") {
					facts.push(["Thread", displayToken(attention.thread_status)]);
				}
				if (attention.attempt_status) {
					facts.push(["Attempt status", displayToken(attention.attempt_status)]);
				}
				if (attention.retry_budget_attempt_count != null) {
					const retryMax =
						attention.retry_budget_max_attempts == null
							? ""
							: ` / ${attention.retry_budget_max_attempts}`;
					facts.push(["Failed attempts", `${attention.retry_budget_attempt_count}${retryMax}`]);
				}
				if (attention.auto_retry_blocked_reason) {
					facts.push(["Auto retry", autoRetryBlockedReasonText(attention.auto_retry_blocked_reason)]);
				}
				if (attention.attention_error_class) {
					facts.push(["Cause", displayToken(attention.attention_error_class)]);
				}
				if (attention.worktree_has_tracked_changes) {
					facts.push(["Patch", "retained"]);
				}
				if (attention.process_alive != null) {
					facts.push([
						"Process",
						attention.process_alive
							? "alive"
							: processLivenessReasonLabel(attention.process_liveness_reason || "process_stopped"),
					]);
				}
				if (attention.worktree_path) {
					facts.push(["Worktree", attention.worktree_path]);
				}
				if (attention.last_activity_at) {
					facts.push(["Last", formatTimestampCompact(attention.last_activity_at)]);
				}
				facts.push(...loopStatusFacts(attention.loop_status));

				if (!facts.length) {
					return "";
				}

				return `
					<div class="attention-facts">
						${facts
							.map(
								([label, value]) =>
									`<span>${escapeHtml(label)} <strong>${escapeHtml(value)}</strong></span>`,
							)
							.join("")}
					</div>
				`;
			}

			function autoRetryBlockedReasonText(reason) {
				return displayToken(reason);
			}

			function statusLabel(label, tone) {
				return `<span class="status-label ${tone}">${escapeHtml(label)}</span>`;
			}

			function inlineStatusFact(label, value) {
				return `<span>${escapeHtml(titleCaseLabel(label))} <strong>${escapeHtml(value)}</strong></span>`;
			}
