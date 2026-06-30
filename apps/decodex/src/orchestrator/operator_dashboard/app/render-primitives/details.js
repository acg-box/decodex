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

			function runDetailKey(run) {
				return `${currentLaneRenderKey(run)}:more-fields`;
			}

			function currentLaneRenderKey(run) {
				const projectKey = run?.project_id || "unknown-project";
				const issueKey =
					canonicalIssueIdentityKey(run?.issue_id) ||
					canonicalIssueIdentityKey(issueDisplayKey(run));
				return `current-lane:${projectKey}:${issueKey || run?.run_id || "unknown"}`;
			}

			function detailsOpenAttribute(detailKey) {
				return detailDisclosureState.get(detailKey) ? ' open data-detail-state="open"' : "";
			}

			function detailStateKey(details) {
				return details.dataset.detailKey || details.dataset.foldKey || "";
			}

			function detailContent(details) {
				return details.querySelector(":scope > .panel-body, :scope > .grid, :scope > .phase-list");
			}

			function rememberDetailOpenState(details, isOpen) {
				const detailKey = detailStateKey(details);
				if (detailKey) {
					detailDisclosureState.set(detailKey, isOpen);
				}
			}

			function setDetailVisualState(details, isOpen) {
				if (isOpen) {
					details.dataset.detailState = "open";
				} else {
					delete details.dataset.detailState;
				}
			}

			function syncDefaultDetailOpenState(details, shouldOpen) {
				const detailKey = detailStateKey(details);
				if (!detailKey || detailDisclosureState.has(detailKey) || details.classList.contains("is-animating")) {
					return;
				}

				details.open = shouldOpen;
				setDetailVisualState(details, shouldOpen);
			}

			function setFoldPanelEmpty(panel, isEmpty) {
				panel.classList.toggle("is-empty", isEmpty);
				if (!isEmpty) {
					return;
				}

				const detailKey = detailStateKey(panel);
				if (detailKey) {
					detailDisclosureState.delete(detailKey);
				}
				const existingTimer = detailAnimationTimers.get(panel);
				if (existingTimer) {
					window.clearTimeout(existingTimer);
					detailAnimationTimers.delete(panel);
				}
				const content = detailContent(panel);
				if (content) {
					clearDetailAnimation(panel, content);
				}
				panel.open = true;
				setDetailVisualState(panel, false);
			}

			function clearDetailAnimation(details, content) {
				details.classList.remove("is-animating");
				content.style.height = "";
				content.style.opacity = "";
				content.style.overflow = "";
				content.style.transform = "";
			}

			function scrollOpenedDetailIntoView(details, content) {
				if (reducedMotionQuery.matches) {
					return;
				}

				window.requestAnimationFrame(() => {
					const viewportHeight =
						window.innerHeight || document.documentElement.clientHeight;
					const contentRect = content.getBoundingClientRect();
					const targetBottom = Math.min(
						contentRect.bottom,
						contentRect.top + Math.min(content.scrollHeight, viewportHeight * 0.62),
					);
					const overflow = targetBottom - (viewportHeight - 28);

					if (overflow > 0) {
						window.scrollBy({
							top: overflow,
							behavior: "smooth",
						});
					}
				});
			}

			function animateDetail(details, shouldOpen) {
				const content = detailContent(details);
				rememberDetailOpenState(details, shouldOpen);

				const existingTimer = detailAnimationTimers.get(details);
				if (existingTimer) {
					window.clearTimeout(existingTimer);
					detailAnimationTimers.delete(details);
				}

				if (!content || reducedMotionQuery.matches) {
					details.open = shouldOpen;
					setDetailVisualState(details, shouldOpen);
					return;
				}

				const isFoldPanel = details.classList.contains("fold-panel");
				const animationMs = isFoldPanel ? FOLD_PANEL_ANIMATION_MS : DETAILS_ANIMATION_MS;
				const openingOffset = isFoldPanel ? "translateY(-3px)" : "translateY(-6px)";
				const startHeight = details.open
					? content.getBoundingClientRect().height
					: 0;

				details.open = true;
				setDetailVisualState(details, shouldOpen);
				details.classList.add("is-animating");
				content.style.overflow = "hidden";
				content.style.height = `${startHeight}px`;
				content.style.opacity = shouldOpen ? "0" : "1";
				content.style.transform = shouldOpen ? openingOffset : "translateY(0)";

				const finishTimer = window.setTimeout(() => {
					if (!shouldOpen) {
						details.open = false;
					}
					clearDetailAnimation(details, content);
					if (shouldOpen && !isFoldPanel) {
						scrollOpenedDetailIntoView(details, content);
					}
					detailAnimationTimers.delete(details);
				}, animationMs + 60);

				detailAnimationTimers.set(details, finishTimer);

				window.requestAnimationFrame(() => {
					const endHeight = shouldOpen ? content.scrollHeight : 0;
					content.style.height = `${endHeight}px`;
					content.style.opacity = shouldOpen ? "1" : "0";
					content.style.transform = shouldOpen ? "translateY(0)" : openingOffset;

					if (shouldOpen && !isFoldPanel) {
						scrollOpenedDetailIntoView(details, content);
					}
				});
			}

