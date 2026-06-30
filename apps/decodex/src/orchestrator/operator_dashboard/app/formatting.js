
			function escapeHtml(value) {
				return String(value).replace(/[&<>"']/g, (character) => {
					switch (character) {
						case "&":
							return "&amp;";
						case "<":
							return "&lt;";
						case ">":
							return "&gt;";
						case "\"":
							return "&quot;";
						case "'":
							return "&#39;";
						default:
							return character;
					}
				});
			}

			function encodeLocalPath(path) {
				return path
					.split("/")
					.map((segment, index) => (index === 0 ? "" : encodeURIComponent(segment)))
					.join("/");
			}

			function localPathHref(value) {
				const rawValue = String(value || "").trim();
				if (
					!rawValue.startsWith("/") ||
					rawValue.startsWith("//") ||
					/[\n\r]/.test(rawValue) ||
					!["/Users/", "/Volumes/", "/tmp/", "/private/", "/var/", "/opt/", "/home/"].some((prefix) =>
						rawValue.startsWith(prefix),
					)
				) {
					return "";
				}

				return `file://${encodeLocalPath(rawValue)}`;
			}

			function linkHref(value) {
				const rawValue = String(value || "").trim();
				return /^(https?|wss?):\/\//i.test(rawValue) ? rawValue : localPathHref(rawValue);
			}

			function linkValueLabel(label, value) {
				const text = String(value || "").trim();
				const labelKey = detailLabel(label).toLowerCase();
				const pullRequestMatch = text.match(/\/pull\/(\d+)(?:$|[/?#])/);
				if (labelKey === "pr" && pullRequestMatch) {
					return `#${pullRequestMatch[1]}`;
				}

				return text;
			}

			function renderValueLink(label, value, className = "value-link") {
				const href = linkHref(value);
				if (!href) {
					return "";
				}

				return `<a class="${className}" href="${escapeHtml(href)}" target="_blank" rel="noreferrer" title="${escapeHtml(href)}">${escapeHtml(linkValueLabel(label, value))}</a>`;
			}

			function metricTokenParts(token) {
				const match = String(token).trim().match(/^(-?\d[\d.,]*(?:[a-zA-Z%]+)?)(?:\s+(.+))?$/);
				if (!match) {
					return { label: token };
				}

				return {
					number: match[1],
					label: match[2] || "",
				};
			}

			function renderMetricGroup(token) {
				const parts = metricTokenParts(token);
				if (parts.number == null) {
					return `<span class="metric-group"><span class="metric-label">${escapeHtml(parts.label)}</span></span>`;
				}

				const label = parts.label
					? `<span class="metric-label">${escapeHtml(titleCaseLabel(parts.label))}</span>`
					: "";
				return `<span class="metric-group"><span class="metric-number">${escapeHtml(parts.number)}</span>${label}</span>`;
			}

			function renderMetricText(text) {
				const tokens = String(text)
					.split(" · ")
					.map((token) => token.trim())
					.filter(Boolean);

				if (!tokens.length) {
					return "";
				}

				return `<span class="metric-text">${tokens
					.map((token, index) => {
						const separator =
							index === 0 ? "" : '<span class="metric-separator"> · </span>';
						return `${separator}${renderMetricGroup(token)}`;
					})
					.join("")}</span>`;
			}

			function setMetricText(node, text) {
				node.innerHTML = renderMetricText(text);
			}

			function displayToken(value) {
				const token = String(value ?? "").trim();
				return token || "none";
			}

			function normalizedDisplayText(value) {
				return String(value || "")
					.toLowerCase()
					.replace(/[^a-z0-9]+/g, " ")
					.trim();
			}

			function displayTextRepeats(left, right) {
				const normalizedLeft = normalizedDisplayText(left);
				const normalizedRight = normalizedDisplayText(right);

				return Boolean(
					normalizedLeft &&
						normalizedRight &&
						(normalizedLeft === normalizedRight ||
							normalizedLeft.includes(normalizedRight) ||
							normalizedRight.includes(normalizedLeft)),
				);
			}

			function compactStateToken(value) {
				return formatDetailToken(value);
			}

			function formatTimestamp(value) {
				if (!value) {
					return "none";
				}

				const parsed = new Date(value);
				if (Number.isNaN(parsed.getTime())) {
					return String(value);
				}

				return new Intl.DateTimeFormat(undefined, {
					dateStyle: "medium",
					timeStyle: "medium",
				}).format(parsed);
			}

			function formatTimestampCompact(value) {
				if (!value) {
					return "none";
				}

				const parsed = new Date(value);
				if (Number.isNaN(parsed.getTime())) {
					return String(value);
				}

				return new Intl.DateTimeFormat(undefined, {
					dateStyle: "medium",
					timeStyle: "short",
				}).format(parsed);
			}

			function formatRelativeTimestamp(value) {
				if (!value) {
					return "none";
				}

				const parsed = new Date(value);
				if (Number.isNaN(parsed.getTime())) {
					return String(value);
				}

				const seconds = Math.max(0, Math.floor((Date.now() - parsed.getTime()) / 1000));
				if (seconds < 5) {
					return "0s";
				}
				if (seconds < 60) {
					return `${seconds}s`;
				}

				const minutes = Math.floor(seconds / 60);
				if (minutes < 60) {
					return `${minutes}m`;
				}

				const hours = Math.floor(minutes / 60);
				if (hours < 24) {
					return `${hours}h`;
				}

				const days = Math.floor(hours / 24);
				return `${days}d`;
			}

			function dashboardSocketUrl() {
				const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
				const host = window.location.host;

				if (!host) {
					return DASHBOARD_WEBSOCKET_ENDPOINT;
				}

				return `${protocol}//${host}${DASHBOARD_WEBSOCKET_ENDPOINT}`;
			}

			function snapshotAgeSeconds(snapshotPublishedAt) {
				if (!snapshotPublishedAt) {
					return null;
				}

				const parsed = new Date(snapshotPublishedAt);
				if (Number.isNaN(parsed.getTime())) {
					return null;
				}

				return Math.max(0, Math.floor((Date.now() - parsed.getTime()) / 1000));
			}

			function snapshotFreshnessMeta(snapshotPublishedAt, readiness, snapshotError) {
				if (snapshotPublishedAt) {
					const ageSeconds = snapshotAgeSeconds(snapshotPublishedAt);
					const staleByAge = ageSeconds != null && ageSeconds >= 30;
					if (
						!snapshotError &&
						readiness.tone !== "danger" &&
						!staleByAge
					) {
						return null;
					}

					return {
						label: formatRelativeTimestamp(snapshotPublishedAt),
						tone:
							snapshotError || readiness.tone === "danger"
								? "danger"
								: staleByAge
									? "warning"
									: readiness.tone,
						title: `Published ${formatTimestamp(snapshotPublishedAt)}`,
					};
				}

				if (snapshotError || readiness.tone === "danger") {
					return {
						label: "Unavailable",
						tone: "danger",
						title: snapshotError || readiness.copy,
					};
				}

				return {
					label: "Pending",
					tone: readiness.tone === "warning" ? "warning" : "muted",
					title: readiness.copy,
				};
			}

			function topbarReadinessLabel(label) {
				switch (label) {
					case "Snapshot ready":
						return "Ready";
					case "State degraded":
						return "Degraded";
					case "Tracker sync paused":
						return "Sync paused";
					case "Listener down":
						return "Listener down";
					case "No snapshot":
						return "No snapshot";
					default:
						return label;
				}
			}

			function dashboardStreamMeta() {
				if (!dashboardStreamState.supported) {
					return {
						label: "unavailable",
						tone: "danger",
						title: "WebSocket unavailable.",
					};
				}

				if (dashboardStreamState.connected) {
					return {
						label: dashboardStreamState.lastEventAt ? "live" : "connected",
						tone: "success",
						title: dashboardStreamState.lastEventAt
							? `Last event ${formatRelativeTimestamp(dashboardStreamState.lastEventAt)}`
							: "WebSocket connected.",
					};
				}

				if (dashboardStreamState.error) {
					return {
						label: "reconnecting",
						tone: "warning",
						title: "WebSocket reconnecting.",
					};
				}

				return {
					label: "starting",
					tone: "muted",
					title: "WebSocket connecting.",
				};
			}

			function formatDuration(seconds) {
				if (seconds == null) {
					return "none";
				}

				const value = Math.max(0, Number(seconds));
				const hours = Math.floor(value / 3600);
				const minutes = Math.floor((value % 3600) / 60);
				const remainingSeconds = Math.floor(value % 60);
				const parts = [];

				if (hours > 0) {
					parts.push(`${hours}h`);
				}
				if (minutes > 0 || hours > 0) {
					parts.push(`${minutes}m`);
				}
				parts.push(`${remainingSeconds}s`);

				return parts.join(" ");
			}

				function formatCompactCount(value) {
					if (value == null) {
						return "none";
					}

					const number = Math.max(0, Number(value));

					if (number >= 1_000_000_000) {
						return `${(number / 1_000_000_000).toFixed(1)}B`;
					}
					if (number >= 1_000_000) {
						return `${(number / 1_000_000).toFixed(2)}M`;
					}
					if (number >= 1_000) {
						return `${(number / 1_000).toFixed(1)}k`;
					}

					return String(Math.floor(number));
				}

			function formatCompactBytes(value) {
				if (value == null) {
					return "none";
				}

				const number = Math.max(0, Number(value));

				if (number >= 1_048_576) {
					return `${(number / 1_048_576).toFixed(1)}MiB`;
				}
				if (number >= 1024) {
					return `${(number / 1024).toFixed(1)}KiB`;
				}

				return `${Math.floor(number)}B`;
			}

			function capturedValue(value) {
				return value || "not captured";
			}

			function numericSeconds(value) {
				if (value == null) {
					return 0;
				}

				const parsed = Number(value);
				return Number.isFinite(parsed) ? parsed : 0;
			}

			function runHasProtocolTelemetry(run) {
				return Boolean(
					run.thread_id ||
						run.turn_id ||
						run.last_event_type ||
						run.last_event_at ||
						run.last_protocol_activity_at ||
						run.effective_model ||
						run.effective_model_provider ||
						run.effective_cwd ||
						run.process_id != null ||
						Number(run.event_count ?? 0) > 0,
				);
			}

			function runTelemetryMissing(run) {
				return run.status === "running" && run.phase === "executing" && !runHasProtocolTelemetry(run);
			}

			function runHasChildAgentActivity(run) {
				const activity = childAgentActivity(run);

				return Boolean(
					activity?.current_bucket ||
						(activity?.buckets?.length ?? 0) > 0 ||
						Number(activity?.event_count ?? 0) > 0,
				);
			}

			function runTelemetryMissingNeedsAttention(run) {
				return runTelemetryMissing(run) && numericSeconds(run.idle_for_seconds) >= RUN_ATTENTION_IDLE_SECONDS;
			}

			function runHasFreshExecution(run) {
				if (typeof run?.has_fresh_execution === "boolean") {
					return run.has_fresh_execution;
				}

				return (
					run?.thread_status === "active" ||
					(run?.thread_active_flags?.length ?? 0) > 0 ||
					["thread_active", "protocol_observed"].includes(run?.execution_liveness) ||
					run?.process_alive === true ||
					(run?.protocol_idle_for_seconds != null &&
						numericSeconds(run.protocol_idle_for_seconds) < RUN_STALE_NO_PROCESS_SECONDS)
				);
			}

			function runProcessStoppedWhileActive(run) {
				return (
					["starting", "running"].includes(run.status) &&
					run.phase === "executing" &&
					run.process_alive === false
				);
			}

			function runStoppedProcessNeedsAttention(run) {
				return runProcessStoppedWhileActive(run);
			}

				function runPhaseLabel(run) {
					if (runStoppedProcessNeedsAttention(run)) {
						return run.process_liveness_reason || "process_stopped";
					}

					return displayToken(run.run_phase || run.phase || run.status);
				}

			function runStaleWithoutKnownProcessAgeSeconds(run) {
				return Math.max(
					numericSeconds(run.idle_for_seconds),
					numericSeconds(run.protocol_idle_for_seconds),
				);
			}

			function runStaleWithoutKnownProcessNeedsAttention(run) {
				return (
					["starting", "running"].includes(run.status) &&
					run.phase === "executing" &&
					!run.wait_reason &&
					run.process_alive !== true &&
					!runHasFreshExecution(run) &&
					runStaleWithoutKnownProcessAgeSeconds(run) >= RUN_STALE_NO_PROCESS_SECONDS
				);
			}

			function runNeedsAttention(run) {
				if (typeof run?.needs_attention === "boolean") {
					return run.needs_attention;
				}

				return (
					run.suspected_stall ||
					run.phase === "stalled" ||
					runStoppedProcessNeedsAttention(run) ||
					runStaleWithoutKnownProcessNeedsAttention(run)
				);
			}

			function runCountsAsRunning(run) {
				if (typeof run?.counts_as_running === "boolean") {
					return run.counts_as_running;
				}

				return (
					["starting", "running"].includes(run.status) &&
					run.phase === "executing" &&
					run.process_alive !== false &&
					!runNeedsAttention(run)
				);
			}

			function runWaitReasonShowsExecutionProgress(run) {
				return (
					run.phase === "executing" &&
					["model_execution", "tool_execution", "protocol_activity"].includes(run.wait_reason)
				);
			}

			function toneForRun(run) {
				if (
					runNeedsAttention(run) ||
					runTelemetryMissingNeedsAttention(run) ||
					["stalled", "failed", "terminated", "interrupted"].includes(run.status)
				) {
					return "tone-blocked";
				}
				if (runTelemetryMissing(run) || runProcessStoppedWhileActive(run)) {
					return "tone-wait";
				}
				if (runWaitReasonShowsExecutionProgress(run)) {
					return "tone-run";
				}
				if (
					(run.wait_reason && !runWaitReasonShowsExecutionProgress(run)) ||
					run.phase === "retry_backoff" ||
					run.phase === "waiting_continuation"
				) {
					return "tone-wait";
				}
				if (run.status === "running" || run.phase === "executing") {
					return "tone-run";
				}
				if (run.status === "completed" || run.status === "merged" || run.status === "succeeded") {
					return "tone-land";
				}
				return "tone-muted";
			}

			function toneForLane(lane) {
				switch (lane.classification) {
					case "ready_to_land":
					case "continue":
						return "tone-land";
					case "wait_for_review":
						return "tone-review";
					case "cleanup_blocked":
						return "tone-wait";
					case "closeout_blocked":
					case "blocked":
						return "tone-blocked";
					case "needs_review_repair":
						return "tone-repair";
					default:
						return "tone-retained";
				}
			}

			function isPostReviewBlocker(lane) {
				if (lane?.shadowed_by_current_lane === true) {
					return false;
				}

				return ["blocked", "needs_review_repair", "closeout_blocked", "cleanup_blocked"].includes(
					lane.classification,
				);
			}

			function postReviewBlockerScope(lane) {
				if (lane.classification === "cleanup_blocked") {
					return "Cleanup";
				}
				if (lane.classification === "closeout_blocked") {
					return "Closeout";
				}
				return "Review";
			}

			function postReviewBlockerTitle(lane) {
				return displayToken(lane.classification);
			}

			function postReviewBlockerStatus(lane, blockerScope) {
				if (lane.review_decision && blockerScope === "Review") {
					return `review ${compactStateToken(lane.review_decision)}`;
				}

				return "needs_attention";
			}

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
