			function toneForQueuedCandidate(candidate) {
				if (candidate.display_classification === "leased_run") {
					return "tone-run";
				}
				if (candidate.classification === "ready") {
					return "tone-ready";
				}
				if (candidate.classification === "claimed") {
					return "tone-retained";
				}
				if (candidate.classification === "waiting") {
					return "tone-wait";
				}
				if (candidate.classification === "closed") {
					return "tone-muted";
				}
				if (
					candidate.reason === "issue_needs_attention" ||
					candidate.reason === "linear_active_label_present"
				) {
					return "tone-blocked";
				}
				return "tone-wait";
			}

			function queuedCandidateRank(candidate) {
				switch (candidate.display_classification || candidate.classification) {
					case "leased_run":
						return 0;
					case "ready":
						return 1;
					case "waiting":
						return 2;
					case "claimed":
						return 3;
					case "blocked":
						return 4;
					case "closed":
					default:
						return 5;
				}
			}

			function compareQueuedCandidates(left, right) {
				const leftPriority = left.priority == null ? Number.MAX_SAFE_INTEGER : left.priority;
				const rightPriority = right.priority == null ? Number.MAX_SAFE_INTEGER : right.priority;

				return (
					queuedCandidateRank(left) - queuedCandidateRank(right) ||
					leftPriority - rightPriority ||
					String(left.created_at).localeCompare(String(right.created_at)) ||
					left.issue_identifier.localeCompare(right.issue_identifier)
				);
			}

			function summarizeQueuedCandidate(candidate) {
				if (candidate.display_classification === "leased_run") {
					return `Shown in ${COPY.runningLane}; excluded from queue.`;
				}
				if (
					candidate.attention?.summary &&
					!queuedCandidateSummaryIsNoise(candidate.attention.summary)
				) {
					return candidate.attention.summary;
				}
				switch (candidate.reason) {
					case "normal_dispatch":
					case "eligible_for_dispatch":
						return "";
					case "issue_needs_attention":
						return "";
					default:
						return displayToken(candidate.reason);
				}
			}

			function queuedCandidateSummaryIsNoise(summary) {
				const normalized = String(summary || "").trim().toLowerCase();
				return normalized === "" || normalized.includes("systemerror");
			}

			function queuedCandidateStatusText(candidate) {
				if (candidate.display_classification === "leased_run") {
					return COPY.runningInline;
				}
				return displayToken(candidate.classification);
			}

			function queuedCandidateReasonText(candidate) {
				if (candidate.display_classification === "leased_run") {
					return "running lane claim";
				}
				if (
					candidate.attention?.worktree_has_tracked_changes &&
					candidate.attention?.retry_budget_attempt_count != null
				) {
					return "worktree_has_tracked_changes";
				}
				if (candidate.attention?.thread_status === "systemError") {
					return displayToken(candidate.attention.thread_status);
				}
				if (candidate.attention?.last_event_type === "item/tool/call") {
					return displayToken(candidate.attention.last_event_type);
				}
				if (candidate.attention?.attention_error_class) {
					return displayToken(candidate.attention.attention_error_class);
				}
				if (candidate.attention?.retry_budget_attempt_count != null) {
					return "retry_budget_attempt_count";
				}
				if (["normal_dispatch", "eligible_for_dispatch"].includes(candidate.reason)) {
					return "";
				}

				return displayToken(candidate.reason);
			}

			function queuedCandidateInlineReason(candidate) {
				const reason = queuedCandidateReasonText(candidate);
				if (!reason) {
					return "";
				}

				if (
					candidate.attention?.attention_error_class &&
					displayTextRepeats(reason, displayToken(candidate.attention.attention_error_class))
				) {
					return "";
				}
				if (
					candidate.attention?.worktree_has_tracked_changes &&
					displayTextRepeats(reason, "worktree_has_tracked_changes")
				) {
					return "";
				}
				if (displayTextRepeats(candidate.attention?.summary, reason)) {
					return "";
				}

				return reason;
			}

			function queuedCandidateNeedsAttention(candidate) {
				return Boolean(
					candidate.attention ||
						candidate.reason === "issue_needs_attention" ||
						candidate.reason === "linear_active_label_present" ||
						candidate.reason === "retry_budget_exhausted",
				);
			}

			function formatDetailToken(value) {
				const token = String(value || "").trim();
				return token || "NONE";
			}

			function formatPriority(priority) {
				return priority == null ? "NONE" : `P${priority}`;
			}
