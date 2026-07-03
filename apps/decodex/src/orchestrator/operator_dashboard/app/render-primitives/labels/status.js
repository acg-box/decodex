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
