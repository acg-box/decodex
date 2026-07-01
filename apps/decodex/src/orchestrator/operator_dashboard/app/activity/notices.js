			function snapshotIsIdle(snapshot) {
				if (!snapshot) {
					return false;
				}

				return (
					snapshotCurrentLaneCards(snapshot).length === 0 &&
					(snapshot.recent_runs?.length ?? 0) === 0 &&
					(snapshot.worktrees?.length ?? 0) === 0 &&
					(snapshot.post_review_lanes?.length ?? 0) === 0
				);
			}

			function connectorBackoffs(snapshot) {
				return Array.isArray(snapshot?.connector_backoffs) ? snapshot.connector_backoffs : [];
			}

			function hasConnectorBackoff(snapshot) {
				return connectorBackoffs(snapshot).length > 0 || (snapshot?.warnings ?? []).includes("tracker_rate_limited");
			}

			function connectorBackoffNotice(backoff) {
				const project = backoff.project_id || "project";
				const connector = displayToken(backoff.connector || "tracker");
				const phase = displayToken(backoff.sync_phase || "external sync");
				const quota = displayToken(backoff.quota_class || "api quota");
				const retryAfter = backoff.retry_after_seconds == null ? "unknown" : formatDuration(backoff.retry_after_seconds);
				const resetAt = formatTimestamp(backoff.reset_at);
				const nextAction = backoff.next_action || "Monitor local lanes.";

				return {
					tone: "warning",
					title: `Sync backoff · ${project}`,
					copy: `${connector} ${phase} paused by ${quota}. Retry in ${retryAfter} at ${resetAt}. ${nextAction}`,
				};
			}

			function summarizeReadiness(snapshotError, snapshot) {
				const warnings = snapshot?.warnings ?? [];
				const trackerBackoff = hasConnectorBackoff(snapshot);

				if (!dashboardStreamState.supported) {
					return {
						tone: "danger",
						label: "WebSocket unavailable",
						copy: "This browser cannot open the dashboard WebSocket.",
					};
				}

				if (dashboardStreamState.error) {
					return {
						tone: "danger",
						label: "WebSocket disconnected",
						copy: "Dashboard stream disconnected; reconnecting.",
					};
				}

				if (snapshot) {
					if (trackerBackoff && !snapshotError) {
						return {
							tone: "warning",
							label: "Tracker sync paused",
							copy: "Serving local state; Linear sync is paused.",
						};
					}

					return {
						tone: snapshotError || warnings.length ? "warning" : "success",
						label: snapshotError || warnings.length ? "State degraded" : "Snapshot ready",
						copy: snapshotError
							? "WebSocket did not deliver a usable snapshot."
							: warnings.length
								? `warnings: ${warnings.map(displayToken).join(", ")}`
								: "Fresh operator snapshot published.",
					};
				}

				return {
					tone: "warning",
					label: "No snapshot",
					copy: dashboardStreamState.connected
						? "WebSocket connected; waiting for operator snapshot."
						: "Connecting to dashboard WebSocket.",
				};
			}

			function dashboardNotices(readiness, snapshotError, snapshot) {
				const notices = [];
				const warnings = snapshot?.warnings ?? [];
				const backoffs = connectorBackoffs(snapshot);

				if (readiness.tone === "danger") {
					notices.push({
						tone: "danger",
						title: readiness.label,
						copy: snapshotError
							? `${readiness.copy} Snapshot stream also failed: ${snapshotError}`
							: readiness.copy,
					});
				} else if (snapshotError) {
					notices.push({
						tone: "danger",
						title: "Snapshot stream",
						copy: snapshotError,
					});
				}

				if (
					readiness.tone === "warning" &&
					!snapshotError &&
					warnings.length === 0 &&
					backoffs.length === 0
				) {
					notices.push({
						tone: "warning",
						title: readiness.label,
						copy: readiness.copy,
					});
				}

				for (const backoff of backoffs) {
					notices.push(connectorBackoffNotice(backoff));
				}

				for (const warning of warnings) {
					if (warning === "tracker_rate_limited" && backoffs.length) {
						continue;
					}
					if (
						warning === "external_observer_status_skipped" &&
						backoffs.length &&
						warnings.includes("tracker_rate_limited")
					) {
						continue;
					}
					const message = warningNotice(warning, snapshot);
					notices.push({
						tone: message.tone,
						title: message.title,
						copy: message.copy,
					});
				}

				for (const accountNotice of codexAccountNotices(snapshot)) {
					notices.push(accountNotice);
				}

				for (const controlEvent of dashboardControlEvents) {
					notices.push({
						tone: controlEvent.accepted ? "warning" : "danger",
						title: controlEvent.accepted ? "Control accepted" : "Control failed",
						copy: `${dashboardControlActionLabel(controlEvent.action)}: ${controlEvent.message}`,
						ackKey: controlEvent.key,
					});
				}

				return notices;
			}

				function codexAccountHasNotice(account) {
					if (!account) {
						return false;
					}

					const status = String(account.status || "").toLowerCase();
					const note = codexAccountNote(account);
					return Boolean(
						codexAccountRefreshFailed(account) ||
							codexAccountNoteLooksError(note) ||
							status.includes("failed") ||
							status.includes("unusable"),
					);
				}

				function codexAccountNoticeTitle(account) {
					if (codexAccountRefreshFailed(account)) {
						return "Codex account token";
					}
					if (codexAccountNoteLooksError(codexAccountNote(account))) {
						return "Codex account usage";
					}

					return "Codex account";
				}

				function codexAccountNoticeCopy(account) {
					const note = codexAccountNote(account);
					const parts = [];
					const noteIncludesRefreshFailure = /refresh failed|token refresh failed/i.test(note);
					if (note && !codexAccountNoteLooksRoutine(note)) {
						parts.push(codexAccountPrivacyText(account, note));
					}
					if (codexAccountRefreshFailed(account) && !noteIncludesRefreshFailure) {
						parts.unshift(codexAccountTokenLabel(account.refresh_status));
					}
					if (!parts.length) {
						parts.push(codexAccountStatusLabel(account));
					}

					return `${codexAccountPrivacyLabel(account)}: ${parts.join("; ")}`;
				}

				function codexAccountNotices(snapshot) {
					const notices = [];
					const seen = new Set();
					for (const account of codexAccountPoolAccounts(snapshot)) {
						if (!codexAccountHasNotice(account)) {
							continue;
						}
						const notice = {
							tone: "danger",
							title: codexAccountNoticeTitle(account),
							copy: codexAccountNoticeCopy(account),
						};
						const key = `${notice.title}:${notice.copy}`;
						if (seen.has(key)) {
							continue;
						}
						seen.add(key);
						notices.push(notice);
					}

					return notices;
				}

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

			function renderNoticeDock(notices) {
				const hasNotices = notices.length > 0;
				nodes.noticeDock.classList.toggle("visible", hasNotices);
				nodes.noticeDock.setAttribute("aria-hidden", hasNotices ? "false" : "true");

				if (!hasNotices) {
					nodes.noticeDock.removeAttribute("open");
					delete nodes.noticeDock.dataset.tone;
					nodes.noticeCount.textContent = "0";
					nodes.noticeLabel.textContent = "notices";
					nodes.noticeList.innerHTML = "";

					return;
				}

				const tone = notices.some((notice) => notice.tone === "danger") ? "danger" : "warning";
				const dangerCount = notices.filter((notice) => notice.tone === "danger").length;
				nodes.noticeDock.dataset.tone = tone;
				nodes.noticeCount.textContent = String(notices.length);
				nodes.noticeLabel.textContent =
					dangerCount > 0
						? pluralLabel(notices.length, "alert")
						: pluralLabel(notices.length, "warning");
				nodes.noticeList.innerHTML = notices
					.map(
						(notice) => `
							<article class="notice-item ${notice.tone}">
								<strong>${escapeHtml(notice.title)}</strong>
								<p>${escapeHtml(notice.copy)}</p>
								${notice.ackKey ? `<button class="control-button" type="button" data-notice-ack="${escapeHtml(notice.ackKey)}">Ack</button>` : ""}
							</article>
						`,
					)
					.join("");
			}
