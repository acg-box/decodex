
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
