
			function renderQueuedCandidates(container, items) {
				if (!items.length) {
					renderRoutineEmptyList(container);
					return;
				}

				container.innerHTML = items
					.map((candidate) => {
						const tone = toneForQueuedCandidate(candidate);
						const blockers = candidate.blocker_identifiers.length
							? candidate.blocker_identifiers.join(", ")
							: "NONE";
						const summary = summarizeQueuedCandidate(candidate);
						const reason = queuedCandidateInlineReason(candidate);

						return `
							<article class="action-card ${tone}">
								<div class="row-head">
									<div class="row-title">
										<div class="kicker">
											<span>Issue</span>
											<span class="mono">${escapeHtml(candidate.issue_identifier)}</span>
										</div>
										<h4>${escapeHtml(candidate.title)}</h4>
									</div>
								</div>
								${summary ? `<p class="row-summary">${escapeHtml(summary)}</p>` : ""}
								<div class="status-line">
									${statusLabel(queuedCandidateStatusText(candidate), tone)}
									${reason ? inlineStatusFact("Reason", reason) : ""}
								</div>
								${renderAttentionFacts(candidate)}
								<div class="grid two card-facts">
									${cardField("State", formatDetailToken(candidate.state))}
									${cardField("Priority", formatPriority(candidate.priority))}
									${cardField("Created", formatTimestampCompact(candidate.created_at), "is-time")}
									${cardField("Blockers", blockers, blockers === "NONE" ? "is-muted" : "")}
								</div>
							</article>
						`;
					})
					.join("");
			}

			function renderActionCards(container, items) {
				if (!items.length) {
					renderRoutineEmptyList(container);
					return;
				}

				container.innerHTML = items
					.map(
						(item) => `
							<article class="action-card ${item.tone}">
								<div class="row-head">
									<div class="row-title">
										<div class="kicker">
											<span>${escapeHtml(item.scope)}</span>
											<span class="mono">${escapeHtml(item.issue)}</span>
										</div>
										<h4>${escapeHtml(item.title)}</h4>
									</div>
								</div>
								${item.summary ? `<p class="row-summary">${escapeHtml(item.summary)}</p>` : ""}
								<div class="status-line">
									${statusLabel(item.status, item.tone)}
								</div>
								<div class="grid two card-facts">
									${item.facts.map(([label, value, valueClass]) => cardField(label, value, cardFactValueClass(value, valueClass))).join("")}
								</div>
							</article>
						`,
					)
					.join("");
			}

			function reviewLaneItems(derived) {
				const rankedItems = [
					...derived.attentionItems
						.filter((item) => ["Review", "Closeout", "Cleanup"].includes(item.scope))
						.map((item) => ({ ...item, sortRank: 0 })),
					...derived.readyItems.map((item) => ({ ...item, sortRank: 1 })),
					...derived.waitingItems
						.filter((item) => item.scope === "Review")
						.map((item) => ({ ...item, sortRank: 2 })),
				];

				return rankedItems.sort(
					(left, right) => left.sortRank - right.sortRank || left.issue.localeCompare(right.issue),
				);
			}
