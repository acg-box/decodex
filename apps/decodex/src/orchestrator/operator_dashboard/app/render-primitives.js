
			function titleCaseLabel(label) {
				return String(label || "").replace(/\b[A-Za-z][A-Za-z0-9]*\b/g, (word) => {
					if (/^[A-Z0-9]+$/.test(word) && /[A-Z]/.test(word)) {
						return word;
					}
					const lower = word.toLowerCase();
					const acronym = FIELD_LABEL_ACRONYMS.get(lower);
					return acronym || `${lower.charAt(0).toUpperCase()}${lower.slice(1)}`;
				});
			}

			function detailLabel(label) {
				return String(label || "").replace(/\b[A-Za-z][A-Za-z0-9]*\b/g, (word) => {
					const lower = word.toLowerCase();
					return FIELD_LABEL_ACRONYMS.get(lower) || lower;
				});
			}


			function resolveTheme(selection) {
				if (selection === "dark" || selection === "light") {
					return selection;
				}

				return themeMediaQuery.matches ? "dark" : "light";
			}

			function renderThemeControls(selection, effectiveTheme) {
				for (const button of nodes.themeButtons) {
					const isActive = button.dataset.themeChoice === selection;
					button.classList.toggle("active", isActive);
					button.setAttribute("aria-pressed", isActive ? "true" : "false");
				}
			}

			function applyTheme(selection, persist = true) {
				themeSelection = ["system", "light", "dark"].includes(selection)
					? selection
					: "system";

				if (persist) {
					try {
						window.localStorage.setItem(THEME_STORAGE_KEY, themeSelection);
					} catch (_error) {
						/* Ignore storage failures and continue with the in-memory choice. */
					}
				}

				const effectiveTheme = resolveTheme(themeSelection);
				document.documentElement.dataset.theme = effectiveTheme;
				document.documentElement.style.colorScheme = effectiveTheme;
				renderThemeControls(themeSelection, effectiveTheme);
			}

			function renderField(label, value, valueClass, labelFormatter, fieldClass = "") {
				const fieldClassName = ["field", fieldClass].filter(Boolean).join(" ");
				const className = ["field-value", valueClass].filter(Boolean).join(" ");
				const valueHtml = renderValueLink(label, value) || escapeHtml(value);
				return `
					<div class="${fieldClassName}">
						<div class="field-label">${escapeHtml(labelFormatter(label))}</div>
						<div class="${className}">${valueHtml}</div>
					</div>
				`;
			}

			function field(label, value, valueClass = "") {
				return renderField(label, value, valueClass, detailLabel);
			}

			function cardField(label, value, valueClass = "") {
				return renderField(label, value, valueClass, titleCaseLabel, "card-field");
			}

			function cardFactValueClass(value, explicitClass = "") {
				return [explicitClass, String(value || "").trim() === "NONE" ? "is-muted" : ""]
					.filter(Boolean)
					.join(" ");
			}

			function optionalCardToken(value) {
				const token = String(value || "").trim();
				return token || "NONE";
			}

			function reviewThreadToken(count) {
				const numericCount = Number(count);
				return Number.isFinite(numericCount) && numericCount > 0 ? String(numericCount) : "NONE";
			}

			function postReviewReadbackFacts(lane) {
				const facts = [];
				if (lane.readback_warning) {
					facts.push(["Readback", lane.readback_warning]);
				}
				if (lane.readback_root_cause) {
					facts.push(["Root cause", lane.readback_root_cause]);
				}
				return facts;
			}

			function loopStatusFacts(loopStatus) {
				if (!loopStatus) {
					return [];
				}

				const facts = [];
				if (loopStatus.review?.status) {
					const checkpoint = loopStatus.review.checkpoint;
					const round =
						checkpoint && checkpoint.round != null ? ` r${checkpoint.round}` : "";
					facts.push([
						"Review",
						`${displayToken(loopStatus.review.phase)} ${displayToken(loopStatus.review.status)}${round}`,
					]);
				}
				if (loopStatus.architecture_recovery?.status) {
					const recovery = loopStatus.architecture_recovery;
					const budget = recovery.budget
						? ` ${recovery.budget.attempt}/${recovery.budget.max_attempts}`
						: "";
					facts.push([
						"Recovery",
						`${displayToken(recovery.status)} ${displayToken(recovery.reason_code)}${budget}`,
					]);
				}
				if (loopStatus.boundary?.disposition) {
					facts.push(["Boundary", displayToken(loopStatus.boundary.disposition)]);
				}
				if (loopStatus.decision_request?.decision_request_id) {
					facts.push(["Decision", loopStatus.decision_request.decision_request_id]);
				}
				if (loopStatus.autonomy) {
					facts.push(["Autonomy", autonomyReadbackLabel(loopStatus)]);
				}
				if (loopStatus.autonomy_objective?.source_ref) {
					facts.push(["Objective", loopStatus.autonomy_objective.source_ref]);
				}

				return facts;
			}

			function loopStatusInline(loopStatus) {
				if (!loopStatus) {
					return "";
				}
				if (loopStatus.decision_request?.reason) {
					return displayToken(loopStatus.decision_request.reason);
				}
				if (loopStatus.architecture_recovery?.status) {
					return displayToken(loopStatus.architecture_recovery.status);
				}
				if (loopStatus.review?.status) {
					return displayToken(loopStatus.review.status);
				}

				return autonomyReadbackLabel(loopStatus);
			}

			function autonomyReadbackHasFreshSourceRefs(loopStatus) {
				const signals = Array.isArray(loopStatus?.autonomy_signals)
					? loopStatus.autonomy_signals
					: [];
				return signals.some((signal) => {
					const sourceRefs = Array.isArray(signal?.source_refs) ? signal.source_refs : [];
					return sourceRefs.length > 0 && String(signal?.freshness || "") === "fresh";
				});
			}

			function autonomyReadbackLabel(loopStatus) {
				if (!loopStatus?.autonomy) {
					return "";
				}
				if (autonomyReadbackHasFreshSourceRefs(loopStatus)) {
					return displayToken(loopStatus.autonomy);
				}

				return "source refs needed";
			}

			function autonomyReadbackSummary(loopStatus) {
				if (!loopStatus) {
					return "none";
				}

				const report = loopStatus.autonomy_report || {};
				const objective = loopStatus.autonomy_objective?.source_ref || "none";
				const reportSourceRefs = Array.isArray(report.source_refs)
					? report.source_refs.length
					: 0;
				const knownGaps = Array.isArray(report.known_gaps)
					? report.known_gaps
					: [];
				const completeness = report.completeness || "unknown";
				const authority = report.authority || "derived_query_view";

				return [
					`objective=${objective}`,
					`signals=${(loopStatus.autonomy_signals || []).length}`,
					`proposals=${(loopStatus.autonomy_proposals || []).length}`,
					`lineage=${(loopStatus.autonomy_lineage || []).length}`,
					`source_refs=${reportSourceRefs}`,
					`completeness=${completeness}`,
					`authority=${authority}`,
					`gaps=${knownGaps.length}`,
				].join("; ");
			}

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

			function renderEmptyState(title, copy = "") {
				const copyAttributes = copy
					? ` title="${escapeHtml(copy)}" aria-label="${escapeHtml(`${title}: ${copy}`)}"`
					: ` aria-label="${escapeHtml(title)}"`;
				return `
					<div class="empty-state"${copyAttributes}>
						<strong>${escapeHtml(title)}</strong>
					</div>
				`;
			}

			function renderRoutineEmptyList(container) {
				container.innerHTML = "";
			}

			function keyedPatchNodeKey(node) {
				if (!(node instanceof Element)) {
					return "";
				}

				return node.dataset.renderKey || node.dataset.detailKey || "";
			}

			function syncElementAttributes(current, next) {
				for (const attribute of [...current.attributes]) {
					if (!next.hasAttribute(attribute.name)) {
						current.removeAttribute(attribute.name);
					}
				}

				for (const attribute of [...next.attributes]) {
					if (current.getAttribute(attribute.name) !== attribute.value) {
						current.setAttribute(attribute.name, attribute.value);
					}
				}
			}

			function patchNode(current, next) {
				if (
					current.nodeType !== next.nodeType ||
					current.nodeName !== next.nodeName
				) {
					current.replaceWith(next.cloneNode(true));
					return;
				}

				if (current.nodeType === Node.TEXT_NODE) {
					if (current.nodeValue !== next.nodeValue) {
						current.nodeValue = next.nodeValue;
					}
					return;
				}

				if (!(current instanceof Element) || !(next instanceof Element)) {
					return;
				}

				// Preserve active accordion animation styles until their timer clears them.
				if (
					current.closest("details.is-animating") &&
					!(current instanceof HTMLDetailsElement)
				) {
					return;
				}

				if (current instanceof HTMLDetailsElement) {
					const detailKey = detailStateKey(current);
					if (detailKey && detailDisclosureState.has(detailKey)) {
						const shouldOpen = detailDisclosureState.get(detailKey);
						if (shouldOpen) {
							next.setAttribute("open", "");
							next.dataset.detailState = "open";
						} else {
							next.removeAttribute("open");
							delete next.dataset.detailState;
						}
					}
				}

				syncElementAttributes(current, next);
				patchChildNodes(current, next, false);
			}

			function patchChildNodes(current, next, animateInsertions = false) {
				const currentChildren = [...current.childNodes];
				const nextChildren = [...next.childNodes];
				const keyedCurrent = new Map();

				for (const child of currentChildren) {
					const key = keyedPatchNodeKey(child);
					if (key && !keyedCurrent.has(key)) {
						keyedCurrent.set(key, child);
					}
				}

				let cursor = current.firstChild;
				const used = new Set();

				for (const nextChild of nextChildren) {
					const key = keyedPatchNodeKey(nextChild);
					let currentChild = key ? keyedCurrent.get(key) : null;

					while (cursor && used.has(cursor)) {
						cursor = cursor.nextSibling;
					}

					if (!currentChild && cursor && !keyedPatchNodeKey(cursor)) {
						currentChild = cursor;
					}

					if (currentChild) {
						used.add(currentChild);
						patchNode(currentChild, nextChild);
						if (currentChild !== cursor) {
							current.insertBefore(currentChild, cursor);
						}
						cursor = currentChild.nextSibling;
					} else {
						const clone = nextChild.cloneNode(true);
						current.insertBefore(clone, cursor);
						if (animateInsertions) {
							markStableListEnter(clone);
						}
						used.add(clone);
					}
				}

				for (const child of [...current.childNodes]) {
					if (!used.has(child)) {
						child.remove();
					}
				}
			}

			function markStableListEnter(node) {
				if (reducedMotionQuery.matches || !(node instanceof HTMLElement)) {
					return;
				}

				node.classList.add("is-list-entering");
				const clear = () => {
					node.classList.remove("is-list-entering");
				};
				node.addEventListener("animationend", clear, { once: true });
				window.setTimeout(clear, 360);
			}

			function animateStableListSize(container, startHeight) {
				if (reducedMotionQuery.matches) {
					return;
				}

				const endHeight = container.getBoundingClientRect().height;
				if (Math.abs(endHeight - startHeight) < 1) {
					return;
				}

				const previousHeight = container.style.height;
				const previousOverflow = container.style.overflow;
				const previousTransition = container.style.transition;
				let cleaned = false;

				const cleanup = (event) => {
					if (event && event.propertyName !== "height") {
						return;
					}
					if (cleaned) {
						return;
					}
					cleaned = true;
					container.classList.remove("is-size-animating");
					container.style.height = previousHeight;
					container.style.overflow = previousOverflow;
					container.style.transition = previousTransition;
				};

				container.classList.add("is-size-animating");
				container.style.height = `${startHeight}px`;
				container.style.overflow = "hidden";
				void container.offsetHeight;

				window.requestAnimationFrame(() => {
					container.style.transition = [previousTransition, "height var(--medium) var(--ease)"]
						.filter(Boolean)
						.join(", ");
					container.style.height = `${endHeight}px`;
					container.addEventListener("transitionend", cleanup, { once: true });
					window.setTimeout(cleanup, 360);
				});
			}

			function renderStableList(container, html) {
				const template = document.createElement("template");
				template.innerHTML = html.trim();
				const startHeight = reducedMotionQuery.matches
					? 0
					: container.getBoundingClientRect().height;

				patchChildNodes(container, template.content, true);
				animateStableListSize(container, startHeight);
			}

			function pluralLabel(count, singular, plural = `${singular}s`) {
				return count === 1 ? singular : plural;
			}

			function pluralize(count, singular, plural = `${singular}s`) {
				return `${count} ${pluralLabel(count, singular, plural)}`;
			}

			function rawSessionHistoryRuns(snapshot) {
				const currentLaneRuns = snapshotCurrentLaneRuns(snapshot);
				const currentLaneIds = new Set(currentLaneRuns.map((run) => run.run_id));
				const currentLaneIssueKeys = new Set(currentLaneRuns.flatMap(issueIdentityKeys));
				return (snapshot?.recent_runs ?? []).filter(
					(run) => !currentLaneIds.has(run.run_id) && !issueMatchesKeySet(run, currentLaneIssueKeys),
				);
			}

			function issueIdentifierFromRunId(runId) {
				const match = String(runId || "").match(/^([a-z][a-z0-9]*-\d+)-attempt-\d+(?:-\d+)?$/i);
				if (match) {
					return match[1].toUpperCase();
				}

				const recovered = String(runId || "").match(/^recovered-([a-z][a-z0-9]*-\d+)$/i);
				return recovered ? recovered[1].toUpperCase() : "";
			}

			function attemptNumberFromRun(run) {
				if (run?.attempt_number != null) {
					return String(run.attempt_number);
				}

				const match = String(run?.run_id || "").match(/-attempt-(\d+)(?:-\d+)?$/i);
				return match ? match[1] : "";
			}

			function issueIdentifierInText(value) {
				const match = String(value || "").match(/(?:^|[^A-Za-z0-9])([A-Za-z]+-\d+)(?=$|[^A-Za-z0-9])/);
				return match ? match[1].toUpperCase() : "";
			}

			function runGroupKey(run) {
				return canonicalIssueIdentityKey(run?.issue_id) || issueDisplayKey(run);
			}

			function isSuccessfulTerminalRun(run) {
				return ["succeeded", "completed", "merged"].includes(run?.status);
			}

			function sessionHistoryRuns(snapshot) {
				return sessionHistoryLanes(snapshot).map((lane) => lane.latest_run).filter(Boolean);
			}

			function normalizeHistoryLane(lane) {
				if (!lane?.latest_run) {
					return null;
				}

				const attempts = Array.isArray(lane.attempts) && lane.attempts.length
					? lane.attempts
					: [lane.latest_run];

				return {
					...lane,
					issue_key: lane.issue_key || issueDisplayKey(lane.latest_run),
					attempt_count: Number(lane.attempt_count ?? attempts.length),
					ledger_outcome: historyLedgerOutcome(lane),
					attempts,
				};
			}

			function historyLedgerOutcome(lane) {
				return lane?.ledger_outcome || {
					ledger_status: "not_loaded",
					final_outcome: "local_attempt_history",
					summary: "Linear history not loaded for this snapshot.",
					record_count: 0,
				};
			}

			function historyLedgerHasRecords(outcome) {
				return ["present", "partial"].includes(outcome?.ledger_status);
			}

			function historyLedgerWasLoaded(outcome) {
				return (outcome?.ledger_status || "not_loaded") !== "not_loaded";
			}

			function toneForHistoryLedgerOutcome(outcome, run) {
				if (
					[
						"needs_attention",
						"terminal_failure",
						"ledger_unavailable",
						"execution_ledger_missing",
					].includes(outcome?.final_outcome)
				) {
					return "tone-blocked";
				}
				if (["unavailable", "partial", "missing"].includes(outcome?.ledger_status)) {
					return "tone-wait";
				}
				if (["closeout", "cleanup_complete", "landed"].includes(outcome?.final_outcome)) {
					return "tone-land";
				}
				if (["review_handoff", "repair_handoff"].includes(outcome?.final_outcome)) {
					return "tone-review";
				}

				return toneForRun(run);
			}

			function historyLaneTitle(lane) {
				const outcome = historyLedgerOutcome(lane);

				if (historyLedgerWasLoaded(outcome)) {
					if (outcome.final_outcome === "ledger_unavailable") {
						return "Run history unavailable";
					}
					if (outcome.final_outcome === "execution_ledger_missing") {
						return "Execution ledger missing";
					}
					return displayToken(outcome.final_outcome || outcome.ledger_status);
				}

				return recentRunTitle(lane.latest_run);
			}

			function historyLaneSummary(lane) {
				const outcome = historyLedgerOutcome(lane);

				if (historyLedgerWasLoaded(outcome)) {
					return outcome.summary || `Latest recorded run event is ${displayToken(outcome.final_outcome)}.`;
				}

				return recentRunSummary(lane.latest_run, lane);
			}

			function historyLaneStatusBits(lane, tone) {
				const outcome = historyLedgerOutcome(lane);
				const run = lane.latest_run;

				if (!historyLedgerWasLoaded(outcome)) {
					const bits = [statusLabel(displayToken(run.status), tone)];

					if (run.wait_reason) {
						const waitReason = displayToken(run.wait_reason);
						if (!displayTextRepeats(recentRunSummary(run, lane), waitReason)) {
							bits.push(inlineStatusFact("Wait", waitReason));
						}
					}
					if (run.continuation_pending) {
						bits.push(inlineStatusFact("Continuation", "Pending"));
					}
					if (run.retry_kind) {
						bits.push(inlineStatusFact("Retry", displayToken(run.retry_kind)));
					}

					return bits;
				}

				const bits = [statusLabel(displayToken(outcome.final_outcome), tone)];

				bits.push(inlineStatusFact("History", displayToken(outcome.ledger_status)));
				if (outcome.closeout_status) {
					bits.push(inlineStatusFact("Closeout", displayToken(outcome.closeout_status)));
				}
				if (outcome.needs_attention_reason) {
					bits.push(inlineStatusFact("Attention", "Recorded"));
				}

				return bits;
			}

			function groupedHistoryLanesFromRuns(runs) {
				const lanes = [];
				const laneIndexes = new Map();

				for (const run of runs) {
					const key = runGroupKey(run);
					const index = laneIndexes.get(key);

					if (index != null) {
						const lane = lanes[index];
						lane.attempt_count += 1;
						lane.attempts.push(run);
						continue;
					}

					laneIndexes.set(key, lanes.length);
					lanes.push({
						issue_id: run.issue_id || key,
						issue_key: issueDisplayKey(run),
						attempt_count: 1,
						latest_run: run,
						attempts: [run],
					});
				}

				return lanes;
			}

			function sessionHistoryLanes(snapshot) {
				if (Array.isArray(snapshot?.history_lanes)) {
					return snapshot.history_lanes.map(normalizeHistoryLane).filter(Boolean);
				}

				return groupedHistoryLanesFromRuns(rawSessionHistoryRuns(snapshot));
			}

			function runThreadSummary(run) {
				if (run.thread_id) {
					return `${run.thread_id} (${run.thread_status || "unknown"})`;
				}
				return run.thread_status || "not captured";
			}

			function runThreadFlagSummary(run) {
				const flags = run.thread_active_flags ?? [];
				return flags.length ? flags.join(", ") : "none";
			}

			function runModelSummary(run) {
				const parts = [run.effective_model_provider, run.effective_model].filter(Boolean);
				return parts.length ? parts.join(" / ") : "not captured";
			}

			function runProcessSummary(run) {
				if (run.process_id == null) {
					return "not captured";
				}
				if (run.process_alive == null) {
					return `${run.process_id} (unknown)`;
				}
				const reason = run.process_alive
					? "process_alive"
					: run.process_liveness_reason || "process_stopped";
				return `${run.process_id} (${processLivenessReasonLabel(reason)})`;
			}

			function processLivenessReasonLabel(reason) {
				return displayToken(reason || "unknown");
			}

			function runExecutionLivenessSummary(run) {
				return displayToken(run.execution_liveness || "liveness_unknown");
			}

			function runOwnershipSummary(run) {
				return displayToken(run.ownership_state || (runCountsAsRunning(run) ? "leased_run" : "unknown"));
			}

			function runLivenessStateSummary(run) {
				return displayToken(run.liveness_state || "unknown");
			}

			function runPolicyStateSummary(run) {
				return displayToken(run.policy_state || "allowed");
			}

			function runTerminalizationSummary(run) {
				return displayToken(run.terminalization_state || "none");
			}

			function runLaneControlConditionsSummary(run) {
				const conditions = run.lane_control_conditions ?? [];
				return conditions.length ? conditions.map(displayToken).join(", ") : "none";
			}

			function runContinuationRecoverySummary(run) {
				const recovery = run.continuation_recovery;
				if (!recovery) {
					return "none";
				}

				const count = `${recovery.recovery_count ?? 0}/${recovery.automatic_continuation_limit ?? 0}`;
				const exceeded = recovery.budget_exceeded ? "budget exceeded" : "within budget";
				const message = recovery.source_error_message
					? `; ${recovery.source_error_message}`
					: "";

				return `${displayToken(recovery.state)} · ${displayToken(recovery.source_phase)} -> ${displayToken(recovery.next_phase)} · ${displayToken(recovery.source_error_class)} · ${count} · ${exceeded}${message}`;
			}

			function runQueueLeaseSummary(run) {
				const leaseState = run.queue_lease_state || (run.run_lease ? "held" : "not_held");

				if (leaseState === "held") {
					return "held";
				}

				return `${leaseState}; ${displayToken(run.execution_liveness || "liveness_unknown")}`;
			}

			function runApprovalSummary(run) {
				const policy = run.effective_approval_policy || "not captured";
				return run.effective_approvals_reviewer
					? `${policy} / ${run.effective_approvals_reviewer}`
					: policy;
			}

			function protocolEventSummary(run) {
				if (run.last_event_type && run.last_event_at) {
					return `${run.last_event_type} @ ${formatTimestamp(run.last_event_at)}`;
				}
				if (run.last_event_type) {
					return run.last_event_type;
				}
				if (run.last_event_at) {
					return formatTimestamp(run.last_event_at);
				}

				return "not captured";
			}

			function protocolActivity(run) {
				return run?.protocol_activity || null;
			}

			function protocolActivityWaitReason(run) {
				const activity = protocolActivity(run);
				if (activity?.waiting_reason) {
					return activity.waiting_reason;
				}
				if (run?.wait_reason) {
					return run.wait_reason;
				}

				return "";
			}

			function protocolActivityFocus(run) {
				switch (protocolActivityWaitReason(run)) {
					case "model_execution":
						return "model execution";
					case "tool_execution":
					case "protocol_activity":
						return "tools";
					case "approval_or_user_input":
						return "approval/user input";
					case "protocol_idleness":
						return "protocol idleness";
					default:
						return "";
				}
			}

			function protocolActivityRecentSummary(run) {
				const events = protocolActivity(run)?.recent_events || [];
				if (!events.length) {
					return "not captured";
				}
				return events
					.slice(-5)
					.reverse()
					.map((event) => {
						const detail = event.detail ? `:${event.detail}` : "";
						return `${event.event_type || "event"}${detail}`;
					})
					.join(", ");
			}

			function protocolActivityDebugSummary(run) {
				const activity = protocolActivity(run);
				if (!activity) {
					return "none";
				}
				const parts = [
					`turn ${activity.turn_status || "none"}`,
					`waiting ${activity.waiting_reason || "none"}`,
					`recent ${protocolActivityRecentSummary(run)}`,
				];

				return parts.join("; ");
			}

			function issueDisplayKey(item) {
				if (!item) {
					return "unknown";
				}
				if (item.issue_identifier) {
					return item.issue_identifier;
				}
				const runIssueIdentifier = issueIdentifierFromRunId(item.run_id);
				if (runIssueIdentifier) {
					return runIssueIdentifier;
				}
				for (const value of [item.worktree_path, item.branch_name]) {
					const identifier = issueIdentifierInText(value);
					if (identifier) {
						return identifier;
					}
				}

				return item.issue_id || "unknown";
			}

			function canonicalIssueIdentityKey(value) {
				const key = String(value ?? "").trim();

				if (!key || key.toLowerCase() === "unknown") {
					return "";
				}

				return key.toUpperCase();
			}

			function issueIdentityKeys(item) {
				if (!item) {
					return [];
				}

				const keys = [item.issue_id, item.issue_identifier, issueDisplayKey(item)]
					.map(canonicalIssueIdentityKey)
					.filter(Boolean);

				return [...new Set(keys)];
			}

			function issueMatchesKeySet(item, keySet) {
				return issueIdentityKeys(item).some((key) => keySet.has(key));
			}

			function currentLaneFreshness(run) {
				if (run?.last_run_activity_at) {
					return {
						label: "Lane activity",
						source: "last_run_activity_at",
						sourceLabel: "live activity",
						timestamp: run.last_run_activity_at,
					};
				}
				if (run?.last_progress_at) {
					return {
						label: "Last progress",
						source: "last_progress_at",
						sourceLabel: "progress",
						timestamp: run.last_progress_at,
					};
				}
				if (run?.last_protocol_activity_at) {
					return {
						label: "Protocol activity",
						source: "last_protocol_activity_at",
						sourceLabel: "protocol activity",
						timestamp: run.last_protocol_activity_at,
					};
				}
				return {
					label: "Lane activity",
					source: "none",
					sourceLabel: "activity",
					timestamp: null,
				};
			}

			function currentLaneFreshnessFact(run, formatter = formatTimestamp) {
				const freshness = currentLaneFreshness(run);
				return [
					freshness.label,
					freshness.timestamp ? formatter(freshness.timestamp) : "not captured",
				];
			}

			function historyRunTimingFacts(run) {
				return [
					["Updated", formatTimestampCompact(run.updated_at)],
					["Attempt", String(run.attempt_number ?? "none")],
					["Status", displayToken(run.status)],
					["Events", String(run.event_count ?? 0)],
				];
			}

			function lifecycleNumber(value) {
				const number = Number(value ?? 0);

				return Number.isFinite(number) ? Math.max(0, number) : 0;
			}

			function historyLaneLifecycleMetrics(lane) {
				const attempts = Array.isArray(lane?.attempts) && lane.attempts.length
					? lane.attempts
					: lane?.latest_run
						? [lane.latest_run]
						: [];
				const provided = lane?.lifecycle_metrics || {};
				const summaries = attempts.map(childAgentActivity).filter(Boolean);
				const captured = lifecycleNumber(
					provided.captured_attempt_count ?? summaries.length,
				);
				const attemptCount = lifecycleNumber(
					provided.attempt_count ?? lane?.attempt_count ?? attempts.length,
				);

				return {
					...provided,
					attempt_count: attemptCount,
					captured_attempt_count: captured,
					missing_attempt_count: lifecycleNumber(
						provided.missing_attempt_count ?? Math.max(0, attemptCount - captured),
					),
					protocol_event_count: lifecycleNumber(
						provided.protocol_event_count ??
							attempts.reduce((total, run) => total + lifecycleNumber(run?.event_count), 0),
					),
					child_event_count: lifecycleNumber(
						provided.child_event_count ??
							summaries.reduce((total, summary) => total + lifecycleNumber(summary.event_count), 0),
					),
					wall_seconds: lifecycleNumber(
						provided.wall_seconds ??
							summaries.reduce((total, summary) => total + lifecycleNumber(summary.wall_seconds), 0),
					),
					tool_call_count: lifecycleNumber(
						provided.tool_call_count ??
							summaries.reduce((total, summary) => total + lifecycleNumber(summary.tool_call_count), 0),
					),
					input_tokens_cumulative: lifecycleNumber(
						provided.input_tokens_cumulative ??
							summaries.reduce(
								(total, summary) => total + lifecycleNumber(summary.input_tokens_cumulative),
								0,
							),
					),
					output_tokens_cumulative: lifecycleNumber(
						provided.output_tokens_cumulative ??
							summaries.reduce(
								(total, summary) => total + lifecycleNumber(summary.output_tokens_cumulative),
								0,
							),
					),
					buckets: Array.isArray(provided.buckets) ? provided.buckets : [],
					phases: Array.isArray(provided.phases)
						? provided.phases.map(normalizeLifecyclePhaseMetrics)
						: [],
				};
			}

			function normalizeLifecyclePhaseMetrics(phase) {
				return {
					...phase,
					phase: phase?.phase || "unknown",
					label: phase?.label || displayToken(phase?.phase || "unknown"),
					attempt_count: lifecycleNumber(phase?.attempt_count),
					recorded_attempt_count: lifecycleNumber(phase?.recorded_attempt_count),
					recovered_attempt_count: lifecycleNumber(phase?.recovered_attempt_count),
					current_snapshot_attempt_count: lifecycleNumber(phase?.current_snapshot_attempt_count),
					captured_attempt_count: lifecycleNumber(phase?.captured_attempt_count),
					missing_attempt_count: lifecycleNumber(phase?.missing_attempt_count),
					protocol_event_count: lifecycleNumber(phase?.protocol_event_count),
					child_event_count: lifecycleNumber(phase?.child_event_count),
					wall_seconds: lifecycleNumber(phase?.wall_seconds),
					tool_call_count: lifecycleNumber(phase?.tool_call_count),
					input_tokens_cumulative: lifecycleNumber(phase?.input_tokens_cumulative),
					output_tokens_cumulative: lifecycleNumber(phase?.output_tokens_cumulative),
					buckets: Array.isArray(phase?.buckets) ? phase.buckets : [],
				};
			}

			function lifecycleBucket(metrics, bucketName) {
				return (metrics?.buckets || []).find(
					(candidate) => String(candidate?.name || "").toLowerCase() === bucketName.toLowerCase(),
				);
			}

			function lifecycleBucketSeconds(metrics, bucketName) {
				const bucket = lifecycleBucket(metrics, bucketName);

				return lifecycleNumber(bucket?.wall_seconds);
			}

			function lifecycleWallSeconds(metrics) {
				const buckets = Array.isArray(metrics?.buckets) ? metrics.buckets : [];
				return Math.max(
					1,
					lifecycleNumber(metrics?.wall_seconds),
					buckets.reduce((total, bucket) => total + lifecycleNumber(bucket?.wall_seconds), 0),
				);
			}

			function formatRuntimeShare(seconds, totalSeconds) {
				const elapsed = lifecycleNumber(seconds);
				const total = Math.max(1, lifecycleNumber(totalSeconds), elapsed);
				if (elapsed <= 0) {
					return { percent: "-", elapsed: "-", total: "-", ratio: "-", text: "-" };
				}

				const percent = Math.round((elapsed / total) * 100);
				const elapsedText = formatDuration(elapsed);
				const totalText = formatDuration(total);
				const ratio = `${compactRuntimeDuration(elapsedText)}/${compactRuntimeDuration(totalText)}`;
				return {
					percent: `${percent}%`,
					elapsed: elapsedText,
					total: totalText,
					ratio,
					text: `${ratio}(${percent}%)`,
				};
			}

			function compactRuntimeDuration(value) {
				return String(value || "-").replaceAll(" ", "");
			}

			function historyLifecycleTokenSummary(metrics) {
				const input = lifecycleNumber(metrics?.input_tokens_cumulative);
				const output = lifecycleNumber(metrics?.output_tokens_cumulative);

				if (input === 0 && output === 0) {
					return "not captured";
				}

				return `in ${formatCompactCount(input)} / out ${formatCompactCount(output)}`;
			}

			function historyLifecycleCaptureSummary(metrics) {
				const captured = lifecycleNumber(metrics?.captured_attempt_count);
				const attempts = lifecycleNumber(metrics?.attempt_count);
				const missing = lifecycleNumber(metrics?.missing_attempt_count);

				return missing > 0 ? `${captured}/${attempts} captured · ${missing} missing` : `${captured}/${attempts} captured`;
			}

			function currentLaneTelemetryFacts(run) {
				const facts = [];
				const freshness = currentLaneFreshness(run);
				const focus = protocolActivityFocus(run);

				facts.push(["run phase", displayToken(run.run_phase || run.phase || run.status)]);
				if (freshness.timestamp) {
					facts.push([freshness.sourceLabel, formatRelativeTimestamp(freshness.timestamp)]);
				}
				if (focus) {
					facts.push(["focus", detailLabel(focus)]);
				}
				if (run.idle_for_seconds != null) {
					facts.push(["lane idle", formatDuration(run.idle_for_seconds)]);
				}
				if (run.protocol_idle_for_seconds != null) {
					facts.push(["agent idle", formatDuration(run.protocol_idle_for_seconds)]);
				}
				return facts;
			}

			function currentLaneReadbackValues(run) {
				return [
					run?.run_phase || run?.phase || run?.status,
					run?.current_operation,
					run?.active_goal_phase,
					run?.public_progress_phase,
				].filter(Boolean);
			}

			function currentLaneVisibleSummary(card, run) {
				const summary = String(card?.detail || "").trim();
				if (!summary) {
					return "";
				}

				return currentLaneReadbackValues(run).some((value) => displayTextRepeats(summary, value))
					? ""
					: summary;
			}

			function renderRunMetaFact(label, value, valueClass = "", title = "") {
				const classAttribute = valueClass ? ` class="${escapeHtml(valueClass)}"` : "";
				const titleAttribute = title ? ` title="${escapeHtml(title)}"` : "";

				return `
					<span class="run-meta-item">
						<span class="run-meta-label">${escapeHtml(detailLabel(label))}</span>
						<strong${classAttribute}${titleAttribute}>${escapeHtml(value)}</strong>
					</span>
				`;
			}

			function renderRunTelemetryMetaItems(run) {
				const facts = currentLaneTelemetryFacts(run);

				if (!facts.length) {
					return "";
				}

				return facts.map(([label, value]) => renderRunMetaFact(label, value)).join("");
			}

				function currentLaneLifecycleMetrics(run, summary = childAgentActivity(run)) {
					const provided = run?.lifecycle_metrics || {};
					const providedPhases = Array.isArray(provided.phases)
						? provided.phases.map(normalizeLifecyclePhaseMetrics)
						: [];
					const fallbackCaptured = summary ? 1 : 0;
					const attemptCount = lifecycleNumber(provided.attempt_count ?? (summary ? 1 : 0));
					const capturedAttemptCount = lifecycleNumber(
						provided.captured_attempt_count ?? fallbackCaptured,
					);
					const buckets = Array.isArray(provided.buckets) && provided.buckets.length
						? provided.buckets
						: childAgentBuckets(summary);
					const metrics = {
						...provided,
						attempt_count: attemptCount,
						run_count: lifecycleNumber(provided.run_count ?? attemptCount),
						recorded_attempt_count: lifecycleNumber(provided.recorded_attempt_count),
						recovered_attempt_count: lifecycleNumber(provided.recovered_attempt_count),
						current_snapshot_attempt_count: lifecycleNumber(provided.current_snapshot_attempt_count),
						captured_attempt_count: capturedAttemptCount,
						missing_attempt_count: lifecycleNumber(
							provided.missing_attempt_count ??
								Math.max(0, attemptCount - capturedAttemptCount),
						),
						protocol_event_count: lifecycleNumber(provided.protocol_event_count ?? run?.event_count),
						child_event_count: lifecycleNumber(provided.child_event_count ?? summary?.event_count),
						wall_seconds: lifecycleNumber(provided.wall_seconds ?? summary?.wall_seconds),
						tool_call_count: lifecycleNumber(provided.tool_call_count ?? summary?.tool_call_count),
						input_tokens_current: provided.input_tokens_current ?? summary?.input_tokens_current ?? null,
						input_tokens_peak: provided.input_tokens_peak ?? summary?.input_tokens_max ?? null,
						input_tokens_cumulative: lifecycleNumber(
							provided.input_tokens_cumulative ?? summary?.input_tokens_cumulative,
						),
						output_tokens_cumulative: lifecycleNumber(
							provided.output_tokens_cumulative ?? summary?.output_tokens_cumulative,
						),
						largest_tool_output_bytes:
							provided.largest_tool_output_bytes ?? summary?.largest_tool_output_bytes ?? null,
						largest_tool_output_tool:
							provided.largest_tool_output_tool ?? summary?.largest_tool_output_tool ?? null,
						large_output_warnings: Array.isArray(provided.large_output_warnings)
							? provided.large_output_warnings
							: childAgentLargeOutputWarnings(summary),
						buckets,
						phases: providedPhases,
					};

					if (!metrics.phases.length && summary) {
						const phase = fallbackLifecyclePhaseForRun(run);
						metrics.phases = [
							normalizeLifecyclePhaseMetrics({
								phase: phase.key,
								label: phase.label,
								attempt_count: attemptCount || 1,
								run_count: 1,
								captured_attempt_count: 1,
								missing_attempt_count: 0,
								protocol_event_count: lifecycleNumber(run?.event_count),
								child_event_count: lifecycleNumber(summary.event_count),
								wall_seconds: lifecycleNumber(summary.wall_seconds),
								tool_call_count: lifecycleNumber(summary.tool_call_count),
								input_tokens_current: summary.input_tokens_current ?? null,
								input_tokens_peak: summary.input_tokens_max ?? null,
								input_tokens_cumulative: lifecycleNumber(summary.input_tokens_cumulative),
								output_tokens_cumulative: lifecycleNumber(summary.output_tokens_cumulative),
								largest_tool_output_bytes: summary.largest_tool_output_bytes ?? null,
								largest_tool_output_tool: summary.largest_tool_output_tool ?? null,
								large_output_warnings: childAgentLargeOutputWarnings(summary),
								buckets: childAgentBuckets(summary),
							}),
						];
					}

					return metrics;
				}

				function fallbackLifecyclePhaseForRun(run) {
					const status = String(run?.status || "").toLowerCase();
					const operation = String(run?.current_operation || "").toLowerCase();
					const reviewPhase = String(run?.loop_status?.review?.phase || "").toLowerCase();

					if (["cleanup_complete", "closeout", "closeout_pending", "landed"].includes(status)) {
						return { key: "closeout", label: "Closeout" };
					}
					if (
						["manual_attention", "manual_attention_pending", "needs_attention", "terminal_failure"].includes(status) ||
						String(run?.phase || "").toLowerCase() === "needs_attention"
					) {
						return { key: "manual_attention", label: "Manual attention" };
					}
					if (reviewPhase === "repair" || status === "review_repair_pending") {
						return { key: "review_repair", label: "Review repair" };
					}
					if (reviewPhase || status === "review_handoff_pending" || operation === "review_writeback") {
						return { key: "review", label: "Review" };
					}

					return { key: "development", label: "Development" };
				}

				function lifecycleMetricDurationFact(metrics) {
					const modelSeconds = lifecycleBucketSeconds(metrics, "Model");
					const activitySeconds = modelSeconds > 0 ? modelSeconds : lifecycleNumber(metrics?.wall_seconds);

					if (activitySeconds <= 0) {
						return null;
					}

					return [modelSeconds > 0 ? "inference" : "activity", formatDuration(activitySeconds)];
				}

				function lifecycleMetricFacts(metrics, { includeAttempts = false } = {}) {
					if (!metrics) {
						return [];
					}

					const facts = [];
					const durationFact = lifecycleMetricDurationFact(metrics);
					const tokenSummary = historyLifecycleTokenSummary(metrics);
					const attempts = lifecycleNumber(metrics.attempt_count);
					const captured = lifecycleNumber(metrics.captured_attempt_count);
					const missing = lifecycleNumber(metrics.missing_attempt_count);
					const childEvents = lifecycleNumber(metrics.child_event_count);
					const protocolEvents = lifecycleNumber(metrics.protocol_event_count);

					if (includeAttempts && (attempts > 1 || missing > 0)) {
						facts.push([
							"attempts",
							missing > 0 ? `${captured}/${attempts} captured` : formatCompactCount(attempts),
						]);
					}
					if (durationFact) {
						facts.push(durationFact);
					}
					if (tokenSummary !== "not captured") {
						facts.push(["tokens", tokenSummary]);
					}
					if (lifecycleNumber(metrics.tool_call_count) > 0) {
						facts.push(["tools", formatCompactCount(metrics.tool_call_count)]);
					}
					if (childEvents || protocolEvents) {
						facts.push([
							"events",
							childEvents && protocolEvents
								? `${formatCompactCount(childEvents)} child / ${formatCompactCount(protocolEvents)} protocol`
								: formatCompactCount(childEvents || protocolEvents),
						]);
					}
					if (metrics.largest_tool_output_bytes != null) {
						facts.push([
							"max output",
							formatLargestOutputValue(metrics.largest_tool_output_bytes),
						]);
					}

					return facts;
				}

				function lifecycleRecoveryDebugSummary(metrics) {
					if (!metrics) {
						return "none";
					}

					const recorded = lifecycleNumber(metrics.recorded_attempt_count);
					const recovered = lifecycleNumber(metrics.recovered_attempt_count);
					const currentSnapshot = lifecycleNumber(metrics.current_snapshot_attempt_count);
					const gaps = Array.isArray(metrics.recovery_gaps)
						? metrics.recovery_gaps.filter(Boolean)
						: [];
					const parts = [
						`recorded ${formatCompactCount(recorded)}`,
						`recovered ${formatCompactCount(recovered)}`,
						`current snapshot ${formatCompactCount(currentSnapshot)}`,
					];

					if (gaps.length) {
						parts.push(`gaps ${gaps.join(", ")}`);
					}

					return parts.join("; ");
				}

				function lifecycleEvidenceDebugSummary(metrics) {
					const attempts = Array.isArray(metrics?.attempt_evidence)
						? metrics.attempt_evidence.filter(Boolean)
						: [];

					if (!attempts.length) {
						return "none";
					}

					return attempts
						.map((attempt) => {
							const evidence = Array.isArray(attempt.evidence) && attempt.evidence.length
								? attempt.evidence.join(",")
								: "none";
							const gaps = Array.isArray(attempt.gaps) && attempt.gaps.length
								? attempt.gaps.join(",")
								: "none";

							return `${attempt.run_id || "unknown"}#${attempt.attempt_number || "?"} ${attempt.phase || "unknown"} ${attempt.source || "unknown"} evidence=${evidence} gaps=${gaps}`;
						})
						.join("; ");
				}
